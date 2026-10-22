# Target settings conversion

Derived from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs` 931–975 and `src/cargo/core/manifest.rs`
832–969, 989–1016, 1102–1148. License texts are retained in `load/path/LICENSE-*`.

Settings start from the exact constructor flags for each target kind, then
apply test, doc, doctest, bench, harness, proc-macro, doc-scrape-examples and
for-host in source order. Preferred `proc-macro` overrides the underscore alias
even when false. Cargo's setters simply assign their own fields; a proc-macro
crate type does not implicitly change the proc-macro or for-host booleans.

A target edition override appends its deprecation warning before parsing.
Parsing uses the existing admitted edition selector with an explicit spelling
and no MSRV, so there is no package default warning, MSRV compatibility check
or unstable-edition feature gate at this point. Parse context/cause is retained.

Build constructors are exposed separately for the later custom-build conversion:
their scrape setting is Disabled, and metabuild uses fixed edition 2018.
Ordinary target conversion calls configure; build conversion uses defaults
without applying an absent TOML target's configure overrides.

All projections and settings/diagnostic ownership use the original Session.
Tests cover each ordinary constructor, false overrides and aliases, crate-type
versus boolean distinction, warning-before-error and future-edition behavior,
and custom-build/metabuild constructor defaults. This module supplies settings
only and does not construct a complete converted target.
