# Recorded manifest read entry

`read.rs::Reader::prepare` joins recorded input to retained pre-normalization
state. It returns `Preparing`, not a successfully loaded manifest, workspace,
selected package or current-source receipt.

The source is Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/mod.rs`: `is_embedded` at 52–55, `read_manifest` at 66–155,
and `read_toml_string` at 175–184. The source member identity already recorded
in `../PROVENANCE.md` is SHA-256
`940a2998ebff9915b4a0d050dcadc4d12e05615c7fcc3a5fb447b7d3fea4ea8e`.
Frontmatter source and behavior are detailed in `../embedded/PROVENANCE.md`;
the complete typed visitor has its own manifest provenance. This note records
the joined call order, not a new source/archive verification.

## Order and retained ownership

1. Classify the native path by its extension: exactly `rs` or no extension is
   embedded. No filesystem lookup or path normalization performs this choice.
2. Request the exact native `Query::File` through the supplied Host. Successful
   EOF, the existing manifest extent bound and UTF-8 conversion precede the
   script gate. Missing input becomes the read error ENOENT; a recorded read
   error, including after a partial prefix, cannot become TOML input.
3. For embedded input only, reject a disabled script flag before frontmatter
   expansion. Ordinary manifest text is borrowed unchanged. Expansion retains
   its actual owned output, including an empty result when the pin produces it.
4. Store the body, parse its selected text into the complete Document, and
   interpret the complete Original. The selected text remains subject to the
   existing manifest bound; document parsing preserves its depth/account law.
5. Run Features and initial workspace configuration. Publish a root declaration
   under its directory key before returning `Preparing`, matching the insertion
   before `normalize_toml` at Cargo `mod.rs:89–94`. This cache entry does not
   attest successful normalization or real/virtual conversion.

The external owner chain is immutable image → `Body` slots → Document slots
→ Original slots → `Preparing` and root-declaration borrowers. `Body` borrows
raw image text and owns optional expanded text; Document can borrow either.
Original also borrows decoded text owned by Document. `Reader` contains append
cursors, not a movable owner bundled with references into itself. The caller
keeps native declaring paths and the separate slot owners alive, then destroys
borrowers before owners. Slot growth publishes admitted single-assignment links
without moving earlier values; slot teardown detaches links iteratively.
No document clone, lifetime extension or implicit typed reparse supplies storage.

The same Session borrows Host and its original Allocation/current GraphBudget.
File lookup, expansion, parser/typed storage, slots, warnings and root insertion
use that account. Slot items and actual link layouts are admitted before
publication/allocation; the first inline slot requests no heap layout. Parsing
and lexical work introduce no fictitious filesystem/hash/comparison events.

## Failure and remaining boundary

The exact failed partial-fence byte advance propagates as the existing
terminal source cause, with a valid cursor and bounded attempted-byte count.
Its cause item is admitted before latching; earlier failure or refusal of that
admission wins. No expanded body, Document, Original or root insertion follows
that failure. Ordinary frontmatter diagnostics remain semantic errors with
their pinned precedence. Resource/source failures are not semantic fallbacks;
prior spent ownership is not refunded during cleanup.

This entry uses an immutable recorded Host. It does not acquire or authenticate
the supplied path, script flag, feature context, Cargo source identity or other
configuration origins. Complete live query-driven cold acquisition and movement
of its already admitted bodies/identities remain outside this entry. So do the
normalization continuation, lazy inheritance/full nested loading, real/virtual
checks, complete source selection and owner association. The actual
before → full cold Cargo success → after → current proof remains required;
stored EOF, stable slots and a cached root declaration do not replace it.

This provenance addition records source inspection only. It claims no executed
read-entry test result, complete loader parity, production activation, release
qualification or Gallery performance result.
