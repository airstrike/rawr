# Required-extension and regex-set strategies

Source: locked globset 0.4.18 `src/lib.rs:271–306,855–1018`, read from the existing
registry crate archive. This slice does not activate a complete GlobSet or
substitute regex matching for the unfinished prefix/suffix strategies.

`required::Strategy::build` consumes the existing FNV table's actual bucket
iterator. For each group it copies/inserts the empty destination list before
opening the group's consuming regex iterator. Each original String remains live
through compilation and destination append. Its compiled Regex has its own
lazy Core cache. A semantic error or original Session refusal stops the whole
construction; neither returns a partial strategy. Queries first test the borrowed
Candidate extension for emptiness, perform the actual map lookup, then visit that
group in stored order. Boolean matching short-circuits at its first match;
matches_into appends every matching global ID without sorting or clearing output.

`set::Builder` preserves MultiStrategyBuilder's longest update, map append, then
literal append. Compilation consumes its String buffer with Kind::All; those
strings drop before the source pattern_len/factory step. The resulting strategy
retains fields in source order: compiled Regex, local-to-global map, then separate
PatternSet execution state. The already-established no-clone Cargo route
permits the source Arc/pool dispatch storage to become an exclusive lazy owner;
the captured pattern count and PatternSet lifetime remain. Boolean queries do
not acquire this set. Overlap acquires and clears it before the inner Regex
impossibility check, traverses matched local IDs in ascending order, and appends
the corresponding global IDs without sorting. The output Buffer belongs to the
enclosing caller. No scratch or cache guard escapes and no return path allocates.

The source string bridge changes Batch's retained source borrow from `&Regex` to
`&[u8]`. Existing emitted storage, one borrowed String and the actual String
buffer all feed the same ordered parser loop. The changed Tree layout is charged
by its actual Buffer allocation. There is no copied regex Text, additional HIR
collection, reference Vec, independent account or AST reinterpretation. Translation
uses the same borrowed-byte helper; all ASTs/HIRs remain alive until the existing
consuming compile returns, with HIRs dropping before ASTs.

The actual map_err boundary eagerly renders the parsing/translation/NFA failure
into an owned diagnostic String under the original Session. Single-regex errors
copy their source pattern first, then render the error; regex-set errors have no
pattern context. Bounded tags/spans replace upstream snippet
wording: parse/translation diagnostics carry pattern index, byte span and cause;
NFA diagnostics carry the structured limit/capture cause. The intermediate typed
failure drops after this conversion, rather than outliving the source boundary.
Semantic rejection remains distinct from a refusal while allocating either
error field; no diagnostic materialization is deferred to an enclosing caller.

Four deterministic regressions follow implementation: required-extension query
order; real regex-set local/global mapping plus cold/impossible/warm scratch;
first semantic failure with correct single/set context; and original-account
refusal across cold overlap, preserving reached output and forbidding retry.
The root owns execution of checks; this document makes no execution, fit or
qualification claim.
