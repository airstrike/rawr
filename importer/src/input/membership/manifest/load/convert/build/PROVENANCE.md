# Build target conversion

Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs:101–141`; constructor defaults in
`core/manifest.rs:891–912`. License texts are retained in `load/path/LICENSE-*`.

`Owner::convert_targets` completes ordinary targets before inspecting build and
metabuild. Disabled build is absent, while an explicit empty multiple-build list
remains present and conflicts with any metabuild declaration. Stem validation
finishes before target construction. Custom target names derive from the actual
script stem and retain the native joined path and selected package edition.
Metabuild names must occur in the completed normalized build-dependency map;
the resulting synthetic target has no file source and uses edition 2018.

The completed source target sequence is retained in consecutive ordinary/build
ownership groups. Remaining manifest checks and the complete driver protocol
are still required; this does not mint Loaded authority. The preceding header
and tail are trusted internal stage inputs, not independent membership receipts.

Containment applies only at the actual missing-stem operation in `stems.rs`. Its fixed
cause is admitted and latched through the original Session; prior or cause
admission refusal wins. Earlier duplicate groups cannot report before a later
missing stem. No arbitrary panic conversion or new source restriction exists.
The existing config Fx table is exposed narrowly for reuse; hashing, bucket
layout and iteration code are unchanged, while each group's paths use admitted
flat storage.

31 connected package/library tests passed in
`/private/tmp/rawr-build-target-connected-tests.log`, including disabled, empty,
single normalized build, empty/nonempty metabuild and conflict-before-stem cases.
Four stem tests passed in `/private/tmp/rawr-build-stem-tests.log`; nine shared
table regressions passed in `/private/tmp/rawr-build-table-regression.log`.
These use pinned nightly and importer features `rustc-driver,test-fixtures`.
Stable rustfmt and diff checks passed.

Independent read-only review found no P0/P1 in this bounded target-conversion
checkpoint, including source order, empty-build presence, Fx grouping, missing-stem
precedence, constructor editions and narrow table exposure.

Membership remains test-gated. Full governed-caller association, actual cold
success/current witness, final manifest conversion and release qualification
remain outstanding. No installed candidate or Gallery timing claim follows.
