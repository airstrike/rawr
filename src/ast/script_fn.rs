//! Module defining script-defined functions.

use super::{FnAccess, StmtBlock};
use crate::{FnArgsVec, ImmutableString};
use std::{fmt, hash::Hash};

/// _(internals)_ A type containing information on a script-defined function.
/// Exported under the `internals` feature only.
#[derive(Debug, Clone)]
pub struct ScriptFuncDef {
    /// Function body.
    pub body: StmtBlock,
    /// Function name.
    pub name: ImmutableString,
    /// Function access mode.
    pub access: FnAccess,
    /// Type of `this` pointer, if any.
    pub this_type: Option<ImmutableString>,
    /// Names of function parameters.
    pub params: FnArgsVec<ImmutableString>,
    /// _(metadata)_ Function doc-comments (if any). Exported under the `metadata` feature only.
    ///
    /// Doc-comments are comment lines beginning with `///` or comment blocks beginning with `/**`,
    /// placed immediately before a function definition.
    ///
    /// Block doc-comments are kept in a single string with line-breaks within.
    ///
    /// Line doc-comments are merged, with line-breaks, into a single string without a termination line-break.
    ///
    /// Leading white-spaces are stripped, and each string always starts with the corresponding
    /// doc-comment leader: `///` or `/**`.
    ///
    /// Each line in non-block doc-comments starts with `///`.
    #[cfg(feature = "metadata")]
    pub comments: crate::StaticVec<crate::SmartString>,
    /// The async body this function runs instead of `body`, once lowered.
    pub(crate) machine: Option<crate::Shared<crate::eval::machine::Machine>>,
}

impl ScriptFuncDef {
    /// Clone this [`ScriptFuncDef`] but with only signature-related info.
    ///
    /// The body of the function is removed, as well as comments (if any).
    #[allow(dead_code)]
    pub(crate) fn clone_function_signatures(&self) -> Self {
        Self {
            name: self.name.clone(),
            access: self.access,
            body: StmtBlock::NONE,
            this_type: self.this_type.clone(),
            params: self.params.clone(),
            #[cfg(feature = "metadata")]
            comments: <_>::default(),
            machine: None,
        }
    }
}

impl fmt::Display for ScriptFuncDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let this_type = self
            .this_type
            .as_ref()
            .map_or(String::new(), |s| format!("{s:?}."));

        write!(
            f,
            "{}{}{}({})",
            match self.access {
                FnAccess::Public => "",
                FnAccess::Private => "private ",
            },
            this_type,
            self.name,
            self.params
                .iter()
                .map(ImmutableString::as_str)
                .collect::<FnArgsVec<_>>()
                .join(", ")
        )
    }
}

/// A type containing the metadata of a script-defined function.
///
/// Created by [`AST::iter_functions`][super::AST::iter_functions].
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "camelCase")
)]
#[non_exhaustive]
pub struct ScriptFnMetadata<'a> {
    /// Function name.
    pub name: &'a str,
    /// Function parameters (if any).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub params: Vec<&'a str>,
    /// Function access mode.
    pub access: FnAccess,
    /// Type of `this` pointer, if any.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub this_type: Option<&'a str>,
    /// _(metadata)_ Function doc-comments (if any).
    /// Exported under the `metadata` feature only.
    ///
    /// Doc-comments are comment lines beginning with `///` or comment blocks beginning with `/**`,
    /// placed immediately before a function definition.
    ///
    /// Block doc-comments are kept in a single string slice with line-breaks within.
    ///
    /// Line doc-comments are merged, with line-breaks, into a single string slice without a termination line-break.
    ///
    /// Leading white-spaces are stripped, and each string slice always starts with the
    /// corresponding doc-comment leader: `///` or `/**`.
    ///
    /// Each line in non-block doc-comments starts with `///`.
    #[cfg(feature = "metadata")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<&'a str>,
}

impl fmt::Display for ScriptFnMetadata<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let this_type = self
            .this_type
            .as_ref()
            .map_or(String::new(), |s| format!("{s:?}."));

        write!(
            f,
            "{}{}{}({})",
            match self.access {
                FnAccess::Public => "",
                FnAccess::Private => "private ",
            },
            this_type,
            self.name,
            self.params
                .iter()
                .copied()
                .collect::<FnArgsVec<_>>()
                .join(", ")
        )
    }
}

impl<'a> From<&'a ScriptFuncDef> for ScriptFnMetadata<'a> {
    #[inline]
    fn from(value: &'a ScriptFuncDef) -> Self {
        Self {
            name: &value.name,
            params: value.params.iter().map(ImmutableString::as_str).collect(),
            access: value.access,
            this_type: value.this_type.as_deref(),
            #[cfg(feature = "metadata")]
            comments: value.comments.iter().map(<_>::as_ref).collect(),
        }
    }
}
