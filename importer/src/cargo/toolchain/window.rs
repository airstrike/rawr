//! The nightlies rawr-import's driver builds on, by their `rustc -vV`
//! `commit-date`.
//!
//! The build script includes this file too: it sets one `rawr_rustc_*` cfg
//! for each compiler API change the building nightly has, and the driver
//! (`driver/compat.rs`) names the API through it. The library refuses a
//! toolchain outside the window before it builds a driver for it.

/// The oldest `commit-date` the driver builds on: the pinned nightly's.
pub(crate) const OLDEST: &str = "2026-07-10";

/// The newest `commit-date` the driver is known to build on.
pub(crate) const NEWEST: &str = "2026-11-01";

/// The nightlies those dates are, for messages.
pub(crate) const RANGE: &str = "nightly-2026-07-11 through nightly-2026-11-02";

/// Each compiler API change inside the window: its cfg and the `commit-date`
/// of the merge that made it. A nightly whose commit is from that day but
/// precedes the merge is the one ambiguity; none of the window's edges is.
pub(crate) const CHANGES: [(&str, &str); 12] = [
    // `DefKind::InlineConst` merged into `DefKind::AnonConst` (#159113).
    ("rawr_rustc_anon_inline_const", "2026-07-11"),
    // `predicates_of` and `explicit_super_predicates_of` became
    // `clauses_of` and `explicit_super_clauses_of` (#160102).
    ("rawr_rustc_clause_queries", "2026-07-28"),
    // `StabilityLevel` and `UnstableReason` moved to `rustc_attr_ir`
    // (#160761).
    ("rawr_rustc_attr_ir", "2026-08-08"),
    // `PredicatePolarity` became `ClausePolarity` (#161396).
    ("rawr_rustc_clause_polarity", "2026-08-20"),
    // `ImplPolarity::Reservation` was removed (#161772).
    ("rawr_rustc_no_reservation_impls", "2026-08-25"),
    // `DefKind::TestBinderConstraints` was added (#161772).
    ("rawr_rustc_test_binder_constraints", "2026-08-25"),
    // `AliasConstKind::Inherent` split into `InherentSelf` and
    // `InherentImpl` (#162117).
    ("rawr_rustc_inherent_const_kinds", "2026-09-01"),
    // `rustc_middle::metadata` became `rustc_middle::middle::resolve`, and
    // `const_of_item` returns an `Option` (#162310).
    ("rawr_rustc_resolve_module", "2026-09-05"),
    // `type const` items were removed, with `DefKind`'s `is_type_const`
    // (#162517).
    ("rawr_rustc_no_type_const", "2026-09-12"),
    // `rustc_driver::run_compiler` became `compiler_entrypoint` (#163661).
    ("rawr_rustc_compiler_entrypoint", "2026-10-02"),
    // A const generic parameter records its argument position (#164285).
    ("rawr_rustc_arg_pos", "2026-10-14"),
    // `GenericArgsRef` became the `Copy` list `GenericArgs` (#165234).
    ("rawr_rustc_generic_args_value", "2026-10-31"),
];

/// Whether a nightly with this `commit-date` is inside the window. Dates
/// are `YYYY-MM-DD`, so they order as text.
pub(crate) fn contains(commit_date: &str) -> bool {
    (OLDEST..=NEWEST).contains(&commit_date)
}
