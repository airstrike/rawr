# Pinned Platform visitor

This visitor follows Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`. Source URL, verified archive and
schema hashes are retained in the adjacent `workspace/PROVENANCE.md` and the
isolated reference harness's `provenance.json`.

| Behavior | Exact source |
|---|---|
| Five distinct optional map fields and their declaration order | Cargo `crates/cargo-util-schemas/src/manifest/mod.rs:1506–1519` |
| Separate kebab-case and underscore fields; later convenience accessors do not merge stored fields | Same `mod.rs:1521–1533` |
| Inheritable dependency's initial serde-value capture, trial and fallback | Same `mod.rs:695–731`; delegated to `dependency::inheritable::Map` |
| Required positional slots, absent map Option defaults | serde_derive 1.0.228 `src/de/struct_.rs:70–95,224–412`; `src/de.rs:451–551,805–835` |
| Original ignored callback component spelling | Cargo `src/cargo/util/toml/mod.rs:207–232`; serde_ignored 0.1.14 `src/lib.rs` |

All five fields keep their own converted dependency maps and presence. Neither
underscore alias is folded into the kebab-case field, and no preference accessor
or workspace/source interpretation runs here. `Platform::interpret` is the
`transformation-method` that creates the private entry storage. Each entry keeps
its exact field identity, source position and span. Maps expose the actual
Simple/Detailed or local/inherited dependency variants and all flattened extras.

Map values convert in source encounter order. Unknown values are traversed by
the shared ignored visitor and record their exact key/span; dependency extras
remain typed data. Duplicates reject before converting the second field payload.
All five sequence slots are required even though absent map fields are optional.
The shared Cursor preserves raw/serde-value extra-element behavior versus
Content/generic TOML exhaustion. Raw synthetic datetimes are visited as maps;
generic TOML datetime replay uses its actual string adapter.

Field selection is bounded by five static names and five retained slots.
Every retained slot, callback and nested payload uses the same original Session
and prospectively charged Buffer. Child capture trials cannot reset the account,
discard admission refusal or allocate after refusal. No attacker-sized container
uses uncharged storage. Spans from the selected nested failure are retained;
outer field context fills only absent spans. Fixed typed nesting and flat generic
projections also bound teardown stack usage on success and partial failure.

The 10 tests exercise all five aliases and their distinct payloads, absent maps,
required positional fields, raw unvisited extras, replay exhaustion, synthetic
datetime dispatch, nested error spans, duplicate precedence and ordered ignored
callbacks. Resource tests cover every recorded refusal and actual storage
attempt, with nonzero original history, plus exact/one-over aggregate item, byte
and string floors. One test compares every field and complete nested payload in
the executed reference's `complete-families` / `target.cfg(unix)` scope. The
reference artifact SHA-256 is
`4bdb6ccb690b4d8eb75a45895d04a659994198666e7c09cfdf24e123cc171b7b`.
No rejected whole-manifest row is attributed to Platform without isolated scope
evidence. The author prepared source only; root owns Cargo, formatting and
execution. This family does not establish whole-Original parity or source,
normalization, workspace or current selection authority.
