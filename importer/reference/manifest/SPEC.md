# Direct pinned PackageIdSpec observations

The separate `spec` binary calls the actual public
`cargo_util_schemas::core::PackageIdSpec::parse` from Cargo
`59800466c5c41c444d264b1010b4d57e85a7117f` (`cargo-util-schemas 0.14.2`).
It uses this existing isolated reference workspace and reviewed lock; it has no
product dependency. The original manifest binary, corpus, observations and
execution evidence are unchanged.

Sources are covered by the existing `provenance.json`:

- `crates/cargo-util-schemas/src/core/package_id_spec.rs`, SHA-256
  `86aac55339ee2c8f512aa73ccbf6e2ea9a13ec9ea143416e271b5d1d0b344d34`:
  parse/from_url/plain parse, lines80–232; Display, lines234–265.
- `crates/cargo-util-schemas/src/core/partial_version.rs`, SHA-256
  `fe796cd12bac41396ca66dd6c093fe0ccd281f186764912057f81d1b112127a0`:
  public complete fields, parsing and derived ordering.
- `crates/cargo-util-schemas/src/core/source_kind.rs`, SHA-256
  `f3b4cc73dabe93722aa64d6397474102399b0f7d17cf8daa37ff70a888589837`:
  custom source-kind order, Git-reference order/query conversion/rendering.

The source is invoked through the public API, not copied or modified. Relevant
dependencies remain URL2.5.8 and SemVer1.0.28 under the verified reference lock.
Existing source licenses remain in this directory. No new provenance or
successful execution claim follows merely from adding this runner.

The corpus in `src/bin/spec/cases.rs` supplies raw inputs, not authored expected
semantic outputs. Each `row:"spec"` reports schema1, id, input, status, name,
version, URL, source kind, rendering and exact error Display/Debug. A successful
version is `{major,minor,patch,pre,build}`; each absent component is JSON null.
Source kind is `{type,reference}`; `reference` is null except for Git, whose
`{kind,value}` distinguishes tag/branch/rev/default (default has empty value).
The final `row:"orders"` contains selected `{left,right,ordering,equal}` pairs;
left/right refer to case IDs and ordering is -1/0/1 from upstream `Ord`.

`status:"ok"` and `status:"error"` are actual upstream results. The sole
expected caught panic, `git+http://a%00/foo`, is emitted as `status:"source"`
with `source_stage:"url-reparse"`, matching the independently verified
source location. `catch_unwind` exists only in this reference binary. Unexpected
panics or disappearance of that expected panic fail the run; they do not become
accepted product errors. The standard panic hook may still print this caught
panic to stderr. The wrapper reports that stderr separately from the artifact.
Display is executed only after successful parsing; a Display panic fails the
binary rather than being misclassified as the known second-parse event.

`run_spec.py` supplies the same owned diagnostic cwd convention as `run.py`:
`probe/existing` and the Unix single-component `probe\existing` exist, while
`probe/missing` does not. Only this cwd's path/URL prefix in error text is replaced
by `${FIXTURE_ROOT}`. Typed output and input bytes are unchanged; a row identifies
diagnostic replacement through `diagnostic_cwd_elided`. Product code intentionally omits
this rejecting path-suggestion probe, so exact wording for those
rows is reference evidence, not a product diagnostic promise.

Root execution commands, from the repository root, with a fresh destination:

```sh
python3 importer/reference/manifest/prepare.py prepare \
  --archive /private/tmp/rawr-p9-toml-supplement-cargo.tar.gz \
  --destination /private/tmp/rawr-p9-spec-reference
cargo +nightly-2026-07-11 build \
  --manifest-path /private/tmp/rawr-p9-spec-reference/Cargo.toml \
  --bin spec --offline --locked --jobs 1
python3 importer/reference/manifest/prepare.py verify-lock \
  --destination /private/tmp/rawr-p9-spec-reference
python3 importer/reference/manifest/run_spec.py \
  --binary /private/tmp/rawr-p9-spec-reference/target/debug/spec \
  --output importer/reference/manifest/spec-observations.jsonl
```

Preparation copies the new binary and corpus with the existing `src` tree.
The wrapper validates framing/IDs/order rows and atomically publishes actual
JSONL. A failed binary writes only `spec-observations.jsonl.failed`; it never
replaces successful evidence. Root must record actual execution, status counts
and artifact digest after running; this source-only addition does not provide
the generated artifact merely from its source.

Root execution used the retained prepared workspace and target at
`/private/tmp/rawr-p9-manifest-reference`, copying only the new bin sources.
The locked offline build passed; all 72 lock entries and the exact package-spec
and source-kind member hashes were verified. The run produced 137 cases
(80 accepted, 56 semantic errors, one source failure), 43 ordering pairs,
and 50,831 bytes. SHA-256:
`ff10b534a2bca238216874524842d121cc972397b9c7dc35d1d761ce7e4a91cb`.
`spec-execution.json` records the binary and source identities. The complete
contained caller test compares fields, acceptance/source status and all pairs
against this artifact; its execution is recorded in the MAP tracker.
