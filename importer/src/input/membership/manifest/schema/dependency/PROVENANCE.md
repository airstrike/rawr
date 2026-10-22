# Complete dependency interpretation

The source pin and archive/schema hashes are recorded in
`manifest/PROVENANCE.md`. This slice implements the original String-path
dependency schema from Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`, not normalization or resolution.

| Contained visitor | Pinned source |
|---|---|
| `Direct` string/map union | `cargo-util-schemas/src/manifest/mod.rs:793–817` |
| `Detailed` and all 18 optional fields | `mod.rs:825–869` |
| `Inheritable` capture, trial, flag, fallback | `mod.rs:695–714` |
| `Inherited` and all six fields | `mod.rs:718–731` |
| Known fields, missing fields, then flattened extras; no positional form | `serde_derive 1.0.228 src/de/struct_.rs:225–371` |
| Initial complete encounter-order capture and sorted replay | `serde-value 0.7.0 src/de.rs:190–289,326–359` |
| Flatten capture and borrowed replay | `serde 1.0.228 src/private/de.rs`, Content and FlatMapDeserializer implementations |
| Generic extra values | `toml 1.1.2+spec-1.1.0 src/value.rs:420–518` |

Initial serde-value capture must finish before the inherited trial starts.
The trial uses sorted captured maps. Its `workspace` Boolean is ordinary data:
`false` is rejected only after all known fields, missing-field checks and
flattened TOML extras succeed. A semantic trial rejection permits the direct
dependency fallback. A resource refusal remains latched and prevents fallback.
Trial storage and diagnostic charges remain spent even when its values drop.

Unknown fields are captured into Content during the struct pass, then converted
to generic TOML only after missing-field checks. They remain complete sorted
extras for later unused-key bookkeeping; they are not ignored callbacks. Content
replay enforces the generic datetime marker's remaining-map rule. The direct
serde-untagged map boundary erases inner semantic error spans, retaining the
outer dependency span. Resource failures retain their identity. The opaque
`Direct` and `Inheritable` wrappers retain their original input span separately
from nested captured fields, whose replay has no original span.

Registry, path-base and package names use their distinct pinned grammars.
The two default-feature spellings remain independent fields. Known duplicate
fields reject before conversion of the duplicate value. Dependency map keys
are checked before their values. Map conversion retains the first equal key
and its origin, and the last fully converted value. No dependency version,
URL, path or artifact spelling is given later resolution validation here.

## Ownership and bounded work

All buffers use the caller's existing Session and its full prospective
replacement-layout admission. Fields, captured extras, converted extras and
map entries retain explicit admitted slots. Text borrows admitted Document
payloads or copies fixed inline text, so output does not borrow a temporary
capture. There is no recursive owned dependency tree: nested extras use the
existing flat Projection and explicit traversal frames.

The known-field lookup and duplicate check examine at most 18 entries. Each
unknown value has one Content pass and one generic TOML pass per reached
struct trial. The inherited wrapper adds one initial serde-value pass and at
most two struct trials. Each map uses allocation-free heapsort over admitted
entries, ordered by admitted native text and encounter ordinal, then one
linear compaction pass. Equal keys retain old key ownership while swapping in
the latest converted value; truncation neither shrinks capacity nor refunds
charges. These operations spend no filesystem, hash or candidate-comparison
account and do not introduce independent limits.

## Focused evidence prepared for root execution

Sixteen tests exercise variants and all inherited fields, false-flag ordering,
semantic fallback, full initial capture, sorted trial ordering, missing versus
flattened conversion, datetime exhaustion, distinct names, sequence rejection,
duplicate-before-value and map replacement. Resource tests refuse every observed
admission with prior usage, check actual allocation events, enforce exact and
one-below aggregate/string limits, traverse and drop over 6,000 nested nodes on
success and failure, and run with the unrelated GraphBudget accounts exhausted.

The complete committed Cargo corpus comparator covers exactly 19 inheritable
maps, two direct maps and one replacement value across all accepted rows.
It compares actual fields, variants, aliases, absence, names and flattened
generic values. Six isolated rejected rows compare bounded tags and first-error
spans. The reference artifact and its execution provenance remain those recorded
by the parent manifest boundary; expected-output tests alone do not establish
that this new visitor has passed execution. Root owns execution and review.
