# Cargo's Unix WalkDir continuation

This module contains the iterator control flow reached by Cargo's physical
walk. It uses the separate traversal transport; it does
not turn early retirement into completion of an ordinary directory reader.

## Pinned source

Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/sources/path.rs::list_files_walk` (858–957), constructs WalkDir with
`follow_links(true)` and `contents_first(false)`. Its other reached defaults are
unsorted iteration, minimum depth zero, maximum depth `usize::MAX`, ten open
streams, root-link following enabled and same-filesystem checking disabled.
The surrounding Cargo filter and selected-path accumulation belong to the
parent module.

The commit's restored Cargo.lock pins `walkdir` 2.5.0, checksum
`29790946404f91d9c5d06f9874efddea1dc06c5efe94541a7d6863108e3a5e4b`, and
`same-file` 1.0.6, checksum
`93fc1dc3aaa9bfed95e02e6eadabb4baf7e3078b0bd1b4d7b6b0b68378900502`.
The corresponding local registry sources were inspected. These checksum values
identify the pinned archives; this change does not claim a new archive check.
WalkDir's MIT license is retained beside this document.

| Contained operation | WalkDir 2.5.0 source |
|---|---|
| Root ownership and iterator state | `src/lib.rs::WalkDir::new` (289), `IntoIterator::into_iter` (539) |
| Root-first, depth-first continuation | `src/lib.rs::IntoIter::next` (684–735) |
| Directory pruning | `src/lib.rs::IntoIter::skip_current_dir` (785), `FilterEntry::next` (1060–1089) |
| Follow before directory push and yield | `src/lib.rs::IntoIter::handle_entry` (840–881), `follow` (960–970) |
| Oldest drain, open, ancestor then list push | `src/lib.rs::IntoIter::push` (901–945) |
| List retirement before ancestor retirement | `src/lib.rs::IntoIter::pop` (947–958) |
| Child handle then reverse ancestor comparisons | `src/lib.rs::IntoIter::check_loop` (973–990) |
| Materialize remaining entries before stream close | `src/lib.rs::DirList::close` (1007–1012) |
| Deferred opening error and per-entry errors | `src/lib.rs::DirList::next` (1017–1034) |
| Type inspection before owned entry path | Unix `src/dent.rs::DirEntry::from_entry` (200–218) |
| Followed or unfollowed metadata and error path clone | Unix `src/dent.rs::DirEntry::from_path` (252–275) |

The Unix ancestry branch is essential: `Ancestor::new` (632–634) copies only
the path. It neither opens a handle nor produces the Windows-only native error
there. `Ancestor::is_same` (646–648) opens a temporary ancestor handle on every
comparison. `same-file/src/unix.rs::Handle::{from_path,from_file}` (60–73)
opens then obtains descriptor metadata; `PartialEq::eq` (31–33) compares device
and inode. The child handle survives all reverse ancestor comparisons. Each
temporary ancestor handle retires before the next comparison or creation of a
loop error; the child retires after the error's ancestor and child path copies.
Those native operations and occurrence checks belong to the transport.

## Ownership and original-account work

`Walk` retains separate list and ancestor buffers in source order. A pushed
directory is opened before either stack grows. An opening error owns its cloned
path immediately, even if Cargo subsequently prunes the directory without
requesting the error. The transport's `opening_error` reads already obtained
data and does not consume the later delivery event. Union opening and collection
are completed by the transport before it returns this token.

At ten open streams, `drain` materializes the oldest stream's remaining entries
and errors into an admitted buffer, preserving encounter order. It then retires
the stream and retains the buffer's consuming iterator. Already closed streams
are a no-op for draining. The oldest index advances only after the new ancestor
and list are both pushed. On pop, the complete list (including unread buffered
paths or deferred errors) drops before its ancestor path. Pruning never drains.

The next call after an opening or enumeration error consumes a logical EOF
event before retirement. The transport implements this post-error continuation
without a further native read. Entry inspection failures instead carry the
owned entry path and leave iteration live, as `DirEntry::from_entry` requires.

Every path copy/join uses the existing native-byte path helper, retaining its
String limit, exact owned allocation and copy charges. Stack and drained-entry
storage use `Buffer<T>` with the actual local element layout and admitted full
replacement extent. Iterator steps, stack mutations and loop comparisons remain
on the original Session. No uncharged upstream Vec, recursive walk, sorter,
native callback or generalized automaton is invoked. The ten-open threshold
does not bound depth or the buffered frontier; both grow only through the same
prospective admissions. Popped storage grants no refund or new budget.

`Entry` retains the fields consumed by Cargo. The unused Unix inode scalar is
not duplicated in it; identity comparisons use the separately reached handle
metadata. Native errors use an owned inline `Failure` with exact depth and
optional path; loop errors own ancestor then child. They remain inside the
inner `Result`. Admission, missing evidence or ownership failures remain outer
`Step` refusals and invalidate the original attempt. Successful `finish` requires
the root consumed and both stacks empty, then delegates complete event/owner
consumption to the traversal cursor. Refused cleanup cannot publish evidence.

## Integration boundary

The API is `Walk::{new,next,skip_current_dir,finish}` with owned `Entry` and
`Failure` outputs. The parent decides Cargo's filter, warning/recovery branches
and path accumulation; the cursor decides whether requested source operations
match acquired evidence. This file neither activates Git selection nor changes
complete-file/directory inference readers. Root owns the combined post-code
validation; this source batch adds no separate test harness or execution claim.
