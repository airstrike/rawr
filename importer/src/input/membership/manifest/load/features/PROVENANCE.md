# Cargo manifest feature context

Contained `Features::new` and query closure from Cargo
`59800466c5c41c444d264b1010b4d57e85a7117f`:
<https://github.com/rust-lang/cargo/blob/59800466c5c41c444d264b1010b4d57e85a7117f/src/cargo/core/features.rs>.
Archive SHA-256:
`dc2728d4e9650b9871142cc3f1f0e449647e0b493775931f0b70ee1fa658a321`.
`src/cargo/core/features.rs` SHA-256:
`43b9479784a684a828876fddf34a3f5e881bcf743c493cd3ae17074c644270d4`.
Cargo is MIT OR Apache-2.0; license copies are retained at
`importer/reference/manifest/LICENSE-MIT` and `LICENSE-APACHE`.

Exact source: `422–485` generates feature bits/queries; `506–585` declares all
27 features (13 stable, 13 unstable, one removed); `603–798` constructs and
queries; `805–921` provides all 54 CLI diagnostic names; `1611–1619` maps docs
channels. Root independently compared both complete tables with pinned source.
`util/toml/mod.rs:66–155` places features after complete Original and before
workspace configuration, publishing accumulated warnings only on full success.

The result borrows the admitted Original string slice and retains explicit bits;
stable query enablement differs from explicit activation/duplicate detection.
Context borrows an admitted flat allow-list and associated diagnostic channel,
plus captured nightly eligibility and exact Path-source locality. The checker
acquires no environment, configuration, version, filesystem or process data.
Flat membership testing preserves the consumed set semantics without requiring
stock tree allocations. Original Session checks borrowed string extents; only
actual warning text/storage allocates, through the parent's prospective writer.
No string copies, replacement scratch, heap help table or recursive teardown are
introduced. Every result/error item is admitted; refusal remains latched across
queries and cannot be consumed as an ordinary feature rejection.

Stable warnings retain exact full text/order. Fixed error categories retain
first-error kind, declaration index, known feature and relevant locality/advice.
Required-feature errors retain hint presence; the caller retains actual hint
text. Version/channel/allow-list error wording is not rendered or claimed equal.
Post-Original feature errors have no upstream TOML span. This is a bounded
diagnostic projection, not a general diagnostic-string framework.

Root executed all 20 focused load tests successfully, including 11 local tests
and the actual 336-case pinned Cargo constructor comparison (158 accepted,
178 rejected). Local tests cover all 13 complete stable warnings, channel docs,
borrowed pointer/order, first-error/query behavior, Original precedence, prior
usage/exact/one-under limits, and every refusal. Warning expectations are
source-derived: Cargo metadata suppresses those successful warnings. Root CLI
check also passed; root owns final formatting and broader checks. Formatting is
mechanical. The detailed read-only source report is recorded separately.

This remains private test-only unresolved data. No loader normalization,
workspace/member/source authority, current proof, production activation or full
membership/product qualification is claimed.
