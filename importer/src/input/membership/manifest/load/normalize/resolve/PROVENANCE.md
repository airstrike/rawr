# Pinned target path selection

Derived from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs`: `legacy_bin_path` (388–409), benchmark legacy
closure and delayed warning append (528–559), missing-path diagnostics and
`target_path` (979–1091). Cargo's MIT/Apache-2.0 license texts are retained in
the sibling `path` directory.

An explicit path is copied without exists checks or lexical normalization. An
implicit path copies the name and first two matching inferred paths in encounter
order. Exactly one match succeeds. Zero or multiple matches attempt the supplied
legacy branch only for edition 2015. Binary legacy probes stop at the first
present path and skip `src/{name}.rs` when a library exists. Benchmark legacy
constructs its path but reaches exists only for the name `bench`.

Multiple matches report the first two paths, with source strip-prefix display,
without running missing-path suggestions or probes. Missing matches construct
wrong/default file and subdirectory paths, calculate the closest-name suggestion,
then probe wrong paths in file-before-subdirectory order. Native exists errors
are false. An interior NUL selects that same native-false result before the Host
query, rather than being treated as missing recorded authority.

Exact diagnostic strings are retained through the admitted target text helper.
Native path display uses UTF-8 chunks and one replacement character per invalid
chunk, matching Unix Path display. The reusable suggestion module supplies the
borrowed closest name; its source algorithm and accounting remain in that module.

Successful resolution returns the path and optional legacy warning separately.
The caller must append binary legacy warnings immediately, but retain benchmark
legacy warnings separately until the entire benchmark normalization succeeds.
Semantic path errors remain `Err(String)` so bins can abort while other targets
accumulate errors and continue. Account or observation failures remain terminal
Session failures and must never be accumulated as semantic diagnostics.

This resolver does not normalize paths, merge targets, or mint loaded-manifest
or current-membership authority. Its caller must supply the already validated
target name, correct target-kind legacy branch, and original Session.

The focused tests cover explicit/single-match fast return, first-two ambiguity,
binary legacy order and library gating, benchmark name/edition gating, reached
wrong-path probes, exact suggestion and diagnostic text, and native NUL behavior.
