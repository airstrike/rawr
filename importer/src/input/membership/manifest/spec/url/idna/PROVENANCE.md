# Contained fixed-entry UTS46 domain conversion

This transformation implements the complete entry reached by url 2.5.8
`Host::parse_cow`: idna 1.1.0 `domain_to_ascii_from_cow` with
`AsciiDenyList::URL`, `Hyphens::Allow`, `DnsLength::Ignore`, fail-fast errors,
ASCII output, and no secondary sink. It returns typed domain data through a
`transformation-method` with private storage and read-only access. It does not
provide URL, host, IPv4, package identity, source or membership authority.

## Exact sources and active data

Registry archives are published at
`https://static.crates.io/crates/<name>/<name>-<version>.crate`. Their pinned
versions/checksums and full URL closure are recorded in `../../URL_CLOSURE.md`.

| Archive | SHA-256 |
|---|---|
| idna 1.1.0 | `3b0875f23caa03898994f6ddc501886a45c7d3d62d04d2d90788d47be1b1e4de` |
| idna_adapter 1.2.2 | `cb68373c0d6620ef8105e855e7745e18b0d00d3bdb07fb532e434244cdb9a714` |
| utf8_iter 1.0.4 | `b6c140620e7ffbb22c2dee59cafe6084a59b5ffc27a8859a5f0d494b5d52b6be` |

| Reached source member | SHA-256 |
|---|---|
| idna `src/lib.rs` | `13d06c4bcb530498b5414d948e0b08c987000eb7a0a8c441f5c25b116298633e` |
| idna `src/uts46.rs` | `9726647c01f315215eca25b7cb706929ff829269d4143de06b4f4718538b6640` |
| idna_adapter `src/lib.rs` | `b80b2f763904dcf818000bf3fd146da4188cd848e46f4981cc984e2f3990cf7c` |
| utf8_iter `src/lib.rs` | `5ac63cc86ea5ff25ac5b2988f172efa9f338ea1214874f16e568c965b210e427` |

The implementation author re-read and hashed these source members. Root verified
the registry source/data files against their pinned archives. Exact reference
features are retained in `importer/reference/manifest/features.json`: idna
alloc/compiled_data/std, idna_adapter compiled_data, ICU normalizer/properties
2.2.0 compiled_data, and provider 2.2.0 baked. Root owns dependency wiring and
effective build checks; this source preparation does not claim a new execution.

The production code uses exact idna_adapter 1.2.2 `Adapter::new` only for
`is_mark`, `is_virama`, `bidi_class`, `joining_type`, and the adapter's immutable
mask values. Construction borrows baked tables. Unlike 1.2.1, 1.2.2 obtains
Virama from its mapper; that property operation does not start a normalization
iterator. No stock `map_normalize` or `normalize_validate` iterator is called.
Both operations use the separately contained fallible `normalize` module;
its initialization and every `next` receive the same original Session.

The idna source is MIT OR Apache-2.0. Both full licenses are copied unchanged
beside this document. Their SHA-256 values are respectively
`b38f11f6096706e6de553dabe2a7ed142d59b6fa8c97e290c67496154745cdd5`
and `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2`.
The normalization subtree has its separately recorded ICU4X provenance/license.

## Source correspondence

| Reached behavior | Exact source |
|---|---|
| URL host dispatch and fixed entry flags | url `src/host.rs:92–109`; idna `src/lib.rs:132–142`; `uts46.rs:542–589` |
| URL ASCII deny list and uppercase handling | idna `uts46.rs:57–68,204–232,276–361` |
| Fastest lowercase/dot prefix, strict prefix labels and retaining one ASCII scalar before non-ASCII for composition | Same `:142–203,1049–1148` |
| ASCII Punycode branch, decoded label ownership, original mixed-case source marker | Same `:1151–1205` |
| Original source-dot restart and normalized-dot continuation | Same `:1114–1131,1238–1383` |
| Character Punycode preconditions, decode and replacement of the current label | Same `:1260–1348` |
| Normalize/validate the entire decoded label, then compare using zip (without a new length equality condition) | Same `:1497–1540` |
| Combining mark, ContextJ, Virama/joining and non-ASCII 1000-scalar guard | Same `:1543–1654` |
| Bidi detection exclusions, first/last/middle classes and numeral compatibility | Same `:1387–1493,1657–1683` |
| Deferred sink writes, extended passthrough, original Punycode spelling reuse | Same `:773–930` |
| Loss replacement for malformed UTF-8 consumed by the mapper | utf8_iter `src/lib.rs:106–228,262–272` |

