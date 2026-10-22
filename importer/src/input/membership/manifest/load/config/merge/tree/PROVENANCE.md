# Flat configuration-value merge

The tree transforms complete initial Values into flat nodes carrying exact
borrowed string data, scalar values, file/environment/CLI/builtin definitions and
source spans. Tables use the closed pinned map. Lists own admitted flat index
entries. Neither contains recursively owned child values. No raw-value parser,
field subset or generic serde conversion is introduced.

Conversion uses the complete, successfully converted Values projection. Each
table retains its original child insertion sequence and zero initial reservation;
child arena indices preserve the projection's numbering. Scalar/key text borrows
the admitted document. Projection storage remains charged while the new flat
ownership is materialized and is then dropped. There is no uncharged clone.

Merge consumes both trees. The incoming flat allocation is moved through an
allocation-free Vec::IntoIter. Tables rebase only inert node indices, preserving
all control bytes, key hashes, deletion state and iteration order. List indices
are checked and rebased likewise. Full node-buffer replacement layouts remain
prospectively charged through the same Session; old and new ownership overlap
under that cumulative account. Consumed or discarded nodes can remain unreachable
in the arena until drop, but they remain charged and cannot become selected roots.

The source merge program is in pinned Cargo config_value.rs:145–219. Pair frames
compute incoming versus old definition priority before type matching. Table
frames own std IntoIter directly, preserving mem::take(new)'s exact incoming
order and allocation. Each occupied entry appends to one shared ConfigKey, with
no pop after nested return. Vacant entries insert the incoming node without
recursion. Explicit admitted continuations reproduce depth-first completion and
first-failure order.

Scalar replacement swaps the complete node, including definition/span, only for
force or a higher definition class. Container/scalar and list/table mismatches
reject regardless of priority. A bounded ConfigMerge tag retains expected type
and the selected incoming source span; outer diagnostic formatting is not copied.
No failed partial Tree is returned by the consuming operation.

Lists retain the old container definition/span. Nonmergeable lists select old or
incoming elements by force/priority. Ordinary force appends new after old;
non-force appends old after new. All cases then perform the source's stable
definition-class ordering. Explicit ordinal fields encode stable ties for the
existing allocation-free heapsort; no hidden stable-sort scratch allocation is
used. Ordinals are reset to the actual concatenation order before sorting.

Node/edge/continuation/ordinal work uses actual item events; existing String
admission bounds borrowed keys and scalar text. All buffers charge full requested
layouts before fallible growth. Joining visits each retained incoming node and
index once. Merging descends the finite tree in source order, each table iterator
advances monotonically, list concatenation is finite and sorting is O(n log n)
over constant-size priority/ordinal comparisons. The table proof covers
closed probe/rehash work. No filesystem or observed-content hashing is invented.

Failure latches retain the original resource cause; failed consumption drops
flat buffers, inert table entries and owned table iterators without recursion.
Partial continuation teardown has constant native stack depth. Focused tests
cover every admission refusal and deep successful/rejected merge under a small
thread stack. The pinned reference covers full outputs/origins and first source
failures; see importer/reference/manifest/config-merge/README.md.

Connection remains downstream: config discovery/includes, required cloning or
other reached operations, effective string getter, environment precedence and
selected-origin resolution must consume this representation through complete
audited transitions. It does not yet establish a current membership witness.
