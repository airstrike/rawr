# Complete pinned recursive profiles

This boundary interprets complete `TomlProfiles` and all nineteen fields of
`TomlProfile`. It uses the existing leaf visitors and complete `spec::Spec`.
It retains unresolved typed data only. Profile merging, Cargo normalization,
workspace loading, source selection and current membership authority do not
follow from interpretation. The containing membership module remains test-only.

## Exact source

Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`crates/cargo-util-schemas/src/manifest/mod.rs`, SHA-256
`fc503d6532663f2b0f3217b53f235b6c24690e9c85116f1364ec134ca78cd92c`:

- Lines 902–914 define the `TomlProfiles` newtype over
  `BTreeMap<ProfileName, TomlProfile>`.
- Lines 916–944 define `TomlProfile` with container `default` and
  `rename_all = "kebab-case"`. The declared nineteen-field sequence is retained
  exactly, including `package`, `build_override`, `trim_paths`, and
  `hint_mostly_unused`. Every field is optional; any omitted positional suffix
  also takes the container default. Underscore spellings are not aliases.
- Lines 1039–1081 define `ProfilePackageSpec`, with derived enum order
  `Spec(PackageIdSpec)` before `All`. String `"*"` selects All; every other key
  runs the complete pinned package-spec parser before its profile value.
- Lines 1085–1260+ define the existing opt/debug/trim-path leaf behavior;
  lines 1733–1760 define StringOrBool's string/bool visitor. Leaf provenance
  remains in the parent profile and trim modules.

`restricted_names.rs:123–192` supplies ProfileName's Unicode alphanumeric,
underscore and hyphen grammar plus case-insensitive reserved names/prefix.
The existing complete scalar name interpreter preserves its streamed casing
comparison and exact original spelling; it does not impose PackageName grammar
or reject an empty profile name absent a source rule.

Serde/derive/core 1.0.228 `src/de/struct_.rs:70+,224+` and
`src/de.rs::deserialize_seq` fix map input order, duplicate-before-second-value
checks and container-default positional suffixes. TOML 1.1.2+spec-1.1.0
`src/de/deserializer/value.rs:157–192` enters newtypes/structs through the actual
value adapter. `array.rs` returns directly from the visitor; Content and generic
TOML replay require exhaustion while raw TOML and serde-value do not. The shared
`Input`/`Cursor` retain this distinction. `walk::run(Mode::Ignored)` performs the
complete unknown traversal, including deferred numeric errors, while Content's
IgnoredAny replay preserves its unit shortcut.

The source is Copyright The Rust Project Developers, MIT OR Apache-2.0;
unchanged licenses and the verified archive provenance are retained in
`importer/reference/manifest`. The
terminal source-reparse failure propagates unchanged from a package map key.

## Representation and ownership

`Profiles` owns one flat arena of `Profile` records. A profile owns only a finite
field buffer, ignored-key buffer, and nonrecursive converted leaf values/maps.
Build-override and package-map values store arena indices. The explicit frame
buffer owns source cursors, destination indices and partially converted maps.
No production recursive call, recursive owned edge, implicit clone or recursive
teardown is used on success or failure. Nested generic unknowns use the existing
flat generic walker, not a second recursive interpreter.

Maps visit every key and child in source order, then heapsort their actual
admitted records by typed key and insertion ordinal. Equal typed keys retain
the first typed/raw key and last child index, matching BTreeMap replacement.
Profiles whose values are superseded remain as flat, fully admitted records;
their declaring Locations retain original root keys, parent indices, field
positions and package-key spellings. A separate admitted `(node, key-index)`
log preserves exact global callback order, including a parent's unknown field
after an earlier nested callback and callbacks from replaced values. The caller
can render paths iteratively without reconstructing ownership from canonical
map keys.

Every frame, arena record, map entry, field slot, ignored key and callback index
uses the same Session/Buffer admission discipline. Requested full replacement
layouts are admitted before allocation; allocator slack grants no capacity.
Borrowed Text continues to point at the original admitted Document after a
temporary Content/serde-value projection is dropped. Converted Spec and other
owned leaf data retain their charged owners. No historical usage resets,
refusal recovery, hidden BTree nodes, guessed per-depth charge or new supported
depth limit is introduced. No graph path/hash/decode/comparison event is invented
for this pure typed work.

## Focused tests and reference scope

Ten new tests exercise actual complete Profiles interpretation. Full typed
results cover all 44 accepted profile-bearing rows from the committed pinned
manifest observations; all 23 rejected selected rows contain only the profile
top-level field, so their failure is attributable to this complete visitor.
The comparison preserves every Option/variant/field, typed ProfileName and
ProfilePackageSpec, semantic key order/replacement, and applicable source spans.
The test helper adapts the existing exact Spec projection into the manifest
reference's tagged representation; it does not reimplement parsing.

Additional cases cover every positional prefix length 0–19, all replay adapter
exhaustion policies, source callback interleaving through typed-key replacement,
name/key-before-value errors, known duplicates before their malformed value,
unknown traversal, depth-1024 replay success/failure/drop through actual admitted
Content staging (without changing the pinned raw parser's depth), terminal key
failure before the profile value, original Document borrowing, every refusal
point and nonzero original-account exact/one-under item/layout/string floors.
Test-only JSON projection may own nested reference transport; the production
arena and continuation ownership stay flat. Root owns execution results and
scoped formatting; test source alone claims no pass or complete T06 witness.
