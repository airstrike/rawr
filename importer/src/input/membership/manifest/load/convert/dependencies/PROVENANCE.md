# Original dependency warning stage

Source: Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs:2047–2070` (`validate_dependencies`) and
`:2925–2935` (`unused_dep_keys`). The same commit's
`crates/cargo-util-schemas/src/manifest/mod.rs:676–681,784–789`
defines the reached `unused_keys` closure. Cargo's MIT/Apache license texts
are retained in the adjacent `load/path` directory.

The helper consumes the original interpreted inheritable map. That schema map
already retains sorted dependency names and sorted extra keys with its pinned
replacement semantics. Simple dependency values contribute no keys. Detailed
and inherited values both collect their complete extra-key list before any
warning for that dependency is appended. Known fields remain excluded by the
original schema interpretation; this helper does not reinterpret extra values.

Each collected key is copied with the existing bounded formatter into an owned
String and retained in `storage::Buffer<String>`. The original Session admits
each iteration, string bound, actual string allocation and buffer replacement
before work. Collection completes before the warning loop. A refused later key
therefore cannot expose an earlier warning from that same dependency. Warnings
already emitted for previous dependencies remain in source order. All failures
latch in the original Session; no collection fallback or budget reset exists.
Flat strings and buffers have bounded, nonrecursive ownership and destruction.

The string-label helper supplies an exact table label. The connected original-map
caller instead passes closed formatting arguments containing the parsed Platform.
The label is formatted per dependency, before collecting keys, including simple
dependencies with no unused keys. There is no eagerly allocated platform label.
The warning text is `unused manifest key: {table}.{dependency}.{extra}`.
The original-map caller follows mod.rs:1559–1592: top normal/dev/build, then
sorted original platforms, each parsed before attribute warnings, keyword
warnings, and normal/build/dev maps. Preferred kebab aliases override underscore
aliases even when empty. The target owner exposes this stage after target/link
validation; it does not claim dependency semantic conversion is complete.

Focused tests cover sorted name/extra order for both variants, quiet simple and
known-field dependencies, verbatim supplied platform labels, refusal while
collecting a later key with the original schema Session, and exhaustive helper
admission failures. All six tests passed with nightly-2026-07-11, offline,
rawr-import library features rustc-driver,test-fixtures and filter
convert::dependencies. Independent read-only review found no actionable blocker
within this helper boundary. Formatting and diff checks passed.
After caller connection, all nine dependency tests and 32 connected
package-normalization tests passed with the same configuration. A separate
read-only review found no blocker in caller ordering, aliases, label formatting
or error propagation. Platform algorithm evidence is recorded separately.
This is not dependency semantic conversion, full manifest conversion, Loaded
authority, a current-input membership proof, or release/performance evidence.
