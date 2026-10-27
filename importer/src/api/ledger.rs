//! Canonical export-path, implementation, and completeness outcomes.

use serde::{Deserialize, Serialize};

use super::{identity, model, Retained};

/// One canonical re-export or alias hop.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Alias {
    from: Box<[Box<str>]>,
    to: Box<[Box<str>]>,
    span: model::Span,
}

/// One imported publicly reachable export path.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Export {
    key: identity::Export,
    target: identity::Definition,
    aliases: Box<[Alias]>,
    reexports: Box<[Reexport]>,
    span: model::Span,
}

/// One compiler-authenticated anonymous import retained only as support evidence.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct AnonymousImport {
    root_alias: Box<str>,
    path: Box<[Box<str>]>,
    target: identity::Definition,
    aliases: Box<[Alias]>,
    reexports: Box<[Reexport]>,
    span: model::Span,
}

/// One exact compiler-recorded hop in a public re-export chain.
#[allow(
    clippy::large_enum_variant,
    reason = "canonical re-export evidence stays inline so identity and span cannot separate"
)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Reexport {
    /// A source item with inseparable compiler identity and diagnostic position.
    Identified {
        kind: ReexportKind,
        item: identity::Reexport,
        span: model::Span,
    },
    /// A compiler-created macro import/export without an addressable item.
    IdentitylessMacro { kind: ReexportKind },
}

/// rustc's closed classification for a re-export hop.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum ReexportKind {
    Single,
    Glob,
    ExternCrate,
    MacroUse,
    MacroExport,
}

/// Stable positioned reasons for a selected path or support rejection.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Reason {
    /// Cargo could not authoritatively join this compiler-observed external crate.
    ExternalAuthorityUnavailable {
        /// The compiler-reported crate spelling.
        crate_name: Box<str>,
        /// The compiler-reported stable crate identity.
        stable_crate_id: Box<str>,
    },
    /// A signature requires a private definition that generated Rust cannot name.
    UnnameablePrivate(identity::Definition),
    /// The compiler reported a shape outside the versioned sidecar language.
    UnknownCompilerShape {
        /// The definition whose compiler shape was unknown.
        definition: identity::Definition,
        /// The exact compiler kind that the pinned schema did not recognize.
        compiler_kind: Box<str>,
    },
    /// A legal public namespace entry has no compiler definition identity.
    UnsupportedPublicPath {
        /// The exact compiler result classification.
        compiler_kind: Box<str>,
    },
    /// A referenced definition was absent from the complete correlated fragments.
    UnresolvedDefinition(identity::Definition),
    /// An alias chain was discontinuous or did not end at the exported path.
    InvalidAliasChain,
    /// Two canonical identities or paths conflicted.
    ConflictingIdentity,
}

/// Exactly one outcome for each compiler-reported selected public path.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Path {
    /// The reachable path and all of its support evidence are complete.
    Imported(Export),
    /// Rawr-side policy explicitly excluded the unchanged path.
    Excluded {
        /// The selected public path.
        key: identity::Export,
        /// The fingerprinted Rawr-side exclusion reason.
        reason: Box<str>,
        /// The policy declaration position.
        policy_span: model::Span,
        /// The unchanged Rust export position.
        source_span: model::Span,
    },
    /// The compiler path exists but cannot become complete API evidence.
    Rejected {
        /// The selected public path.
        key: identity::Export,
        /// Stable structured rejection evidence.
        reason: Reason,
        /// The unchanged Rust position responsible for rejection.
        span: model::Span,
    },
}

/// The provenance role through which a definition became required support.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum SupportRole {
    /// Referenced by a definition's public shape.
    Definition,
    /// Referenced by a relevant implementation.
    Implementation,
}

/// Exactly one completeness outcome for each transitive support edge and role.
#[allow(
    clippy::large_enum_variant,
    reason = "the support ledger keeps each complete or rejected proof bundle inline"
)]
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Support {
    /// The definition and all recursively referenced evidence are present.
    Complete {
        /// The referenced support identity.
        id: identity::Definition,
        /// The definition whose shape or implementation required it.
        provenance: identity::Definition,
        /// The semantic role of this exact support edge.
        role: SupportRole,
    },
    /// The support identity is retained with its stable failure.
    Rejected {
        /// The referenced support identity.
        id: identity::Definition,
        /// The definition whose shape or implementation required it.
        provenance: identity::Definition,
        /// The semantic role of this exact support edge.
        role: SupportRole,
        /// Stable structured rejection evidence.
        reason: Reason,
        /// The referring source position.
        span: model::Span,
    },
}

