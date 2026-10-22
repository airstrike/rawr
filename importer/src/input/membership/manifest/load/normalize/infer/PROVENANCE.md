# Pinned target inference

Derived from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs`, lines 718–790 (`inferred_bins`,
`infer_from_directory`, `infer_any`, `infer_file`, `infer_subdirectory`, and
`is_not_dotfile`). Cargo's MIT/Apache-2.0 notices and license texts are retained
in the sibling `path` directory.

The binary entry point probes `src/main.rs` first using exists semantics, then
reads `src/bin`. A present directory also satisfies exists. Directory inference
keeps encounter order and duplicates. Non-UTF-8 names and dotfiles are skipped
before type inspection. A native type error selects the non-directory branch;
it does not skip an otherwise matching `.rs` filename. Directory entries probe
their own `main.rs`, with name ownership occurring before that exists query.
Extension and stem spelling, as well as component-based `strip_prefix` with
absolute-path fallback, use the same standard Path operations as the source.

The recorded Stream cursor supplies only actually reached type outcomes and
permits interleaved exists probes on the original Session. Open errors yield an
empty result; a terminal native iteration error is ignored as Cargo's
`filter_map(Result::ok)` does, but the cursor must still be explicitly consumed.
Unobserved evidence and account refusal remain terminal Session failures.

All path buffers, names, result storage and bounded traversal work pass through
the original Session. The returned items are concrete inferred names and paths,
not loaded-manifest or current-membership authority. This boundary does not yet
perform target merging, naming diagnostics, sorting, or normalization.

Focused deterministic tests exercise main-first inference, encounter order and
duplicates, dot/non-UTF-8 skip, native type-error behavior, subdirectory probes,
extension/stem spelling, absolute-path fallback, ignored native terminal/open
errors, and refusal on missing required observations.
