# Initial workspace configuration

This private step consumes the completed typed Original. It implements pinned
Cargo `src/cargo/util/toml/mod.rs:232–301` (`to_workspace_config` and the borrowed
contents of `to_workspace_root_config`), lint verification at 2678–2763 and
dependency warning rendering at 2925–2935. Cargo commit is
`59800466c5c41c444d264b1010b4d57e85a7117f`; the source member SHA-256 is
`940a2998ebff9915b4a0d050dcadc4d12e05615c7fcc3a5fb447b7d3fea4ea8e`.
Licenses are retained under `importer/reference/manifest`.

The embedded branch returns a default root declaration before inspecting any
workspace conflict or lint/dependency fields. Ordinary configuration uses the
schema's package-over-project preference after both aliases were interpreted.
Conflict precedes root checks; lint verification precedes sorted dependency
optional/public checks; all checks precede dependency-extra warnings. Lint tools
and names use the typed map order, configured extras preserve insertion order,
and unsupported tools skip their names. The only expected extra is exactly
`rust.unexpected_cfgs.check-cfg`. Warning text is preserved in full.

`Configuration` retains the defining manifest directory and complete borrowed
workspace declaration, including members/defaults/exclusions/inheritable fields
and metadata. An absent declaration in the embedded root denotes Cargo's empty
default workspace. This avoids cloning an already admitted tree. No accessor
normalizes paths, reads files or inserts anything into a workspace cache.
Configuration is unresolved data, not a discovered or validated workspace.

Warnings use checked concatenated lengths, per-string checks, exact full byte
layouts and fallible reservation before writing; the warning vector uses the
same caller Session's admitted Buffer. Every result item is admitted. Rejections
retain a bounded category and source span; lint-name suggestion classes remain
distinct in the static expectation. No resource refusal is converted to a
semantic rejection, and warning prefixes remain internal on failed loading.

## Executed evidence

Six focused tests passed, covering alias/conflict/embedded branches, complete
lint warning variants, first error ordering, dependency extras, every refusal
point and exact original item/byte/string floors with graph counters exhausted.
Independent source review found no P0/P1 (confidence 0.98). CLI and no-default
compile checks and scoped format/diff checks passed.

`importer/reference/manifest/run_workspace.py` invokes the exact pinned Cargo
binary against 25 offline isolated manifests. Four succeed and 21 reject; all
match the contained configuration result. The committed observation hash is
`bbdf723b2ab41c29705333be92c1e257139e15adaa1b64ca1a7e1221790a13b6`.
`workspace-execution.json` records binary/script identity and exact command.
The metadata command suppresses manifest warnings: this corpus proves acceptance
and first rejection, while warning text/order relies on focused tests and source
comparison. It does not prove complete loader parity or embedded expansion.

The joined `load::configure` now runs complete typed Original → Features →
workspace configuration in that order and returns the borrowed configuration,
feature state and owned warning batch together. The feature closure is detailed
in `features/PROVENANCE.md`. All 20 focused loader tests pass, including the
336 executed pinned feature cases and joined error/refusal ordering. This is
initial configuration only: cache insertion, normalization/inheritance,
workspace discovery, package selection and full before/cold/after/current proof
remain outstanding. The containing membership module remains cfg(test); this
step grants no production reuse or Gallery performance evidence.