Only branches fixed by this entry are removed: optional hyphen restrictions,
DNS-length verification, non-fail-fast replacement output, Unicode-output
selection, the secondary ASCII sink, and tld/bidi arguments to a closure that
always returns false. The remaining source order is retained. In particular,
semantic validity is checked before the output sink phase; bidi checks run after
the entire non-prefix domain has been processed. Strict prefix labels are
already valid for either direction and stay outside the materialized buffer.

Empty domains/labels, repeated/trailing dots, permitted ASCII punctuation,
unrestricted hyphen positions and arbitrarily long admitted ASCII labels remain
supported. A later Host visitor owns the empty-host check. The only fixed label
work limits are the source's Punycode payload ceiling of 2000 code units and
non-ASCII label ceiling of 1000 scalars. No 63-byte DNS label cap or 253-byte DNS
domain cap is added. Uppercase/Unicode mapping and validity, malformed UTF-8,
joining context and bidi cannot fall back to an unchecked spelling.

## Ownership, failures and bounded work

`Ascii::interpret` checks the original Session and admits the input byte length
through `Session::string` before traversal, including borrowed/empty results.
Unchanged bytes remain borrowed; the complete modified result owns a charged
byte Buffer. It exposes no allocating string conversion. All original source
label markers, domain characters, temporary decoded characters, Punycode
storage, normalized continuation storage and sink output remain within that
same original account. Each actual new character or byte slot receives item and
full replacement-layout admission through Buffer. Temporary ownership remains
spent after semantic failure or successful replacement; allocator bonus
capacity never grants credit.

Character buffers track their actual UTF-8 extent, admitting the complete
current string length before pushing each scalar. Truncation subtracts exactly
the already-admitted removed widths without refunding cumulative ownership.
The ASCII sink similarly checks its full byte length before each push. The
contained encoder temporarily owns its own Punycode bytes before they are
copied into the sink; both actual layouts/copies are charged. The `xn--` prefix
is written before encoding, matching the source sink order.

Public-private result shape is `Step<Result<Ascii, Invalid>>`. The fixed semantic
error is an admitted logical item. Resource/allocation failure stays in the
outer Step and is never converted into semantic invalidity, a retry or a
borrowed fallback. The internal Failure adapter only preserves these two
channels; it allocates nothing. Entry/session checks prevent another traversal
after refusal. Immutable Unicode properties and input slice iteration do not
mint filesystem, hashing or decoding operation events.

All owned containers are flat. Domain slicing, input prefixes, label markers,
insertion positions and scalar/byte extents refer to prior admitted values.
Index increments correspond to consumed source bytes; decoded positions use
the pinned 2000-unit bound. The only unbounded-by-constant loops walk admitted
source/normalized/output extents or perform the pinned ContextJ searches and
Punycode work. The normalization module supplies its own complete continuation
and ordering bounds. No recursion, custom recursive Drop, hidden sort scratch,
filesystem/environment query or native command is introduced here.

## Focused validation contract

The 13 tests invoke the actual contained full entry. Five differential tests
make 1316 deterministic comparisons with the exact dev-only idna 1.1.0 entry,
including complete bytes and Borrowed/Owned result classification. They cover
every ASCII byte, every non-ASCII byte in multiple contexts, malformed/truncated
UTF-8, all mapped dot forms, canonical composition/combining runs, ignored and
disallowed scalars, non-BMP input, ContextJ, bidi, Punycode validation, prefix
restart, lowercase reuse and the existing work boundaries.

Other tests check explicit supported outputs, empty/long ASCII domains, exact
original input string ceilings on passthrough, every observed request refusal
and actual allocation attempt, permanent refusal after failure in early or
delayed phases, and exact/one-under cumulative item/byte/string limits with prior
usage. Reference/fixture construction uses ordinary test allocations; production
does not. Root owns all Cargo/native execution and formatting. These source-ready
tests are not a claim of executed differential parity or whole URL/Original
completion.
