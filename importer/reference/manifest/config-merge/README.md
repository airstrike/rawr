# Pinned complete value merging

`program.rs` retains unchanged sections from Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/context/config_value.rs`: ConfigValue and the nonmergeable list
constants; from_toml_inner; merge and merge_helper; desc and definition; and
is_nonmergeable_list. Copyright the Cargo contributors, MIT OR Apache-2.0.
License texts are in the contained loader's path/ directory.

The reference includes the already retained unchanged Definition and ConfigKey
programs. Tables use the exact rustc-hash 2.1.2 dependency and pinned standard
library, rather than a random or sorted map. Test-only scaffolding supplies
String errors, KeyOrIdx, formatting macros and a with_context adapter that
retains each source context and its inner error. It does not claim anyhow's
rendered byte-for-byte output. The parse entry calls the actual pinned
toml::Table parser, then the unchanged conversion function.

`reference.rs` accepts one JSON row per line with a steps array. Each step has
source, definition [class, origin], and force. It executes complete conversion and
successive merges, then records the entire resulting typed value with every
definition origin, or the actual first rejection's key path and expected type.
Successful table output is sorted only for serialization; merge iteration is
never sorted. Observations retain their complete input rows, so they can be
replayed directly through the harness.

The 870 observations were generated on nightly-2026-07-11 aarch64-apple-darwin
using the unchanged cached rustc-hash 2.1.2 rlib and existing manifest-reference
toml 1.1.2+spec-1.1.0 / serde_json 1.0.150 rlibs. Compile edition 2021 with explicit
extern paths and the manifest-reference dependency search path. The table's group
width makes this merge observation fixture target-specific; the corresponding
test is gated to aarch64/little-endian/64-bit. Other targets require regeneration
and pin correspondence before connection, not acceptance inferred from this run.

Cases cover all pairs of scalar/list/table values across four definition classes
and both force settings, stable list priority under sequential merges, exact
same-class file origins, all nonmergeable-list paths, the missing wildcard part,
occupied sibling path accumulation, nested first failures and empty roots.
There are 420 full successes and 450 merge rejections. The contained test compares
the rejection's expected type and the source span independently located using the
reference's selected key path. Fixtures use valid TOML; earlier invalid TOML and
unsupported-value conversion are covered by the separate initial-value suite.

This proves this configuration-value operation, not discovery/include traversal,
CLI/environment precedence, typed getter selection, or current source authority.
