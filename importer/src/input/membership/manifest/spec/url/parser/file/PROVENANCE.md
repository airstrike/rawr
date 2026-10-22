# Native Unix file URL construction

The `Url::from_file_path` constructor follows pinned url 2.5.8
`src/lib.rs:2553–2570,2948–2980` for Unix. License texts remain in the adjacent
parser/output directory. It allocates the initial `file://` serialization before
rejecting a relative path, then traverses native components after the root.
Each retained component gets a separator and SPECIAL_PATH_SEGMENT percent
encoding of its original bytes. Root-only paths receive one trailing slash.

This constructor does not parse its serialization. Native parent components
remain present; redundant separators/current components follow native Path
iteration. Non-UTF-8 bytes, NUL and percent signs retain exact percent-encoded
representations. Scheme/path offsets are fixed at 4/7 with no query or fragment.

The existing Output admission mechanism now accepts raw byte slices and the
exact pinned special-segment encoding set. Its string entrypoint delegates to
that mechanism unchanged. Original Session bounds cover path length, component
iteration, full prospective encoded length and physical output growth. Refusal
cannot become a relative-path error or restart parsing.

Tests cover native components, non-UTF-8 bytes, relative rejection and initial
allocation refusal. The shared percent-encoding reference checks the new set
against the pinned dependency for every byte and complete string encodings.
Cargo IntoUrl adapter tests separately cover constructor/error-format refusal.
This is constructor evidence, not canonical source identity or production reuse.
