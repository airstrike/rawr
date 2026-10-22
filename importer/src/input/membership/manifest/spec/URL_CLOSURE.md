# Pinned PackageIdSpec URL execution closure

Cargo commit: 59800466c5c41c444d264b1010b4d57e85a7117f.
Source deductions only; no complete contained URL parser or product authority follows.

## Verified registry pins

- form_urlencoded 1.2.2: `cb4cb245038516f5f85277875cdaa4f7d2c9a0fa0468de06ed190163b1581fcf`; cached archive verified.
- icu_normalizer 2.2.0: `c56e5ee99d6e3d33bd91c5d85458b6005a22140021cc324cea84dd0e72cff3b4`; cached archive verified.
- icu_properties 2.2.0: `bee3b67d0ea5c2cca5003417989af8996f8604e34fb9ddf96208a033901e70de`; cached archive verified.
- idna 1.1.0: `3b0875f23caa03898994f6ddc501886a45c7d3d62d04d2d90788d47be1b1e4de`; cached archive verified.
- idna_adapter 1.2.2: `cb68373c0d6620ef8105e855e7745e18b0d00d3bdb07fb532e434244cdb9a714`; cached archive verified.
- percent-encoding 2.3.2: `9b4f627cb1b25917193a259e49bdad08f671f8d9708acfd5fe0a8c1455d87220`; cached archive verified.
- smallvec 1.15.2: `8ed6a63f02c8539c91a8685a86f4099661ba3da017932f6ebbea6de3f0fa7c90`; cached archive verified.
- url 2.5.8: `ff67a8a4397373c3ef660812acab3268222035010ab8680ec4215f38ba3d0eed`; cached archive verified.
- utf8_iter 1.0.4: `b6c140620e7ffbb22c2dee59cafe6084a59b5ffc27a8859a5f0d494b5d52b6be`; cached archive verified.

## Reached source members (SHA-256)

- `/private/tmp/rawr-p9-pinned-cargo-full/cargo-59800466c5c41c444d264b1010b4d57e85a7117f/crates/cargo-util-schemas/src/core/package_id_spec.rs`: `86aac55339ee2c8f512aa73ccbf6e2ea9a13ec9ea143416e271b5d1d0b344d34`.
- `/private/tmp/rawr-p9-pinned-cargo-full/cargo-59800466c5c41c444d264b1010b4d57e85a7117f/crates/cargo-util-schemas/src/core/source_kind.rs`: `f3b4cc73dabe93722aa64d6397474102399b0f7d17cf8daa37ff70a888589837`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/url-2.5.8/src/lib.rs`: `51f47a93654528f3b8cb92c94fc26ad25775860d84197fff0c3124a687f8c5e3`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/url-2.5.8/src/parser.rs`: `2dd4210e1773fc5d3fdc15ce506ecff30dbf8eb358a79bbbc2835cc30d513c3a`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/url-2.5.8/src/host.rs`: `24bba45b14cb843ea0fe5c857284d458097a13a84847f5c6a15a14c9e1334335`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/idna-1.1.0/src/lib.rs`: `13d06c4bcb530498b5414d948e0b08c987000eb7a0a8c441f5c25b116298633e`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/idna-1.1.0/src/uts46.rs`: `9726647c01f315215eca25b7cb706929ff829269d4143de06b4f4718538b6640`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/idna-1.1.0/src/punycode.rs`: `0cc6fa84989898ccf6de0a665a287d7488a6eba4b178bea065d311ecabd81604`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/idna_adapter-1.2.2/src/lib.rs`: `b80b2f763904dcf818000bf3fd146da4188cd848e46f4981cc984e2f3990cf7c`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/icu_normalizer-2.2.0/src/lib.rs`: `a4ddf07687cf76033c8cff7f29550f5688f55e6363572881dd250a812ac6b39f`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/icu_normalizer-2.2.0/src/uts46.rs`: `b2f7093211a0bf946ee59927fb988a2915643f29823bd5709417b8950747e2eb`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/icu_normalizer-2.2.0/src/provider.rs`: `5850afc7ae842c7af74ce029be256944c64f5d0b51d95725a8366f5af22163e9`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/form_urlencoded-1.2.2/src/lib.rs`: `766b5d679064e01f7e6cce6f127a23885f79806ce3bccc49e4fe41933b83fd8d`.
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/percent-encoding-2.3.2/src/lib.rs`: `641903d385581e76f31ef8c991d83c2a5d7fbbc2879d93909c539037e4e1e3e2`.

The twelve registry source members above were compared byte-for-byte with
members of the separately checksum-verified pinned crate archives. Cargo source
uses the previously verified full Cargo archive; this note adds no new Cargo
archive verification claim.

