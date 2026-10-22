# Pinned URL Input cursor

This module contains transformation data only. It does not establish a complete
URL, PackageIdSpec, original manifest, loader or current-source proof.

Source: `url` 2.5.8, registry archive SHA-256
`ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed`.
The exact member is `url-2.5.8/src/parser.rs`, SHA-256
`2dd4210e1773fc5d3fdc15ce506ecff30dbf8eb358a79bbbc2835cc30d513c3a`.
The archive/member verification is recorded in `../../../URL_CLOSURE.md`.
The applicable MIT/Apache-2.0 licenses are retained alongside this file.

## Correspondence

- `parser.rs:195–336`: Input storage, filtered iterator, raw `Chars` remainder,
  clone/peek/prefix/count behavior, UTF-8 borrowing and iterator size hint.
- `parser.rs:225–241,379`: absolute parsing trims only boundary U+0000–U+0020.
  `parser.rs:1745–1753` supplies C0/space and tab/LF/CR predicates. Internal
  U+0009/U+000A/U+000D are skipped; Unicode whitespace is not substituted.
- `parser.rs:1209–1251,1556–1589,1658–1686`: path/query/fragment bulk scanners
  bypass filtering to keep exact raw substring boundaries. `next_raw` exposes
  that operation without exposing mutable ownership or a retargetable cursor.
- The pinned callback is absent at the reached `Url::parse` entry
  (`lib.rs:626–631`). Setter trimming and diagnostic-only callback scans are not
  part of this interface. String-prefix operations cover reached consumers;
  callers can use `split_first` for character/predicate decisions.

`count_matching` uses `usize` for the count of consumed scalar values. Its count
cannot exceed the admitted UTF-8 byte length. The original account's pinned
string allowance is 1 MiB (`importer/src/cargo.rs:58`), below the upstream u32
count domain; no count truncation, wraparound, or new support cap is introduced.
Actual URL component offsets remain a separate checked u32 conversion in Output.

## Ownership and work

`Input::new` checks the original spelling's string allowance before trimming or
scanning. It retains only the borrowed `Chars` cursor, with no heap allocation,
retained owned-item charge, or filesystem/hash/comparison event. Clones retain
the same source and are inline checkpoints. UTF-8 returned by `next_utf8` is the
exact source slice, not a reconstructed character buffer.

Each operation visits at most its raw remainder or supplied prefix. A monotone
full iteration visits each input scalar once. Peeks and clones do not imply a
global linear-work proof: parser orchestration must bound its own repeated
scans. The outer parser must check the same Session before post-failure delegate
work; pure cursor operations do not introduce another account or failure latch.

## Prepared tests

Four focused tests compare filtered/raw/UTF-8 traversal, prefix/peek/count/rewind,
exact borrowed pointers and remaining slices against the pinned source, including
all ASCII boundary/interior characters and multibyte Unicode. They also cover
original-spelling refusal before trim, exact string allowance with nonzero prior
item/byte usage, terminal refusal, and exhausted unrelated graph accounts.

`tests/upstream.rs` is the unchanged contiguous `parser.rs:195–336` extraction.
`tests/reference.rs` supplies the separate predicate/absent-callback shim and
accessors to its otherwise private methods. Keep the extraction outside rustfmt.
Root owns archive comparison, registration, compilation and test execution; this
document does not claim those prepared tests have run.
