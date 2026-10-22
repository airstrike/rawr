# Recorded configuration traversal source

source.rs retains unchanged _load_file/load_includes from Cargo commit
59800466c5c41c444d264b1010b4d57e85a7117f, context/mod.rs:1384–1454.
Cargo contributors, MIT OR Apache-2.0; license texts remain in the contained
loader path directory. These are review excerpts, not a compiled differential
harness or evidence of an end-to-end Cargo comparison.

The contained tests use actual recorded file outcomes and assert full final
values/origins, source-order failures, repeated paths and native component
aliases, disabled includes, inline CLI, resource floors and deep teardown.
Individual conversion, merge, include extraction and resolution operations have
separate pinned-source comparison evidence. Discovery/CLI set lifetimes and
lazy ordinary/credential loading remain caller integration work.
