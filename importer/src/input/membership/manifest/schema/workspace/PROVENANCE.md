# Pinned Workspace visitor

This data interpretation follows Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`, from
<https://codeload.github.com/rust-lang/cargo/tar.gz/59800466c5c41c444d264b1010b4d57e85a7117f>.
The verified archive SHA-256 is
`dc2728d4e9650b9871142cc3f1f0e449647e0b493775931f0b70ee1fa658a321`.
Cargo `crates/cargo-util-schemas/src/manifest/mod.rs` has SHA-256
`fc503d6532663f2b0f3217b53f235b6c24690e9c85116f1364ec134ca78cd92c`.
The reference harness retains reproducible source materialization and exact
dependency provenance in `importer/reference/manifest/provenance.json`.

| Behavior | Exact source |
|---|---|
| All eight fields, declaration order, kebab-case names and optional map defaults | Cargo schema `manifest/mod.rs:124–144` |
| Complete non-inheriting workspace package defaults | Same `mod.rs:146–209`; delegated to `package::Defaults` |
| Direct dependency map, without inherited-dependency trials | Same `mod.rs:142,695–712,735–869`; delegated to `dependency::direct::Map` |
| Ordinary workspace lint table | Same `mod.rs:143,1607–1676`; delegated to `lints::Table` |
| Generated sequence visitor and all required positional slots | serde_derive 1.0.228 `src/de/struct_.rs:70–95`; `src/de.rs:451–551,805–835` |
| Source-order map dispatch, duplicate rejection before the second value, unknown callbacks | serde_derive 1.0.228 `src/de/struct_.rs:224–412` |
| Raw array visitor returns without requiring exhaustion | toml 1.1.2+spec-1.1.0 `src/de/deserializer/array.rs`; shared `value::Cursor::finish` retains the differing replay policies |
| Ignored path spelling, map components, sequence indices, transparent Some/newtype wrappers | Cargo `src/cargo/util/toml/mod.rs:207–232`; serde_ignored 0.1.14 `src/lib.rs` |

The Rust `Default` derive on `TomlWorkspace` is not a Serde container default.
Missing map fields are absent Options; a sequence must supply all eight fields.
An extra raw or serde-value sequence element stays unconsumed. Content and
generic TOML replay retain their explicit exhaustion checks. The visitor uses
the shared source adapter, including raw datetime's synthetic map and generic
TOML datetime's string dispatch. It does not prefilter these accepted shapes.

Each successful conversion returns opaque `Workspace` data through
`transformation-method`, with private entry storage and read-only accessors.
Entries retain source order, exact field identity, map key or sequence position,
and original spans. Every nested value is the actual converted payload, not an
input replay awaiting validation. Member/exclude/resolver strings remain exact
typed strings; there is no path normalization, resolver interpretation or member
selection here. Metadata uses the complete generic TOML walker and its ordered
tables, float bit patterns, datetime components and Document borrow.

Unknown outer fields and nested package unknown fields retain ordered callback
components. A nested callback is copied into the outer callback buffer only
after the package visitor returns; that retained copy receives its own original
Session/Buffer admission. The nested package also retains its own callbacks.
No path strings are rendered or allocated in the product visitor. Dependencies
and lint extras are generic typed payloads, not ignored callbacks.

Fixed field selection examines at most eight static names and eight converted
entries per input field. All attacker-sized containers, every callback copy,
and each retained field use the original charged Buffer. Children receive the
same original Session; there is no account reset, refund, fallback or authority
created by successful conversion. Admission refusal remains permanent. Errors
keep their selected tag and child span; outer context fills only missing spans.
Typed schema nesting is fixed, and generic subtrees remain flat projections,
so success and partial-failure drops do not recurse with arbitrary TOML depth.

The 12 tests exercise the actual Workspace and delegated visitors. They include
all fields, raw and replay sequences, duplicate-before-value precedence,
source-order callbacks, borrowed metadata and datetime adapter distinctions.
Every observed refusal checks the original request prefix and allocator-attempt
count. Cumulative tests use nonzero original usage and exact/one-over item,
byte and string limits. Two tests compare the complete accepted Workspace and
one isolated version rejection (including span) with the executed 130-case
reference artifact, SHA-256
`4bdb6ccb690b4d8eb75a45895d04a659994198666e7c09cfdf24e123cc171b7b`.
The exact accepted scope is `workspace-and-inheritance`; the exact rejected
scope is `workspace-keeps-version-space`. The author prepared source only;
root owns formatting, Cargo and test execution. This is not whole-Original
parity and conveys no workspace, source or selected-owner authority.