impl Support {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Complete { id, provenance, .. } => {
                id.retain(retained)?;
                provenance.retain(retained)
            }
            Self::Rejected {
                id,
                provenance,
                reason,
                span,
                ..
            } => {
                id.retain(retained)?;
                provenance.retain(retained)?;
                reason.retain(retained)?;
                span.retain(retained)
            }
        }
    }

    pub(crate) const fn id(&self) -> &identity::Definition {
        match self {
            Self::Complete { id, .. } | Self::Rejected { id, .. } => id,
        }
    }

    pub(crate) const fn provenance(&self) -> &identity::Definition {
        match self {
            Self::Complete { provenance, .. } | Self::Rejected { provenance, .. } => provenance,
        }
    }

    pub(crate) const fn role(&self) -> SupportRole {
        match self {
            Self::Complete { role, .. } | Self::Rejected { role, .. } => *role,
        }
    }
}

impl Alias {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retain_path(&self.from, retained)?;
        retain_path(&self.to, retained)?;
        self.span.retain(retained)
    }

    pub(crate) fn new(from: Box<[Box<str>]>, to: Box<[Box<str>]>, span: model::Span) -> Self {
        Self { from, to, span }
    }

    /// The canonical path before this alias hop.
    pub fn from(&self) -> impl ExactSizeIterator<Item = &str> {
        self.from.iter().map(AsRef::as_ref)
    }

    /// The canonical path after this alias hop.
    pub fn to(&self) -> impl ExactSizeIterator<Item = &str> {
        self.to.iter().map(AsRef::as_ref)
    }

    #[allow(
        clippy::wrong_self_convention,
        reason = "from/to are the domain directions of one alias edge"
    )]
    pub(crate) fn from_parts(&self) -> &[Box<str>] {
        &self.from
    }

    pub(crate) fn to_parts(&self) -> &[Box<str>] {
        &self.to
    }

    /// The compiler metadata diagnostic position of this alias or re-export.
    pub const fn span(&self) -> &model::Span {
        &self.span
    }
}

impl Export {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        self.key.retain(retained)?;
        self.target.retain(retained)?;
        retained.slice(&self.aliases)?;
        for alias in &self.aliases {
            alias.retain(retained)?;
        }
        retained.slice(&self.reexports)?;
        for reexport in &self.reexports {
            reexport.retain(retained)?;
        }
        self.span.retain(retained)
    }

    pub(crate) fn new(
        key: identity::Export,
        target: identity::Definition,
        aliases: Box<[Alias]>,
        reexports: Box<[Reexport]>,
        span: model::Span,
    ) -> Self {
        Self {
            key,
            target,
            aliases,
            reexports,
            span,
        }
    }

    /// This path to `target` instead: a type alias's path to the type it
    /// names (`chrono::Duration` to `TimeDelta`).
    pub(crate) fn retargeted(&self, target: identity::Definition) -> Self {
        Self {
            target,
            ..self.clone()
        }
    }

    /// The selected source-visible path identity.
    pub const fn key(&self) -> &identity::Export {
        &self.key
    }

    /// The single canonical definition reached by this path.
    pub const fn target(&self) -> &identity::Definition {
        &self.target
    }

    /// The exact compiler-resolved alias/re-export chain.
    pub fn aliases(&self) -> impl ExactSizeIterator<Item = &Alias> {
        self.aliases.iter()
    }

    /// Exact use/glob/extern/macro hops reported by the pinned compiler.
    pub fn reexports(&self) -> impl ExactSizeIterator<Item = &Reexport> {
        self.reexports.iter()
    }

    /// The unchanged Rust export position.
    pub const fn span(&self) -> &model::Span {
        &self.span
    }
}

