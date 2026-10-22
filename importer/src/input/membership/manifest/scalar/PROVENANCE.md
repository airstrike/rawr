# Borrowed pinned manifest scalars

This is the scalar slice, not complete typed-manifest, loader, workspace,
selected-owner or current-source authority. Publicly constructible raw fields,
unrestricted deserialization and unchecked raw-value fallback are absent. The
private-field parsing transformations apply `smart-constructor-newtype` and
`structured-error`; errors retain fixed tags/positions/characters while the
enclosing visitor retains the original document and key/value spans.

## Source identity and license

The implementation adapts semver 1.0.28 and Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`. Both are MIT OR Apache-2.0; this
extraction uses their MIT terms, reproduced verbatim in `LICENSE-MIT` (the two
upstream MIT files have identical contents).

Cargo source archive SHA-256:
`dc2728d4e9650b9871142cc3f1f0e449647e0b493775931f0b70ee1fa658a321`.
The local source root is
`/private/tmp/rawr-p9-pinned-cargo-full/cargo-59800466c5c41c444d264b1010b4d57e85a7117f`.
Registry source root is
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.

| Source member | SHA-256 |
|---|---|
| semver 1.0.28 archive | `8a7852d02fc848982e0c167ef163aaff9cd91dc640ba85e263cb1ce46fae51cd` |
| semver `src/parse.rs` | `2d9cb28a72a813968c16da29a9690f8e53f1ce00727a1dbe67aec3ddb5ed59ed` |
| semver `src/impls.rs` | `30bac3940b9bab840dc59280832c9afbb342abf47c51740a1b83b43c1646c56b` |
| semver `src/error.rs` | `d0311d2d3ec0c1c04f3067a59aed52bac57329e47801be67b09199d145c7f73e` |
| semver `src/lib.rs` | `e56f4d7c7774c45e61026fb0050955d67601c9ad18bdf93f8921d9898067c119` |
| Cargo `crates/cargo-util-schemas/src/core/partial_version.rs` | `fe796cd12bac41396ca66dd6c093fe0ccd281f186764912057f81d1b112127a0` |
| Cargo `crates/cargo-util-schemas/src/manifest/rust_version.rs` | `12d3b6b0b5af0e8ea847f800bf725e5f765cba2d2a7041e21d347deb8c03329b` |
| Cargo `crates/cargo-util-schemas/src/restricted_names.rs` | `bb47af6a7a1f7780b21d7a2bb6d2e34a6808a814e1286eee028bf9ccfaf1f8a1` |

`semver::Version::parse` and `VersionReq::parse` are called only by `cfg(test)`
reference checks. The current lock selects 1.0.28 with the archive checksum above.
These unconditional FromStr/parse/Ord bodies do not change with the optional
serde feature; std enables its Error trait implementation. Production uses the
contained borrowed code and does not call the allocating semver implementation.

## Exact transformation map

| Contained operation | Pinned operation and preserved behavior |
|---|---|
| `version::parse` | semver `parse.rs:25–81`: required major/minor/patch, numeric u64 overflow and leading-zero order, prerelease before build, exact trailing rejection. No trimming. |
| `version::{numeric,identifier}` | semver `parse.rs:156–260`: ASCII identifier grammar, arbitrarily long identifier spelling, empty segment and numeric prerelease leading-zero rejection. Safe borrowed substrings replace `Identifier::new_unchecked` and its owned representation. |
| `version::{Pre,Build}::cmp` | semver `impls.rs:51–159`: release/prerelease ordering, numeric versus lexical segments, segment count, and build numeric leading-zero tie-break. Full `Version::Ord` follows semver `lib.rs:157–164`, including build metadata; `cmp_precedence` excludes build. |
| `partial::parse` | Cargo `partial_version.rs:82–120`: first full Version trial, then complete VersionReq trial; requirement failure maps to prerelease first, build second, generic failure last; successful requirement requires exactly one Caret comparator and rejects only a raw spelling beginning with `^`. |
| `partial::{requirement,comparator,op,wildcard}` | semver `parse.rs:84–105,262–404`: ASCII-space rules, all comparator operators, wildcard placement, prerelease/build parsing, error replacement and 32-comparator guard. Build metadata is validated and discarded on the requirement path. |
| `Partial` representation/comparison | Cargo `partial_version.rs:6–13,60–80`: Option presence is retained, including Option ordering for pre/build; full Version success retains build metadata. `matches` follows `:38–57`; conversion to full Version follows `:15–24`. |
| `Rust::parse` | Cargo `manifest/rust_version.rs:58–108`: complete PartialVersion parse, then prerelease rejection before build rejection, then retained major/minor/patch. This slice does not implement later MSRV compatibility/normalization. |
| `Name::parse` | Cargo `restricted_names.rs:36–87,124–244`: separate package/registry/path-base/profile/feature grammars, package `::` components, first-digit error precedence, feature `dep:` before slash before character checks, complete profile character checks before reserved words. Empty profile names remain accepted. |

The call site chooses package-version `.trim()` versus workspace-version raw
parsing, as dictated by manifest `mod.rs:368–384` and ordinary semver serde.
No scalar constructor trims universally. In particular, `" ^1.2"` is accepted
by PartialVersion's raw-prefix check, and `"1.2.3+build "` reaches requirement
fallback and drops build metadata. Neither is normalized into the full-version
path. The first inherited/scalar visitor still supplies its exact surrounding
conversion and failure ordering.

## Storage, refusal and work

The scalar functions perform no heap allocation. Their results contain only
borrowed string slices, fixed-width numbers, Options and finite enums; errors
also contain only fixed fields. There is no Box/Vec/String, hidden clone,
recursive ownership or fallible allocation to precharge inside these functions.
Caller admission remains mandatory for the input per-string bound, actual
retained typed item/layout and enclosing diagnostic. A borrowed result does not
refund prior usage and does not authorize downstream owned materialization.
Formatting writes to the supplied Formatter; a caller materializing its output
must separately count and admit that actual output and replacement layout.

PartialVersion only observes the fully parsed requirement's length and, when
that length is one, its single comparator. Requirement parsing therefore retains
the first comparator and count rather than constructing the upstream vector.
Every later comparator is still parsed before deciding that count is invalid.
Upstream recursion parses comparators left-to-right, returning errors before its
single final allocation/unwind. The contained loop preserves that order and
checks the same 32-comparator guard at the same post-comma position. It does not
short-circuit after seeing a second comparator. The empty/star requirement has
zero comparators. No intermediate comparator spelling acquires parsed authority.

Version parsing advances monotonically through the input. Its numeric fields
fail on u64 overflow, while identifiers advance one admitted byte at a time.
Partial parsing performs at most one full-version pass, one complete requirement
pass and two failure-classification substring scans. The requirement loop has
at most 32 comparator iterations and never revisits a consumed comparator.
The identifier/comparison routines scan each visited segment a fixed number of
times; their work is linear in compared byte lengths, independent of numeric
identifier magnitude. They never parse arbitrary-length numeric identifiers
into a machine integer for comparison. Native-stack depth is constant throughout.

Name parsing performs a complete monotone character pass. Profile reserved checks
compare streamed lowercase characters against 21 fixed ASCII words and one
five-character prefix. Each check stops after the fixed word length plus one
output character, so the extra work is fixed after the initial character pass.
The stream produces at most three chars per input char and owns no expanded
string. Input limits and the finite source program bound this work; no filesystem,
candidate-comparison or hashing charges are invented.

## Executed dependency/cfg closure

The only non-std production dependency reached by this slice is unicode-ident
1.0.24, specifically `is_xid_start` and `is_xid_continue` (`src/lib.rs:261–282`).
Its archive checksum is
`e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75`, matching both
the current product lock and pinned Cargo lock. Direct archive-member hashes
were compared with the local extracted files:

| Member | SHA-256 |
|---|---|
| `unicode-ident-1.0.24/src/lib.rs` | `078912e7c64ba3ea1081b88c2610deba31eeb057c28f9d78e929db4856b7774f` |
| `unicode-ident-1.0.24/src/tables.rs` | `369c476ba91a39fce55563b692a3f598daf7f026cfe0a3796cfed1102fb3b190` |

The package has no features, no build script and no dependencies. Its library is
unconditionally no_std. Both functions use the same unconditional immutable
ASCII masks/trie/leaf tables; there is no feature/cfg-selected alternative or
allocation. This extraction calls their existing safe API and adds no unsafe
code or exception. Dev-only benchmarks/test dependencies are outside the closure.

Profile comparisons also use Rust's char classification and lowercase mapping.
The governed toolchain is nightly-2026-07-11, rustc
`375b1431b7d89d1c2e2bc168c011848ae12b7d14`, as recorded in
`importer/src/cargo/toolchain.rs:18–22`. Its installed rust-src
`library/alloc/src/str.rs:433–467` uses the same per-character lowercase table as
`char::to_lowercase`, except contextual Greek sigma. Both possible sigma outputs
are non-ASCII, so neither can match any retained ASCII reserved word or prefix.
This proves the streamed comparisons equivalent without allocating `to_lowercase`.
That inspected alloc source has SHA-256
`09d3899280a0086cddf19479873931b9475e0d6b7a2e0b7f74644ea538108068`.
The compiler's Unicode classification/mapping provenance remains part of the
existing governed toolchain identity; an unrelated compiler is not equivalence
evidence. No additional ASCII-only restriction is applied to valid names.

## Focused evidence

The scalar unit filter is `input::membership::manifest::scalar`. Tests call
the actual contained functions. Direct semver 1.0.28 comparison covers full
versions, long borrowed identifiers, complete projections, full and precedence
ordering, and requirement acceptance/count/first comparator. Additional cases
exercise Cargo PartialVersion/RustVersion expected branches, exact structured
error precedence, Unicode namespace/name differences, profile empty acceptance,
reserved Unicode casing and streamed casing versus std's full lowercase result.

Tests are source-ready; the implementation agent did not run Cargo, rustc,
native fixtures, formatting or commits. Root owns execution and records outcomes.
The independent complete Cargo-schema reference must exercise the integrated
scalar visitors, including call-site trimming, spans, typed-key collisions and
the original account with nonzero prior usage. Scalar checks alone do not prove
that complete typed interpretation or any later authority boundary is ready.

Root execution passed all ten scalar tests, both in the initial focused run and
in the final 39-test manifest filter. The tests compare actual pinned semver
behavior; Cargo-specific typed projections remain covered only to the stated
reference-comparison scope. No complete Original or membership is certified.

## Exact version diagnostic rendering

`version/diagnostic.rs` adapts semver 1.0.28 `src/error.rs` for the reached
Version/VersionReq rejection variants. Display writes directly to its formatter;
the enclosing caller must admit any retained diagnostic string. Position names
and debug character quoting match the pinned implementation, including its
explicit NUL spelling. The upstream IllegalCharacter variant belongs to the
standalone prerelease/build parsers, outside these two reached grammar paths.

Two differential tests compare rendered failures directly against the pinned
crate, covering numeric, identifier, wildcard, delimiter, comparator-count,
control-character and Unicode failures. All 17 scalar tests passed offline on
nightly-2026-07-11 with rustc-driver,test-fixtures. Independent read-only review
found no blocker; format and diff checks passed. This adds error rendering only,
not Cargo's dependency context or complete dependency construction.
