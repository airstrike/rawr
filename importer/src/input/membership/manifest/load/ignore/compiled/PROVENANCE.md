# Borrowed ignore builder to compiled owner

Pinned source: `ignore 0.4.26/src/gitignore.rs:338–352`, archive SHA-256
`b915661dd01db3f05050265b2477bcc6527b3792388e2749b41623cc592be67d`.

Builder::build borrows the builder. It first counts ignores with one full scan,
then whitelists with a second full scan, and only then invokes the complete
GlobSet compiler. A returned regex failure is displayed at this exact map_err
boundary into a new owned outer `Glob { glob: None, error }`. The inner source
globset display preserves the optional pattern prefix and existing rendered
regex message. The prior strings remain live during formatting and then drop;
the outer string is not an ownership move pretending to be the required copy.
Mandatory Aho failure stays a distinct typed cause. Its unchanged fixed original
account reachability proof is in `../glob/compiled/PROVENANCE.md`; this boundary
does not invent a generic source-panic policy.

After compilation, the prepared root is cloned through the existing admitted
native Path copy, with no second strip or normalization. The metadata Vec obtains
exactly len admitted element slots before any element clone. Each source Glob
comes from add_line(None, rule), so its absent from path requires no ownership;
the original String is copied first, actual String second, flags last. Each
String remains independently owned by the builder and the compiled matcher.
No consuming simplification, AST clone or engine clone replaces source work.

Matcher retains source field/drop order: set, root, globs, ignore count,
whitelist count, match scratch. This replaces the source pool-only Arc and
zero-capture Vec factory with an exclusively owned, initially absent scratch
Buffer. The actual caller proof excludes Clone/sharing, concurrent use and
escaping guards. This scratch is independent of each Regex's lazy Core cache
and each RegexSet's lazy PatternSet. Query integration owns the first reached
acquisition and preserves its retained capacity thereafter. Build performs no
scratch allocation or fictional pool-dispatch debit.

All count, full-layout, clone and rendering admissions remain on the original
Session. Refusal retains the first cause and spent work, publishes no Matcher,
and leaves the borrowed builder owned by its caller. Ordinary stack cleanup
requests no fresh admission. Query/path selection and production activation are
outside this construction slice.

## Cargo query and invocation domain

`query.rs` composes the reached `matched_path_or_any_parents(...).is_ignore()`
call, preserving `gitignore.rs:223–270`: empty return before stripping, one root
strip, direct result before parents, nearest-parent order including the empty
parent, directory-only filtering, and last global rule first. A direct or parent
whitelist stops the search just as an ignore does. The private query retains a
matched index until deciding the predicate; all original/actual rule metadata
remains owned by the matcher. It acquires the independent lazy scratch before
Candidate preparation, and releases each mutable borrow between parent queries.
The Buffer and all inner engine caches survive for subsequent paths.

The reached caller is Cargo `path.rs:595–624`. Every Git entry (`:830`), symlink
or subrepository recursion (`:822–827`), filesystem entry (`:877`) and applicable
walk-error re-filter (`:943`) uses the same closure. Only a successful component
`path.strip_prefix(root)` at `:614` produces its relative input; prefix failure
returns false, and exact Cargo.toml/Cargo.lock bypass ignore. Package root is
absolute (the source records this invariant at `:708`; current native observed
paths enforce it in `membership/observed.rs:123`). Pinned standard-library
`Path::_strip_prefix` and `Components::as_path` (`std/src/path.rs:2781,728,805`)
trim leading separators and dot components from the suffix. It therefore has
neither a root nor a leading `./`. Ignore's absolute root cannot byte-prefix
match it, so ignore's stripping leaves that caller suffix rootless.

This excludes the source assertion's failure within that actual invocation
domain; it is not a generic source-panic translation. The private query guards
domain violation as internal State. The pending physical selection connection
must retain the same absolute package-root owner and component-prefix proof;
an arbitrary relative path/root pair is insufficient (relative root `foo` and
raw `foo//bar` would be a counterexample). No native invocation was needed for
this source proof, and no production selection authority follows from it alone.
