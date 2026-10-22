# Pinned initial configuration value conversion

The reference retains the unchanged `ConfigValue` enum and
`ConfigValue::from_toml_inner` body from Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/context/config_value.rs`. Copyright the Cargo contributors,
MIT OR Apache-2.0; license texts are in the contained loader's `path/` directory.

Test scaffolding supplies a fixed string definition, std HashMap, KeyOrIdx and a
string-error `bail!` adapter. It does not copy Cargo's outer anyhow formatting.
These adapters expose the inner conversion's accepted values, rejected type and
reverse key/index error path. Successful table output is sorted solely for JSON
comparison. Input parsing uses the actual pinned `toml::Table`, including the
root-table versus nested generic-value distinction and complete parsing before
configuration conversion. No production implementation calls this reference.

Root compiled with nightly-2026-07-11 and the existing manifest-reference rlibs
for toml 1.1.2+spec-1.1.0 and serde_json 1.0.150, using rustc `-L dependency` and
explicit `--extern` paths. Feed one JSON string per stdin line. The 14 retained
observations include seven successful trees, six unsupported-type failures and
one prior TOML integer conversion error. No Cargo child or Gallery run occurred.

The observations validate initial conversion, not configuration discovery,
include handling, merge order, effective getter behavior or current membership.
