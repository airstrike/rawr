# Edition conversion preamble

Derived from Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`:
`src/cargo/util/toml/mod.rs` 1314–1377, `src/cargo/core/features.rs`
183–245 and 330–367, and
`crates/cargo-util-schemas/src/manifest/rust_version.rs` 16–45.
License texts are retained in `load/path/LICENSE-*`.

This primitive accepts the already normalized optional edition text and already
validated `scalar::Rust` components. It does not reparse Rust versions. The five
edition spellings, bounded u16 future-year classification, stable flags and
first stable versions match the pinned source. A parse failure retains Cargo's
context separately from its exact cause, rather than guessing a final error
renderer or discarding that cause.

Every edition first-version requirement is a complete `^1.minor.0` requirement
(minor 31, 56 or 85). Cargo's RustVersion compatibility turns absent candidate
minor and patch into zero. Consequently the exact restricted predicate is
candidate major equals 1 and candidate minor-or-zero is at least the required
minor. Candidate patch is an unsigned integer and the required patch is zero;
it cannot fail that lower bound. A candidate major 2 is incompatible, even
though it sorts later. Existing Partial::matches tests component equality, so
it is not the caret predicate and is deliberately not substituted here.

An absent edition always selects 2015. The warning's suggested edition uses the
latest compatible stable first-version candidate, or the default 2015 if none
qualifies. The future edition has no stable first version and is excluded from
that suggestion. Warning text and failure ownership use the original Session
and existing admitted formatting helpers.

This type proves edition selection/MSRV checks only. Namespace checks, unstable
edition feature authorization and surrounding package conversion remain in the
caller. Tests cover exact parse contexts/causes, numeric spelling edge cases,
all MSRV thresholds, partial components, other-major rejection, warning branches,
and terminal original-account refusal.
