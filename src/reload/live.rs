//! What a reload reaches of the code and values that outlive the calls
//! that made them: the async bodies suspended (#2117), the callables
//! native code holds (#1746) and those a marked `main` gave (D553.5), and
//! the cells native code holds script values in (#2026). Any program
//! thread notes one as it makes it, and the reload reads them all, so the
//! lists share one lock.

use std::sync::{Mutex, MutexGuard, PoisonError, Weak};

/// Each list of what lives.
pub(crate) struct Live {
    /// Each async body that has suspended, while its future lives: a
    /// reload that changed the function one waits in can't resume it.
    pub(crate) suspended: Weaks<Mutex<crate::future::Step>>,
    /// Each callable script code gave native code, while native code holds
    /// it: code of the generation that made it, which runs when the
    /// callable doesn't rebind in a newer one.
    pub(crate) kept: Weaks<crate::future::Held>,
    /// Each callable a marked `main`'s own frame gave native code, while
    /// native code holds it, which is in `kept` too.
    pub(crate) held: Weaks<crate::future::Held>,
    /// The shared cells native code holds script values in, which a save
    /// that changes a script type's fields rewrites in place.
    pub(crate) cells: Weaks<crate::Locked<crate::Dynamic>>,
}

static LIVE: Mutex<Live> = Mutex::new(Live {
    suspended: Weaks::new(),
    kept: Weaks::new(),
    held: Weaks::new(),
    cells: Weaks::new(),
});

/// The lists, for one change or read. A thread that panicked while it
/// held them left each list whole, as each change is one push or sweep.
pub(crate) fn live() -> MutexGuard<'static, Live> {
    LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Weak references to what lives, in the order noted. The ones that went
/// are swept once as many were noted as lived at the last sweep, so a list
/// stays within twice the live ones.
pub(crate) struct Weaks<T> {
    all: Vec<Weak<T>>,
    swept: usize,
}

impl<T> Weaks<T> {
    const fn new() -> Self {
        Self {
            all: Vec::new(),
            swept: 0,
        }
    }

    /// Note `weak`.
    pub(crate) fn push(&mut self, weak: Weak<T>) {
        if self.all.len() >= self.swept.max(16) * 2 {
            self.sweep();
        }
        self.all.push(weak);
    }

    /// Each one noted that still lives, in the order noted.
    pub(crate) fn alive(&mut self) -> Vec<std::sync::Arc<T>> {
        self.sweep();
        self.all.iter().filter_map(Weak::upgrade).collect()
    }

    fn sweep(&mut self) {
        self.all.retain(|weak| weak.strong_count() > 0);
        self.swept = self.all.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list keeps only what lives once it sweeps, and keeps what lives in
    /// the order it was noted.
    #[test]
    fn weaks_sweep_what_went() {
        let mut weaks = Weaks::new();
        let kept: Vec<_> = (0..40).map(std::sync::Arc::new).collect();
        for (index, value) in kept.iter().enumerate() {
            weaks.push(std::sync::Arc::downgrade(value));
            let gone = std::sync::Arc::new(index);
            weaks.push(std::sync::Arc::downgrade(&gone));
        }
        assert!(
            weaks.all.len() <= 2 * 40 + 1,
            "a full list sweeps before it grows"
        );
        let alive: Vec<_> = weaks.alive().iter().map(|value| **value).collect();
        assert_eq!(
            alive,
            (0..40).collect::<Vec<_>>(),
            "only the live ones stay, in order"
        );
    }
}
