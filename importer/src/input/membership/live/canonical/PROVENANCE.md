# Unconnected canonical observation acquisition

In this implementation checkpoint, `live::canonical` calls
one private `canonical::System` seam, implemented by `native::System` with the
safe `rawr_canonical::canonicalize` interface. Importer remains unsafe-forbidden.
There is no recipe/current-admission routing, child, callback supplied by an
external consumer, alternative canonicalizer, initialization token or retry.
Native qualification and complete before/actual-cold/after/current association
remain separate prerequisites; merely adding the dependency is not activation.

The query uses the existing absolute NUL-free observed::Path domain and retains
exact native spelling. This producer does not add a PATH_MAX input restriction,
normalize components, convert Unicode, change cwd, inspect source type or add a
supported-source filter. Callers still owe the existing source/CWD binding; a
malformed observation-domain request is not a pinned source-rejection witness.

The native spelling owns precisely input length+1 bytes, admitted before reserve
and copy; CStr borrows that vector without shrink/CString conversion. Checked
overflow is terminal. The initialized 1024-byte transfer array is fixed stack
storage. A successful borrowed CStr is copied only after admitting the actual
returned path length. No output-length guess, post-allocation debit or allocation
for error output occurs. Partially written error bytes are never inspected.

Fixed transfer storage follows directory/SAFETY.md and the existing live
file scratch precedent. Fixed array/native stack scratch and kernel-internal work remain
distinct from heap layouts. The one actual canonical operation prospectively
spends one original graph.paths(1) debit under the existing observation mapping;
this does not itself qualify transitive native work or hidden variable ownership.
Those installed obligations remain with later qualification.

| Storage/operation | Original prospective charge |
|---|---|
| Exact query path | Existing string limit, one owned item, exact bytes |
| Native terminated input | One owned item, exact query length+1 bytes |
| Canonical invocation | One graph path debit before System call |
| Successful output | Existing string limit, one owned item, actual returned bytes before copy |
| Transfer array and inline record | Fixed stack storage; no invented heap charge |
| Later collection/encoding | Caller separately admits actual retained container/copy work |

Only `rawr_canonical::Error::Native` carrying an actual positive raw OS code
becomes `Value::Canonical(Err(code))`. Code107 is retained without conversion
through nix::Errno; EINTR is not retried. Missing/nonpositive raw codes, pointer
mismatch, missing termination, unsupported ABI and invalid successful native
path framing are outer terminal Encoding errors. None can select raw fallback.
Original resource errors propagate without a record; spent work is not refunded.
Successful return builds only a checked inert observation, not a freshness receipt.
Shared observed::Path admission now uses Allocation::limited with the existing
`membership native path bytes` label and ordering. This ensures an input or
returned-output string-limit refusal latches the original allocation account;
the previous free limit check did not. No canonical-only preflight is added.

Deterministic tests exercise the same producer through a private System seam;
they perform no native calls. Coverage includes exact native-byte input/output,
initialized transfer storage, positive/native107/EINTR error retention, malformed
and unsupported outcomes, all cumulative ownership floors with prior usage,
path-work floor, output refusal before a second attempted acquisition, invalid
queries and an input longer than the transfer array. Added shared string-limit
regressions use limit2: an oversized input must stop a later fitting query;
an accepted `/p` input with oversized `/resolved` output must stop the next
native call despite ample aggregate capacity. The byte-refusal test leaves
enough unused capacity for another query/input, distinguishing the latch from
mere exhaustion. Root verified all nine tests, including the native fixture
below, after the independent review correction. The implementation agent ran
no tests.

One macOS64 native adapter fixture calls live::canonical through native::System
for root, a test-owned absolute repeated-component spelling, an absolute symlink
and a missing path. It retains exact query bytes and compares native bytes/errno
to std under a quiescent temporary fixture. This serial integration test does
not establish concurrency, native qualification, current membership or Gallery
performance. All filesystem writes are confined to its exclusive temp directory.

Suggested root check: the importer library filter
`input::membership::live::canonical`, plus scoped formatting and the existing
live/canonical transport cohort. The rawr-canonical serial native suite remains
separate qualification evidence, not part of these deterministic producer tests.
