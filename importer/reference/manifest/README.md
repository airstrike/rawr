# Pinned original-manifest reference

This test-only harness executes the direct Cargo 59800466 operation:
`DeTable::parse -> Deserializer::from(document.clone()) -> serde_ignored ->
cargo_util_schemas::manifest::TomlManifest`. It then projects every retained
public typed field and enum variant. It does not run Cargo normalization,
workspace loading, package selection, package commands, or a product parser.

The recorded reference run contains 130 cases: 80 accepted and 50 rejected.
All 72 resolved dependency entries passed the pinned lock check. A second locked
build/run after formatting reproduced the 271,503-byte output exactly (SHA-256
`4bdb6ccb690b4d8eb75a45895d04a659994198666e7c09cfdf24e123cc171b7b`).
See `observations.jsonl`, `features.json` and `execution.json`. This establishes
reference behavior; there is no complete product differential parity claim.

## Exact source and isolated execution

`cargo-util-schemas 0.14.2` comes from the verified Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`, not an older cached registry schema.
`provenance.json` records the archive URL/checksum, the complete schema crate's
source-member checksums, the Cargo manifest/lock and the direct caller. The
bootstrap materializes the unchanged complete Cargo archive into an owned new
directory. Keeping its root manifest preserves exact workspace inheritance;
the harness is a separate workspace beside the materialized source. It has no
product dependency or product workspace membership. Its committed manifest has
only relative paths. Upstream licenses are retained.

Python 3.11 or later is required. The bootstrap does not fetch, run Cargo, or
execute fixtures. Supply the archive recorded in `provenance.json`; the retained
archive can be reused. The following commands are for the root execution owner,
serially, using the existing offline nightly and one job:

```sh
python3 importer/reference/manifest/prepare.py prepare \
  --archive /private/tmp/rawr-p9-toml-supplement-cargo.tar.gz \
  --destination /private/tmp/rawr-p9-manifest-reference
cargo +nightly-2026-07-11 build \
  --manifest-path /private/tmp/rawr-p9-manifest-reference/Cargo.toml \
  --offline --locked --jobs 1
python3 importer/reference/manifest/prepare.py verify-lock \
  --destination /private/tmp/rawr-p9-manifest-reference
cargo +nightly-2026-07-11 tree \
  --manifest-path /private/tmp/rawr-p9-manifest-reference/Cargo.toml \
  --offline --locked --edges features
python3 importer/reference/manifest/run.py \
  --binary /private/tmp/rawr-p9-manifest-reference/target/debug/rawr-manifest-reference \
  --output /private/tmp/rawr-p9-manifest-reference-output.jsonl
```

The example paths are command arguments only; they are not compiled or recorded
as authority. Any fresh owned destination works. The bootstrap rejects an
existing destination and archive/member mismatches. Do not point Cargo at this
unmaterialized repository manifest: the relative `cargo/` dependency exists
only after preparation.

On the first build, Cargo may update the copied original Cargo lock to the
isolated package graph. That seed retains the original version choices. Do not
use `generate-lockfile` to throw away the seed. `verify-lock` requires every
resolved registry version/checksum to occur in the original lock, requires
exact singleton interpreter packages, and permits only the two intended local
packages. A mismatch is a preparation failure requiring root inspection before
reference observations are accepted. No equivalence exception is built in.
After successful verification, copy the generated isolated `Cargo.lock` beside
this README. Subsequent bootstraps use that reviewed lock, and subsequent
builds/runs must include `--locked`.

Root may need to acquire missing registry archives by their pinned lock
checksums before offline compilation. In particular, the actual schemas use
jiff **0.2.31**, default features disabled, with `std,serde`; an older cache is
not a substitute. The exact jiff and jiff-static archives were checksum-verified for the recorded
run and feature inspection. Cargo's
schema optional `unstable-schema` remains disabled. Root must inspect the actual
feature graph (`features.json` records the executed reference resolution):
relevant TOML features are `std,serde,parse,display,preserve_order`,
with the exact parser, Serde, Serde derive, serde-value, serde-untagged, SemVer,
URL and other active dependency versions. This harness claims neither whole
Cargo feature equivalence nor equivalence with the separate product graph.

`run.py` creates a fresh owned cwd, with a `probe/existing` file and the Unix
single-component `probe\existing` spelling; `probe/missing` is absent. This
makes the actual upstream exclusively diagnostic existence branch repeatable.
Only this temporary cwd's pathname/URL prefix is replaced by `${FIXTURE_ROOT}`
in error message/display. `diagnostic_cwd_elided=true` identifies such a row.
Semantic projection and error spans are unchanged. The runner captures the
built binary directly; it does not invoke Cargo. It checks JSON and unique IDs
and publishes the final artifact atomically. A nonzero binary exit instead
writes an `.failed` artifact with the actual observations and returns failure.
The output reports byte count and SHA-256. Never use a failed artifact as proof
that fixture expectations passed.

For listing or a focused observation:

```sh
/private/tmp/rawr-p9-manifest-reference/target/debug/rawr-manifest-reference --list
python3 importer/reference/manifest/run.py \
  --binary /private/tmp/rawr-p9-manifest-reference/target/debug/rawr-manifest-reference \
  --output /private/tmp/rawr-p9-manifest-case.jsonl \
  --case datetime-marker-first