impl AnonymousImport {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.root_alias)?;
        retain_path(&self.path, retained)?;
        self.target.retain(retained)?;
        retained.slice(&self.aliases)?;
        for alias in &self.aliases {
            alias.retain(retained)?;
        }
        retained.slice(&self.reexports)?;
        for reexport in &self.reexports {
            reexport.retain(retained)?;
        }
        self.span.retain(retained)
    }

    pub(crate) const fn new(
        root_alias: Box<str>,
        path: Box<[Box<str>]>,
        target: identity::Definition,
        aliases: Box<[Alias]>,
        reexports: Box<[Reexport]>,
        span: model::Span,
    ) -> Self {
        Self {
            root_alias,
            path,
            target,
            aliases,
            reexports,
            span,
        }
    }

    /// The selected Rawr root under which rustc resolved the anonymous binding.
    pub fn root_alias(&self) -> &str {
        &self.root_alias
    }

    /// The compiler-reported binding location, retained as provenance rather than a name.
    pub fn binding_path(&self) -> impl ExactSizeIterator<Item = &str> {
        self.path.iter().map(AsRef::as_ref)
    }

    /// The exact semantic target imported without binding a name.
    pub const fn target(&self) -> &identity::Definition {
        &self.target
    }

    /// The exact compiler-resolved alias chain.
    pub fn aliases(&self) -> impl ExactSizeIterator<Item = &Alias> {
        self.aliases.iter()
    }

    /// Exact compiler-recorded re-export provenance.
    pub fn reexports(&self) -> impl ExactSizeIterator<Item = &Reexport> {
        self.reexports.iter()
    }

    /// The resolved underscore binding position.
    pub const fn span(&self) -> &model::Span {
        &self.span
    }
}

impl Reexport {
    fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Identified { item, span, .. } => {
                item.retain(retained)?;
                span.retain(retained)
            }
            Self::IdentitylessMacro { .. } => Some(()),
        }
    }

    pub(crate) const fn identified(
        kind: ReexportKind,
        item: identity::Reexport,
        span: model::Span,
    ) -> Self {
        Self::Identified { kind, item, span }
    }

    pub(crate) const fn identityless_macro(kind: ReexportKind) -> Self {
        Self::IdentitylessMacro { kind }
    }

    /// The compiler's closed re-export kind.
    pub const fn kind(&self) -> ReexportKind {
        match self {
            Self::Identified { kind, .. } | Self::IdentitylessMacro { kind } => *kind,
        }
    }

    /// Exact use-item identity, absent only for compiler macro-use/export hops.
    pub const fn item(&self) -> Option<&identity::Reexport> {
        match self {
            Self::Identified { item, .. } => Some(item),
            Self::IdentitylessMacro { .. } => None,
        }
    }

    /// Use-item metadata diagnostic position, absent for identityless macro hops.
    pub const fn span(&self) -> Option<&model::Span> {
        match self {
            Self::Identified { span, .. } => Some(span),
            Self::IdentitylessMacro { .. } => None,
        }
    }
}

impl Path {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::Imported(export) => export.retain(retained),
            Self::Excluded {
                key,
                reason,
                policy_span,
                source_span,
            } => {
                key.retain(retained)?;
                retained.string(reason)?;
                policy_span.retain(retained)?;
                source_span.retain(retained)
            }
            Self::Rejected { key, reason, span } => {
                key.retain(retained)?;
                reason.retain(retained)?;
                span.retain(retained)
            }
        }
    }

    /// The selected public path shared by every closed outcome variant.
    pub const fn key(&self) -> &identity::Export {
        match self {
            Self::Imported(export) => export.key(),
            Self::Excluded { key, .. } | Self::Rejected { key, .. } => key,
        }
    }
}

impl Reason {
    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        match self {
            Self::ExternalAuthorityUnavailable {
                crate_name,
                stable_crate_id,
            } => {
                retained.string(crate_name)?;
                retained.string(stable_crate_id)
            }
            Self::UnnameablePrivate(definition) | Self::UnresolvedDefinition(definition) => {
                definition.retain(retained)
            }
            Self::UnknownCompilerShape {
                definition,
                compiler_kind,
            } => {
                definition.retain(retained)?;
                retained.string(compiler_kind)
            }
            Self::UnsupportedPublicPath { compiler_kind } => retained.string(compiler_kind),
            Self::InvalidAliasChain | Self::ConflictingIdentity => Some(()),
        }
    }
}

fn retain_path(path: &[Box<str>], retained: &mut Retained) -> Option<()> {
    retained.slice(path)?;
    for segment in path {
        retained.string(segment)?;
    }
    Some(())
}

