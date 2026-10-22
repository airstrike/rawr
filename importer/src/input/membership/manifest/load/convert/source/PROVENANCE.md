# Bounded source identity records

Pinned Cargo 59800466c5c41c444d264b1010b4d57e85a7117f:
`core/source_id.rs:101–133,582–607,674–701` and
`cargo-util-schemas/src/core/source_kind.rs:5–97`.

This is a candidate-record and interning primitive, not a completed SourceId
constructor/manifest/source-selection program. The candidate retains an owned
fetch Url, a separately transformed Canonical, owned literal selector/label
strings and every precise variant. Updated versions transfer existing validated
scalar components into owned pre/build identifiers; there is no version reparse.
Registry versus sparse registry derives solely from the admitted URL's exact
`sparse+` prefix, so callers cannot supply an invalid sparse/kind combination.
Literal Git selectors are copied without query decoding.

Internal sorted equality includes kind, every precise field, and canonical URL.
Its order is private and may use lexical prerelease/build ordering because only
exact equality is observable here. Public comparison excludes precise and follows
Cargo's explicit SourceKind order, then canonical URL. Full handle equality adds
the stable owner scope and retained record identity; foreign owners cannot alias.
No hashing adapter is exposed. Pinned observable hashes must be implemented at a
reached caller, independently of private ranks (Cargo hash discriminants differ
from public SourceKind order).

Externally owned Slots retain immutable records; the owner carries its sole
writer and an admitted sorted Buffer of references. Scope borrows prevent moving
or reopening History while handles remain live. An incumbent hit retains its
entire first representation; later labels are not filled in. All candidate
payload copies and canonicalization precede lookup, including hits. Each actual
comparison and visited string pair is admitted. Index growth conservatively
admits all existing index entries before push, and insertion admits the shifted
suffix separately. Slots and Buffer admit their full requested physical layouts
before allocation. No failure refunds spent work; interrupted insertion latches
the owner across a replacement Session. Teardown is flat/iterative.

The precise argument currently exercises complete internal key representation;
it is NOT Cargo's `with_precise` operation. That operation clones an existing
Inner and canonical URL without calling CanonicalUrl::new again. Its future
constructor must preserve that distinct source sequence. Likewise caller-owned
URL construction/clone order, successful per-GlobalContext crates.io memoization,
actual process entry/prelude/history, inherited parent identity, and first-winner
concurrency remain explicit integration obligations. Empty History establishes
storage only, never absence of producer incumbents or completed current authority.
Independent Cargo children require independent histories; nested manifests of
one process must not reset the owner.

Tests cover canonical-equivalent originals and label first-wins, all represented
precise variants/public equality separation, version build identity, sorted
insertion, derived sparse kind, literal selectors, foreign-owner refusal, and
every admission failure with original history and a fresh Session retry. These
are deterministic representation tests, not a complete producer or Gallery proof.

Validation: all six source tests passed offline on nightly-2026-07-11 with
rawr-import library features rustc-driver,test-fixtures. Independent review's
comparison-refusal latch finding was corrected and verified with exhaustive
comparison-admission refusal followed by fresh-Session access. Identical handles
skip key comparison admissions, matching Cargo's pointer fast path. Format and
diff checks passed. No CLI, release qualification, or Gallery run occurred.
