# Complete retained version requirements

The grammar remains the existing `scalar/partial.rs` specialization of pinned
semver 1.0.28 `src/parse.rs` VersionReq/comparator parsing. One private capture
path now serves both callers: PartialVersion's allocation-free count/first
projection and this complete owned requirement. No grammar was duplicated.
PartialVersion retains its original full-version trial and extra projection
restrictions; the complete requirement bypasses those restrictions.

Private capture events occur after nonempty prerelease interpretation, after
nonempty build interpretation, and on completed comparators. Refusal from a
capture is separate from grammar failure and cannot enter wildcard diagnostic
replacement. The complete caller retains each prerelease before later build or
delimiter parsing; build metadata is admitted, owned and immediately dropped,
matching the reached source temporary. Failed parsing never refunds that work.

Actual transformed identifier storage is a String with its exact admitted byte
layout, not semver Identifier's inline-eight-byte/varint layout. This explicit
representation therefore allocates for short nonempty identifiers too. Empty
identifiers allocate nothing. All actual storage uses the original Session.
The source's at-most-32 recursive comparator frames become a fixed flat array
of 32 optional comparator slots; each reached slot is admitted as an item.
Only complete grammar success reserves the exact final comparator vector,
matching semver parse.rs:380–402's terminal reservation before unwind. Moving
staged comparators into that already admitted vector does not clone strings.
Failure/drop has constant bounded stack depth and at most 32 owned identifiers.

The retained data preserves every operator, major/minor/patch option and
prerelease in encounter order. Build identifiers are validated but not retained,
as in semver Comparator. Wildcard-only requirements retain zero comparators.
The caller must separately distinguish absent requirements from explicit `*`.
Existing Rejection categories remain available for the later Cargo contextual
error wrapper; exact Display formatting is not added by this module.

Differential tests compare every retained comparator against pinned semver,
including max32, all operators, partial/wildcard forms and metadata. Failure
tests cover malformed later comparators, retained/transient identifier ordering,
no final vector allocation on grammar error, and refusal at every reached
admission. Existing PartialVersion tests are unchanged. Parent executes tests
after editing stops. No dependency construction or membership authority is
conferred by this requirement value.

Validation: all 15 scalar tests passed offline on nightly-2026-07-11 with
rawr-import library features rustc-driver,test-fixtures, including five new
complete-requirement tests and the existing PartialVersion/Version tests.
Independent read-only review found no blocker in the bounded retention change;
format and diff checks passed. This is development evidence only.