/// One edge of a selected crate's name index: `name` is a public child
/// of the module whose id is `parent`, of compiler `kind`, defining `id` (a
/// def-path hash). The crate root is the edge with an empty parent and name.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Name {
    parent: Box<str>,
    name: Box<str>,
    kind: Box<str>,
    id: Box<str>,
}

/// A demanded dependency's complete name index under one root alias:
/// every public path, names only, sorted by parent module. A module public
/// under two paths is listed once and reached through either edge.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    root_alias: Box<str>,
    names: Box<[Name]>,
}

/// The first segment of a path through a macro's `$crate`, after the root
/// of the dependency the macro came from: `__rawr_crate_<name>_<id>`, the
/// root of the crate defining the macro, which the driver finds.
pub const CRATE_SEGMENT: &str = "__rawr_crate_";

/// What a name index says about a Rust path below its root alias.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lookup {
    /// The path names an item, or a member of one (which extraction decides).
    Present,
    /// The path runs through a re-exported foreign crate (`pub use other;`),
    /// which the index does not walk.
    ThroughCrate,
    /// Segment `missing` is not a public child of the module `parent` names,
    /// with the closest child name, if any is close.
    Missing {
        parent: Box<str>,
        missing: Box<str>,
        suggestion: Option<Box<str>>,
    },
}

impl Index {
    pub(crate) fn new(root_alias: Box<str>, names: Box<[Name]>) -> Self {
        Self { root_alias, names }
    }

    /// The root alias this index answers for.
    #[must_use]
    pub fn root_alias(&self) -> &str {
        &self.root_alias
    }

    fn children(&self, parent: &str) -> &[Name] {
        let start = self.names.partition_point(|name| &*name.parent < parent);
        let end = self.names.partition_point(|name| &*name.parent <= parent);
        &self.names[start..end]
    }

    /// Resolve `path` (below the root alias) through public module children.
    #[must_use]
    pub fn lookup(&self, path: &[&str]) -> Lookup {
        let Some(root) = self.children("").first() else {
            return Lookup::Present;
        };
        let mut module = &*root.id;
        // A macro's `$crate` names a crate the index does not hold: the
        // driver resolves it on demand.
        if path
            .first()
            .is_some_and(|segment| segment.starts_with(CRATE_SEGMENT))
        {
            return Lookup::ThroughCrate;
        }
        for (position, segment) in path.iter().enumerate() {
            let children = self.children(module);
            let mut found = children.iter().filter(|name| &*name.name == *segment);
            let Some(first) = found.next() else {
                let parent = std::iter::once(&*self.root_alias)
                    .chain(path[..position].iter().copied())
                    .collect::<Vec<_>>()
                    .join("::");
                let limit = (segment.len() / 3).max(1);
                let suggestion = children
                    .iter()
                    .filter_map(|name| Some((distance(segment, &name.name, limit)?, &name.name)))
                    .min()
                    .map(|(_, name)| name.clone());
                return Lookup::Missing {
                    parent: parent.into(),
                    missing: (*segment).into(),
                    suggestion,
                };
            };
            let found = std::iter::once(first).chain(found).collect::<Vec<_>>();
            if found.iter().any(|name| &*name.kind == "crate") {
                return Lookup::ThroughCrate;
            }
            match found.iter().find(|name| &*name.kind == "module") {
                Some(next) if position + 1 < path.len() => module = &next.id,
                _ => return Lookup::Present,
            }
        }
        Lookup::Present
    }

    pub(super) fn retain(&self, retained: &mut Retained) -> Option<()> {
        retained.node()?;
        retained.string(&self.root_alias)?;
        retained.slice(&self.names)?;
        for name in &self.names {
            retained.string(&name.parent)?;
            retained.string(&name.name)?;
            retained.string(&name.kind)?;
            retained.string(&name.id)?;
        }
        Some(())
    }
}

/// Edit distance (insert, delete, substitute) up to `limit`, else `None`.
fn distance(left: &str, right: &str, limit: usize) -> Option<usize> {
    let right = right.chars().collect::<Vec<_>>();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    for (row, left) in left.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, right) in right.iter().enumerate() {
            let substitute = previous[column] + usize::from(left != *right);
            current.push(
                substitute
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        previous = current;
    }
    let distance = previous[right.len()];
    (distance <= limit).then_some(distance)
}
