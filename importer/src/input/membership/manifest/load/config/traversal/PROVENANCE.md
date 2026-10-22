# Normal recorded configuration traversal

Cargo 59800466c5c41c444d264b1010b4d57e85a7117f supplies
`src/cargo/util/context/mod.rs:1349–1376,1691–1718` and
`crates/cargo-util/src/paths.rs:433–473`. The implementation composes existing
recorded discovery, Files include loading, Tree merging and Session host queries.
It supplies no native producer, complete source membership or reuse authority.

The built-in root precedes home ownership and ancestor initialization. Sealed
`__CARGO_TEST_ROOT` Unicode values override the explicit search stop, including
empty values; missing/non-Unicode values retain the explicit stop. Path component
equality controls the inclusive stop. Each visited ancestor performs selection,
complete file/include loading and non-forced merge before canonicalization.
Normal file loads reconstruct Files with a fresh seen buffer while keeping the
path/document writers, so prior merged origins remain alive and repeated
includes across different normal loads do not become false cycles.

Seen directories retain canonical success or native-error raw fallback. Complete
ancestors precede home canonicalization; both canonical home and raw home are
checked with Path equality. Home fallback copying remains eagerly reached after
canonicalization, matching unwrap_or. Only a positive recorded native errno or
the local native NUL conversion selects raw fallback. Missing/resource/malformed
observations remain terminal. Relative query/CWD association is still incomplete
and cannot claim a pinned semantic rejection, source exclusion or cold-only lane.

Borrowed scans/comparisons, path ownership, seen storage and merges use the same
original Session account. Seen lookup is a directly accounted linear collection;
there is no hidden HashSet allocation or uncharged comparison work. Disposal
does not refund spent work. Producer qualification, complete
before/actual-successful-cold/after/current association and original limits remain.

The local representation issue is how to retain a small ancestor-directory set
without introducing unaccounted table ownership. This is an implementation choice
within the established original-account boundary, not new architecture.

| | **A — linear accounted Buffer** | B — bounded hash set |
|---|---|---|
| Work | **Each comparison explicitly admitted; quadratic in ancestor depth** | Expected linear traversal, with hashing and collision work to contain |
| Ownership | **Existing fallible Buffer layouts and teardown** | Additional bucket growth and capacity accounting |
| Equality | **Borrowed standard Path component equality** | Must preserve exactly the same Path equality/hash relationship |
| Output | **Membership checks only; no iteration-dependent output** | Same result if containment and equality are preserved |

**Recommend A for this local collection.** Typical filesystem ancestor depth is
small; this expectation grants no exemption or new depth cap. Every reached path,
comparison and retained directory remains charged under the original graph/work/
ownership limits, including unusually deep input. A bounded hash set is preferable
only if actual depth/cost evidence warrants its extra containment work. Neither
choice may use byte equality in place of Path component equality.

Focused recorded composition tests cover include precedence and fresh seen,
load failure before missing later queries, canonical/raw duplicate home with
component equality, native fallback, missing canonical refusal, and environment
override/non-Unicode inclusive stop. They are not native or product qualification.
