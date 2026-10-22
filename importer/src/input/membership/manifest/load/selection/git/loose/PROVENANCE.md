# Closed loose-reference lookup and HEAD stage

This implements the complete byte decoder, default loose-only store constructor,
fixed `find_loose("HEAD")` search and gix-discover's reached HEAD stage. The
source-shaped Git read adapter feeds `read::one`; `head::check` accepts only that
stage, not a repository. The enclosing supplied-metadata recognizer now composes
gitdir-file expansion and common-directory/object/refs checks. Initial directory
metadata/current-directory acquisition, complete discovery and current freshness
remain enclosing requirements. Direct HEAD parse failure is
accepted only after reached decoder ownership and refusal have been handled.

The exact Cargo59800466c5c41c444d264b1010b4d57e85a7117f lock pins:

| Archive | SHA-256 |
|---|---|
| gix-ref0.65.0 | 9bbfbce1dfd7d7f8469ddef6d3518376aff664348f153cbe0fc3e58ef993d24e |
| gix-validate0.11.2 | 7bc6fc771c4063ba7cd2f47b91fb6076251c6a823b64b7fe7b8874b0fe4afae3 |
| gix-hash0.25.1 | cb0926d3819c837750b4e03c7754901e73f68b8c9b690753a6372a1bed4eedce |
| faster-hex0.10.0 | 7223ae2d2f179b803433d9c830478527e92b8117eab39460edae7f1614d9fb73 |
| bstr1.12.3 | 5cee35f73844aa3014bb606320a6c1f010249dbdf43342fe54b5a4f6a8ed4b79 |

Archives were checksum-verified before extraction; no downloaded code ran.
Source and member hashes under `/private/tmp/rawr-gix-pinned-sep26`:

| Source | SHA-256 |
|---|---|
| gix-ref/src/store/file/loose/reference/decode.rs | fe788bfe3537f0590abf8d57e6bd0f471cff3b94addb469e199d69e475f12c62 |
| gix-ref/src/parse.rs | 05c3e83e50ddf17a8040c71f22c94ee6f28737caa014625b82cac63d22b3e8e5 |
| gix-validate/src/reference.rs | c4f55edd126be1a28520fc30c8b257f001096fd4afdd24a2b5a535af4d0e42e5 |
| gix-validate/src/tag.rs | 637c67f706573da76bf3af90c30dbb30fe5aa88ed2447041676ff7a44cab4bf4 |
| faster-hex/src/decode.rs | b61ac87fdb0428d0e47fb9ed90e938654c992fb8aaae77fbfc25a896aefbb2cc |
| faster-hex/src/lib.rs | f33f6ac0a69cae21f098d400570c10cf769705fbb901b2a5a72ba08374706458 |
| bstr/src/impls.rs | eacabe264ab591e0dd31413ea36722fc0c5441d243eda02a46202487f239ef8e |
| gix-ref/src/store/file/loose/mod.rs | a9d8aaa91a1d9d236608b29fffc5383bb510d6d413b46f92b6e715a2bcdfdde4 |
| gix-ref/src/store/file/find.rs | f5c1ec346f4a1d97762f1257d163491f4e966177416c89d6e70553a0eb45e7f9 |
| gix-ref/src/name.rs | a099f48b86fa930db93c8d59d834c5a1dae19394747e7683e885d01da8fd41ff |
| gix-discover/src/is.rs | fc32ee101c9a5cc8317db8f6bd4a26351d2240eba7e44ab17adbbeaedcd89f1e |
| gix-fs/src/snapshot.rs | 35623f02e5473f21d4749e026022e33eea403f10407d9a476b204616938ed96a |

`store.rs` contains only default Store::at from loose/mod.rs:37–60, then
find.rs:380–409 and its fixed HEAD search. Cargo's parallel feature selects
gix-features0.48.1 threading.rs:16–18: outer Arc and parking_lot0.12.5 RwLock.
SharedFileSnapshotMut::new (gix-fs snapshot.rs:89) constructs an empty snapshot;
lock_api0.4.14 RwLock::new stores UnsafeCell::new(None) and RawRwLock::INIT.
That raw lock contains only AtomicUsize initialized to zero. No custom destructor,
lock method, parking/TLS initialization or inner Arc allocation is reached here.
The private `Empty` payload retains those actual fields and the always-None Arc
slot, with no locking API. The actual outer Arc is allocated only after the
original Session admits its full padded layout (two AtomicUsize header words plus
payload: 32 bytes aligned to 8 on the pinned aarch64 target). The store owns its
directory copy before this allocation, and drops that path before the Arc. No
high-level gix or parking dependency is added and no packed lookup is implemented.

