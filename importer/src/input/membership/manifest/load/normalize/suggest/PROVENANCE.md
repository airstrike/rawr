# Closest target-name suggestion

Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/edit_distance.rs:1–109`; its unchanged first 125 lines form the
test-only reference. The caller owns `closest_msg` suffix rendering.
The supported compiler pin is rustc `375b1431b7d89d1c2e2bc168c011848ae12b7d14`,
nightly-2026-07-11. Its `library/alloc/src/str.rs:213–227,433–466` supplies
contextual sigma behavior; `library/core/src/char/methods.rs:825–831,1293–1299`
supplies Cased and Case_Ignorable definitions. License texts are retained in
`../../path/LICENSE-MIT` and `../../path/LICENSE-APACHE`.

The production helper has no `str::to_lowercase` allocation: it directly retains
the identical lowered characters in an admitted buffer. Non-sigma mappings use
the pinned allocation-free `char::to_lowercase`; sigma scans the original string
backward/forward across Case_Ignorable scalars exactly as pinned std. Stable
lowercase/uppercase predicates plus the pinned Lt table implement Cased.
No unstable titlecase/case_ignorable crate features are introduced.

`generate.py` mechanically evaluates the pinned unicode_data.rs skip-search
tables over the scalar domain and emits equivalent half-open ranges for
Case_Ignorable and Lt. The generated file records the exact source SHA256.
Generation verifies pinned counts (459 ranges/2789 codepoints and 10/31), and
is an offline maintainer action; production has no Python dependency. ASCII
Case_Ignorable uses std's separate five-character branch.

Both inputs are lowered per candidate, as Cargo does. The longer/shorter swap,
length rejection, common prefix/suffix removal, three-row restricted
Damerau–Levenshtein recurrence, limit three, and first equal-distance candidate
are preserved. Owned char/row buffers use prospective admitted storage; decoded
UTF-8 length is checked before each character retention. Candidate probes,
context scans, property-search steps, prefix/suffix comparisons, rows and cells
are admitted before work, on the original sticky Session. Matrix arithmetic is
bounded by the lengths of already-admitted char/usize layouts; the sentinel row
is never incremented before it has rotated to the finite initial row.

Tests compare with the unchanged pinned distance reference, std lowercase at
all sigma-property interval boundaries, tie/threshold behavior, every admission
refusal, and exact/one-short item/string/allocation bounds.
