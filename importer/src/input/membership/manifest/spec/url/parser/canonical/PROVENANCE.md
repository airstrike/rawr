# Cargo CanonicalUrl transformation

Source: Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/canonical_url.rs::CanonicalUrl::new`; URL 2.5.8
`lib.rs::{cannot_be_a_base,take_after_path,set_path,restore_after_path}`,
`path_segments.rs::{new,pop_if_empty,pop,extend}` and
`parser.rs::{Context,parse_path_start,parse_path}`.

The adapter owns a separate clone of the accepted fetch URL. It checks the
cannot-be-base predicate, removes exactly one trailing empty segment, checks the
plus-scheme exception, actually reparses the GitHub suffix under HTTPS, lowers
that resulting path and runs the native setter, then strips the final `.git`
with pop-and-push segment semantics. Query/fragment ownership is detached and
restored with adjusted offsets around each mutation. Native file URLs are not
reparsed; parent components and percent spelling retain their source meaning.

Path context is explicit: unchanged parse wrappers select Parse; the setter and
segment operations select their pinned contexts, including the different
percent/slash/backslash encoding sets and shared file-drive/dot processing.
Only the GitHub HTTPS result reaches lowercase/set_path; this serialized path is
ASCII because the existing parser percent-encodes every non-ASCII UTF-8 byte.
Thus scalar lowercase iteration equals Unicode `str::to_lowercase` on the actual
reached value, including when the original repository path contains Unicode.

All actual owned buffers, clones, reads and writes use the incoming Session.
There is no new budget, reset, ambient observation, or SourceId authority.
The canonical type is only comparison identity; it does not grant interning,
registry selection, URL validity outside its input type, or caller completion.

The HTTPS reparse is reached only for an accepted hierarchical literal
`github.com` host. Valid serialized authority and admitted input size make its
source unwrap non-failing within this closure; unexpected parser/offset failure
is an internal invariant failure, not a newly classified source panic. The
preceding read-only standalone pinned-source probe reached 520 such inputs,
reported zero reparse failures and 96 changed serializations; this was supporting
bounded evidence, not a universal proof of arbitrary mutated/unbounded URLs.

Tests use a source-shaped Cargo transformation with the unchanged pinned URL
dependency as the differential parser/setter reference. They cover scheme and
host guards, single versus repeated trailing slash, query/fragment, default and
nondefault ports, dot/percent/backslash segment differences, Unicode source
paths, native file construction and preservation of the original fetch URL.
Every admission point is replayed with refusal and existing account history;
refusal must remain terminal before another parser invocation.

Three canonical tests and the encompassing 31-test parser suite passed on
nightly-2026-07-11, offline rawr-import library features rustc-driver,test-fixtures.
Independent read-only review found no blocker in canonicalization or its setter
contexts. The narrow Canonical reexport makes the result available to later
source conversion; it does not activate production membership or prove Gallery
performance. Original fetch URL ownership remains distinct from this result.
