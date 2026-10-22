# Complete finite manifest collections

These private transformations implement all enclosing collections for Cargo's
`features`, `target`, `patch`, `replace`, and `bin`/`example`/`test`/`bench`
fields. They consume an admitted Document adapter and return complete converted
family data. They do not construct a top-level Manifest or Original, supply a
profile-key placeholder, normalize dependencies, or establish source/workspace
authority. The boundary implements the settled containment decision.

## Primary source correspondence

The Cargo source is commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
archive SHA-256 `dc2728d4e9650b9871142cc3f1f0e449647e0b493775931f0b70ee1fa658a321`.
Its local source root is
`/private/tmp/rawr-p9-pinned-cargo-full/cargo-59800466c5c41c444d264b1010b4d57e85a7117f`.
The complete `crates/cargo-util-schemas/src/manifest/mod.rs` SHA-256 is
`fc503d6532663f2b0f3217b53f235b6c24690e9c85116f1364ec134ca78cd92c`.
Registry sources are under
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.
The exact archive identities and member hashes are retained in
`importer/reference/manifest/provenance.json`.

| Reached behavior | Exact source |
|---|---|
| All five collection types and their use by all eight Manifest fields | Cargo schema `mod.rs:44–63` |
| FeatureName deserializes String, then validates without changing retained spelling | Same `:1440–1450,1488–1495`; `restricted_names.rs`, `validate_feature_name` |
| Ordinary BTreeMap converts each key and value in encounter order, then inserts | serde_core 1.0.228 `src/de/impls.rs:1512–1563` |
| Target vector elements use the complete 15-field Target | Cargo schema `mod.rs:1334–1366`; existing `schema::Target` correspondence |
| Platform values retain all five dependency fields and aliases | Same `:1508–1533`; existing `schema::platform` correspondence |
| Patch values use PackageName/direct-dependency maps; replacement values use Direct | Same `:62–63,745–817`; existing `schema::dependency` correspondence |
| Ignored callback fires before delegating IgnoredAny, without wrapping its nested visitor | serde_ignored 0.1.14 `src/lib.rs:447–452` |
| Callback original map key and sequence index tracking | Same `:1343–1447` |
| Some/newtype wrappers do not render as path components | Cargo `src/cargo/util/toml/mod.rs:207–232` |
| Raw scalar/datatype dispatch, synthetic date map, key and value spans | toml 1.1.2+spec-1.1.0 `src/de/deserializer/value.rs:87–130`, `table.rs:126–171`; toml_datetime 1.1.1+spec-1.1.0 `src/de.rs:29–58` |
| Target's raw sequence may leave trailing values unconverted | toml `src/de/deserializer/array.rs:21–26`; existing Cursor preserves the distinct replay exhaustion rules |

Cargo/Serde are MIT OR Apache-2.0. The complete Cargo licenses and source-license
hashes are retained in `importer/reference/manifest/LICENSE-MIT`,
`LICENSE-APACHE`, and `provenance.json`. This subtree specializes the declared
visitor control flow and uses the existing contained scalar/container code;
it introduces no registry dependency or copied allocating upstream container.

## Exact representation and order

`Features`, `Platforms`, `Patch`, `Replace`, and `Targets` are private
`transformation-method` types. Their fields cannot be assembled by a consumer.
The map Entry retains the first converted key and its original Key, plus the
last complete value and its declaring value span. Unchanged text borrows the
Document, including decoded Document-owned strings; copied inline datetime text
does not borrow temporary capture storage. Read access exposes converted values
and origins, with no unchecked raw interpretation route.

Every ordinary map key compares by its exact String spelling. FeatureName's
validation preserves that spelling; its parsed key is retained separately.
Keys validate before their values, but no later key is inspected ahead of an
earlier value. All values, including eventually replaced values, complete their
actual visitor before the map is ordered. Stable ordering by key plus encounter
ordinal and linear compaction retain BTreeMap's first key and last value. The
implementation does not reuse this comparator for PackageIdSpec keys.

