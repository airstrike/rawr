# Private directory foundation review

This is the narrow directory boundary. After independent source/accounting
and installed-compatibility review, `store::bundle::names` now uses the reviewed
adapter in production. `bundle::paths` preserves prospective output/path charges
and the same original budget. The safe crate interface and this integration do
not alone qualify the complete replay host or establish release readiness.
Production unsafe code is confined to `src/sys.rs`; the importer, CLI, and process
protocol keep their existing unsafe-forbidden declarations.

## ABI evidence and scope

The exercised host is `aarch64-apple-darwin`, macOS 26.4 build 25E246. The installed
SDK is `/Library/Developer/CommandLineTools/SDKs/MacOSX26.2.sdk`. The compiler is
nightly-2026-07-11, rustc commit `375b1431b7d89d1c2e2bc168c011848ae12b7d14`.
The implementation is compiled for 64-bit macOS. Other targets return an
explicit unsupported I/O error; no opaque fallback can produce a proof. The
importer's existing supported compiler host remains unchanged. Other macOS
architectures have not been exercised by this slice.

The modern layout is from SDK `usr/include/sys/dirent.h:102–108`:

| Field | Offset | Width |
|---|---:|---:|
| inode | 0 | 8 |
| seek offset | 8 | 8 |
| record length | 16 | 2 |
| name length | 18 | 2 |
| type hint | 20 | 1 |
| name including terminating NUL | 21 | up to 1024 |

The name limit is 1023 bytes. The parser uses native-endian integer loads from
checked byte slices, with no struct cast or aligned-pointer assumption. Pinned
libc requires four-byte record framing. XNU's legacy-to-modern conversion uses
eight-byte records, but that conversion is not every filesystem's native
extended-record path; the parser does not impose an extra eight-byte restriction.

SDK `usr/include/dirent.h:174–181` deliberately makes public `getdirentries`
unavailable with modern 64-bit inodes. We do not call it. The installed
`usr/lib/system/libsystem_kernel.tbd` exports `___getdirentries64`; Mach-O adds
one underscore to C symbol `__getdirentries64`.

