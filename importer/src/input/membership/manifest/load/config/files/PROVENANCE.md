# Recorded configuration file traversal

Files implements the reached `_load_file` and `load_includes` operations at
pinned Cargo context/mod.rs:1384–1454. Complete files are obtained through the
existing original-account Host. Text read/UTF-8 validation precede parsing;
definition construction and complete Values conversion follow successful parse.
CLI/discovery origin class propagates unchanged to every descendant.

Scoped Slots retain owned native paths and parsed Documents outside all
returned Trees. Files owns only append cursors and a borrowed visited-path list.
The caller creates owners in dependency order. Returned values borrow the stable
owners, not the Files cursor or mutable Session; later loads cannot move existing
documents or paths. No unsafe lifetime extension, leaked owners or raw pointers
are introduced. Slot destruction is iterative and document/tree ownership is
flat, including partially completed traversal.

The visited set has membership-only semantics. An admitted list compares native
Path components, preserving repeated-separator/current-directory equality while
retaining parent-directory distinctions. Entry paths are copied without lexical
normalization; resolved include paths are normalized at their existing boundary.
Every comparison admits an item and the prior path length; the incoming length
is admitted before the finite scan. The worst-case quadratic comparison count
consumes the original item account rather than depending on hash-table internals.
Paths are never removed from the set after a child completes. Duplicate siblings
and shared descendants therefore fail before another read, just like source.

Path ownership and seen insertion precede reading, including NUL handling. A
mandatory path with interior NUL produces terminal ConfigPathNul before any
record query, representing std's InvalidInput without fabricating an OS errno.
It is an ordinary path-conversion error, not a translated panic or source-cause change.
The caller supplies absolute entry/cwd geometry as established by real loading.

Each frame retains the including value, an empty accumulator with its definition,
and an owned allocation-free iterator over its admitted include list. Extraction
and complete syntax validation happen before frame construction. The loop resolves
the next include, fully descends, merges its completed result into the parent
with force=true, then advances the next sibling. Finally the including value is
force-merged. Optional absence creates no visited entry. Failed merges return
immediately before later sibling observations. All frames, values, paths and
documents remain charged during overlap; errors return no partially merged Tree.
There is no recursive call or recursive continuation destruction.

includes=false still inserts/reads/parses/converts but leaves include untouched.
The separate includes entry accepts complete inline CLI values and does not insert
a fictitious root filename. Files deliberately retains seen across load calls;
callers must scope fresh sets per normal-discovery file and inline argument, and
share them across CLI file arguments (or unmerged discovery as required by that
source caller). No discovery or lazy-load ordering is inferred here.

Tests verify full resulting values and origins, duplicate/diamond/component-alias
cycles, conversion and merge failure order, inline/disabled includes, terminal NUL,
same-account exact item/byte floors and 256-deep success/failure teardown on a
128 KiB native stack. The source excerpts are retained for independent review;
these traversal tests are not represented as a full executable Cargo comparison.
This boundary does not establish current membership, owner reuse or product speed.
