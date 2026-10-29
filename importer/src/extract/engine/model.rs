//! The extraction values the engine handles, shared by the driver, which
//! writes them, and the library, which replays them. Their serialized form
//! is the extraction protocol's, field for field.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Id {
    pub(crate) authority: Authority,
    pub(crate) def_path_hash: String,
    pub(crate) namespace: Namespace,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Authority {
    Compiler {
        crate_name: String,
        stable_crate_id: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Namespace {
    Type,
    Value,
    Macro,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Export {
    pub(crate) binding: Binding,
    pub(crate) root_alias: String,
    pub(crate) path: Vec<String>,
    pub(crate) namespace: Namespace,
    pub(crate) target: ExportTarget,
    pub(crate) aliases: Vec<Alias>,
    pub(crate) reexports: Vec<Reexport>,
    pub(crate) span: Span,
    /// A re-exported enum variant, as `pub use Alignment::Center;`: the
    /// variant's name, and `target` is its enum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) variant: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Binding {
    Nameable,
    AnonymousImport,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ExportTarget {
    Definition { id: Id },
    Unsupported { compiler_kind: String },
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reexport {
    pub(crate) kind: ReexportKind,
    pub(crate) evidence: ReexportEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ReexportEvidence {
    Identified { item: ReexportId, span: Span },
    IdentitylessMacro,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReexportKind {
    Single,
    Glob,
    ExternCrate,
    MacroUse,
    MacroExport,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReexportId {
    pub(crate) crate_name: String,
    pub(crate) stable_crate_id: String,
    pub(crate) def_path_hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Alias {
    pub(crate) from: Vec<String>,
    pub(crate) to: Vec<String>,
    pub(crate) span: Span,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Span {
    pub(crate) file: String,
    pub(crate) byte_start: u32,
    pub(crate) byte_end: u32,
    pub(crate) line_start: u32,
    pub(crate) column_start: u32,
    pub(crate) line_end: u32,
    pub(crate) column_end: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScalarPredicateProof {
    pub(crate) owner: Id,
    pub(crate) parameter: u32,
    pub(crate) trait_id: Id,
    pub(crate) scalar: Scalar,
    pub(crate) outcome: ScalarPredicateOutcome,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Scalar {
    Unit,
    Bool,
    Char,
    F64,
    I64,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ScalarPredicateOutcome {
    Absent,
    Positive { implementation: Id },
    Unresolved,
}

/// A trait implementation or blanket application as the protocol writes
/// it. The engine owns `relevance` and `associated`, which several requests
/// add to; every other field is fixed by the implementation and kept as
/// written.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Implementation {
    pub(crate) key: String,
    pub(crate) id: Id,
    pub(crate) relevance: Vec<Id>,
    pub(crate) trait_ref: Box<RawValue>,
    pub(crate) self_type: Box<RawValue>,
    pub(crate) safety: Box<RawValue>,
    pub(crate) polarity: Box<RawValue>,
    pub(crate) const_impl: bool,
    pub(crate) default_impl: bool,
    pub(crate) generics: Box<RawValue>,
    pub(crate) associated: Vec<Id>,
    pub(crate) span: Box<RawValue>,
    pub(crate) origin: Box<RawValue>,
}

/// A sidecar's body: `{"handshake":…,` then these fields.
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Body<'body> {
    pub(crate) reachable: &'body [Export],
    pub(crate) definitions: Vec<&'body RawValue>,
    pub(crate) implementations: Vec<&'body Implementation>,
    pub(crate) scalar_predicate_proofs: Vec<&'body ScalarPredicateProof>,
    pub(crate) index: &'body [Box<RawValue>],
}
