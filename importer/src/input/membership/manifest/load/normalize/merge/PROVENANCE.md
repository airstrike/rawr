# Explicit and inferred target selection

Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs:590–606,784–898` supplies the fast-path predicate,
explicit/inferred suppression, edition-2015 warning and final target ordering.
License texts are in `../../path/LICENSE-MIT` and `../../path/LICENSE-APACHE`.

The fast path requires disabled discovery and both fields on every declared
target, and preserves declared order. Slow selection retains missing fields,
suppresses inference by either explicit name or joined Path equality, and
short-circuits the path join when the name already matches. Admitted sorted key
buffers implement membership without hidden hash-table allocation. Warning
paths retain inference encounter order before final unstable name sorting.

The target formatter measures pure display inputs before exact reservation and
bounds every second-pass write. Source paths use Display rather than assuming
native paths are UTF-8. Drafts grant no target-validation or manifest authority.

Root's combined normalization suite passed 63 tests. Independent focused review
of merge, formatting and suggestion helpers found no actionable blocker
(confidence 0.93). These remain test-gated prerequisites for the composed target
stage, not installed product behavior or performance evidence.
