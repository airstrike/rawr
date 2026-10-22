# Target-group normalization

Derived from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs`: binary normalization (275–347), benchmark
legacy-warning placement (528–559), and common target normalization (635–705).
Cargo's MIT/Apache-2.0 license texts are retained in the sibling `path` directory.

The group joins the existing explicit/inferred selection, field validation,
ordered inference and path resolver. Fast groups preserve declared order and
validate/normalize each target without discovery. Slow binary groups validate
and resolve each target in turn. Slow common groups validate every selected
target before resolving any target path. Selection retains absent versus empty
declarations and performs the pinned sort and autodiscovery behavior.

A binary semantic path failure returns the exact owned diagnostic through the
inner Result and stops the group. Common semantic failures append critical
reports and omit the failing target, then continue. Session failures remain the
outer Step and cannot be treated as those semantic reports. Binary legacy
warnings are appended before path normalization; benchmark legacy warnings are
held until the entire group succeeds. Earlier common validation warnings remain
in their original order.

The embedded input is an admitted synthetic draft, not a discovery bypass.
`autobins=false` selects the normal complete-fields fast path, while explicit
true performs ordinary inference and selection. Its borrowed name/path have a
separate scratch lifetime and are copied under the Session; normalized outputs
cannot borrow an outer owning package through this temporary view.

Each output retains its original target by reference where present, its required
name and normalized path. No unknown target fields are removed. This proves one
normalized group only; full-manifest loading, conversion, and current membership
authority remain separate transformations.

Focused tests cover every group's fast path, inferred binary sorting, binary
versus common validation ordering, common error accumulation and omission,
deferred benchmark warnings on later refusal, and embedded default/explicit
autodiscovery behavior.

`all_targets.rs` composes the existing library completion with binaries,
examples, tests and benchmarks in `util/toml/mod.rs:393–451` order. Original
package flags override embedded defaults. Virtual manifests retain no groups;
this stage does not certify their later conversion. Connected Reader tests
exercise all four groups, retained payloads, accumulated binary critical reports
and rejection before later group validation.

Development validation: 59 normalization tests and 20 connected package/library
tests passed with pinned `nightly-2026-07-11`, importer features
`rustc-driver,test-fixtures`. Logs: `/private/tmp/rawr-target-groups-tests.log`
and `/private/tmp/rawr-target-groups-connected-tests.log`. Stable rustfmt and
diff checks passed. Independent read-only review accepted the group composition,
embedded flags, error ordering, deferred warnings and ownership seams with no
actionable blocker (confidence 0.93).

Membership remains test-gated in `input.rs`; these are development checks,
not production-path activation, complete Loaded authority, release qualification,
an installed candidate or Gallery timing evidence.
