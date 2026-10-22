# Ordered live capture

The native operations are supplied by the independently reviewed
`rawr-directory` Stream foundation (commit `408aeec1`). Its source and ABI
provenance are in `directory/src/stream/PROVENANCE.md` at the repository root.

Capture retains the caller's original membership Budget. Native opens include
prospective close work; reads, framing and fallback probes consume graph work.
Union temporary storage and retained rows/names use the same allocation account.
No account is replaced or refunded. Copy/sort work is bounded by the foundation's
fixed transfer extent and prospectively admitted storage.

A Pending entry borrows both the native entry and its exact output row. Skipping
it records no inspection. Consuming inspect performs the source type operation
once and records its success or native error. This allows Cargo's dot/non-UTF8
filter to run before type lookup, and path probes between entries, as required
by pinned Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`,
`src/cargo/util/toml/targets.rs:733–790`.

Only an observed terminal EOF or native enumeration error permits finish.
Admission/framing failure poisons the capture; an unfinished stream cannot
produce a transcript. Open and union-open failures remain distinct from partial
ordinary enumeration. Capture is acquisition data only; it is not connected to
successful selector completion or current-source admission.

Focused independent live-capture review accepted the scoped probe correction
with no remaining blocker (confidence 0.93). Root validation passed 13 combined
stream tests, 25 Host regression tests, 14 stored-codec regression tests and the
no-default-features production check. The physical filesystem rejected a
non-UTF8 fixture with EILSEQ; that branch is covered by explicitly recorded
native-byte replay data instead of a claim of live filename creation.
