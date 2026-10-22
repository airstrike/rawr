# Pinned opening-path NFC

This is the pure conditional path step in gix 0.85.0
`open/repository.rs:200–224`, connected after local/worktree StageOne. It transforms
git-dir, common-dir, worktree, then the distinct opening cwd, in source order.
The git-dir Cow is owned; the other owners are borrowed and replaced only when
normalization returns owned text. Invalid UTF-8 and already-NFC spelling retain
the original Cow and storage. This completes neither RefStore opening nor native
mapping/producer authority.

Sources are the Cargo `59800466c5c41c444d264b1010b4d57e85a7117f` lock cohort:

| Crate | Archive SHA-256 |
|---|---|
| gix-utils 0.3.3 | `66c50966184123caf580ffa64e28031a878597f1c7fceb8fe19566c38eb1b771` |
| unicode-normalization 0.1.25 | `5fd4f6878c9cb28d874b009da9e8d183b5abc80117c40bbd187a1fde336be6e8` |
| tinyvec 1.11.0 | `3e61e67053d25a4e82c844e8424039d9745781b3fc4f32b8d55ed50f5f667ef3` |
| tinyvec_macros 0.1.1 | `1f3ccbac311fea05f86f61904b462b55fb3df8837a366dfc601a0161d0532f20` |

Archives were verified before extraction under `/private/tmp/rawr-gix-pinned-sep26`.
The reached sources are gix-utils `str.rs:3–37`; unicode-normalization
`quick_check.rs`, `decompose.rs`, `recompose.rs`, `normalize.rs`, `lookups.rs`,
`perfect_hash.rs`, and `tables.rs`; tinyvec `tinyvec.rs:967–995` and
`arrayvec.rs:1096–1111,1938–1945`. They are MIT OR Apache-2.0 licensed; the attached
licenses accompany the extracted Unicode code. No dependency is added.

`tables.rs` contains two unchanged excerpts from the verified Unicode 17 table:
the contiguous canonical combining/composition/decomposition declarations before
`COMPATIBILITY_DECOMPOSED_CHARS`, and the complete `qc_nfc` function. Only the
surrounding module imports and extraction note differ. Extracted-file SHA-256:
`e60c4d3df135efc6c9946b50312a9806ff62d29490187b0880018e906ee2de97`.
Keep this generated table out of rustfmt. The compatibility, stream-safe and
decomposition-only public APIs are not reached by this NFC path.

QuickCheck's Maybe branch performs the actual original-character/NFC iterator
comparison; it may allocate and stop on the first mismatch. A failed check then
creates a fresh NFC iterator for String collection. Both iterators retain the
source's separate decomposition and recomposition buffers. The four-slot inline
buffers retain stale primitive cells on truncation; a fifth push drains into an
eight-slot Vec, then heap growth follows requested Vec capacity. Actual fallible
replacement storage and its transfers are prospectively admitted without using
allocator bonus capacity as authority. Checked Hangul scalar construction
replaces only source-proved unchecked conversions, not normalization rules.

Combining-class sort uses the same safe pinned standard-library stable sort on
`(u8,char)`. The source is nightly-2026-07-11's core
`slice/sort/stable/mod.rs:29–136` and alloc `slice.rs:856–875`: insertion sort
through20, otherwise scratch length
`max(n-n/2, min(n, 8_000_000/size_of::<T>()), SMALL_SORT_GENERAL_SCRATCH_LEN)`.
The4096-byte stack buffer covers through512 entries for this eight-byte tuple;
above that boundary the small-sort minimum is already dominated. The exact
scratch heap Layout is admitted before calling the closed sort. Its one real
operation is charged; finite linearithmic comparison bounds are not represented
as fictional executed Items. The comparator is a pure u8 key with no allocations
or external state.

Pinned String `FromIterator<char>`/`Extend<char>` starts empty, reserves the NFC
iterator's default zero lower bound, then pushes each scalar. The contained
String uses the same requested byte growth and owns the completed allocation
through conversion into PathBuf. Every scan, buffer operation, lookup, actual
replacement transfer and allocation uses the original Session; first refusal
prevents further transformation. No per-path account is reset.

Focused checks cover composition and equal-class stability, Hangul and astral
tables, already-normalized Maybe cases, invalid native bytes, preserved owned
capacity, both buffer spills, wide-sort scratch refusal, and real codec/StageOne
control of all four opening paths. These remain component checks, not Gallery
or mapped-reference qualification.

Root validation on 2026-09-30: all44 focused Git tests passed in0.02s after
1m12s compilation with the pinned toolchain, offline/locked, one job and no debug
info/incremental output. Log: `/private/tmp/rawr-p9-git-precompose-sep30.log`.
Independent source/artifact review found no actionable defect. Root separately
compared both generated table excerpts byte-for-byte with the pinned source;
the extracted-file hash above is unchanged. Scoped formatting used the installed
nightly-2026-08-29 rustfmt with style edition2021; the compiler remained pinned.
