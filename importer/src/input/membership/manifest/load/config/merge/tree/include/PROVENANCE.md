# Configuration include extraction

This is the reached `GlobalContext::include_paths` operation from pinned Cargo
`59800466c5c41c444d264b1010b4d57e85a7117f`, context/mod.rs:1458–1546.
The input is the complete converted/merged flat Tree, not a field-only TOML
parser. Unsupported values in unknown fields have therefore already been checked
by complete initial conversion.

The root entry is removed before conversion when requested, including on a later
semantic failure. Retaining the entry instead leaves the original tree intact.
Cargo clones the subtree in that case, and consumes it in the removal case.
Here immutable admitted document strings and definitions are borrowed; flat nodes
remain owned and charged by Tree. Looking up path/optional in each include table
replaces removal from Cargo's disposable local table. No iteration depends on
those local removals and no mutated local table escapes. There is no recursive
clone or uncharged owned pathname allocation. The root table's actual removal
still uses the pinned closed map operation and retains its tombstone behavior.

List conversion completes before path validation. Each row checks path before
optional; unknown fields do not participate. String rows retain their definition;
table rows retain the table definition, not either child's definition. A later
type error therefore wins over an earlier invalid path. Syntax checks proceed
in list order: extension exactly `toml`, any of `*?[]`, then either brace. This is
not glob parsing. ConfigValue strings are UTF-8, so the source's non-UTF-8 skip
branch is unreachable from this extraction. Native Path extension semantics are
used unchanged. Extraction performs no normalization, resolution or existence
observation; those operations are the separate resolve boundary below.

The output buffer's full replacement layouts and items use the original Session.
Each row is visited once for conversion and once for validation. String admission
bounds the finite extension and character scans. Existing table bounds apply to
the fixed-key map operations. Removed subtrees stay charged until the original
flat arena drops; no recursive destruction is introduced. Resource refusal
latches the original cause and blocks subsequent work. A typed ConfigInclude
diagnostic retains expected shape and source span; Cargo's formatted message is
not claimed byte-for-byte.

## Reached path resolution

Include::resolve follows ConfigInclude::resolve_path at context/mod.rs:2370.
File and file-CLI definitions supply exactly one parent; other classes supply
cwd. Root selection precedes join even for absolute include text. Missing first
parent is invalid internal origin geometry, not the different second-parent
source failure. The caller must provide the absolute file/cwd geometry established
by actual loading. No lossy origin serialization occurs at this boundary.

Existing admitted native join and Cargo lexical normalization run in that order.
Both temporary allocations remain charged under the original Session. Mandatory
includes return the normalized path without a metadata probe. Optional includes
use the existing recorded followed-metadata predicate: all present kinds mean
true, absence and OS errors mean false; missing/wrong records and resource
failures remain terminal and cannot silently skip the file.

Pinned std Path::exists calls metadata.is_ok. Its Unix path adapter rejects an
interior NUL before stat (sys/helpers/small_c_string.rs, both CStr and CString
branches). Optional resolution therefore checks admitted normalized bytes for
NUL and skips without an observation. Check after normalization: a NUL-bearing
component can be removed by `..`. Mandatory NUL paths still return unchanged to
the later read boundary. This correction does not broaden the observation
codec's accepted native paths or translate missing witnesses into false.

Five additional tests cover 56 pinned lexical results, every admitted lexical
refusal, all metadata kinds/absence/OS error, zero queries for mandatory and NUL
optional paths, terminal missing observations, and recorded whole file through
complete extraction and resolution on the same original Host account.

Discovery, traversal/cycle handling and merge timing remain caller work. These
operations grant no current membership authority or reload reuse by themselves.
