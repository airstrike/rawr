# Library normalization

Source: Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs:147–217` (`normalize_lib`),
`:587–608` (`is_normalized` / `are_normalized_`), `:708–715`
(`inferred_lib`), `:1133–1141` and `:1166–1194` (library/target names),
`:1204–1221` and `:1243–1260` (deprecated fields), and
`src/cargo/util/toml/mod.rs:301–395` (caller order), `:2883–2910`
(`deprecated_underscore`). Cargo's license texts are retained in
`../../path/LICENSE-MIT` and `../../path/LICENSE-APACHE` relative to this file.

`package::Complete::normalize_library` performs the next concrete stage after
patch and package normalization. The normalized package supplies name and
edition; the original package/project supplies autolib before its normalized
false replacement. Missing/unparseable editions have Cargo's default 2015 at
this stage. Original features are borrowed unchanged, corresponding to the
intervening nonvalidating `normalize_features` clone. Virtual manifests skip
these stages. Completion is only a library-stage result, never successful full
loading, real/virtual conversion, or membership authority.

The fast branch requires autolib=false and either no declared library or both
name and path present. Every other branch probes src/lib.rs, even with a fully
specified explicit library. Probes use the recorded exists predicate, so
present directories count. Missing inference without a declaration returns no
library. Declared names are retained exactly; only default package names
replace hyphens with underscores. Name errors precede proc_macro deprecation,
which precedes crate_type deprecation and missing-path handling. Presence of
false/empty alias values still triggers the pinned diagnostics. Both aliases
remain in the borrowed typed payload for later field access; this stage does
not validate crate-type contents or discard unknown target keys.

Only edition 2015 reaches the legacy src/{name}.rs exists probe. Its spelling
is built before the edition check; success retains the exact legacy warning.
A legacy spelling containing NUL follows Unix's source-local metadata error:
exists is false before any filesystem call, so it cannot become a malformed
recorded path. Final paths use the existing bounded lexical path normalizer.
The supported Unix configuration does not execute the Windows reserved-name
warning branch. No new platform or input exclusion is introduced.

The result owns only actual replacement name/path data and borrows the full
original admitted target. Source clones are eliminated without manufacturing a
fresh account. All lookup visits, warnings, derived strings, output items and
path operations use the caller's original Session. Derived text uses checked
length arithmetic and one pre-admitted exact reservation; partial results drop
normally without refunds. Prior refusal is checked before target work. There
is no process, scan worker, hash, store write, limit change or admission reset.

Connected tests run Reader -> patches -> package -> library against recorded
host evidence. They cover fast-path no-probe behavior, explicit slow-path
probe consumption, default names, directory inference, legacy edition and NUL
behavior, ordered diagnostics, retained payload/feature identity, virtual
skip, first refusal, and exact/one-short cumulative item and byte accounts.
These tests do not claim completion of the later targets, dependency passes,
workspace recursion or full manifest conversion.
