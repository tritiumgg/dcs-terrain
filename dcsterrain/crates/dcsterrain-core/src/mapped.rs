//! Arrays that live in a file: a read-only view of it, not a copy, so their
//! pages are the OS's to keep or drop and cost the process no private memory
//! (ADR 0008).

use std::fs::File;
use std::io;
use std::ops::Deref;
use std::sync::Arc;

use memmap2::{Mmap, MmapOptions};

/// A read-only view of part of a file, shared by the columns in it.
#[derive(Debug, Clone)]
pub struct View(Arc<Bytes>);

#[derive(Debug)]
enum Bytes {
    Mapped(Mmap),
    /// Bytes in memory, read as a view of a file is.
    Held(Vec<u8>),
}

impl From<Vec<u8>> for View {
    fn from(b: Vec<u8>) -> View {
        View(Arc::new(Bytes::Held(b)))
    }
}

impl View {
    /// `len` bytes of `file` from `offset`, asked of the disk at once: page
    /// by page a cold view would read a piece at a time.
    pub fn of(file: &File, offset: u64, len: usize) -> io::Result<View> {
        // SAFETY: the view is only read. A file cut short under it would
        // fault, which is why a view is taken only of a file whose length the
        // caller has checked against its table of contents, and held open.
        let map = unsafe { MmapOptions::new().offset(offset).len(len).map(file)? };
        prefetch(&map);
        Ok(View(Arc::new(Bytes::Mapped(map))))
    }

    pub fn bytes(&self) -> &[u8] {
        match &*self.0 {
            Bytes::Mapped(m) => m,
            Bytes::Held(b) => b,
        }
    }
}

#[cfg(windows)]
fn prefetch(map: &Mmap) {
    use windows_sys::Win32::System::Memory::{PrefetchVirtualMemory, WIN32_MEMORY_RANGE_ENTRY};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let range = WIN32_MEMORY_RANGE_ENTRY {
        VirtualAddress: map.as_ptr() as *mut _,
        NumberOfBytes: map.len(),
    };
    // SAFETY: the range is this mapping. It is only a hint: on failure the
    // pages are read as they are touched.
    unsafe { PrefetchVirtualMemory(GetCurrentProcess(), 1, &range, 0) };
}

#[cfg(unix)]
fn prefetch(map: &Mmap) {
    // Only a hint: on failure the pages are read as they are touched.
    let _ = map.advise(memmap2::Advice::WillNeed);
}

/// An array read from a file, or held where a view could not align it.
#[derive(Debug, Clone)]
pub enum Column<T> {
    Owned(Vec<T>),
    Viewed {
        /// Keeps `ptr` valid.
        view: View,
        ptr: *const T,
        len: usize,
    },
}

// SAFETY: a viewed column is shared read-only memory of `T`, as a `&[T]` is.
unsafe impl<T: Sync> Send for Column<T> {}
unsafe impl<T: Sync> Sync for Column<T> {}

impl<T: bytemuck::Pod> Column<T> {
    /// The `len` elements at byte `at` of `view`, in place when they align,
    /// and else copied. `None` when they run past it.
    pub fn in_view(view: &View, at: usize, len: usize) -> Option<Column<T>> {
        let bytes = at
            .checked_add(len.checked_mul(size_of::<T>())?)
            .and_then(|end| view.bytes().get(at..end))?;
        Some(match bytemuck::try_cast_slice::<u8, T>(bytes) {
            Ok(s) => Column::Viewed {
                view: view.clone(),
                ptr: s.as_ptr(),
                len,
            },
            Err(_) => {
                let mut v = vec![T::zeroed(); len];
                bytemuck::cast_slice_mut(&mut v).copy_from_slice(bytes);
                Column::Owned(v)
            }
        })
    }
}

impl<T: Clone> Column<T> {
    /// The elements to change, copied out of a view if they lie in one.
    pub fn to_mut(&mut self) -> &mut Vec<T> {
        if let Column::Viewed { .. } = self {
            *self = Column::Owned(self.to_vec());
        }
        match self {
            Column::Owned(v) => v,
            Column::Viewed { .. } => unreachable!("just copied"),
        }
    }
}

impl<T> Column<T> {
    /// The bytes of private memory it holds.
    pub fn private_bytes(&self) -> usize {
        match self {
            Column::Owned(v) => v.len() * size_of::<T>(),
            Column::Viewed { .. } => 0,
        }
    }
}

impl<T> Default for Column<T> {
    fn default() -> Column<T> {
        Column::Owned(Vec::new())
    }
}

impl<T> From<Vec<T>> for Column<T> {
    fn from(v: Vec<T>) -> Column<T> {
        Column::Owned(v)
    }
}

impl<T> Deref for Column<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        match self {
            Column::Owned(v) => v,
            // SAFETY: `in_view` took these from a slice of the view, which
            // `view` keeps mapped and nothing writes.
            Column::Viewed { ptr, len, .. } => unsafe { std::slice::from_raw_parts(*ptr, *len) },
        }
    }
}

impl<T: PartialEq> PartialEq for Column<T> {
    fn eq(&self, other: &Column<T>) -> bool {
        **self == **other
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_column_reads_its_files_bytes_aligned_or_not() {
        let s = Scratch::new();
        std::fs::create_dir_all(&s.0).unwrap();
        let path = s.0.join("f.bin");
        let bytes: Vec<u8> = (0..1024u32).flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(&path, &bytes).unwrap();
        let view = View::of(&File::open(&path).unwrap(), 4, 4092).unwrap();
        let aligned = view.bytes().as_ptr().align_offset(4);
        let c: Column<u32> = Column::in_view(&view, aligned, 8).unwrap();
        assert_eq!(c[0], (aligned / 4 + 1) as u32);
        assert_eq!(c.private_bytes(), 0);
        let copied: Column<u32> = Column::in_view(&view, aligned + 1, 8).unwrap();
        assert_eq!(copied.private_bytes(), 32);
        assert!(Column::<u32>::in_view(&view, 4000, 100).is_none());
    }
}
