# Ordered directory call transport

This transport joins the reviewed private directory foundation to membership
observation storage and replay. It does not complete Cargo target inference or
grant current-source, loaded-manifest, or successful-selector authority.

The native source audit and ABI/safety boundary are recorded in
`directory/src/stream/PROVENANCE.md` and `directory/SAFETY.md` at repository root.
The relevant pinned Cargo consumer is `infer_from_directory` in `targets.rs`,
lines 733–790: it examines entries in encounter order, skips names before some
type queries, and can perform additional path probes between entries. Later
sorting cannot replace this order because earlier diagnostics and ambiguity
selection observe it.

## Observation and wire boundary

`Query::DirectoryStream` has its own value and codec tags. Existing sorted
`Directory` observations retain their meaning. The observation image version is
4 since a separate traversal transcript was added. This complete-stream shape
and completion contract are unchanged; earlier image versions are rejected,
never reinterpreted as traversal evidence.

The transcript distinguishes an initial open error, a union-collection error
after an observed mount result, and a started stream. A started stream records
raw name bytes, raw type hints, only the type queries actually reached, and its
terminal EOF or native error. Mount-probe failure remains an explicit errno
alongside the ordinary-stream behavior selected by the pinned source.

Rows are neither sorted nor deduplicated. Unknown type hints remain unknown
until the caller reaches inspection; the resulting kind or errno belongs to that
specific row. Invalid names, nonpositive errnos, inconsistent known type hints,
and impossible union terminal shapes are rejected during retention and decoding.
The decoder preadmits row/name storage under the original membership budget.

## Live acquisition and replay

`live::stream::Capture` retains the reviewed stream and original budget. Its
pending entry keeps the native entry and output row together. Interleaved probes
run through a closure which latches refusal before the operation; a graph-budget
error cannot be caught and turned into a completed transcript. Early drop or
resource refusal does not produce a record.

`host::stream::Cursor` borrows the image without retaining a mutable Host borrow,
so replay can perform the same interleaved probes. A checked, non-reused scope
identity binds each cursor and entry to its issuing Host in addition to the image
identity. Two Hosts over the same image and graph cannot exchange cursors or use
one another's accounts. The scope allocates no registry and carries no budget.

Replay must reach exactly the stored inspections and terminal result, then
consume the cursor with `finish`. A pending cursor prevents Host completion.
Extra inspection, omitted inspection, foreign Host use, resource refusal, and
abandoned iteration cannot mark the record consumed. Actual fallback inspection
charges the original graph's path budget; known hints do not fabricate a probe.

## Focused evidence boundary

The tests cover ordered and duplicate rows, skipped inspection, explicit native
errors, old-version/truncated/invalid encodings, exact and one-short ownership
budgets, abandoned cursors, and foreign-image and same-image foreign-Host use.
The live fixture exercises capture through retention, encoding, decoding and
Host completion. A separate byte-exact non-UTF-8 replay fixture exercises skipped
raw names because the installed filesystem rejects creation of that native name
with `EILSEQ`; it is not evidence that such a native fixture ran successfully.

These checks establish this transport boundary only. They do not establish
complete target inference, current membership proof, or release qualification.
