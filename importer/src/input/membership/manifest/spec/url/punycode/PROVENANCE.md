# Contained Punycode correspondence

This is the internal Punycode transformation reached by the pinned IDNA program.
Its result is neither a normalized domain nor a URL/package identity. Construction
and read-only access use `smart-constructor-newtype`; semantic failure is a
`structured-error` separate from terminal original-account refusal.

## Exact source

The source root is
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/idna-1.1.0`.
The registry archive checksum is
`3b0875f23caa03898994f6ddc501886a45c7d3d62d04d2d90788d47be1b1e4de`.
The enclosing `../../URL_CLOSURE.md` records the complete pinned dependency
inventory and archive/source verification; this slice does not claim closure of
the other IDNA/ICU operations.

| Exact member | SHA-256 |
|---|---|
| `src/punycode.rs` | `0cc6fa84989898ccf6de0a665a287d7488a6eba4b178bea065d311ecabd81604` |
| `src/uts46.rs` | `9726647c01f315215eca25b7cb706929ff829269d4143de06b4f4718538b6640` |
| `LICENSE-APACHE` | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| `LICENSE-MIT` | `b38f11f6096706e6de553dabe2a7ed142d59b6fa8c97e290c67496154745cdd5` |

Both complete licenses are retained beside this document. The source's license
choice is MIT OR Apache-2.0, copyright 2013 the rust-url developers. Production
does not call the upstream allocating `SmallVec`, `String`, or public convenience
wrappers. The extracted Bootstring arithmetic has no target/feature cfg branch.

## Guard boundary and retained control flow

| Contained operation | Exact upstream source and retained behavior |
|---|---|
| Bootstring parameters and bias adaptation | `punycode.rs:21–39`: base 36, thresholds 1/26, skew 38, damp 700, initial bias 72 and scalar 128; same integer division and update order |
| Byte source | `punycode.rs:110–137`: both cases accepted as encoded digits; unchanged base bytes converted individually to chars and ASCII-lowercased; the internal path does not run the external base-ASCII test |
| Character source | `punycode.rs:139–163`: only lowercase encoded letters accepted; unchanged base chars returned without lowercasing, including non-ASCII chars if directly supplied |
| Decoder | `punycode.rs:165–263`: clear reusable insertions, last delimiter split, leading delimiter retained in encoded suffix, checked digit product/addition/weight and code-point addition, same bias/position update and scalar rejection |
| Decoder iterator | `punycode.rs:266–314`: final insertion positions and base have the same order and exact remaining length; source and scratch are independently borrowed |
| Encoder | `punycode.rs:355–450`: input-order ASCII emission, checked input count and internal global product, delimiter if any basic characters, next scalar minimum scan, internal delta arithmetic, variable-length digits and bias order |
| Digit output | `punycode.rs:454–460`: lowercase ASCII letters then ASCII decimal digits |
| Work guards | `uts46.rs:31–44`: encoded payload at most 2000 code units, non-ASCII label at most 1000 Unicode scalar values; these are existing program guards, not new resource-account limits |
| Byte decode caller | `uts46.rs:1151–1157`: after `xn--`, reject terminal `-` and payload over 2000 before `Decoder<u8, InternalCaller>` |
| Character decode caller | `uts46.rs:1260–1321`: ASCII/precondition checks, terminal delimiter and payload-length checks precede `Decoder<char, InternalCaller>` |
| Encode caller | `uts46.rs:108–114,1625–1634`: caller writes `xn--`; the label check constrains only non-ASCII labels over 1000, preserving ASCII fast paths |

The local constructors apply the two existing work guards and otherwise preserve
the internal algorithm. In particular, they do not add the DNS 63-byte rule,
forbid a terminal delimiter, recognize an `xn--` prefix, lowercase encoder input,
normalize Unicode, validate IDNA context/bidi, or manufacture replacement
characters. The later UTS46 layer owns those operations and their fail-fast or
replacement/error behavior. Internal Punycode can successfully return empty or
ASCII-only output that a particular UTS46 caller must reject.

ASCII encoder inputs longer than 1000 remain allowed through the same linear
ASCII emission and pinned checked product. The exact low-level product permits
`input_length + 1 <= 3855`; larger all-ASCII inputs fail after their basic output
has been attempted, as in the internal source. No partial encoded buffer is
returned. Non-ASCII inputs over 1000 are rejected before entering the encoder,
matching the reached UTS46 guard. Direct reference comparison therefore covers
the guarded domain; the original internal function alone does not enforce these
caller guards.

## Ownership and accounting

`encode` and the two reusable `Decoder` methods take the caller's existing
`Session`. Their outer `Step` carries terminal refusal; their inner `Result`
carries a complete transformed output or a fixed semantic `Invalid` tag.
`Session::check` runs before reading input or clearing scratch. A retained
semantic tag requests one actual item and can itself be refused. A refused
request therefore cannot be mistaken for a semantic trial rejection.

The encoded result privately owns `Buffer<u8>`. Before each actual ASCII output
byte, it checks the resulting string length and delegates item/layout admission
to `Buffer::push`. Growth uses the existing full replacement layout sequence
4, 8, 16, …; allocator slack never authorizes a push. All previous layouts remain
spent even when replacements or partial outputs are dropped. Output is exposed
as a read-only byte slice; any later string copy is the caller's actual ownership
transition and must use the same account.

The reusable decoder privately owns `Buffer<Insertion>`, one concrete
`{position: usize, c: char}` record per successfully decoded non-basic scalar.
Each insertion is prospectively charged by the same Buffer, including growth
past the upstream inline capacity of 59. Existing records are shifted in place;
clearing or a semantic failure never shrinks/refunds admitted capacity. Sorting
uses the existing allocation-free heapsort, with no hidden sort scratch.

The successful `Decode` iterator borrows the source base and decoder scratch;
it creates no character vector or UTF-8 string. Its source lifetime is independent
of its scratch lifetime. Holding the iterator prevents another mutable decode
on that scratch. Subsequent char/string materialization is not speculatively
charged here: its caller must admit its real records/layouts and expanded string
length. Reuse belongs to the same original-account interpretation; this type is
not a new account or permission to reset budgets.

## Work, arithmetic and teardown proof

These are source deductions, independently testable against the exact reference:

1. With payload length `n <= 2000`, every insertion consumes at least one suffix
   code unit. Base length plus insertion count is at most `n`; every count and
   position cast to `u32` is therefore exact. Updating the previous insertions
   performs at most `m(m-1)/2 <= 1,999,000` constant-time comparisons for
   `m <= 2000`; it does not charge fictitious filesystem/comparison events.
2. Before insertion at `i`, the old positions are distinct. Shifting precisely
   those at or after `i` leaves a hole at `i` and preserves distinctness. The new
   position is in `0..=old_length`, so induction gives distinct in-range positions
   for the complete output. Consequently the source stable sort has no equal-key
   case; allocation-free heapsort produces exactly the same order in
   `O(m log m)` comparisons. Removing the source iterator's retry loop is valid:
   when no insertion occupies a position below output length, the next base unit
   must occupy it. No gap can cause an unbounded loop.
3. Digit accumulation retains all checked operations and their order. Each
   continued digit multiplies the weight by at least 10; `u32` overflow terminates
   the digit loop before its threshold counter could overflow. In `adapt`, the
   halving/damping occurs before the addition; its final multiply/add operates
   only after delta is reduced to at most 455. A code-point overflow or invalid
   Unicode scalar is rejected before constructing an insertion.
4. For non-ASCII encoder inputs, `n <= 1000`. Each completed outer pass handles
   at least one distinct scalar, hence at most `n` passes, each with two bounded
   input scans. The source's RFC 3492 arithmetic bound is retained as an actual
   checked guard: `(char::MAX - 128) * (n + 1) < u32::MAX`. Internal unchecked
   delta operations have the same guarded domain. Generalized digit output has
   at most eleven bytes per scalar because each continued step divides `q` by
   at least 10. This is a work/output bound, never a guessed allocation charge.
   ASCII-only inputs do no quadratic pass and are source-length linear until
   the original account or the pinned integer guard refuses them.
5. The only retained containers are flat buffers of primitive bytes or fixed
   insertion records. Input slices, iterator state, and semantic tags are fixed
   size. Success, partial semantic failure, refusal and allocation failure all
   have constant-depth teardown; no recursive node, fallback, or custom Drop
   stack exists.

## Focused validation contract

`tests.rs` exercises both source kinds, case distinctions, delimiter quirks,
non-ASCII internal bases, empty/basic/non-BMP encoding, exact iterator length,
reused capacity, partial semantic errors, overflow, work boundaries, all
prospective refusal points, and the original cumulative item/byte/string floors.
The original-budget fixture pre-exhausts GraphBudget paths, hashes and decoding;
pure Punycode conversion still succeeds at its exact allocation floor.

`tests/reference.rs` compares with the complete unchanged pinned internal source,
not the behaviorally different public wrappers. It covers deterministic scalar
families, insertion reorder, every byte value, digit/case/delimiter combinations,
Unicode character input, malformed/overflow inputs, and guard boundaries. Tests
may use ordinary allocations to construct fixtures/reference outputs; those
allocations are not production transformation paths. Root owns the reference
module, exact SmallVec dev dependency and all execution. Source readiness alone
is not a claimed differential test run or complete IDNA/URL acceptance.
