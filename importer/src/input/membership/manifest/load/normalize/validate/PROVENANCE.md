# Target validators

Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs:1139–1260` and
`src/cargo/util/restricted_names.rs:33–35`; deprecated alias wording comes from
`src/cargo/util/toml/mod.rs:2883–2910`. License texts are retained in
`../../path/LICENSE-MIT` and `../../path/LICENSE-APACHE`.

Binary validation checks name, effective crate-types, then effective proc-macro.
Nonempty crate-types and proc-macro=true append separate critical reports and
skip their respective alias checks even in edition 2024. Preferred spelling
presence wins over its underscore spelling, including empty lists and false.
Common example/test/benchmark validation checks name, then proc-macro alias,
then crate-type alias. Missing/trim-empty names reject; binary names additionally
reject exactly deps/examples/build/incremental. The pinned supported Unix cfg
does not run Cargo's Windows reserved-name warning branch.

`Errors` reuses admitted warning-text retention but is a distinct channel;
critical reports do not become semantic rejection or ordinary warnings. Source
target fields remain borrowed through the shared projection reader. Transient
crate-type text slots and report parts use admitted buffers, with full text and
allocation admission before report retention. Tests exercise the same Session
through interpretation, both report channels, every refusal, and exact/one-short
item, per-string and cumulative allocation limits.

This does not perform path selection, inference, unique-name checks, target
conversion or successful manifest loading. The caller supplies the normalized
or inferred name associated with the original target.
