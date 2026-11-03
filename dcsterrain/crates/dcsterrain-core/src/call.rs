//! The call a request is answered for: whether its caller has given up on
//! it, and how far its work has come.
//!
//! A server sets a ticket on the thread that answers, around each request.
//! A request answered with none set runs to its end and reports to nobody,
//! as `query` answers. An operation takes a [`Call`] before it spreads its
//! work, and the pool's workers see the ticket through it, not through
//! their own thread.
//!
//! Giving up is checked where the work loops, often enough that a call stops
//! well within a second of being cancelled. Work stopped part way is never
//! kept: whatever was cut short fails, and a failure writes no cache entry.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::source::Refusal;
use crate::types::Issue;

/// The least time between two reports: a client shows them to a person,
/// and one per candidate would outnumber the answer.
const EVERY: Duration = Duration::from_millis(250);

/// Where progress goes: the share of the work done, from 0 to 1, and what
/// the work is doing.
pub type Report = Box<dyn Fn(f64, &str) + Send + Sync>;

/// One call's state, shared by the thread that answers and whoever may
/// cancel it.
pub struct Ticket {
    cancelled: AtomicBool,
    report: Report,
    /// When the last report went, and its share.
    last: Mutex<Option<(Instant, f64)>>,
}

impl Ticket {
    pub fn new(report: Report) -> Arc<Ticket> {
        Arc::new(Ticket {
            cancelled: AtomicBool::new(false),
            report,
            last: Mutex::new(None),
        })
    }

    /// Asks the call's work to stop. It stops at its next check, and the
    /// request fails as cancelled.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<Ticket>>> = const { RefCell::new(None) };
}

/// Runs `f` with `ticket` as the call of this thread, and then the one
/// before it, however `f` ends.
pub fn within<R>(ticket: Arc<Ticket>, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Arc<Ticket>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|c| *c.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(CURRENT.with(|c| c.replace(Some(ticket))));
    f()
}

/// The call this thread answers for, or none.
#[derive(Clone, Default)]
pub struct Call(Option<Arc<Ticket>>);

impl Call {
    /// The calling thread's call. Take it before spreading work over the
    /// pool, whose threads have none.
    pub fn current() -> Call {
        Call(CURRENT.with(|c| c.borrow().clone()))
    }

    /// Whether the caller has given up: work that sees it should end as
    /// soon as it can, and its caller then [`check`](Self::check).
    pub fn stopped(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|t| t.cancelled.load(Ordering::Relaxed))
    }

    /// The request's failure once its caller has given up.
    pub fn check(&self) -> Result<(), Vec<Issue>> {
        if self.stopped() {
            Err(cancelled())
        } else {
            Ok(())
        }
    }

    /// Reports `share` of the work done, from 0 to 1, as `doing`: the first
    /// report always, then one at most every quarter second, never less
    /// than the last. Safe to call from any worker: one that finds another
    /// reporting goes on.
    pub fn progress(&self, share: f64, doing: &str) {
        let Some(t) = &self.0 else {
            return;
        };
        let Ok(mut last) = t.last.try_lock() else {
            return;
        };
        let share = share.clamp(0.0, 1.0);
        let now = Instant::now();
        let due = match *last {
            None => true,
            Some((at, was)) => share > was && now.duration_since(at) >= EVERY,
        };
        if due {
            *last = Some((now, share));
            (t.report)(share, doing);
        }
    }
}

/// A request whose caller gave up.
pub fn cancelled() -> Vec<Issue> {
    vec![Issue::new("cancelled", "the caller cancelled the call", "")]
}

/// Work cut short because its caller gave up, as the refusal a read or a
/// trace fails with: whoever turns it into the request's issues fails the
/// request as [`cancelled`] instead.
pub fn cut_short() -> Refusal {
    Refusal("the caller cancelled the call".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counting() -> (Arc<Ticket>, Arc<Mutex<Vec<f64>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let t = Ticket::new(Box::new(move |share, _| s.lock().unwrap().push(share)));
        (t, seen)
    }

    #[test]
    fn a_thread_with_no_ticket_is_never_stopped_and_reports_to_nobody() {
        let c = Call::current();
        assert!(!c.stopped());
        assert!(c.check().is_ok());
        c.progress(0.5, "nothing");
    }

    #[test]
    fn a_ticket_holds_only_within_and_cancelling_stops_its_call() {
        let (t, _) = counting();
        within(t.clone(), || {
            let c = Call::current();
            assert!(!c.stopped());
            t.cancel();
            assert!(c.stopped());
            assert_eq!(c.check().unwrap_err()[0].code, "cancelled");
            // A worker given the call sees it too.
            let seen = std::thread::scope(|s| s.spawn(|| c.stopped()).join().unwrap());
            assert!(seen);
        });
        assert!(!Call::current().stopped());
    }

    #[test]
    fn the_ticket_before_comes_back_after_a_panic() {
        let (outer, _) = counting();
        let (inner, _) = counting();
        inner.cancel();
        within(outer, || {
            let boom = std::panic::AssertUnwindSafe(|| within(inner, || panic!("boom")));
            let r = std::panic::catch_unwind(boom);
            assert!(r.is_err());
            assert!(!Call::current().stopped());
        });
    }

    #[test]
    fn reports_the_first_then_rising_shares_spaced_apart() {
        let (t, seen) = counting();
        within(t, || {
            let c = Call::current();
            c.progress(0.1, "a");
            c.progress(0.2, "b");
            std::thread::sleep(EVERY);
            c.progress(0.05, "back");
            c.progress(0.3, "c");
        });
        assert_eq!(*seen.lock().unwrap(), vec![0.1, 0.3]);
    }
}
