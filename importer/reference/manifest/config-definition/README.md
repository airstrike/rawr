# Pinned configuration Definition reference

The complete Definition enum and its implementations through Display are copied
unchanged from Cargo commit 59800466c5c41c444d264b1010b4d57e85a7117f,
`src/cargo/util/context/value.rs`; std imports are prepended and exterior blank
lines trimmed. Copyright the
Cargo contributors, MIT OR Apache-2.0 (license texts in the contained loader's
`path/` directory). `reference.rs` is test-only catch_unwind instrumentation.

Root compiled this source using rustc +nightly-2026-07-11 --edition 2024 and ran
seven cases. Both root-level file variants panic at the second parent; five
normal/non-file cases succeed. The stderr location names the original temporary
extraction. No full Cargo command or real root-level config file was used.
Source-level construction routes are `_load_file`, `cli_args_as_table` and
`ConfigInclude::resolve_path`; complete selected-value/caller evidence remains
required before production connection. No production catch_unwind is authorized.

Reproduce: compile `reference.rs` beside `definition.rs` with the pinned rustc,
then run the resulting binary. No Cargo dependencies are required.
