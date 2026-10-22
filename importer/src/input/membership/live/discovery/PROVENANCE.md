# Closed live configuration discovery projection

This private producer returns only an independently
checked `observed::Record`; neither live acquisition nor codec/Host replay supplies
current-source authority. Actual cold Cargo still executes warning output and
must succeed. Complete before/cold-success/after/current association, loading,
canonicalization and manifest-driver connection remain separate obligations.

## Exact retained source

Cargo 59800466c5c41c444d264b1010b4d57e85a7117f
`src/cargo/util/context/mod.rs:1644–1689` constructs both legacy and `.toml`
paths before acquisition. same-file 1.0.6 `src/unix.rs` opens read-only, then
fstats that File; equality compares device and inode only. Pinned
nightly-2026-07-11 std `sys/fs/unix.rs:1390` uses `cvt_r(open64)`; its
`sys/pal/unix/mod.rs:245–255` retries Interrupted. The descriptor metadata call
at Unix filesystem line 1409 uses one-shot `cvt(fstat64)`.

The native seam reuses existing `live::native` nix 0.27.1 operations:
O_RDONLY|O_CLOEXEC, one-shot fstat/stat and close. Raw errno is read immediately
instead of casting nix::Errno, preserving codes omitted from that enum (such as
Darwin 107). Open retries every EINTR under a fresh original path-work debit;
fstat and fallback stat do not retry. Nonpositive native errno is a terminal
encoding error, never invented fallback data. Resource/missing-observation
errors propagate separately. No file-body read, metadata preprobe, regular-file
test, nonblocking flag, symlink suppression, Unicode conversion or command exists
in the discovery syscall interface.

Pinned std `path.rs:1341` appends a slash only when the existing directory's last
byte is not a slash for these fixed relative basenames. Unix has no Windows
prefix/verbatim branch, and neither fixed filename is rooted or empty. The
producer preserves repeated separators, dot/dot-dot and native bytes verbatim.
Existing observation Path validation remains absolute and NUL-free. Joined
spellings do not receive an invented whole-path cap or component normalization.

## Ownership and prospective accounting

Issue: both source spellings are constructed early, whereas native C-string
conversion occurs only at a reached open or fallback stat.

| | A — terminate both buffers early | **B — convert at reached operation** |
|---|---|---|
| Storage | One allocation per spelling | **Separate exact spellings and reached conversion** |
| Refusal order | Eager second conversion ownership | **Preserves branch-specific conversion timing** |
| Work | Fewer copied bytes | **Charges actual extra buffers/copies prospectively** |

Recommend and implement B as the routine storage realization.
The original allocation account pays the retained query directory, both exact
joined spellings, and each reached length-plus-one native input before each
fallible exact reservation/copy. The second conversion does not exist for
legacy success with warn=false. One converted input spans an open retry loop;
it is dropped before fstat. No CString/PathBuf constructor, hidden conversion,
capacity-based bypass, budget reset or new allowance is used. These are actual
adapter allocations, not a claim of equality to std's heap allocation strategy.

The original graph account reserves one possible descriptor cleanup before each
reached open loop; a failed open leaves that reservation spent. Every open
attempt, descriptor fstat and fallback stat receives its own prospective debit.
Fixed two-slot ownership and scalar transcript construction need no variable
result allocation. Path copying/scanning and comparison remain bounded pure work
over the actual admitted native bytes; no invented hash/decode/candidate work or
omitted-renderer charge/refund is added.

Successful descriptors enter one private two-slot scope owner, never escape,
and remain independently owned. A failed fstat closes its descriptor before
fallback or the deprecation boundary. With warning enabled, successful first
and second acquisitions overlap through device/inode comparison and the logical
both-name warning boundary. The second source scope closes before the first.
Second-open or second-fstat failure reaches the deprecation boundary with only
the first handle alive. Equal identities and warn=false emit no logical warning
event. This projection omits all terminal-only rendering/output work and does not assert
that hypothetical current warning delivery would succeed.

Cleanup is prepaid and performed once even after any later first resource
refusal. Slot removal occurs before one-shot close, so Drop cannot retry it;
close errors are ignored like source File destruction. A private test diagnostic
hook only records the logical boundary and its live slots; production inherits
an empty implementation, with no public callback or synthetic success value.

## Focused supplied evidence

Tests are supplied for root execution, not reported as run by this implementation.
Deterministic fixtures execute the same acquisition state machine through its
private native seam, checking exact reached event order, no eager fallback,
two-handle overlap, warning placement, fstat-failure close ordering, positive
native errno versus missing/invalid outcomes, all metadata kinds and fixed-name
Path::join correspondence. Every graph-work cut and every byte/item ownership
cut across seven branch/retry scenarios checks first refusal and mandatory
cleanup. Native conversion is omitted on unreachable second branches; second
conversion refusal after first acquisition still closes the first descriptor.

A separate source-shaped std fixture compares complete transcripts for both
families and warn modes over absent/legacy/toml/both, hard-link/symlink identity,
directories, character-device fallback/open, dangling symlink, non-directory
parent and native bytes. Existing invalid-UTF8 directory coverage depends on
fixture creation; only the previously observed EPERM is explicitly recorded as
unexercised success coverage. This is a quiescent fixture comparison against
source-shaped std open/metadata operations, not a full Cargo differential or
atomic filesystem snapshot. No arbitrary FIFO is opened in a native fixture.
