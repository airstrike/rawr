# Private observer contract

This executable is an explicitly built, private qualification artifact. It is not
a supported product sampler, ordinary CLI dispatch path or installed binary.
The current candidate tuple is aarch64-apple-darwin, the repository's pinned
nightly, libc 0.2.189 from Cargo.lock, and the explicitly selected CLT macOS26.2 SDK.
The separate observer build receipt records its exact invocation, source tree,
lockfile, compiler/Cargo, artifact, selected SDK root and header/settings identities.
The producer supplies SDKROOT and exact encoded linker sysroot arguments. Receipt
validation rechecks those files; a different SDK or target needs another reviewed
compatibility contract, not fallback selection. Ordinary/evidence builds keep
their existing SDK policy. No native call was used to establish this source contract.

`native.rs` independently defines only the SDK's proc_bsdinfo layout (136 bytes,
alignment8; PID12, PPID16, UID20, PGID100, birth seconds120/microseconds128), with
compile-time size/alignment/offset assertions. The selected SDK headers are:

- libproc.h SHA256 `246d87709fc6b9157ce5cf3c475656ac48e0e1ae8bbdc46cf45acd34294448cd`
- sys/proc_info.h SHA256 `e427fa96b348537b21552b9de71e01039410bcad2cedee5584c5e5fddafd70fc`
- SDKSettings.json SHA256 `95882a9cea02396e76f3e406a5728269b2a1a11bcd241430a84c9e63d09a6cd3`

The contract source is Apple's pinned [xnu-12377.1.9 libproc wrapper](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.1.9/libsyscall/wrappers/libproc/libproc.c),
[proc_info.c](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.1.9/bsd/kern/proc_info.c),
[vfs_subr.c](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.1.9/bsd/vfs/vfs_subr.c), and
[vfs_cache.c](https://github.com/apple-oss-distributions/xnu/blob/xnu-12377.1.9/bsd/vfs/vfs_cache.c).
The wrapper preserves positive proc_pidinfo counts and maps syscall failure to0;
the kernel copies and reports the entire BSD structure. Only exactly136 is consumed.
The requested positive PID, live status, nonzero ancestry/group and valid birth
fraction are required. Zero/partial/oversized results reject without reading fields.
`proc_pidpath` returns strlen excluding NUL. A zero-initialized4096-byte buffer is
used; successful count is1..4095 and the first NUL must be exactly at that count.
Bytes after that terminator are irrelevant native scratch. Raw path bytes are
retained; invalid UTF8 is not replaced. Thread-local errno is cleared immediately before each call and captured
immediately on unsuccessful calls; success never reports stale errno. All pointers refer to initialized live stack storage; no
pointer/reference survives a call. Calls may block and therefore live only here.

BSD identity brackets each path observation; identity, artifact file custody and
CG row are rechecked at visibility and immediately before the single AX action.
The authenticated receipt now carries the executable digest/size already sealed
by BuiltArtifactProof, avoiding a new product hash. The observer's independently
read file must match that seal as well as the receipt's exact path digest. This is
a non-snapshot association, conditional on its exact no-exec continuation,
not an executing-vnode or exec-generation proof.

Transport is a launch/build/sequence-bound Unix socket inherited directly as
stdin, with one pending command and one partial response. Each body is bounded
by the existing64KiB output ceiling; outgoing header adds4bytes. Serialization
and pending copies can overlap (at most2×64KiB+4bytes); the receiver has one
64KiB buffer, and decoded finite records may overlap their bounded wire frame.
Reserves are fallible before growth. Parent and helper stderr each use the same
existing64KiB ceiling. ps stdout/stderr each have that ceiling; decoding has at
most3 selected rows. The native path uses4096 stack bytes and at most4095 owned
bytes. CG uses its existing4096-row bound and AX its existing safe dependency.
These are finite custody descriptions, not a new aggregate budget or exclusions.

The parent owns the original t0/deadline. It receives the first CG timestamp,
not response-arrival time. The product CPU/RSS series retains the same ps fields,
PID selection and sum/high-water interpretation; one extra selected row records
observer samples separately. Observer first/last sample times, sampled CPU/RSS,
actual ps child count and spawn/reap times are retained in private reports. These
are sampled quantities, not inferred complete lifetime CPU or an overhead
subtraction. ps helpers share the observer's process group. Successful samples
require ps exit and both EOFs. Failure reports retain attempted kill/reap outcomes;
if parent cancellation interrupts them, that unknown retirement is rejected.

Normal completion requires a final authenticated product capsule plus EOF,
successful CLI reap, exact window disappearance, terminal observer protocol,
observer stderr/protocol EOF and successful observer reap, all under the original
post-press10s grace. Final capsule decoding and parent observer reads are bounded
nonblocking polls with no worker/Drop joins. Failure signals only still-owned
CLI and observer children after checking their cached/current exit status, then
polls cleanup; signal success is not retirement. The receipt-derived runner PID
never authorizes a signal from this parent. This gap exists before authentication
and after the CLI reaps its runner but before log joins and CLI exit: a live CLI
does not prevent reuse of the runner's PID/PGID. Missing/unreaped or unknown
descendant outcomes remain incomplete. This checkpoint cannot authorize live
qualification until the remaining runner lifetime/cleanup ownership gap closes. No retry or second press follows
an ambiguous action. Failed transport uses a separate raw drain solely to retain
actual EOF; it cannot regain acceptance.

Only the active release/descriptive launch is connected here. Three lanes remain
required; ordinary/inactive external parity, per-ordinal no-exec implications,
actual installed-library/kernel compatibility, permissions, freshness, native
interference, timings and bounded retirement require the existing consolidated
qualification. This source contract grants no diagnostic/native probe or accepted
sample and does not relax that gate.