The borrowed literal HEAD first runs Partial validation, which reaches the tag
grammar without Complete's additional uppercase/slash test. A single BString
equivalent Vec is cleared and reused across all five names. name.rs:77–94 appends
each segment separately, yielding requested capacities 8,16,16,16,32, with full
replacement layouts admitted at growth. The borrowed constructed name feeds the
actual read path, while its buffer remains alive through decoding. That buffer
retires before a missing search allocates the NotFound path `HEAD`.

`head::check` follows is.rs:60–88: temporary joined HEAD path, exists predicate,
then store creation and find. Pinned std small_c_string.rs:24–26,58–63 uses a
384-byte stack buffer only for shorter paths. At length 384 or above,
alloc/ffi/c_str.rs:274–290 allocates len+1 and copies before checking for NUL.
The adapter admits and owns that reached conversion before validation, so prior
refusal wins over local NUL rejection; the short branch uses fixed stack storage.
A source-local NUL yields MissingHead without an observation query. Direct HEAD
ReferenceCreation is accepted after decoding;
alternate success moves its name into MisplacedHead, while alternate decode/read
errors and total NotFound remain errors. The store scope ends before the caller
can start the remaining discovery checks. No current-directory/path normalization
or no-repository fallback is invented by this stage.

The HEAD predicate join and both loose-read joins use the shared source-shaped
Git join (`std/path.rs:3081–3085,1341–1428`): copy the parent, then separately
push separator and suffix with admitted full growth layouts. Existing temporary
and error scopes are preserved. The CString/exists predicate is shared with the
enclosing recognizer without changing the source-local NUL threshold or owner
lifetime; copied-byte and validation work is admitted before execution.

`Candidate` is exactly the five constructed full names from gix-ref find.rs:76–156
for constant HEAD. Its successful-read caller clones that full name before
`Reference::try_from_path` parses content (:47–94). Symbolic parsing owns the
trimmed target before validation and moves it into success/error. Parse failures
copy all original bytes; invalid-byte validation additionally owns a one-byte
diagnostic. Partial name ownership remains alive through target/error construction.
These buffers use exact admitted copies, not unaccounted BString conversions.

`name.rs` retains gix-validate Complete→tag Validate error order and component
indices. Sanitization and branch-specific reserved-name checks are not reached.
Byte strings remain native bytes; no UTF8 check occurs. bstr suffix checks call
slice ends_with; find_byte calls exact memchr2.8.2, whose allocation-free ARM
closure is documented in the existing matcher dfa/search/PROVENANCE.md. One actual
search event is admitted before calling it. Sequential validation admits byte
events; no new cap or diagnostic truncation is introduced.

Cargo enables gix sha1. gix-hash/lib.rs:96–105 defaults Kind to SHA1 and its
kind.rs:91 supplies40 hex characters. ObjectId::from_hex then allocates only a
stack20-byte digest. The supported target is aarch64-apple-darwin: faster-hex decode.rs:105,
225–254 therefore selects the scalar table validation and fallback at297–303,
without x86 dispatch/cache/unsafe. `hash.rs` preserves both table construction
and the second validation pass after gix's ASCII validation. Its fixed40-byte
precondition permits a fixed helper event and stack result; impossible private
input is terminal State, not a source parse branch. No hashing/object lookup runs.

All copies and semantic results use the original Session; a refusal cannot be
reclassified as native/parse error or discovery fallback. No partial successful
Reference escapes after failure. Attached parent git/LICENSE-* cover gitoxide;
LICENSE-FASTER-HEX is copied from its verified archive.

The post-implementation comparison copies the unchanged tag validation enum/body
into test-only reference.rs, supplying only byte-string aliases/utility methods
and the existing error type outside that copied body. SHA-256:
2c261cb1962e23632fdf2f22a38b9c268b2f30185b81dd4a4b96e89b1b5ff9dc.
Keep it out of rustfmt. Small checks exercise exact error precedence and byte
classes, all five candidate names, hash width/delimiters, owned errors and terminal
refusal. They establish no native or product qualification.
