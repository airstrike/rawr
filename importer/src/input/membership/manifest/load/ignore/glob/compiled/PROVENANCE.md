# Complete private GlobSet continuation

Source: locked `globset 0.4.18`, `src/lib.rs:446–552`, `:792–856`, and
`MultiStrategyBuilder::{prefix,suffix}`. The compiled owner consumes the actual
Collection, allocates the source seven-slot strategy Vec before conversion, and
converts extension, basename, literal, suffix, prefix, required-extension and
regex-set builders in that order. The enum has these seven concrete strategies;
there is no partial matcher, callback dispatch, engine substitution or fallback.
Remaining Rules retain reverse source-local drop order; the later strategy Vec
drops first on failure. Edge construction borrows the existing owned Strings
through Aho construction, then moves the map and longest scalar before dropping
the literals. Successful Set drop retains the source Vec element order.

Queries accept the actual borrowed Candidate. Boolean queries stop at the first
matching strategy. Matches clear the caller's retained Buffer before the empty
test, append every strategy's IDs in source order, sort, then deduplicate. The
existing admitted allocation-free comparison sort is used for plain usize IDs:
equal IDs have neither distinct payload identity nor side effects. Its actual
comparisons are admitted; no source-library sort allocation is fabricated.
Dedup is in place with admitted comparisons and final truncation. Every refusal
uses the original Session latch; reached partial output is never success.

Edges truncate the byte path to min(length,longest), keeping its beginning for
prefix or its end for suffix. Actual Aho overlapping matches are filtered by
start==0 or end==truncated_length, respectively. No UTF-8 boundary requirement is
introduced; global IDs follow actual overlap order and the retained source map.

## Mandatory Aho construction failure

The source calls `AhoCorasick::new(&self.literals).unwrap()`. The private result
therefore retains `Failure::Aho` separately from `Failure::Regex`; it does not
render an Aho error as a source regex error or invent a source-panic policy.

On the supported >=32-bit host, Aho's SmallIndex limit is 2,147,483,646. The
current original account (`cargo.rs` Limits::standard) admits 2,500,000 Items
(3,000,000 in the existing measurement account) and 1,048,576 bytes per string.
`membership/document.rs` forwards these requests to the same cumulative
`input::Allocation`, without resetting or refunding them.

* PatternId is bounded by the actual collection iterations and stored pattern
  rows, each admitted as an Item, before mandatory construction.
* PatternLength is bounded by Literal's growing UTF-8 String request before the
  value can reach the mandatory builder.
* Each actual noncontiguous state, sparse transition, match link and dense
  transition row is inserted through Aho storage::Buffer::push, which admits an
  Item. Reaching any corresponding SmallIndex overflow requires more Items
  than this original account permits. The prospective next ID cannot overflow
  before the already-owned rows have crossed that account ceiling.

Thus these mandatory semantic failures are unreachable under the unchanged
production account; original refusal happens first. Conversion size failures
remain internal to the pinned Engine's actual optional conversion fallback.
The generic private Admission API still retains the mandatory typed cause,
since it does not itself encode those fixed limits. Renew this argument if the
limits, supported host width, Aho SmallIndex representation, collection/storage
admission, or mandatory failure variants change. This is no production ignore
activation or qualification claim.
