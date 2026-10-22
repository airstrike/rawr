# Private outer Regex owner

This component connects the completed private Core to the two reached outer
regex-automata 0.4.14 meta::Regex queries. It does not connect
physical membership, qualify a release candidate or execute upstream libraries.

Pinned source root:
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/regex-automata-0.4.14/src/`.
Globset source is the existing locked `globset-0.4.18.crate` archive, not an
assumed unpacked registry path. Ignore source is the retained
`/private/tmp/rawr-ignore-0.4.26.crate` archive, SHA256
`b915661dd01db3f05050265b2477bcc6527b3792388e2749b41623cc592be67d`.

## Construction and retained owners

`meta/regex.rs:3480–3507` parses all ASTs, translates all HIRs, then invokes
`build_many_from_hir:3598–3616`. The existing Batch is retained through translation.
Its consuming `Translated::compile` keeps both vectors alive until the complete
outer owner returns; HIRs drop before ASTs, matching the source local bindings.
It calls Info::with once, without an earlier property collection or second HIR
reference vector. `translate::Properties::with_owned` keeps the source temporary
HIR reference vector alive through the entire continuation. The properties move
into the first admitted Info Arc and are not recollected for Core.

`meta/strategy.rs:80–190` first tests the actual union's Start property. Every
nonempty emitted batch reaches this branch and selects no prefilter. An admitted
Info Arc clone then moves into Core::new; the original Info stays outside. After
Core completes, the actual guard sequence is evaluated, not replaced with a
generic unsupported-strategy result:

1. ReverseAnchored::new:909: End, then Start. Emitted patterns reach Start and
   decline before engine availability or reverse strategy construction.
2. ReverseSuffix::new:1122: automatic prefilter configuration, then Start.
3. ReverseInner::new:1503: automatic prefilter, LeftmostFirst kind, then Start.
   All kind declines before the Start read, while LeftmostFirst reaches it.

Only these reached declined branches permit publishing Core as the strategy.
A changed producer reaching extraction or a reverse strategy build is a terminal
internal State error, not a semantic decline or an accepted partial matcher.
Original NFA semantic failures retain their structured cause in Failure;
original Session refusal propagates directly and never invokes a fallback.

The owner graph follows `meta/regex.rs:235–269,3608–3616`:

```
Regex
  imp: Arc<Compiled>
    strategy: Arc<Core>
    info: Info
  execution
    factory: Arc<Core>
    cache: Option<CoreCache>
