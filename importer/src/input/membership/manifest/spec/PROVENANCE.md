# Complete package-spec interpretation

The caller in `../spec.rs` follows Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`,
`crates/cargo-util-schemas/src/core/package_id_spec.rs:80–190,228–244`.
Exact source hashes and URL execution closure are recorded in `URL_CLOSURE.md`.
The source is MIT OR Apache-2.0; the retained Cargo licenses are
`importer/reference/manifest/LICENSE-MIT` and `LICENSE-APACHE`.

The complete entry preserves the raw `://` guard, first URL semantic fallback,
compound-source query rules and second parse, then fragment, partial-version and
package-name interpretation. Only the rejecting non-URL filesystem
diagnostic suggestions are omitted. Only a semantic failure of the actual
second URL parse is classified as terminal source failure; a resource refusal keeps its first
cause and cannot become fallback.

`Spec` owns its converted name, optional typed partial version, normalized URL
and source kind. No value borrows a temporary URL. Git query values already owned
by decoding move into the result; borrowed values are admitted and copied before
query mutation. Fragment ownership survives removal and optional reparse. The
second parse borrows the first normalized serialization without copying an
obsolete URL solely to preserve its lifetime.

Derived Spec ordering compares name, optional typed partial version, normalized
URL serialization and optional source kind. The reached source kinds use Cargo's
manual order (Path, Registry, SparseRegistry, Git), while Git references use
their declaration order (Tag, Branch, Rev, DefaultBranch). Unreachable local
registry/directory kinds do not alter relative ordering of reached kinds.

All new strings admit their actual byte layout before fallible allocation and
retain the resulting String without shrinking. Every operation shares the
caller's Session; no alternate account or recovered semantic trial clears its
terminal state. Converted Spec data grants no package membership, loader,
workspace, current-node or release authority.

Focused execution evidence is recorded in the MAP tracker when the complete
caller and its differential reference tests pass; this source document alone
does not assert execution or qualification.

# Contained Git query component

The query component interprets the Git-reference query operation used inside
PackageIdSpec. Its existing source correspondence, accounting and focused
evidence below remain valid; complete spec interpretation is described above.

## Source correspondence

- Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
  `crates/cargo-util-schemas/src/core/source_kind.rs:87–119`, source SHA-256
  `f3b4cc73dabe93722aa64d6397474102399b0f7d17cf8daa37ff70a888589837`. `GitReference::from_query`
  folds ordered decoded pairs with last-recognized-pair replacement. `ref`
  aliases Branch; unknown keys leave the current reference unchanged.
- form_urlencoded `1.2.2`, crate archive SHA-256
  `cb4cb245038516f5f85277875cdaa4f7d2c9a0fa0468de06ed190163b1581fcf`,
  `src/lib.rs:39–104`, member SHA-256
  `766b5d679064e01f7e6cce6f127a23885f79806ce3bccc49e4fe41933b83fd8d`. The cached member was
  compared byte-for-byte with its checksum-verified archive member. Empty `&`
  components are skipped; each component splits at its first `=`; plus maps to
  space before percent decoding; malformed escapes remain literal. UTF-8 errors
  are repaired using the same standard-library lossy chunk grouping.
- Cargo and form_urlencoded are MIT OR Apache-2.0. The project retains those
  licenses at the repository root. Copyright 2013–2016 The rust-url developers
  applies to the form decoding source from which the contained transitions derive.

`Bytes` fuses plus replacement and percent decoding without transforming a plus
produced by a percent escape. Keyword equality is checked against decoded bytes:
lossy UTF-8 can insert only U+FFFD, never an ASCII keyword character. Thus the
comparison has the same outcome as fully repaired decoded key strings.

Only the final recognized pair contributes retained output. Decoding unknown or
overwritten values cannot cause semantic rejection; form decoding is total.
Keeping the final borrowed slice and materializing it once preserves the selected
kind/value while eliminating nonexistent intermediate allocations. Source-only
values are not promoted to authority: the resulting Reference has private fields
and is born from this exact transformation (`smart-constructor-newtype`).

## Ownership and work

The shared interpretation Session must be active before scanning. Unchanged
final values borrow input storage; changed values allocate an exactly counted
byte vector. Invalid decoded UTF-8 needs a second, exactly counted repaired
vector. Each actual vector/string uses the original per-string allowance, one
item, its full requested Layout and fallible reserve before copying. Both
layouts remain spent. The output typed slot is owned and charged by its caller.
No allocator bonus is used to authorize a later allocation or growth.

No ownership refusal selects another pair. Every layout/admission/allocation
failure latches in Session and is returned as Refused; even a later entirely
borrowed query refuses before scanning. There is no semantic trial here.

Pair splitting is linear in admitted query bytes. Each key comparison visits at
most the fixed keyword length plus one byte, for four fixed keywords. Final
value comparison/count/copy and possible UTF-8 repair use a fixed number of
linear passes. `Bytes::count` cannot exceed its source slice length. Layout and
repair arithmetic are checked. Neither recursive traversal nor variable sort
scratch is used. Drop is flat. Pure decoding consumes no graph path/hash/decode
or candidate-comparison event.

## Focused checks

The focused test module compares results directly with pinned form_urlencoded
1.2.2 and Cargo's exact reference fold across named cases, all 256 byte escapes,
and 2,048 selected two-byte sequences. Additional tests exercise borrowed output,
exact cumulative decoded/repaired layouts, every prospective refusal and a
nonzero original-account byte floor with exhausted graph work counters.
These tests are not complete typed-manifest differential or URL qualification.

Root execution passed all five focused query tests, including 2,560 generated
pinned-decoder comparisons in addition to named cases. Independent focused
source review found no P0/P1 (confidence 0.98). This review covers the query
transformation and its account; it does not qualify the enclosing URL parser.
