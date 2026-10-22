# Source-directed traversal capture and replay

This is a private ownership boundary. Cargo commit
59800466c5c41c444d264b1010b4d57e85a7117f sources/path.rs:570–635,858–957
and its WalkDir2.5.0/same-file1.0.6 pins supply the reached control flow.
The full iterator mapping and license are in
manifest/load/selection/walk/PROVENANCE.md. Native directory ownership, ABI and
union correspondence remain governed by directory/src/stream/PROVENANCE.md
and directory/SAFETY.md; this introduces no second native directory adapter.

## Occurrences and ownership

Owner borrows the original Budget once. begin assigns a nonreused invocation
ordinal and retains exact root/role. An unfinished or refused Capture leaves
Owner pending and prevents another invocation. Directory and identity-handle
open attempts share monotonically assigned ordinals, including failed attempts.
Only private, non-Clone tokens identify active owners; runtime scope/invocation
cannot be reconstructed from serialized IDs. Replay additionally binds the
issuing Host and Stored image. Dropped tokens prevent consuming completion.

The live frame table owns actual native streams and loop descriptors, while
returned event names borrow retained bytes after their mandatory type query.
No refill, descriptor drop or stack mutation occurs while a native entry is
borrowed. Full replacement heap layouts include the native stream's inline
8KiB transfer buffer; neither allocator slack nor the ten-open threshold grants
unadmitted storage. Buffered recursive entries remain separately charged by the
source continuation. Event, name, path, frame and replacement ownership use
the original cumulative account. Cleanup after refusal makes no admissions or
allocations and publishes no record.

OpenFailed and UnionFailed remain deferred source data. Their tokens expose
the already-known error so WalkDir can clone its error path before ancestor
construction, even if the subsequent predicate prunes it. Next distinguishes
opening failure, enumeration failure, typed entry and EOF. A logical next after
native failure returns EOF without another native read. Pruned closure is legal
only before the first next; Drained/Exhausted require consumed EOF. The source
caller chooses the transition; stored close tags cannot direct control flow.
Union collection and all its original charges finish before a successful open
is published, including when that directory is immediately pruned.

## Unix loop identities

The supported Unix ancestor stack retains paths, not Windows-style persistent
ancestor handles. On a followed directory symlink, same-file1.0.6 opens a child
file, obtains descriptor metadata and retains it through reverse ancestor
comparisons. Temporary ancestor files open/stat/compare/drop in turn. The
transport stores open failure separately from descriptor-stat failure, and only
successful descriptors produce owned identity tokens requiring Release.

The same-file source OpenOptions::open uses pinned std sys/fs/unix.rs::open_c
(nightly-2026-07-11,1390) and cvt_r. Capture therefore reserves one possible close,
charges every open attempt and retries EINTR. It uses the existing safe native
System open/fstat/close boundary, including lossless immediate errno capture.
Descriptor metadata supplies device/inode; pathname metadata is not substituted.
Stat failure drops the just-opened descriptor. Pure identity comparison is
admitted logical work, not a fabricated native probe.

## Stored data and activation boundary

Query::Traversal carries root, role and invocation; schema4 has its own value
and codec tags. Shape validation preserves exact event/row order, mandatory
WalkDir type results and positive native errors. Replay enforces source history,
scope, occurrence, union error constraints and complete owner/event consumption.
Neither decoding nor Host::finish proves current source or complete selection.
Complete DirectoryStream/file readers retain their mandatory EOF/error contract;
old images clean-miss under the new identity.

The actual explicit-include branch is connected through Finished::package(),
Package::included(), retained matcher owners, Walk and Files::project(). Both
builders and matchers retain source declaration/drop order; Filter borrows them.
Directory includes bypass matcher queries only after component-prefix/special
manifest checks. The branch preserves nested package/target pruning, symlink
facts, loop warning text and recoverable error entries. No-include packages
return an explicitly unresolved branch, never an invented no-repository result.

Membership retains its existing cfg(test) activation gate. Full Git discovery,
repository/index traversal, relative native loading, live cold/current selector
association and release qualification remain outside this private checkpoint.
The integrated fixtures are post-implementation checks, not production Gallery
measurements or native platform qualification.
