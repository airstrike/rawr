# Pinned platform parsing and warnings

Source: Cargo commit `59800466c5c41c444d264b1010b4d57e85a7117f`, workspace
`crates/cargo-platform` 0.3.3: `src/lib.rs` platform validation, parsing and
warning passes; `src/cfg.rs` tokenizer, parser and display; `src/error.rs`
diagnostic formatting. License texts are retained in `load/path/LICENSE-*`.
The actual caller is Cargo `util/toml/mod.rs` 1572–1592.

Outer cfg recognition is exact prefix/suffix matching. Named targets permit
Unicode alphanumerics, underscores, hyphens and dots, including an empty name.
Named validation's parenthesis-specific diagnostic keeps priority over the
first invalid character. Cfg parsing retains only ASCII-space skipping, ASCII
identifiers, raw flags, unescaped quoted values, operator recognition, literal
Boolean conversion, lookahead behavior, trailing commas and exact error classes.
Cfg errors render the stripped inner original, named errors the complete input.

Explicit admitted frames replace recursive parser calls. Flat preorder nodes
retain borrowed identifier/value text with private parent/child/sibling links.
Raw-name equality converts bare true/false to literals even when raw, while
raw operators remain ordinary identifiers. Display follows those links without
allocation or recursion. No normalized cfg string is allocated during parsing;
the later caller's admitted formatter controls reached rendering. Named parsing
retains its source-owned string. No arbitrary depth restriction or support
subset is introduced; original account refusal remains terminal.

Attribute and keyword checks are separate full leaf-order passes. Attribute
checks retain raw display while comparing raw-independent names. Keyword checks
recognize only nonraw true/false identifiers, excluding Boolean literal nodes.
Every warning is preserved in order, including duplicates and the pinned typo
`should not have have`. The caller must run both passes before dependency table
diagnostics and only format platform names at reached use sites.

Tests exercise pinned source success/error cases and exact display, raw operator
and Boolean distinctions, inner/outer error originals, space-only tokenization,
warning ordering/text, deep parsing/display/drop and refusal at every reached
parser admission. These are source-shaped deterministic tests, not a newly run
external pinned-crate differential fixture or membership qualification.

Focused execution on nightly-2026-07-11, offline rawr-import library with
rustc-driver,test-fixtures: six platform tests, nine dependency caller tests,
and 32 connected package-normalization tests passed. Independent read-only
reviews found no actionable blocker in the parser and the separately reviewed
caller. Stable rustfmt and diff checks passed. This module remains test-gated;
these results establish neither production activation nor Gallery performance.

## Retained dependency platform copy

`Platform::copy` copies the admitted flat node sequence and its exact links,
retaining raw identifier spelling and the original text lifetime. Named targets
receive admitted owned text. This preserves pinned cargo-platform's Clone value
semantics under the existing flat/borrowed representation; it does not claim
identical recursive heap layout. Atom traversals, nodes and replacement storage
are admitted through the original Session. No parsing or warning pass repeats.

All eight platform tests passed offline on nightly-2026-07-11 with
rustc-driver,test-fixtures, including original-owner drop/link preservation and
every copy-admission refusal. Independent read-only review found no blocker;
format and diff checks passed. Dependency/native integration remains separate.
