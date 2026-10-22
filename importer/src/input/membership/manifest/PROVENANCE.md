# Pinned typed interpreter correspondence

This is the original-manifest interpretation boundary. It borrows admitted
`rawr_toml::document::Document` values and the caller's original `Admission`.
`membership::Budget` already implements that trait using the original cumulative
`Allocation`. These types do not acquire files, establish EOF or freshness,
load workspaces, normalize manifests, select packages, or bind a Cargo owner.

The controlling Cargo source is commit
`59800466c5c41c444d264b1010b4d57e85a7117f`, verified archive SHA-256
`dc2728d4e9650b9871142cc3f1f0e449647e0b493775931f0b70ee1fa658a321`.
The complete schema `crates/cargo-util-schemas/src/manifest/mod.rs` has SHA-256
`fc503d6532663f2b0f3217b53f235b6c24690e9c85116f1364ec134ca78cd92c`.
Primary sources are under
`/private/tmp/rawr-p9-pinned-cargo-full/cargo-59800466c5c41c444d264b1010b4d57e85a7117f`
and `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.
The source inventory is `/private/tmp/rawr-p9-typed-manifest-loader-plan.md`,
SHA-256 `ccc4fb532bd4be07c1c13f884d5af29c001b9fd09976f7e29f239a48745089e0`.

## Implemented correspondence

| Contained code | Exact upstream source |
|---|---|
| `value::Input::view` | toml 1.1.2+spec-1.1.0 `src/de/deserializer/value.rs:87–130`: i64/u64/i128/u128 dispatch, deferred float refusal, datetime map dispatch |
| `value::Cursor` | toml `src/de/deserializer/table.rs:126–171`, `array.rs:21–26,69–87`; source order and on-demand value conversion |
| `datetime::Text` | toml_datetime 1.1.1+spec-1.1.0 `src/datetime.rs:232–292`: canonical date/time/fraction/offset spelling |
| synthetic datetime key/value | toml_datetime `src/de.rs:29–58`; key emission precedes normalized value production |
| `walk`, ignored mode | serde_core 1.0.228 `src/de/ignored_any.rs:113–210`; recursively consumes values and accepts 128-bit visitor values |
| Content mode | serde 1.0.228 `src/private/de.rs:313–508`; recursive capture in encounter order, no i128/u128 visitor |
| serde-value mode | serde-value 0.7.0 `src/de.rs:190–289,326–359`; complete source-order capture, BTreeMap order in replay, no 128-bit visitor |
| generic TOML mode | toml `src/value.rs:420–518`; i64 integer domain and first-key datetime marker with early map return |
| replay span erasure | Serde Content and serde-value contain no TOML spans; outer source adapter supplies missing error context |
| replay exhaustion | serde 1.0.228 `src/private/de.rs:1099–1113,2053–2067` and toml `src/value.rs:532–552` check Content/generic TOML sequence/map exhaustion; raw TOML and serde-value `ValueDeserializer` invoke their visitors directly without that check |
| ignored replay | serde `src/private/de.rs:1477–1483,2426–2431` immediately returns unit for Content; serde-value `src/de.rs:415–419` and toml `src/value.rs:611–615` forward ignored values through `deserialize_any` |
| generic TOML datetime replay | toml `src/value.rs:528–531` emits the normalized datetime as a string; raw `DeValue` emits the synthetic map instead |
| `schema::Target` | Cargo schema `mod.rs:1334–1366`; all 15 fields, including independent underscore aliases |
| Target map/sequence behavior | serde_derive 1.0.228 `src/de/struct_.rs:70–95,224–412`, `src/de.rs:451–551,805–835`; sequence missing fields differ from optional map fields |
| `schema::package` | Cargo schema `mod.rs:149–225,369–599,624–666`; complete 33-field package and 17-field workspace defaults, inheritance and positional visitors |
| `schema::primitive` | Cargo schema `mod.rs:1690–1798`; exact scalar unions, package versus workspace version conversion and finite badge maps |
| `schema::lints` | Cargo schema `mod.rs:1536–1676`; complete lint families, borrowed Content flattening and source-specific enum dispatch |
| `schema::hints` | Cargo schema `mod.rs:1678–1687`; optional generic metadata with required positional Option |
| `schema::Paths` | Cargo schema `mod.rs:1223–1269`; string-vector completion precedes trim enum conversion |

Datetime parsing uses the exact type/`FromStr` already reached by the admitted
document; the document module reexports that type without a new dependency.
The normalizer never calls the allocation-bearing `Time::fmt` or
`Datetime::to_string`. It writes the same components into 35 inline bytes:
10 date, 1 separator, 8 time, 10 fractional and 6 offset. Accepted pinned parser
values satisfy the component widths. Fractional seconds use nine stack digits,
trim trailing zeros, and retain one zero when all digits are zero. Pinned Date
and Offset formatting directly target the supplied writer without allocation.

## Ownership and work

`session::Session` borrows the existing admission authority; it has no numeric
limits or independently reset usage. Admission/layout/allocation failure is
retained permanently. `Stop::Rejected` is a semantic trial result and may be
discarded only by a pinned fallback; `Stop::Refused` cannot authorize fallback.
`finish` checks the retained fatal error even if a caller discards an intermediate
result. Checks after failure admit nothing and do not scan another input.

`storage::Buffer` admits each actual retained node, edge, reference, continuation,
scratch index and diagnostic item. Growth doubles the admitted extent from four
slots. It checks the complete Layout, obtains admission, allocates a separate
fallible replacement, and moves the old items. This makes replacement accounting
explicit even if the old allocator returned excess capacity. Prior allocations
remain spent; no capacity shrink, clone, Box conversion or implicit collection
growth is used. Borrowed string contents have no fictitious heap allocation.
Inline datetime buffers and fixed local scalars are covered by their enclosing
actual layout when retained, or statically bounded stack storage when temporary.

Generic traversal uses explicit admitted Enter/Children frames. Cursors advance
monotonically and retain no recursive owned descendants. A generic pass visits
each ordinary input node once plus the fixed synthetic datetime entries;
first-marker TOML conversion can visit less. Ignoring captured Content checks the
terminal latch and returns unit without allocating a continuation, scanning a
descendant or checking its strings again. Its already admitted flat storage stays
with the caller and has nonrecursive destruction. Serde-value and generic TOML
ignored replay still consume their descendants through the generic visitor.
Direct numeric dispatch tries at most four integer conversions. Serde-value
capture sorts each completed map only after all of its initial source-order
conversions. Sorting admits the temporary edge-index ownership before comparing,
uses allocation-free heapsort, and relinks the existing edges. It performs
O(m log m) comparisons over already admitted key bytes for a map of m entries.
No filesystem, candidate comparison or hash account is charged for this work.
No hash seed, generic owned DOM, recursive clone or recursive teardown is used.
Node/edge/frame/index vectors have flat element destruction, including partial
failure. The input document retains its independently implemented iterative Drop.

The adapter has separate lifetimes for temporary captured storage and retained
Document payloads. Replay copies fixed inline scalars and keeps original text
borrows; converted output cannot borrow the temporary projection's inline text.
Generic conversion can therefore return after that capture is dropped without
copying unchanged strings, reparsing them, or extending a borrow. The corresponding
test retains a decoded Document-owned string and converted datetime after explicit
capture teardown and checks the original string pointer.

Enum visitors inspect the source discriminant and container cardinality before
the `deserialize_any` scalar dispatcher. `Input::shape` exposes only those
constant-time facts; it neither converts overflowing numbers nor synthesizes a
datetime map. The caller checks the original Session before inspection. Retained
adapter mode controls the separate unit-payload rules; it is not inferred from
the shape alone.

Capture provenance controls exhaustion on replay. Generic TOML's first-marker
return leaves raw and serde-value trailing entries unconsumed, while Content
replay rejects them without converting those remaining values. Generic TOML
replay also checks container exhaustion and presents retained datetime values as
normalized strings, rather than recreating the raw synthetic datetime map. Dependency
flattening introduces that Content stage even after initial serde-value capture.

Target string arrays are consumed once; unknown fields run the same explicit
ignored-value machine. The positional Target visitor reads exactly its 15
declared fields and does not convert trailing array values. Its Content and
generic TOML adapters still reject unconsumed elements after the visitor returns. Trim-paths owns an
admitted vector of borrowed strings before its enum pass; a later non-string
therefore wins over an earlier invalid enum spelling. Successful scalar output
and chosen union values are distinct converted data, rather than a raw checked
marker.

## Integration boundary and remaining work

There is no `Original` constructor. The generic machine, complete Target, package/default, lint and hint
visitors, profile leaves and trim-paths union are private primitives. Complete
top-level/workspace/platform/dependency and recursive profile interpretation,
dependency inheritance trials, whole-manifest bookkeeping, profile typed-key
equality/replacement and the complete PackageIdSpec URL closure must be connected
before exposing whole-manifest typed success. The scalar and query subtrees carry their own provenance. No supported
shape is rejected merely because that integration is unfinished; these partial
primitives are not a production whole-manifest acceptance route.

The root's first focused run passed 24 tests: ten scalar, five query, and nine
core/schema tests (`/private/tmp/rawr-p9-typed-manifest-focused.log`). The corrected
focused suite passed all 36 tests in 1.93 seconds
(`/private/tmp/rawr-p9-typed-manifest-corrected.log`). These tests exercise every
core refusal event before allocation, original
item/byte/string exact and floor accounts with nonzero prior usage, allocator
bonus refusal, over 6,000 nested generic nodes on success/failure and destruction,
absence of fictitious GraphBudget events, and actual native acquisition through
Stored/Host/Document to the generic and Target interpreters under one account.
Exhaustion regressions cover raw, serde-value, Content, generic TOML, and serde-value followed
by Content. Deep captured Content is also ignored with the original item/byte
accounts exactly exhausted, no new admission or descendant storage, and an
already latched refusal still prevents success.
The root owns execution, differential evidence, formatting and commits.
Expected-output tests are not themselves evidence of a successful reference run.

## Executed checkpoint evidence

The complete focused `input::membership::manifest` filter passed 39 tests under
`rustc-driver,test-fixtures`, including the corrected replay distinctions and
seven resource/deep/integration tests. Three comparisons use the committed
pinned Cargo observations: all 15 fields of five Target instances, eight direct
metadata values, and four scoped rejection/span cases. They exercise the actual
contained visitors; they do not compare an unimplemented whole Original.

Focused independent review found three replay P1s (adapter-specific exhaustion,
ignored Content traversal and TOML datetime dispatch). After correction, the
same reviewer found them closed with no remaining P0/P1 or material regression
in that boundary (confidence 0.98). Query's separate focused review found no
P0/P1 (confidence 0.98). These are development boundary reviews, not the required
fresh full candidate review or qualification.

Final focused log SHA-256: `c9e6be8a9428c5a76d76b990df5daafe267bc0e5e863c070540f7613b86473d3`.

## Replay ownership and profile leaf checkpoint

The separate capture/payload lifetimes and complete profile leaf visitors pass
all 49 focused manifest tests. The nine profile tests include 22 comparisons
against 20 committed pinned-reference rows, checked numeric domains, normalized
inline decimal text, spans, terminal refusal and cumulative exact/floor limits.
`schema/profile/PROVENANCE.md` records the reached source correspondence. The
profile container, recursive profile maps and PackageIdSpec remain downstream.

Independent read-only review of the shared lifetime and intrinsic-shape boundary
found no P0/P1 (confidence 0.98). The reviewer confirmed original Document borrows,
copied inline payloads, replay span erasure, Session checks and flat destruction.
This is a focused development review; complete candidate review is still required.

## Package, lint and hint checkpoint

All 86 focused manifest tests pass after connecting the complete package/default
and lint/hint visitors. Package comparisons cover exactly 23 accepted
package/project occurrences, the complete 17-field workspace defaults, and two
isolated error/span cases. Lints compare six complete accepted scopes and six
first-error/span cases; hints compare three accepted scopes and one error. All
reference data is the committed 130-case pinned Cargo execution.

The new resource tests exercise every recorded refusal before allocation, original
aggregate/string floors with prior usage, map replacement and deep metadata/ignored
teardown. Typed package data also survives temporary generic replay, retaining
inline datetime text in a description. Package-name validation still rejects the
same text as a leading-digit name. RustVersion delegates to PartialVersion's
requirement fallback, which accepts ASCII spaces; its caller does not perform
package SemVer's explicit Unicode trim. These source rules corrected two initially
incorrect test expectations; production conversion did not change for those fixes.

Independent source reviews found no P0/P1 in the complete package/primitive slice
(confidence 0.97) or lints/hints (0.93). These reviews do not establish complete
Original, loader, membership, candidate or release acceptance.

The final focused log is `/private/tmp/rawr-p9-typed-families-corrected.log`,
SHA-256 `751944b4b04440d507d28cb8e23ca7ca483ecc1fbb18c600a817b9841528b7dc`.
The importer no-default and import-baseline CLI compile checks pass, as do scoped
format and diff checks. These checks do not exercise the outstanding CLI reuse
route or establish an installable universe.
