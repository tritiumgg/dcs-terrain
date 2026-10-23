//! A scratch directory for tests that write an extract.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh directory under the system temp, removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "dcsterrain-core-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        );
        // A crashed run may have left one under a reused pid.
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
