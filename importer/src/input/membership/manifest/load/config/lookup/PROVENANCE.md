# Optional path lookup over complete prepared configuration

The input bundle borrows complete merged ordinary values and, when already
loaded, complete credential values. It does not load either tree. The surrounding
loader must establish the pinned values()/credential-values timing and ownership;
constructing this bundle does not authorize eager reads or ignored load failures.
The key is the existing feature-checked, two-component path-base request, so the
unrelated root-key ConfigValue clone branch is not reached.

The complete source path is context/de.rs deserialize_option → Value<String>
deserialize_struct → ValueSource::with_deserializer → deserialize_string →
get_string_priv → ValueDeserializer's definition field → Definition::deserialize.
ConfigRelativePath is a transparent Value<String> wrapper. Relevant source is
retained unchanged in importer/reference/manifest/config-lookup.

The operation order is intentional:

1. has_key reads raw exact environment presence, then the prefix only if absent,
   then configuration only if neither presence check succeeds. Only genuine
   absence returns None. Prefix-only presence can subsequently fail as missing.
2. ValueSource retains an environment definition, reads raw exact presence again,
   then get_cv. Already loaded credential values take precedence over ordinary
   values; missing credential keys fall through, but wrong intermediate types do
   not. Raw environment presence wins only when its class is strictly higher.
3. The String getter repeats get_cv before reading exact environment text. Invalid
   UTF-8 is ignored as a string but was present for earlier presence/origin tests.
   Valid environment text is admitted before final type/priority selection, even
   when CLI wins or a wrong final config type will reject. A wrong intermediate
   table rejects before that read. A wrong final type rejects despite env text.
4. Only after String succeeds does the definition wire conversion run. This is
   the ValueSource origin, not the inner String getter's separate origin choice.
   Paths undergo the pinned lossy UTF-8 round trip. Empty CLI paths become None.
   BuiltIn emits integer zero while Definition expects a tuple and therefore
   yields an ordinary bounded ConfigDefinition rejection before resolution.
5. The resulting private Relative bundle resolves through the existing
   definition/root operation and admitted native join. No builtin workspace
   fallback occurs inside this component.

The get_cv clone is replaced by an immutable view of complete converted data.
It retains the string (if any), exact borrowed definition and source span; other
final types remain distinguishable as non-string until the environment read.
There is no partial-field parsing or generic clone of recursive table ownership.
The immutable trees/documents outlive all borrowed selections. This equivalent
borrowed representation introduces no new source/cap/command authority.

Each table access and view is admitted through the original Session. Repeated
environment/prefix observations use the same Host, copying query storage through
its original Budget and rejecting missing records. UTF-8 scans are finite over
the admitted image; actual decoded strings pass String limits before retention.
The source case-mismatch warning produces no lookup result and ignores warning IO
failure; this component preserves functional lookup results, not warning text/IO.
Complete loader/CLI diagnostic delivery remains outside this operation.

Valid UTF-8 origins borrow existing paths. Invalid origins use two finite
utf8_chunks passes with the same invalid-sequence grouping as to_string_lossy:
calculate exact expansion, admit String and full byte layout, then fill a fallibly
reserved buffer. OsString::from_vec and PathBuf::from(OsString) transfer that Unix
buffer without another allocation. CLI/environment/builtin handling owns no
hidden recursive data. Refusals latch before later decode, resolution or fallback.

The 552 reference cases validate the full value protocol and final path. Focused
tests also cover missing observations, original string-limit refusal before CLI
selection/final-type rejection, earlier parent rejection, origin expansion/refusal,
and a single Stored codec→Host EOF document→Values→merge→optional path→resolve
chain on one account. No source producer/current proof or release claim follows.
