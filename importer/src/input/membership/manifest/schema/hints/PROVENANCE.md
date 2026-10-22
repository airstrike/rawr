# Pinned Hints visitor

Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`crates/cargo-util-schemas/src/manifest/mod.rs:1678–1690`, defines the complete
Hints struct: one optional generic TOML value, `mostly-unused`. The schema
member SHA-256 is
`fc503d6532663f2b0f3217b53f235b6c24690e9c85116f1364ec134ca78cd92c`.
The verified archive URL/checksum and full source inventory are retained in
`importer/reference/manifest/provenance.json`.

Serde derive 1.0.228 `src/de/struct_.rs:70–95,224–412` and
`src/de.rs:451–551,805–835` control map versus positional behavior. The ordinary
map may omit the field, but its positional visitor requires one element even
though the member is an Option. A duplicate known map field rejects before
visiting its value. Unknown fields invoke IgnoredAny and retain their callback
keys in encounter order.

The shared Input/Cursor/walk boundary follows the exact raw TOML, Content,
serde-value and generic TOML source adapters documented in
`manifest/PROVENANCE.md`. Raw and serde-value positional arrays may return with
unconsumed elements; Content and generic TOML require exhaustion. Unknown raw
values still perform their actual any-dispatch and nested conversion. A raw
native datetime can present a synthetic unknown-key map to this struct, while
Content ignored replay may return immediately. No alternate parser or
unconditional scalar preflight changes these branches.

`Hints` is produced by the actual interpretation transformation with private
fields and read-only accessors (`transformation-method`). It retains the
converted optional Projection, declaring key and ordered unused callback keys.
Generic payloads remain flat and borrow the original Document; generated date
text is inline. All retained buffers, replacement layouts and record items use
the original Session. Admission failure stays latched and cannot authorize
later work. The schema adds only fixed ownership depth above a flat generic
Projection, including partial-failure teardown.

Eight tests cover map/positional presence, unknown callbacks and deferred raw
errors, source-adapter exhaustion, borrowed decoded values, duplicates, and
refusal at every recorded request after prior account history. Three accepted
Hints scopes and one isolated rejection compare against the committed pinned
reference artifact, SHA-256
`4bdb6ccb690b4d8eb75a45895d04a659994198666e7c09cfdf24e123cc171b7b`.
The complete generic projection includes table order, float bits and datetime
components. The worker performed source-only preparation; root owns execution.
This data does not establish a complete Original, normalization, membership,
source freshness or current authority.
