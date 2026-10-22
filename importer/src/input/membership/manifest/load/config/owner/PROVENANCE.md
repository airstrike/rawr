# Recorded configuration owner

Pinned Cargo 59800466c5c41c444d264b1010b4d57e85a7117f, util/context/mod.rs:
values:637–639, get_cv:833–841, configure:1178–1183,
load_file:1378–1380, CLI composition:1550–1624, credentials:1772–1814.

Owner composes existing recorded file/include/discovery, ordinary traversal,
CLI values, and optional-path getter under the caller's original Session. It
borrows path/document owners and retains the single Files append cursor
across operations. Existing consuming CLI/traversal entry points remain wrappers;
the new mutable-cursor entries preserve all read/merge/canonicalization order.
Resetting seen replaces only the include-cycle buffer, never document/path
owners or prior account usage. Owner does not acquire native observations.

configure_files represents the reached CLI merge stage only. Empty argv performs
no ordinary initialization. Nonempty argv completes its CLI table before first
ordinary initialization and reuses existing ordinary values when supplied.
It is not all of GlobalContext::configure, whose subsequent unstable/term/network
getters and diagnostics still need their actual command integration. Actual
Unicode argv, cwd, raw Cargo home, search stop and source context association are
caller obligations already retained by the governed CLI/traversal boundaries.

values initializes ordinary configuration once. The shared optional-path getter
now accepts a get_cv callback; existing Tables callers use exactly the same
protocol. Owner's callback first checks an already-loaded credential overlay and
only on absence reaches ordinary values. A wrong intermediate credential table
remains an error. get_cv never initiates credential loading. Raw environment
presence and prefix checks still precede their reached get_cv; presence does not
skip the later value/origin getters or lazy configuration failure.

Found returns borrowed document text and historical Definition with their actual
owner lifetimes instead of artificially tying those copies to an immutable Tree
borrow. This permits the repeated lazy getter calls without extending lifetimes,
cloning trees/strings, leaking storage, or retaining references across mutation
of the originating owner data. Paths/documents outlive every owned tree.

load_credentials checks the filled overlay first, admits raw-home ownership,
selects the Credentials family with warn=true, and performs complete includes
under fresh discovery seen state. Missing credentials DO NOT fill the overlay.
A later reached call repeats physical discovery. For a selected file, complete
read/include/conversion precedes normalize_credentials, which removes legacy
root token and conditionally creates registry. Only then values initializes or
reuses ordinary configuration, including for an empty credential file.
merge_credentials clones matching ordinary subtrees only, merges each credential
top-level entry with a fresh empty merge key and force=true, and returns the
partial overlay. Owner publishes it only after full success. Tree/table source
correspondence and clone containment are documented by their implementation.

One owner represents one command replay attempt. A failed consuming merge may
have consumed ordinary state, so Owner retains the already-admitted returned
Stop and refuses subsequent owner operations with that same Stop. This is local
ownership containment: it does not assert that Cargo memoizes configuration
errors or that Session::reject globally latches. Session's trial-parser rejection
semantics are unchanged. Original host/resource failure keeps its existing
Session latch and precedence. Returning the retained Stop adds no diagnosis,
allocation, observation or admission. A new command replay requires its own
properly scoped owner and original account; this type grants no retry authority.

All actual file paths, documents, trees, merges, observed queries, subtree copies
and overlap retain the original account and first resource failure. There are no
new budgets, per-config caps, refunds or recursive owners. Tests exercise recorded
codec/Host composition, CLI-before-normal failure, lazy and repeated state,
credentials includes, unsuccessful publication, arbitrary-key fallback, path
origin, failed-owner containment and exact original item/byte floors. Root runs
the tests; this source checkpoint does not itself claim a passing qualification.

This owner is not yet called by complete manifest normalization or current
membership admission. Actual configure/auth dispatch timing, patch-first and
nested/workspace normalization, selected-image/raw-home/CWD binding, and complete
before/actual-successful-cold/after/current physical source association remain
necessary. No current receipt, cold-only fallback, input support restriction,
production reuse, Gallery timing, or T06 completion is supplied by this module.
