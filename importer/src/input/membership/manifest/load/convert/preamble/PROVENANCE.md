# Real conversion preamble

Derived from Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs:1294–1397`. License texts are retained at
`../../path/LICENSE-MIT` and `../../path/LICENSE-APACHE`.

The checks preserve followed directory metadata before package-name namespace
authorization, normalized Rust-version and edition selection, unstable-edition
authorization, original project alias handling, and metabuild authorization.
Version components are reused from the existing typed normalization payload;
no new version parser or native subprocess is introduced. The edition helper
retains parse context separately from its exact cause. Edition `future` sorts
after 2024 for project-alias rejection and retains its prior feature gate.

`all_targets::Owner::preamble` borrows the actual normalized package, original
manifest, features and manifest parent. Connected Reader tests explicitly
complete the dependency/lint tail before invoking it. This helper does not
enforce the entire conversion protocol or confer Loaded authority: the closed
driver and following embedded, resolver, target and manifest conversion checks
remain required.

Validation uses pinned nightly and importer features `rustc-driver,test-fixtures`:
24 connected package/library tests (`/private/tmp/rawr-real-preamble-final-tests.log`),
edition checks (`/private/tmp/rawr-edition-tests.log`) and followed predicate
checks (`/private/tmp/rawr-directory-predicate-tests.log`). The directory test
covers every recorded metadata kind, absence and ordinary OS errors, using the
existing original Session. Missing observations still refuse rather than
becoming absent directories. Stable rustfmt and diff checks passed.

Independent read-only review accepted the pinned edition/MSRV behavior,
directory and feature ordering, future-edition project rejection and original
accounting with no actionable blocker (confidence 0.94). Test totals were five
edition checks and eleven Session host checks in addition to the 24 connected
package/library tests.

Membership remains test-gated. No production activation, complete importer
acceptance, installed candidate or Gallery performance measurement is claimed.
