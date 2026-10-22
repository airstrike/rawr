# Build target path conflicts

Derived from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs` 2495–2518 and 1535–1550, and target-kind
descriptions in `src/cargo/core/manifest.rs` 259–267. License texts are retained
in `load/path/LICENSE-*`.

Ordinary targets precede build targets. Each path-backed target contributes its
exact `package_root.join(path)`; metabuild has no path and contributes nothing.
Native Path Hash/Eq controls grouping, including separator/current-directory
equivalence without parent-component normalization or filesystem observations.
The first equivalent key retains its spelling and each group retains target
encounter order.

The first admitted Fx PathTable uses entry-style merge. Its consuming iteration
is filtered into a second fresh PathTable with ordinary insert. The pinned
filtered iterator has a zero lower size hint: std HashMap::from_iter constructs
an empty map, and hashbrown 0.17.1 map.rs 4867–4882 reserves zero before inserting
each survivor. The second map's iteration, not sorted paths or first-map order,
determines warning order.

Joined paths live in existing Slots, preserving stable borrowed keys while
allowing source-ordered incremental joins. Flat group buffers retain borrowed
names and fixed descriptions instead of cloning inert target metadata. Both
tables share the closed adapter with separately evidenced Path keys; there is no duplicate hashing or layout
implementation. Path hashing requires the adapter's explicit pinned usize hash
operation in addition to its previous string operations.

Complete warning rendering uses the original Session and prospective formatter,
including native path display. The caller must perform no-target rejection
before this helper and links/dependency checks afterward. This is a warning
stage and grants no converted-manifest or current-membership authority.

Tests cover first native spelling and member order, parent-component distinction,
two-map warning order using the independently qualified table, native non-UTF-8
display, empty input, and refusal at every reached admission. The outer connected
fixture exercises actual converted ordinary/build Targets mapping and
no-target → duplicate warnings → links precedence. Metabuild has no path by
construction and is explicitly skipped by this helper.

Focused validation on nightly-2026-07-11 with rustc-driver,test-fixtures:
32 connected package-normalization tests, 5 conflict tests and 31 config-merge
regression tests passed. Independent read-only review found no actionable code
blocker and required the separate Path provenance recorded above. These checks
cover test-gated implementation only, not production activation or Gallery speed.
