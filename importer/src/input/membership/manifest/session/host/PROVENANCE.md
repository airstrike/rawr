# Same-account loader observations

The typed Session now borrows the existing immutable observation Host. Host's
Admission implementation delegates to its original Budget; neither component
creates a replacement account. Typed interpretation and subsequent filesystem
observations can therefore interleave without resetting spent work.

`is_file` and `exists` query followed metadata, matching Cargo's source predicates
in build/readme defaults and workspace ancestor discovery. Only recorded absence
or an ordinary recorded OS error becomes false. An unobserved query, invalid
path/type, allocation failure or graph refusal remains terminal. The first cause
is retained in `Error::Host`; later operations neither consume more records nor
replace that cause. Query path ownership and every repeated path check remain
the existing Host's responsibility.

`document` obtains a recorded whole file and then calls the existing admitted
document parser on the same Budget. The recorded read outcome precedes manifest
size, UTF-8 and syntax checks. A missing file remains None. Returned documents
borrow immutable image bytes independently of the mutable Host reference, so
Original/configuration and further observations can share one Session safely.
This raw-document operation does not implement embedded-script expansion.

`environment_prefix` preserves the separate predicate reached by Cargo commit
`59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/context/mod.rs::GlobalContext::has_key`: after an absent exact
environment key, Cargo checks keys starting with the key plus an underscore
when `env_prefix_ok` is true. The caller supplies those exact prefix bytes.
Positive and negative answers are distinct typed observations; neither an
absent exact value nor a missing observation substitutes for this predicate.
The existing immutable Host, original string/allocation budget and Session
failure latch are reused. Codec query tag 8 and answer tags 10/11 extend the
existing framing without changing older encodings; old readers reject the new
tags. Decoding remains historical data, never freshness authority. Complete
sealed-environment acquisition, optional typed configuration lookup and its
merge/type-error/definition precedence remain the loader's responsibility.

All 23 focused Host/document/session tests passed after correcting the combined
fixture's query sort order. Five new tests cover all metadata kinds, native
paths, absent versus unobserved files, post-failure nonconsumption, read-error
precedence, EOF parse errors, and exact/floor original ownership through whole
Document → Original → initial configuration → metadata. Exhausted path work
retains its actual budget error. Independent source review found no P0/P1
(confidence 0.98). CLI compilation passed; this evidence is not qualification.

The underlying parser/whole-file provenance remains in the existing membership
document and host tests. Full live acquisition, workspace loading, normalization,
selection and before/cold/after/current association remain required. This
test-only join grants no production cache or membership authority.
