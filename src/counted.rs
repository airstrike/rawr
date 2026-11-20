//! `counted!(Site)`: one count of a lock the locks bench reads, at the
//! site that takes it. Without the `lock-counters` feature it expands to
//! nothing, so no build that ships pays for it.

/// Count one taking of the lock at `$site`, a `counters::Site`.
macro_rules! counted {
    ($site:ident) => {
        #[cfg(feature = "lock-counters")]
        $crate::counters::bump($crate::counters::Site::$site);
    };
}
