# Cargo directory stream

This is a distinct source-shaped interface inside the existing private
foundation. Existing `Reader`, `collect` and `Names` retain their old behavior.
The new interface supplies acquisition data only. It is not connected to stored
membership, current-source admission or binary target normalization by itself.

## Frozen sources

- Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
  `src/cargo/util/toml/targets.rs:733–790`: ordered iteration, ignored read errors,
  dot/non-UTF8 filtering before file_type, known and unknown type behavior.
- Pinned nightly-2026-07-11 standard library `std/src/sys/fs/unix.rs:978–1005`,
  `:1069–1096`, `:1140–1150`, `:1993–2002`: Darwin ReadDir terminal error,
  descriptor-relative entry metadata, d_type dispatch, and following opendir.
- Apple Libc `71bbe350ab79eef58113991d817ccc6165061a64`,
  `gen/FreeBSD/opendir.c:102,133–285,301–313,325–447` and
  `gen/FreeBSD/readdir.c:52–196`. The root's original inspected source files and
  SHA-256 inventory remain in `/private/tmp/rawr-p9-darwin-directory-audit`.
  `_filldir` enumerates a separately reopened descriptor; its original lookup
  descriptor does not undergo union retargeting. Its sorted pointer table only
  marks duplicate/whiteout entries: visible iteration remains encounter order.
- [XNU private directory header at the pinned source revision](https://raw.githubusercontent.com/apple-oss-distributions/xnu/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/dirent_private.h):
  buffers of at least 1024 bytes provide a trailing four-byte unsigned status;
  bit zero is the successful EOF indication.
- `libc` 0.2.189 `src/unix/mod.rs:1033–1048`: the aarch64 macOS symbol is
  `fstatat`; `$INODE64` applies to the non-aarch64 macOS declaration. SDK
  `sys/stat.h:158–175,393` and `sys/fcntl.h:178` establish the corresponding
  stat layout, fstatat declaration and AT_SYMLINK_NOFOLLOW value.

This code implements the cited control flow over the already reviewed raw
reader and collector. It introduces no copied libc DIR implementation, hidden
libc directory allocation or general descriptor API.

## States and association

`Opening::open` prospectively admits a fresh following open and mount probe.
Unlike the existing no-follow Reader, Cargo's open follows a final symlink.
`Mount::Failed(errno)` retains a failed fstatfs result and selects Cargo's
ordinary branch; it is not relabeled as a known ordinary mount. Other failures
remain explicit errors. The fixed mount result can be retained before consuming
`Opening::finish`, which performs union collection when required. No entry can
be obtained from an unfinished Opening.

For union mode, finish admits and opens only `openat(original, ".", ...)`.
The existing bounded collector runs on that independently owned descriptor,
preserving whiteout and first-nonzero-inode precedence and native visible order.
The enumeration descriptor closes before finish returns. The original lookup
descriptor remains alive in Stream. A failed union collection never returns a
Stream or partial Names; its failure remains an open-completion failure.

Ordinary iteration remains lazy. Entry names borrow the fixed transfer buffer;
union entry names borrow the admitted Names storage. A returned Entry also
borrows its own stream's private lookup descriptor. Rust borrowing prevents a
mutable refill, move or drop while that entry remains live. Names, raw type
hints and type results are exposed, but neither a descriptor nor a general
lookup operation can escape.

`Entry::file_type` consumes one reached entry. Known d_type values return their
source type directly. Every other value takes the exact descriptor-relative
fstatat fallback with AT_SYMLINK_NOFOLLOW. A fixed 1024-byte stack buffer holds
the already framed native name and terminator after prospective name/copy
charges. No caller-supplied path, canonicalization or unrelated path stat can
replace this association. Native metadata errors remain explicit data for
Cargo's map_or(false), whereas refusal/framing failures poison iteration and
cannot turn into successful EOF.

## End and failure semantics

Pinned Apple __readdir_r returns a nonzero error only when its internal entry
pointer is null. The pinned Rust caller initializes its result pointer to null,
so that native error terminates the source iterator. Ordinary partial entries
followed by an error stay distinct from both zero-return EOF and open failure.
The error is delivered in the failing call; subsequent calls have no entries.

Ordinary readdir clears the final status word before every raw read. The Stream
reproduces that operation. After a positive read, an EOF bit is considered only
when the returned prefix leaves the whole status word outside entry bytes.
After that batch is consumed, no extra syscall may replace the established EOF
with a later error. A full prefix's trailing padding is never interpreted as a
status word. Stale flags from a previous read are cleared before another read.

The union collector deliberately retains its existing rule: a positive batch's
EOF status does not terminate collection, since another call may traverse a
lower layer. Only its actual zero return establishes completion. No failed
read is converted into EOF, and an admission/framing refusal remains terminal.

## Work and tests

No new allowance or independent account is introduced. The caller supplies the
original Admission at every reached boundary. `Work::Reopen` accounts for the
owned enumeration open and prospective close; `Work::EntryType` marks only an
actual fallback syscall. Existing Name/Copy/Record and collector ownership
charges cover the remaining bounded work. Fixed transfer, stat and terminated
name storage are stack extents, not hidden requested heap allocations. A shared
private refusal latch prevents a consumed refused type query from later
completing as EOF. Partial owned descriptors and Names drop without refunds.

Deterministic tests cover lazy encounter order/duplicates, union collapse and
original-versus-enumeration descriptor association, failed mount fallback,
partial read errors, union-open failure, every unrecognized type hint, each
prospective refusal boundary, the ordinary EOF-tail rules and sticky type
refusal. A compile-fail doctest covers entry/refill lifetimes. Installed tests
cover following symlink opens, independent CLOEXEC reopen positions, anchored
lookup after pathname replacement, file/directory/dangling-link/removed-entry
fallback, and native encounter order/type correspondence with std. C and Rust
checks cover stat size/alignment/mode offset and width, flags and signatures.
They do not claim a live union mount was available or that binary normalization
and current-source transcript integration are complete.

Root owns test execution and independent safety/ABI/semantics review before
activation. This document records the implementation boundary, not release
qualification or Gallery performance evidence.