```

The supported interface selects only compiled-in fixtures. A direct binary run
outside `run.py` is useful for debugging but can produce ambient, nonportable
path suggestions. Preserve the wrapper for the committed portable artifact.

## Projection and comparison contract

`src/project.rs` destructures all public schema structs without `..` and matches
all enum variants without a catch-all. Adding a field or variant therefore
requires a projection update at compile time. It never calls the schema's
`Serialize` implementation. Aliases remain distinct original Rust fields;
`Option` is explicitly tagged `Some` or `None`. Inheritable values, detailed and
simple dependencies, lint level/config, build modes, trim-path modes, and other
unions retain explicit type/variant tags. StringOrVec's wrapper contains the
actual converted vector; the typed object has already discarded whether its
source was a string or a one-element vector.

BTree maps are ordered arrays of `[typed_key, typed_value]` entries, reflecting
the actual typed map iteration and replacement. Generic TOML tables similarly
retain the actual `preserve_order` iteration. Structural JSON object member
order is not significant; order-sensitive typed containers are always arrays.
Profile package keys expose complete PackageIdSpec name, PartialVersion,
normalized URL and SourceKind/GitReference components, including optional
prerelease/build components. They do not rely on potentially collapsing Display
spellings. RustVersion's accessor exposes all three private version components.
The sole private inherited-field member is a zero-sized WorkspaceValue whose
only successful deserialization means true; that exact invariant is labelled
in source and provenance.

Generic TOML float payloads include all 64 IEEE bits as 16 hex digits, sign and
finite/NaN/infinite classification. JSON never carries a NaN/inf number. Generic
integers and typed numeric fields use exact JSON integer tokens; consumers must
use a lossless integer parser, not a JavaScript Number round trip. Datetimes
retain date/time/offset components and their optional seconds/nanoseconds,
including `Offset::Z` versus `Custom { minutes: 0 }`, plus upstream Display text.
PathValue retains UTF-8 and `OsStr::as_encoded_bytes`; these originate from an
upstream TOML String, not a native source-path observation.

Each JSONL row includes exact source, stable ID, purpose, source-derived optional
acceptance expectation, actual acceptance, and either complete `typed` state or
an error. `observed_unused` is the sorted set observed so far, while
`unused_callbacks` preserves callback order. On success Cargo's actual
`_unused_keys` field receives that sorted set and is also in the typed projection.
On failure the callbacks are diagnostic observations, not a returned partial
manifest. Flattened dependency/lint extras retain their actual converted values;
they are not replaced by an unused-name list.

Errors distinguish the document and typed phases and expose upstream message,
Display and applicable span as half-open UTF-8 byte offsets. The upstream typed
schema does not expose per-field success spans or discarded intermediate trial
state; this transport does not invent them. Compare the applicable first error,
span and governed bounded classification, not an imagined public trace of failed
trials. Exact source and paired fixtures make failure order observable.

## Corpus

The two TOML files fill every retained manifest/package/workspace/target/profile/
dependency/lint field family. `src/fixtures.rs` contains the complementary fixed
matrices and modest generated recursive inputs. `--list` is the exact machine
readable inventory. The corpus covers:

- Presence and both alias slots, including an invalid shadow alias; map order
  and sorted ignored paths versus callback order.
- Positional structs, profile container defaults, required positional Options,
  all 19 profile positions, direct array leftovers and complete syntax errors.
- Known-field and missing-field precedence versus delayed flattening; initial
  serde-value capture, sorted inheritance trial, successful false rejection,
  fallback and retained extras.
- IgnoredAny/direct TOML, generic TOML, serde-value, u32/i8 and custom i64 numeric
  domains; floating-point special values, signed zero, underflow and overflow.
- Native and synthetic datetime maps, first-key early returns, reordered marker
  keys, staged capture and flattened conversion. The eight special datetime
  cases initially have `expected_acceptance=null` to capture actual observations
  without pretending the unexecuted staging outcomes are known.
- Full staged trim-path String conversion before enum validation; enum unit
  payloads, all debug and trim-path modes, other scalar/sequence unions.
- Equal typed PackageIdSpec keys and URL-normalized equal keys; complete partial
  versions, Unicode/namespaces, valid URL/source/query forms and rejecting forms;
  controlled existing/missing diagnostic paths and key-before-value rejection.
- Modest nested accepted and failing profile trees. These are compatibility
  fixtures, not upstream recursion, allocation or teardown stress tests.

Expected acceptance is source-derived fixture intent, not a golden output.
When it disagrees with the actual pinned call, retain the emitted evidence and
inspect the exact source before correcting intent or product semantics. Deep
accepted/failing trees and refusal-point/account-chain tests required
belong in the later actual product interpreter tests, not a model recreated in
this reference. There is no product comparator yet, no current/cold association,
and no membership, support, budget or release qualification in these artifacts.