[Apple Libc telldir.h at 71bbe350ab79eef58113991d817ccc6165061a64](https://github.com/apple-oss-distributions/Libc/blob/71bbe350ab79eef58113991d817ccc6165061a64/gen/FreeBSD/telldir.h#L78)
declares a `size_t` result with arguments `int`, `void *`, `size_t`, and
`__darwin_off_t *`. This is the exact foreign declaration in `sys.rs`: 32-bit fd,
64-bit count/result, and an aligned writable 64-bit `off_t`. A returned all-ones
`size_t` is the error sentinel; errno is captured immediately. The installed
symbol was linked and called in the focused tests: fd -1 returned all 64 bits set
and EBADF, positive batches parsed successfully, and a later successful zero
return established EOF. No signed/int return-width guess from fdf is used.

The `fstatfs` binding and structure come from pinned `libc` 0.2.189. SDK
`sys/mount.h` and the Rust declaration agree on structure size 2168, flags offset
64, flags width 4, and MNT_UNION 0x20. `tests/abi.c` checks those declarations
directly against the SDK; the Rust tests independently check the libc offsets.
The actual mount probe also ran in every live directory fixture. `statfs` storage
starts zeroed (all its integer/array fields accept zero), so no assumption that
the syscall initializes reserved fields or padding is needed. Only successful
probes yield a mode; a failed probe is an error, not an ordinary-mode default.

## Descriptor, storage, and return lifetime

`Directory::open` uses a borrowed C string and O_RDONLY, O_DIRECTORY, O_CLOEXEC,
and O_NOFOLLOW. The path and its conversion storage belong to the caller; the
caller must admit them before constructing them. Open returns a newly owned file
description. `OwnedFd` closes it on every failure/drop path. There is no public
raw-fd, clone, dup, arbitrary-fd constructor, openat service, or lookup capability.

This independence matters because
[XNU getdirentries_common at f6217f891ac0bb64f3d375211650a4c1ff8ca1ea](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/vfs/vfs_syscalls.c#L10888)
can replace the descriptor's vnode when traversing a MNT_UNION lower directory.
The stream cannot retarget an unrelated caller lookup fd. The live tests check
CLOEXEC and independent positions for two fresh opens. They do not claim a live
union mount was exercised.

One initialized caller-owned 8192-byte array is borrowed mutably for each raw
read. The syscall receives precisely that writable extent and a live `off_t`;
neither pointer escapes or is retained. Only `buffer[..returned_count]` becomes
a batch, after a checked bound. Old bytes and the optional four-byte status tail
are never parsed. The buffer borrow outlives every returned entry; the compile-
fail doctest rejects refill while such an entry remains live. Entry names can
outlive the descriptor because they borrow the caller's buffer, not its fd.

A positive batch never establishes EOF from the kernel's tail flag. Pinned XNU
may require another call to traverse a lower layer after an upper layer's final
positive batch. Only a successful zero return terminates collection. EINTR is
a terminal I/O error with its original errno. Pinned XNU updates the descriptor
offset after the native readdir operation and before testing its error; an
interrupted read can therefore advance past entries whose bytes were not
returned. Retrying would allow a later EOF to authenticate an incomplete set.
Pinned Libc propagates that error, and this reader does the same. All errors
and admission refusal terminate the operation
without returning any partial `Names`. A terminal reader cannot be reused to
mint another EOF. The complete collector always opens a fresh stream, so a
previously consumed public raw reader cannot produce a complete `Names` value.

## Framing and pinned membership semantics

Every record and exhausted-batch probe is admitted before inspection. Each
record must have a full header, a positive aligned length of at least 24 bytes,
and an end within the returned prefix. An inode-zero record is discarded after
that framing check without interpreting stale/malformed name fields. A present
record must have a nonempty name within both the native field and its record,
one terminator at the declared length, no embedded NUL, and no slash. Name scans
are admitted before reading the name payload. Unknown type hints are preserved;
they never replace authoritative filesystem metadata. Malformed framing is an
error, including malformed trailing bytes after otherwise valid records.

The collector preserves the pinned behavior in
[Libc opendir.c](https://github.com/apple-oss-distributions/Libc/blob/71bbe350ab79eef58113991d817ccc6165061a64/gen/FreeBSD/opendir.c#L133)
and
[readdir.c](https://github.com/apple-oss-distributions/Libc/blob/71bbe350ab79eef58113991d817ccc6165061a64/gen/FreeBSD/readdir.c#L139):

- Dot entries and inode-zero records do not become names.
- Ordinary directories suppress whiteouts but retain duplicate occurrences.
- In union mode, inode-zero records are removed before duplicate selection.
  The first physical nonzero occurrence of each native byte name wins. A first
  whiteout suppresses the name even if a lower ordinary entry follows; a later
  whiteout does not suppress an earlier ordinary entry.
- Sorting by `(native bytes, physical ordinal)` reproduces stable first-winner
  selection. A second in-place ordinal sort restores the original visible order.
  The optional consuming `Names::sorted` supplies canonical native byte order.

No lossy UTF-8, Cargo-private exclusions, filesystem-shape exclusions, or
fallback reader is introduced. The collector's result describes names only;
source/path/type/content consistency checks remain the caller's responsibility.

## Prospective resource seam

`Admission` has no counters, limits, reset, fallback, or graph dependency. The
caller must keep the same original cumulative authority through collection and
later canonical sorting. The foundation exposes distinct prospective events:

| Event | Operation admitted before it occurs |
|---|---|
| Open, Mount, Read | Actual descriptor/probe/syscall attempt, including EINTR/error/EOF attempts |
| Record, Name | Fixed record framing and bounded native-name scan, including discarded records |
| Retain | Temporary physical-record ownership, before any vector growth or name allocation |
| Allocate | Complete requested new heap layout and alignment before fallible reservation |
| Copy | Upper bound on copied bytes, including old allocated slots during replacement |
| Order, Compare | In-place sort/collapse steps and bounded native byte comparisons |
| Visible | Logical cardinality after collapse, before returning/transferring the result |

Raw union duplicate and whiteout storage is charged before retention; it is not
the visible-member limit. A limit of one visible member can admit many physical
duplicates and whiteouts within the original work/storage accounts. The focused
fixture admits 61 physical records, collapses to one visible name, and succeeds
with a visible limit of one; adding a second distinct visible name fails.

The entry vector doubles an explicitly tracked admitted capacity and calls
`try_reserve_exact` only after the complete requested layout and possible old
copy have been admitted. Each name requests its byte length before allocation
and copying. Excess capacity provided by an allocator does not silently admit
more logical slots. No temporary pointer table, hash table, stable-sort buffer,
string formatter, or shrink-to-fit allocation is used. Name ownership transfers
as an already admitted `Vec<u8>`; the caller must admit any later OsString/path
container or byte copy separately. Compaction reuses the original vector.

Pinned Rust `alloc::raw_vec::{try_reserve_exact,grow_exact,finish_grow}` requests
the layout for `len + additional` and then records that capacity. These requested
layouts are explicit here; they are not a claim about allocator arena size,
rounding, metadata, or total process RSS. Fixed stack storage (the transfer array,
2168-byte mount structure, scalar parser/sort state) is distinct from retained
heap requests. Direct syscalls still involve kernel-internal work; an admitted
call and its fixed transfer extent are not a byte accounting of kernel memory.
There is no variable user-space libc DIR buffer or hidden directory collection.

The safe heapsort can stop on refusal before every comparison/move and uses no
auxiliary heap storage. Its work is bounded by admitted physical entry count and
name lengths. The importer must keep this pure work distinct from filesystem
path probes and store-candidate comparisons. Its exact mapping to the existing
GraphBudget accounts is an integration review condition, not a new allowance or
foundation-owned CPU cap. Exhaustive event-floor fixtures prove no subsequent
event or underlying read occurs after refusal; focused growth fixtures inspect
that reservation has not happened when its admission is refused.

## Evidence status

The final `cargo +nightly-2026-07-11 test -p rawr-directory --offline --locked
--jobs 1` run passed 21 unit tests and one compile-fail lifetime doctest after
the independent-review EINTR correction. The live fixture compares 403 names
over multiple raw batches against a test-only std directory reference, including
an empty directory, a symlink entry, and a multibyte name. Opening a missing path,
a regular file, or a final symlink fails explicitly. No libc directory iterator
is used in production.

The first live fixture attempted to create `native-\xff-\xfe` and received EPERM
before enumeration. The live fixture therefore uses a valid multibyte name;
invalid UTF-8 names remain covered by exact native-byte parser and sorting
fixtures. This is a limit of the exercised environment, not a product exclusion.
Synthetic tests cover union layers, both whiteout precedences, malformed
inode-zero name payloads, terminal interruption/error/EOF separation, stale buffer tails,
malformed lengths/types/names, all admission floors, and visible versus temporary
membership counts.

Exact source manifests remain under `/private/tmp/rawr-p9-darwin-directory-audit`
and `/private/tmp/rawr-p9-getattrlistbulk-audit`; the foundation review snapshot
adds SDK, libc archive/source, and installed Rust source hashes. Public Apple
Libc/XNU commits have not been joined to installed binaries. Runtime ABI checks
and declaration checks support the specific exercised contract without claiming
that identity or live union-filesystem qualification.

The later read-only installed correspondence report at
`/private/tmp/rawr-p9-installed-directory-audit/REPORT.md` closes a narrower host
question. It joins the inspected disassembly to the actually loaded image UUIDs
and code bytes, follows the installed binding slots, and verifies that installed
libc's union loop calls the same loaded `__getdirentries64` symbol as this
foundation. It confirms read-to-zero/error propagation, inode-zero filtering,
stable first-record selection, actual unsigned-byte strcmp binding, and both
whiteout precedences. That is installed implementation correspondence for the
relevant interface, distinct from public-source/installed-kernel identity and
from a live union fixture. Its manifest and loaded correspondence are retained
beside that report. Independent reassessment accepted this correspondence at
confidence0.94; no live union mount or kernel rebuild is an additional gate.

The actual loaded libc UUID is `66EBD321-6899-3863-BA24-5CFC3076A0CB`;
platform UUID `D93EC420-F465-3A3A-916D-A04AF9B023EB`; kernel-library UUID
`51565B39-F595-3E96-A217-FEF29815057A`. The inspected libc union loop at
unslid0x1803cf58c calls the installed directory syscall until zero, then filters
inode-zero records and stably removes duplicate/whiteout records. Its comparator
binding resolves to `_platform_strcmp$VARIANT$Base`, platform offset0x4790,
despite a misleading disassembler stub label. The directory syscall binding
equals the linked `__getdirentries64`, kernel-library offset0x29bc, syscall0x158.
The retained report SHA-256 is
`0257e94d6161f2d6428e5286a06c16a63879fcb1116a9ad6dfc845f09758a06f`.

The concrete importer adapter is private to
`importer/src/store/bundle/directory.rs`. Open, Mount, Read,
and native Record probes consume original observation work; full requested
Allocate layouts consume original decoded ownership. Name scanning and
Copy/Order/Compare remain bounded pure work with no invented filesystem or
candidate-comparison debit. Visible checks only collapsed logical cardinality.
The caller precharges its terminated native path vector and owned error path
before constructing them, borrows CStr without a CString/shrink conversion,
precharges the visible output container, and transfers existing native name
allocations into OsString. Path construction uses the existing precharged
bundle helper. All seven focused adapter tests passed, as did the importer
no-default library check and scoped format/diff checks at the foundation
checkpoint. Production `bundle::names` now delegates to this exact adapter;
the provisional nix directory feature is removed. Three focused production
entry-point tests cover sorted complete names/paths, current budget/error
propagation, and exact/floor path/output ownership after actual name collection.
All ten adapter and production-entry tests passed in the root's focused run.

The subsequent retained-runtime caller review found an additional std pathname
allocation outside this directory reader. `store::path` now prospectively admits
the pinned std input conversion (stack below 384 bytes, one `length + 1` allocation
otherwise) and canonicalization's simultaneous 1024-byte libc buffer and at most
1023 copied Rust bytes. Each actual pathname conversion is charged separately;
descriptor operations and this reader's borrowed CStr do not acquire that charge.
Canonicalization, native spelling, mount normalization and root-equality checks
are preserved. This is accounting under the original graph budget, with no new
pathname limit or unsafe operation.

The source boundary is nightly-2026-07-11 std
`sys/helpers/small_c_string.rs:22–29,53–58`, alloc
`ffi/c_str.rs:274–296`, and std `sys/fs/unix.rs:2113–2123`.
The std helper and filesystem source file hashes are respectively
`6f8b59e7841fcd20e24809e2f182b7120e4c2edff2917877e6de2a5cf5436a73`
(small C string helper) and
`c99d678dade85023e462c6b0c1dcc8dbd9cf808c300fdad37d38c1a10587fe65`
(Unix filesystem implementation). Apple Libc commit
`71bbe350ab79eef58113991d817ccc6165061a64`,
[`stdlib/FreeBSD/realpath.c`](https://raw.githubusercontent.com/apple-oss-distributions/Libc/71bbe350ab79eef58113991d817ccc6165061a64/stdlib/FreeBSD/realpath.c)
lines 107–108 and 340–344 establishes the native allocation and output bound.
The installed libc disassembly above corroborates both realpath variants' 0x400
allocation at `0x1803e952c` and `0x180390a2c`; that disassembly's SHA-256 is
`0eeaf38bb6b65d2c11b84e413851d08f7d78f318ec2f0f2c195b93c4b1ebb899`.

The final independent source review found no remaining foundation safety or
accounting blocker (confidence0.95), including the corrected terminal EINTR
behavior and the adapter's original-account ownership. It approved production
integration subject to focused checks of the actual `bundle::names` entry point
and preserved `bundle::paths` charging/error propagation. Those consumer checks
belong to the integration commit; this foundation grants no release readiness.

## Distinct Cargo Stream extension

`stream.rs` adds `Opening -> Stream -> borrowed Entry` within the same
private foundation. It leaves the existing Reader/Names contract intact. Its
exact source, state, ownership, native error and EOF rules are documented in
[src/stream/PROVENANCE.md](src/stream/PROVENANCE.md).

The added unsafe operations remain solely in `sys.rs`: a following open with
existing CLOEXEC/directory flags; an entry-independent `openat(original, ".")`
that creates the union enumeration descriptor; and entry-only fstatat against
the original lookup directory with AT_SYMLINK_NOFOLLOW. No descriptor, general
lookup API, arbitrary raw path constructor or unsafe consumer is exported.
The original descriptor cannot be retargeted by the separate union reader.
All paths/names are terminated and live for the syscall; fstatat receives
aligned initialized storage of libc's exact stat type and retains no pointers.

On supported aarch64 macOS the new ABI checks require stat size 144, alignment
8, st_mode offset 4 and width 2, plus AT_SYMLINK_NOFOLLOW 0x20. The pinned libc
binding calls plain fstatat on aarch64 (its `$INODE64` suffix is for other macOS
architectures). The expanded C SDK fixture checks the same type/signature;
installed Rust tests call the actual operations. Those tests do not establish
live union-mount coverage, wider architecture support or complete membership
integration. The new unsafe interface requires its own independent review
before any source-admission activation.

The independent focused review accepted this foundation extension with no
actionable safety, ABI or source-semantics blocker (confidence 0.94). It reviewed
the Stream, sys boundary, collector/framing support, both importer Work mappings,
tests and pinned Cargo/std/Apple sources. Reviewed SHA-256 identities were
`4aaeb90371202a2476b2b5594690bf3b111807f3e5d4d2bb0d3e848a6f22fdaf`
for `src/stream.rs` and
`8bb994d90df39eda3a669081b2680dfcec862b9ad772f5e0e055a7471bf25b70`
for `src/sys.rs`. Root validation passed 33 directory tests, two compile-fail
lifetime checks and the C ABI SDK syntax check. This acceptance covers only the
foundation on the exercised macOS ABI; target inference, stored/current-source
integration, live union qualification and release/performance evidence remain
outside its scope.