Platforms, Patch sources and Replace keys are plain strings. They accept empty
or otherwise uninterpreted strings here. Patch's nested package names retain
their distinct PackageName grammar. Direct dependency version/URL/path spellings
and extras remain delegated to the complete existing direct visitor. In
particular, direct `workspace=false` is retained extra data; it does not start
the inherited dependency trial. Badges are already a separate complete family.

Maps require the source's map visitor, and target vectors require its sequence
visitor. Both use Input::view rather than a physical-TOML-table shortcut, so
synthetic date maps and source-specific replay dispatch remain observable. A
native datetime can therefore yield Replace's synthetic string-key/simple-value
entry; generic TOML replay of that datetime is a string and rejects as a map.
Outer context fills only a missing semantic-error span and never rewrites a
terminal refusal.

Target vectors retain every element in original order. Each target includes all
15 fields and independent aliases through the existing Target visitor. The outer
vector visits all its elements; the inner Target's positional visitor retains
its own permitted early return. No eager scalar conversion of its unconsumed
trailing elements is added.

## Independent callback ownership

Platforms copies each completed Platform's original ignored-field records into
an independent admitted callback buffer before map compaction. Each record has
the original outer key and ignored inner field. Therefore callbacks from
discarded duplicate values survive, and their order remains encounter order
even when retained map entries are sorted. Targets similarly retains original
element indices and ignored fields separately from its converted payloads.

These are relative callback components, not final rendered unused strings.
The later complete Manifest collector must prefix its actual field position and
preserve Cargo's rendered-string ordering/dedup behavior. This subtree does not
derive callbacks from winning values or reinterpret nested dependency extras as
callbacks. Existing family visitors retain their local callback record before
the actual ignored traversal; copied outer records are distinct actual storage
and receive their own admission.

## Original ownership and work bounds

All retained entries, string-vector elements, callback records and nested
converted values use the caller's existing Session. Every actual Buffer growth
precharges its full checked replacement Layout, then performs the fallible
allocation; allocator slack supplies no logical credit. No new budget, guessed
input-sized allocation, hidden collection, hash seed, clone, path/hash/decoder
event, or candidate-comparison event is introduced.

For m entries, conversion follows m successive cursor entries plus each reached
family visitor's already contained work. Map finalization performs O(m log m)
comparisons over admitted key bytes and O(m) compaction, with allocation-free
heapsort. Callback copying visits each emitted callback once. Target vector
traversal is monotone over its elements. Fixed typed nesting and existing flat
generic projections make both completed and partial-result destruction
independent of manifest nesting depth; no recursive container is added here.

Refusal is terminal across every nested visitor, retained callback copy and map
finalization. Entry failure returns no partial family. Failed conversions and
discarded values do not refund original usage.

## Focused tests and execution boundary

Fourteen tests exercise the actual contained functions: ten representation,
ordering/origin, callback, and payload-lifetime cases; three resource/work cases;
and one direct committed-reference comparison. The latter covers all nine
available complete containers: eight in `complete-families`, plus Features in
`names-unicode-namespace`. It compares complete nested payloads, aliases,
presence, typed key order, all target fields, and ordered callback lists. The
reference artifact is `importer/reference/manifest/observations.jsonl`, SHA-256
`4bdb6ccb690b4d8eb75a45895d04a659994198666e7c09cfdf24e123cc171b7b`.

Additional tests cover invalid keys before values, earlier-value failure before
later keys, unrestricted outer strings, direct versus inherited dependency
behavior, synthetic datetime replay, duplicate conversion and first-key/last-value
origins, callback survival after discarded Platform values, complete target
vectors, unconsumed inner positional extras, and Document-owned text surviving
temporary capture teardown. Synthetic duplicate replay is explicitly constructed
as test data; it is not claimed to be accepted duplicate TOML syntax.

Every observed request is refused in turn across all five families and selected
semantic failures; the tests check the retained refusal, exact request prefix,
actual allocation attempts, and absence of further work after the latch. Exact
and one-below item/byte/string tests retain prior original usage and exhaust all
unrelated GraphBudget accounts. A 1,025-entry reverse map exercises actual growth,
complete conversion, sorting and flat teardown.

The author prepared source only. Root owns registration, formatting, Cargo/native
execution and recording results. Prepared reference comparisons are not a claim
of executed parity, whole Original acceptance, or later loader authority.
