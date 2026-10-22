# Embedded manifest expansion

The implementation contains the complete `ScriptSource::parse` and
`embedded::expand_manifest` behavior from Cargo
`59800466c5c41c444d264b1010b4d57e85a7117f`:
`src/cargo/util/frontmatter.rs:1–199,243–327` and
`src/cargo/util/toml/embedded.rs:6–46`.

It preserves shebang and Rust Pattern_White_Space distinctions, opener limits,
full-fence-before-partial searches, partial diagnostic quirks, CRLF/space/tab
trimming order, multiple-frontmatter-before-infostring rejection, spans and
visible-span order, and complete prefix/comment output bytes. Diagnostic tags
retain the source cause and numeric fields without unaccounted message strings;
the source plus retained spans supply infostring text. Tests render those tags
for direct comparison with the reference's actual messages.

Only the exact failed partial byte advance becomes a bounded
terminal source cause. It records a valid cursor and attempted byte count;
successful advances retain the source's ordinary diagnostic. No production
panic or catch_unwind is used. All source/resource failures use the original
Session. The newline-plus-fence search pattern fits a fixed 256-byte stack
buffer. Output uses one prospectively admitted full layout, with no allocation
for empty output. Whole-document storage is not charged as a decoded scalar.

The exact-source reference wrapper and 335 observations live in
`importer/reference/manifest/frontmatter-panic/`. The test compares 59 successful
outputs, 247 returned errors with complete messages/spans, and 29 source panics
against terminal failures. This is expansion evidence, not a successful full
loader or membership witness. Read/UTF-8 errors, the script gate, manifest extent
admission and subsequent Document/Original construction must be joined in the
containing read entry before activation.
