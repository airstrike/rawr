# Pinned lint families

The implementation follows Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`. Its verified source archive has
SHA-256 `dc2728d4e9650b9871142cc3f1f0e449647e0b493775931f0b70ee1fa658a321`.
The archive URL and complete schema member hashes are also retained in
`importer/reference/manifest/provenance.json`.

| Behavior | Exact pinned source |
|---|---|
| Inherited workspace key and ordinary nested string maps | Cargo `crates/cargo-util-schemas/src/manifest/mod.rs:1536–1607` |
| Level versus Config selection | Same `mod.rs:1611–1631`; serde-untagged 0.1.9 `src/lib.rs:914–924`, `src/map.rs:41–70` |
| Required level, default i8 priority, flattened generic TOML table | Cargo schema `mod.rs:1653–1676` |
| Known-field duplicate checks, encounter-order Content capture, declaration-order missing fields, deferred flatten conversion | serde_derive 1.0.228 `src/de/struct_.rs:224–412` |
| Flat-map iteration and borrowed Content replay | serde 1.0.228 `src/private/de.rs:3184–3437` |
| i8 accepts only its signed/unsigned callbacks through 64 bits | serde_core 1.0.228 `src/de/impls.rs:376–381` |
| Raw enum shape and cardinality before scalar dispatch | toml 1.1.2+spec-1.1.0 `src/de/deserializer/value.rs:223–258`, `table.rs:70–89` |
| Raw unit payloads accept empty array/table | Same `table_enum.rs:21–43` |
| Generic TOML enum and unit payload behavior | Same `src/value.rs:564–599,740–770` |
| Borrowed Content enum cardinality and unit callback | serde 1.0.228 `src/private/de.rs:2266–2274,2365–2404,2849–2858` |
| serde-value enum cardinality and unit callback | serde-value 0.7.0 `src/de.rs:369–402,492–497` |
| Erasure of inner Config errors | serde-untagged 0.1.9 `src/error.rs:11–18`, `src/lib.rs:914–924` |

The Cargo schema member's SHA-256 is
`fc503d6532663f2b0f3217b53f235b6c24690e9c85116f1364ec134ca78cd92c`.
Sources can be retrieved from the pinned Cargo archive and the exact crates.io
package versions above. No upstream source or dependency manifest is modified
by this family implementation.

`Inherited`, `Table`, `Tool` and `Config` have private construction through the
actual visitor transformations (`smart-constructor-newtype` and
`transformation-method`), with read-only accessors. `Lint::Level` and
`Lint::Config` preserve the selected union. Workspace presence and known field
keys remain auxiliary declaring-origin data. Raw TOML and its reached captures
cannot produce a unit/None payload; explicit `workspace=false` rejects, while
absence retains the upstream false default. Ordinary workspace lint tables do
not treat a tool named `workspace` specially.

Every unknown Config value is completely captured in Content mode before
missing-level/default-priority completion. Only then is each captured value
converted through the shared generic TOML walker. The Config table itself is
an ordinary table; its own first datetime-marker key is not a generic Value
marker. Its *values* retain the Content replay exhaustion rule. Temporary
capture borrows original Document text or stores generated datetime text inline.
The two-lifetime Input permits converted output to outlive local staging without
self-reference, implicit cloning or borrowed references into freed captures.

Both lint-name maps sort after source-order conversion using allocation-free
heapsort over charged entries. Equal string keys retain the first key/origin and
last converted value. Config extras additionally restore first-insertion order,
matching TOML's preserve_order map. The only temporary ordering value is a
charged entry's ordinal; no uncharged BTreeMap, IndexMap, sort scratch or rendered
key copy is introduced. Key comparison is finite, bounded by already admitted
string bytes and O(n log n) comparisons; grouping and compaction are linear.

All retained entry/capture/output buffers use the original Session and Buffer.
Full replacement layouts are prospectively admitted; allocator bonus capacity
never grants authority. Fixed converted records and enum/number temporaries
also request item admission. Replaced or failed temporary data does not refund
cumulative work. Admission failure remains latched and is never caught as a
semantic fallback. Schema ownership has fixed nesting depth, and every generic
subtree is a flat Projection, so success and partial-failure teardown do not
recurse with attacker-controlled TOML depth.

Config's serde-untagged bridge erases nested errors before the outer input fills
its span. The implementation keeps the selected bounded tag and resets Config
failure spans to that same outer lint input; the surrounding map applies missing
context only. Enum dispatch uses the shared intrinsic Shape query, avoiding
conversion of rejected huge scalars, datetime values and unselected unit
payloads. Empty array/table unit payloads are accepted for raw and generic TOML;
Borrowed ContentRef and serde-value reject them. FlatMapAccess uses
ContentRefDeserializer (`private/de.rs:3348–3388`); the owned
ContentDeserializer has a distinct empty-map unit allowance (`:1301–1319`)
and must not be substituted for this reached borrowed adapter.

The 12 lint tests exercise actual visitors, not a simulated Cargo parser. Six
accepted scopes and six isolated rejected cases compare directly against the
committed 130-case reference artifact, SHA-256
`4bdb6ccb690b4d8eb75a45895d04a659994198666e7c09cfdf24e123cc171b7b`.
They compare complete ordered typed values/variants and applicable first-error
spans. Other focused tests exercise adapter differences, both missing/flatten
phases, borrowed capture lifetime, replacement and every recorded refusal with
prior account history and allocator-attempt counts. The implementation author
performed source-only preparation; root owns Cargo, formatting and execution.
These families are typed data and confer no Original, normalized manifest,
workspace, selected-owner, source freshness or current-proof authority.