```

Core's Arc allocation occurs after the strategy guards. The independent factory
Arc clone occurs before the Compiled Arc allocation, as in the source pool
factory capture. Actual replacement Arc layouts are admitted via storage::shared.
The execution cache is initially absent, including when the selected engine is
dense. The factory invokes Core::create_cache through the same original Session;
only a complete returned cache may become Some. This preserves the full captures,
GroupInfo, lazy Pike/backtracker, absent onepass, hybrid and absent reverse-only
cache construction already contained in core/cache.rs.

Field declaration order preserves destruction: imp before execution; Compiled's
strategy before its Info; factory before its cached value. Thus the independent
factory keeps Core alive after imp drops. Cache-owned GroupInfo remains owned
through the later cache destruction. This replaces only the boxed dynamic call
and pool arbitration/storage; no fictional pool allocation or return admission
is charged. There is no fallible or allocating guard-return operation.

## Empty sets and anchoring proof

This is the caller's emitted regex domain, not a general meta::Builder API.
Globset `lib.rs:466–474` returns GlobSet::empty before compiling any strategy.
The regex set constructor is called only under `!regexes.is_empty()` at 548;
RequiredExtensionStrategyBuilder::build:1005 compiles each retained regex string
individually. Glob::compile_matcher likewise uses a single pattern. An empty glob
is still one pattern and compiles normally.

The emitter writes `(?-u)^` before the entire expression and `$` after it; its
special recursive-prefix branch also ends in `$`. The HIR concat implementation
flattens concat and drops Empty nodes, but preserves Look nodes and does not
collapse a failed child into a replacement root. Prefix/suffix property scans
therefore see outer Start/End before any consuming or failing child. Union over
one or more such patterns retains both flags. This follows regex-syntax 0.8.11
`hir/mod.rs` concat and properties, already contained in the local HIR arena and
properties code.

Zero HIRs have neither anchor property. The general upstream meta builder would
enter its prefilter path; this owner never claims that work happened. A direct
zero-expression call is terminal internal misuse. Empty Batch preparation still
works and the actual empty GlobSet path must continue to skip this constructor.
The tests separately exercise zero-HIR Info and minimum=None, an empty glob, and
a real translated failing anchored byte class. None are conflated with an
impossible-query shortcut.

## Query ordering and refusal

`meta/regex.rs:530–542` converts a Boolean input with earliest=true, checks
RegexInfo::is_impossible, acquires its cache and invokes the strategy.
`which_overlapping_matches:1193–1210` checks impossibility before cache acquisition
and preserves the supplied PatternSet. Our Core Boolean API already carries
earliest semantics into the actual engine dispatch; overlap uses Core's distinct
overlap path, including retained partial IDs on semantic fallback.

The reached globset calls provide complete borrowed byte haystacks. Start is zero,
end equals haystack length and Anchored is No. The first two source impossibility
conditions at `meta/regex.rs:2022–2060` therefore short-circuit without their
anchor-property RHS. The remaining predicate reads the actual retained union:
minimum=None returns false immediately; a short span returns true; maximum is
used only after Start and End both hold. Maximum=None returns false. There is no
extra empty-pattern, empty-input or zero-pattern success/skip condition.

Every guard/cache/query mutation is reached through the same Session. A query
refused before acquisition cannot publish a cache. Factory refusal discards only
the partial cache. Query refusal after successful initialization retains that
complete cache and its reached progress but the Session remains terminal. A later
impossible input cannot turn refusal into false. Overlap never clears the
separate caller-owned Set; its owning RegexSet stage must acquire and clear it
before invoking this inner query.

## Concrete caller, clone and migration closure

The pinned Cargo `path.rs` is retained at
`/private/tmp/rawr-pinned-cargo-path-sep26.rs`, commit
`59800466c5c41c444d264b1010b4d57e85a7117f`, SHA256
`ad2da0382396387c739a6e5d28dd53b735b55bb0af82ae347916ad3798bf52cf`, as verified by
the root against the committed membership provenance. This is a source proof,
not a package or native probe.

`_list_files:570–635` constructs exclude, then include matchers as function locals.
Its two filter closures borrow them. Neither matcher escapes or is cloned. The
Git path passes the borrowed filter to `list_files_gix:689–850`; the filter is
called by its sequential consumer loop, not captured by the gix dirwalk iterator.
Subrepository recursion and filesystem fallback receive the same borrowed filter
after a previous query has returned. `list_files_walk:858–957` invokes it from
WalkDir's sequential filter_entry/consumer and may invoke it again on an error
path. Package/source clones do not capture these later function-local matchers.

GlobSetBuilder::build delegates to GlobSet::new over borrowed glob metadata.
Required-extension conversion clones extension keys and compiles new regex
values; it does not clone a compiled matcher. Ignore GitignoreBuilder::build
clones rule/root metadata and creates its own scratch owner. Although the general
upstream Regex::clone creates a fresh cache and derived RegexSet/Gitignore clones
share their scratch pools, none of those compiled-owner clones are reached by
this Cargo caller. No private Clone API is needed for this bounded route.

The future enclosing private selection owner must live outside traversal frames,
preserve these two matcher lifetimes across recursion/fallback/parent checks,
and borrow one Regex mutably per query. Its source computation is sequential and
requires no sharing, concurrent calls, reentrant factory or post-acquisition
thread migration. A new parallel or cloned caller must establish its separate
ownership relation before connection; one mutable Session or private visibility
alone would not prove it. Required-extension entries already retain distinct
Regex values, therefore distinct lazy caches. PatternSet and ignore-index owners
remain separate from this Core-cache owner.

## Deterministic checks

The `nfa::regex::tests` filter exercises the real parse/translate/compile route,
source ownership after AST/HIR release, source strategy guard order, byte/empty/
EOI semantics, both match kinds, Unicode bytes, existing overlap IDs, impossible
queries without cache creation, minimum=None and exact anchor gates, independent
cold owners and warm reuse, factory Arc/Info lifetime, structured NFA failure,
and every reached cold/warm query refusal plus outer strategy/Arc publication
refusals. Tests do not execute a native matcher or qualify membership. Execution
of these tests is owned by the root; this source batch makes no pass claim.
