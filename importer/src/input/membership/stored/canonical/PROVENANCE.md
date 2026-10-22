# Canonicalization observation data and semantic transport

A separately isolated native foundation exists, but this slice calls
none of it and adds no dependency or production activation. It supplies only
capability-free recorded data for the later configuration traversal. Actual
loaded native behavior, transitive ownership/work and concurrency qualification,
live original-account acquisition and full current association remain separate.

Query::Canonical appends after Discovery without changing existing query order.
The exact input uses existing observed::Path: absolute native bytes, NUL-free,
with no lexical normalization or Unicode conversion. Value::Canonical contains
either another checked Path or a positive native errno. Success therefore
retains exact absolute NUL-free output; it does not prove that a native function
actually returned it or that it currently names the same filesystem object.
No arbitrary realpath-output cap, component restriction or source policy is
introduced. Neither generic Absent/Error nor Link values can substitute for this
typed result. A missing observation, malformed native result or resource failure
is not representable as its native-error branch.

The codec appends query tag 10 and value tags 13 (success) and 14 (native error).
Older exhaustive tag readers reject these tags; existing tags and schema retain
their meaning, and old images have no canonicalization authority. Decoder paths
reuse the existing checked Path reader. The record checker rejects nonpositive
errno and cross-typed values. Successful input/output buffers are separately
retained and prospectively charged through the original allocation account;
errors own only the input path plus the record layout. Complete encoded image
allocation and graph decode admission reuse the existing codec machinery.

Host::canonicalize copies/adopts no output: it borrows successful bytes from the
immutable image. Each lookup charges its actual native query copy and one
original graph path check, including repeats, native errors and misses. Session
transports this as `Step<Result<&[u8], i32>>`: only the inner Err is a recorded
native canonicalization failure eligible for the source's spelling fallback.
Outer Host failures latch under the same first-cause Session and remain terminal.
No output rendering, path cloning, fresh allowance or current-source proof is
introduced by transport.

Local source NUL-conversion semantics remain with the traversal caller, before
the typed host query, as with configuration discovery. Directly passing NUL to
Session transport produces the host encoding failure; the wrapper never
mislabels it as a native error. Any needed relative-input/CWD proof likewise
belongs to caller connection rather than an invented canonicalization policy.

Seven focused tests are supplied for root execution: codec native-byte/error
round trips and cross-type rejection; new tags, malformed paths/errno and
truncation; exact retention/decode ownership floors; borrowed Host output and
exact lookup; repeated original-account/graph floors; Session native-error
continuation; and terminal missing/malformed/refused query latching. They do not
repeat native ABI tests or claim a complete realpath differential.
