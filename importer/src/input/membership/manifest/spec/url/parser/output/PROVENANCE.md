# Pinned URL serialization storage

Output owns charged transformation bytes, not a complete URL or original
manifest proof. The public-within-parser surface has no arbitrary setters or
unrestricted mutable storage.

Source: `url` 2.5.9, registry archive SHA-256
`f00cd2407bb42c0ef17be88d4cf020f5cceffd121665bac9b8b9493a369b3b1a`.
`url-2.5.9/src/parser.rs` has SHA-256
`9f868941a6291c323bf8020db4a733b0b460397b693164cb7da863e69ecfc69e`;
`src/lib.rs` has SHA-256
`208c0d522e74e04bafe1107b58acd2c932095b013a86de60e9126f15cb73f0c8`.
The verified archive/member provenance is recorded in `../../../URL_CLOSURE.md`.
MIT/Apache-2.0 license texts are retained alongside this file.

## Correspondence

- `parser.rs:19–38`: complete reached Controls, Fragment, Query, SpecialQuery,
  Path and Userinfo sets. Path is Query plus `?`, `^`, a backtick, `{` and
  `}`, so url 2.5.9 encodes `^` in paths. Non-ASCII bytes, C0 and DEL use
  uppercase `%HH`;
  unselected ASCII (including existing percent escapes) passes unchanged.
  Setter-only PathSegment/SpecialPathSegment sets are not reached here.
- `parser.rs:1192–1214,1417–1430,1534–1612,1635–1683`: path, opaque path, query
  and fragment appends use those exact sets. Fixed iterator fragments replace
  implicit String extension, with no intermediate encoded allocation.
- `parser.rs:1749–1756`: component offsets distinguish semantic u32 overflow
  from arithmetic/layout overflow. Successful inline offsets need no separate
  owned record; retaining the static semantic error costs one original item.
- `parser.rs:1303–1331,1358–1363,1448–1489` and `lib.rs:387–408,1581–1668`:
  reached truncation, path removal, slash-prefix insertion and tail removal can
  use UTF-8-preserving in-place shifts. The caller retains each exact state
  transition and offset update. Output alone does not implement those rules.

## Original-account ownership

The only requested heap storage is `storage::Buffer<u8>`. Every appended byte is
an admitted item. Buffer checks full geometric replacement layouts before
allocating, remembers its admitted extent independently of allocator bonus
capacity, and never refunds replaced layouts or removed items. Output adds no
fictitious heap charge for its inline struct, fixed UTF-8/hex arrays, pure scans,
copies, or comparisons. No graph account is read, reset or charged.

Append/insert check their full resulting string length before emitting bytes.
Encoding checks its input string length before a finite expansion-count scan,
then checks the full resulting output string length before emission. Checked
length/layout arithmetic refusal is terminal through the original Session.
No independent limit is introduced. Empty operations still check the latch.

A refusal during a multibyte append or insertion restores the previous content
length so that data remains valid UTF-8; admitted capacity and all spent charges
remain. Insert admits its added slots/layouts before shifting existing bytes.
Remove/truncate/pop require no replacement layout and retain the admitted
capacity. Boundary checks use constant-time continuation-byte classification.
`as_str` performs safe borrowed UTF-8 validation and latches State if an internal
invariant is violated; neither unsafe conversion nor an implicit copy is used.

## Work and teardown

An encoding count/emission visits input bytes a fixed number of times and emits
at most three bytes per input byte. Growth moves are bounded by the sum of the
admitted geometric layouts. Each insertion/removal copies at most the retained
output length; truncation of u8 slots has no recursive destructor. Pop examines
at most four valid UTF-8 bytes. `as_str` validates at most the retained output
length; callers must include repeated validations/shifts in their finite parser
work argument, rather than claim O(1) access or global linear work. The owner has
flat, stack-independent failure teardown.

## Prepared tests

Thirteen tests cover every byte's exact encoding-set membership and complete
ASCII/Unicode output against pinned percent-encoding 2.3.2; UTF-8 edits against
String; invalid boundaries/internal bytes; semantic u32 offset boundaries and
diagnostic-item refusal; checked length overflow; every admission refusal with
prior usage; multibyte rollback; exact/floor original item/layout/string limits;
pre-copy string refusal; retained-capacity reuse and geometric replacement; and
exhausted unrelated graph accounts.

The reference dependency is pinned `percent-encoding = 2.3.2`, archive SHA-256
`9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220`.
`tests/reference.rs` uses the exact reached upstream set expressions and direct
`percent_encode`/`utf8_percent_encode` calls; it does not substitute public URL
preprocessing for a component comparison. Root owns dependency registration,
compilation, formatting and execution; no executed result is asserted here.
