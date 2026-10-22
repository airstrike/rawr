# Pinned frontmatter source-failure observation

Root executed this test-only wrapper on 2026-09-06 with nightly-2026-07-11.
It includes the unchanged source from Cargo
`59800466c5c41c444d264b1010b4d57e85a7117f` through `RAWR_CARGO_SOURCE`.
The reference links existing `rawr-manifest-reference` dependency artifacts,
including pinned Winnow 1.0.3. It is not a product dependency or panic handler.

Reproduce by building this file with rustc, edition 2024, setting
`RAWR_CARGO_SOURCE` to the verified Cargo tree, supplying `-L dependency` for
that reference package's debug/deps directory and explicit `--extern` paths for
`serde_json`, `cargo_util_schemas` and `winnow`. Send one JSON string per stdin
line; `observations.jsonl` retains all six source strings and actual outcomes.
The stderr log retains panic locations and both bounds/UTF-8 failure reasons.

Observed: four panics and two ordinary returned errors. The complete-fence
search precedes the failing partial search. This evidence alone does not prove
complete expansion or loader equivalence.

SHA-256 identities:

- Wrapper: `ff175f0a67ea653cfda9c21effed2a86adff462b982be82465c3ea330a780621`
- Executed binary: `910b7409f5b6754a8448fbc2d3875351c9eed49aa35560b7917543e9484c4cbe`
- Observations: `eecab7eef39172a4bf4ac059f34733ffaa0f5df4872def785e771a0627da7aed`
- Stderr: `cb62ae8aae69f0b135f1825e912ed4d46927d6604d83f78b7b9dfd24e109e8f4` (panic paths normalized to `~/.rustup`)
- Cargo frontmatter.rs: `4a7b8ace3771f0e805aa2e542569878eaede405e2998ff99c09916a4635c11c7`
- Cargo embedded.rs: `1a40a7c36007c4a0001412d147a2dcef1729ca38b39ed795e3ee0b41c62ee445`

## Expanded corpus and regeneration

`expansion.jsonl` is a separate, expanded corpus of **335 cases: 59 successful
expansions, 247 returned errors and 29 panics**. These counts describe the current
recorded run, not the six-case `observations.jsonl` or its `stderr.log` above.
The expanded corpus covers whitespace/shebangs, fence lengths and partial
matches, UTF-8 boundaries, infostrings, line endings and trailing content.

`generate.py` takes an already compiled copy of `reference.rs`; it neither builds
nor invokes Cargo. It reads only the 335 `source` strings from the existing
`expansion.jsonl`, preserving order and repeated cases, and feeds each string to
the executable as one JSON input line. Saved expected outcomes are never inputs
to the reference calculation. The corpus is the explicit source inventory;
regeneration does not reconstruct a different matrix from inferred patterns.

For the root execution owner, with the actual compiled reference path supplied:

```sh
python3 importer/reference/manifest/frontmatter-panic/generate.py \
  --binary /path/to/compiled/frontmatter-reference \
  --output /private/tmp/rawr-frontmatter-expansion-regenerated.jsonl \
  2> /private/tmp/rawr-frontmatter-expansion-regenerated.stderr.log
```

Omitting `--output` replaces `expansion.jsonl` only after a zero process exit,
exactly one well-formed observation per source, and exact source/order checks.
`--sources` selects another copy of the same 335-case source inventory. The script
prints actual outcome counts, byte length and SHA-256; counts are reported rather
than copied from the saved expectations. Output JSON object keys are sorted and
JSONL framing uses LF without splitting Unicode separators inside strings.
Reference panic-hook stderr passes through to the caller; the earlier six-case
stderr artifact is not modified. No generator execution is claimed by this
addition, and expanded observations alone do not establish complete loader or
membership authority.