## Exact reached operations

1. PackageIdSpec::parse (`package_id_spec.rs:80`) tests raw `://` before parsing
   URLs. Invalid URL parsing falls through to the complete plain spec parser.
   The diagnostic-only non-URL path probe omission is separate from this
   URL trial; resource refusal cannot become its semantic fallback.
2. Url::parse has no base, no query encoding override and no violation callback.
   `parser.rs:379–474` trims boundary C0/space and skips ASCII tab/newline inside
   its Input iterator, normalizes scheme case and dispatches file/special/other.
   Base-dependent relative branches and diagnostic closure formatting are
   unreachable at this entry. Successful opaque URLs can still reject later for
   missing path segments; accepting only conventional HTTP/file/git URLs would
   narrow the source grammar.
3. For a compound source scheme, PackageIdSpec handles git, registry, sparse and
   path with distinct query and strip/reparse rules (`:111–153`). A compound
   scheme initially uses non-special URL host handling; stripping its prefix and
   reparsing may switch to special URL normalization. Reusing the first parsed
   host cannot replace the second parse. Sparse retains its compound scheme.
4. Git query parsing uses form_urlencoded 1.2.2, with plus substitution, valid
   percent escapes, malformed escape preservation, UTF-8 loss repair, empty-pair
   skipping and first-equals splitting (`form_urlencoded/src/lib.rs:39–104`).
   GitReference::from_query (`source_kind.rs:100`) recognizes branch/ref/rev/tag
   and keeps the last recognized pair; unknown pairs do not clear it. Pairs never
   reject semantically. The contained query helper borrows the final pair and
   materializes only its actual transformed storage, preserving all output.
5. Removing query retains the parsed fragment. In stock URL this owns a temporary
   fragment (`url/src/lib.rs:1637`); extracted storage can retain spans/references
   where equivalent. Removing fragment also calls opaque-path trailing-space
   stripping (`:1582`). Follow exact ordering, including subsequent strip/reparse.
6. Final fragment parsing is not form decoding. Split @ or applicable final :,
   otherwise alphabetic-first fragment means a name and another first character
   means a PartialVersion of the final path component (`package_id_spec.rs:159`).
   Empty fragments reject. The complete final PackageName validator still runs.
7. Url ordering compares normalized serialization (`url/src/lib.rs:2840`).
   PackageIdSpec derives name -> optional PartialVersion -> optional URL ->
   optional SourceKind ordering; GitReference declared variant order is
   Tag, Branch, Rev, DefaultBranch. Typed map key equality is not raw spelling.

## Ownership not covered by a sink

- URL parser serialization, input-with-tabs host collection, percent-decoded
  host storage, normalized host output, IPv4 parsing vectors, userinfo/path/query
  percent expansion and fragment preservation own storage. IPv4 vectors can be
  replaced by exact fixed local values after proving the full rejection order.
  Invalid/successful URL trials and prefix reparse remain part of the accounting.
- Special hosts reach idna::domain_to_ascii_from_cow with AsciiDenyList::URL
  (`host.rs:92`). Ordinary opaque hosts have a separate forbidden-character and
  percent-encoding path (`:123`). Unicode hosts cannot be excluded.
- idna::uts46::process's output Write interface does not contain its storage:
  domain_buffer SmallVec<char;253>, already_punycode SmallVec<...;8>, and label
  buffers SmallVec<char;59> can grow (`uts46.rs:773,1055,1160,1319`). Punycode has
  its own pinned limits/work semantics; neither guessed linear work nor a new
  DNS-size support cap follows.
- Pinned idna_adapter is 1.2.2, not the also-cached 1.2.1. Its compiled-data
  property constructors borrow immutable tables, but map_normalize and
  normalize_validate delegate to ICU normalization iterators. These iterators
  own a SmallVec<CharacterAndClass;17> (`icu_normalizer/src/lib.rs:566`), extend
  it during decomposition/combining collection, and use stable sort_by_key
  (`:543–555`) which can also allocate for wide admitted inputs. The iterator
  API is therefore not an allocation-free dependency boundary.
- ICU combining-mark stability must be retained. An unstable sort on only CCC
  changes equal-class order; an explicitly admitted index tie-break can preserve
  that order only with corresponding traversal/output proof. Static compiled
  tables and fallible contained normalization need separate actual cfg/version
  and requested-layout evidence before activation.

This local inventory identifies reached implementation, not a completed URL
ownership/work proof. The active contained interpreter must not publish Original
until that remaining scalar closure is complete. No support/cap/command change
or broad qualification follows from this audit.
