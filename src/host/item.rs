//! Static host item declarations.

use crate::{host, receiver, ImmutableString};

pub(crate) type ImportedSelector = crate::host::type_def::ImportedSelector;

/// The semantic kind of a static host item.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    /// A compile-time value declaration.
    Value,
    /// A compile-time value associated with a host type.
    AssociatedValue,
    /// A free function in a module namespace.
    Function,
    /// A symbolic asynchronous operation supplied by its owning facade.
    Operation,
    /// A symbolic stream operation supplied by its owning facade.
    StreamOperation,
    /// A function whose meaning is supplied by the compiler.
    IntrinsicFunction,
    /// A bracketed list-builder macro whose meaning is supplied by the compiler.
    IntrinsicMacro,
    /// A method whose meaning is supplied by the compiler.
    IntrinsicMethod,
    /// A receiver-free function associated with a host type.
    AssociatedFunction,
    /// A function with a typed receiver associated with a host type.
    Method,
    /// A dependency's `macro_rules!` macro, expanded from its source.
    MacroRules,
}

impl Kind {
    /// Whether this kind lives in the macro namespace.
    ///
    /// As in Rust, one path may name a macro and a value independently:
    /// `column![..]` and `column(..)` are two items under one name, and
    /// call syntax picks between them.
    #[must_use]
    pub const fn is_macro(self) -> bool {
        matches!(self, Self::IntrinsicMacro | Self::MacroRules)
    }
}

/// A dependency's `macro_rules!` macro: its definition as the crate's
/// source writes it, and the path `$crate` reads as (the dependency's root
/// and the defining crate's segment). It is parsed into rules once, at its
/// first expansion.
pub struct MacroRules {
    source: ImmutableString,
    krate: ImmutableString,
    rules: once_cell::sync::OnceCell<Result<crate::resolve::macro_rules::Rules, ImmutableString>>,
}

impl MacroRules {
    /// The definition, from `macro_rules!` through the close of its body.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The path `$crate` reads as.
    #[must_use]
    pub fn krate(&self) -> &str {
        &self.krate
    }

    /// The macro's rules, parsed from its source at the first call.
    pub(crate) fn rules(&self) -> Result<&crate::resolve::macro_rules::Rules, ImmutableString> {
        self.rules
            .get_or_init(|| crate::resolve::macro_rules::Rules::parse(&self.source))
            .as_ref()
            .map_err(Clone::clone)
    }

    fn digest(&self) -> [u8; 32] {
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        hasher.update(self.krate.as_bytes());
        hasher.update([0]);
        hasher.update(self.source.as_bytes());
        hasher.finalize().into()
    }
}

impl std::fmt::Debug for MacroRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacroRules")
            .field("krate", &self.krate)
            .finish_non_exhaustive()
    }
}

impl PartialEq for MacroRules {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.krate == other.krate
    }
}

impl Eq for MacroRules {}

/// One validated static host item declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Definition {
    path: host::path::Path,
    kind: Kind,
    receiver: Option<receiver::Mode>,
    signature: host::signature::Definition,
    outcome: host::usage::Outcome,
    evaluation: host::usage::Evaluation,
    stream_identity: Option<host::stream::Version>,
    imported: Option<ImportedSelector>,
    guard: Option<Guard>,
    /// The output borrows what the call takes: the call opens a scope
    /// frame the statement it is in runs inside (D558 tier 1).
    opens: bool,
    /// A `macro_rules!` macro's source.
    rules: Option<crate::Shared<MacroRules>>,
    /// The proof-backed application this imported method stands for: its
    /// call reaches the application's own adapter.
    application: Option<[u8; 32]>,
}

/// How an imported method's borrow guard output is reached: Rawr types the
/// call `&T` for a `Deref` guard and `&mut T` for a `DerefMut` one, whose
/// value at runtime is a place each read and store of calls the method
/// again (`crate::island::place`).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Guard {
    /// Read through `Deref`: the call gives a copy of the target.
    Shared,
    /// Read and stored through `DerefMut`: the call gives a place.
    Mutable,
    /// An `Option<&T>` lent from the receiver: the call gives a copy of
    /// the target in `Some`, or `None`.
    OptionShared,
    /// An `Option<&mut T>` lent from the receiver: the call gives a place
    /// in `Some`, or `None`.
    OptionMutable,
}

/// A deterministic proof of a host item's complete callable contract.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint {
    path: ImmutableString,
    kind: Kind,
    receiver: Option<receiver::Mode>,
    signature: host::signature::Fingerprint,
    outcome: host::usage::Outcome,
    evaluation: host::usage::Evaluation,
    stream_identity: Option<host::stream::Version>,
    imported: Option<[u8; 32]>,
    guard: Option<Guard>,
    opens: bool,
    rules: Option<[u8; 32]>,
    application: Option<[u8; 32]>,
}

impl Definition {
    #[cfg(feature = "rust-import")]
    pub(crate) fn same_contract_except_imported(&self, other: &Self) -> bool {
        self.path == other.path
            && self.kind == other.kind
            && self.receiver == other.receiver
            && self.signature == other.signature
            && self.outcome == other.outcome
            && self.evaluation == other.evaluation
            && self.stream_identity == other.stream_identity
            && self.guard == other.guard
            && self.opens == other.opens
            && self.rules == other.rules
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let path = self
            .path
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::path::Path>())?;
        let signature = self
            .signature
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::signature::Definition>())?;
        let selector = self
            .imported
            .as_ref()
            .map_or(Some(0), ImportedSelector::retained_bytes)?;
        std::mem::size_of::<Self>()
            .checked_add(path)?
            .checked_add(signature)?
            .checked_add(selector)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn fingerprint_retained_bytes(&self) -> Option<usize> {
        let signature = self
            .signature
            .fingerprint_retained_bytes()?
            .checked_sub(std::mem::size_of::<host::signature::Fingerprint>())?;
        std::mem::size_of::<Fingerprint>()
            .checked_add(self.path.display_len()?)?
            .checked_add(signature)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_clone(&self) -> Result<Self, rawr_import::surface::Error> {
        Ok(Self {
            path: self.path.try_clone()?,
            kind: self.kind,
            receiver: self.receiver,
            signature: self.signature.try_clone()?,
            outcome: self.outcome,
            evaluation: self.evaluation,
            stream_identity: self.stream_identity,
            imported: self
                .imported
                .as_ref()
                .map(ImportedSelector::try_clone)
                .transpose()?,
            guard: self.guard,
            opens: self.opens,
            application: self.application,
            rules: self.rules.clone(),
        })
    }

    /// Declare a dependency's `macro_rules!` macro at `path`, from its
    /// `source`, its `$crate` reading as `krate`.
    pub fn macro_rules(
        path: impl Into<ImmutableString>,
        source: impl Into<ImmutableString>,
        krate: impl Into<ImmutableString>,
    ) -> Result<Self, host::Error> {
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::MacroRules,
            receiver: None,
            signature: host::signature::Builder::new().finish(),
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: Some(crate::Shared::new(MacroRules {
                source: source.into(),
                krate: krate.into(),
                rules: once_cell::sync::OnceCell::new(),
            })),
        })
    }

    /// A `macro_rules!` macro's source, for a [`Kind::MacroRules`] item.
    #[must_use]
    pub fn macro_rules_source(&self) -> Option<&MacroRules> {
        self.rules.as_deref()
    }

    /// Declare a typed value in a module namespace.
    pub fn value(
        path: impl Into<ImmutableString>,
        ty: host::type_ref::Type,
    ) -> Result<Self, host::Error> {
        let mut signature = host::signature::Builder::new();
        signature.output(ty);
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::Value,
            receiver: None,
            signature: signature.finish(),
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Declare a typed value associated with a host type.
    pub fn associated_value(
        path: impl Into<ImmutableString>,
        ty: host::type_ref::Type,
    ) -> Result<Self, host::Error> {
        let mut signature = host::signature::Builder::new();
        signature.output(ty);
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::AssociatedValue,
            receiver: None,
            signature: signature.finish(),
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Declare a free function in a module namespace.
    pub fn function(
        path: impl Into<ImmutableString>,
        signature: host::signature::Definition,
    ) -> Result<Self, host::Error> {
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::Function,
            receiver: None,
            signature,
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Declare a receiver-free symbolic operation with zero or more inputs and one output.
    pub fn operation(
        path: impl Into<ImmutableString>,
        signature: host::signature::Definition,
    ) -> Result<Self, host::Error> {
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::Operation,
            receiver: None,
            signature,
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Declare a receiver-free symbolic stream operation with exact inputs and one item output.
    pub fn stream_operation(
        path: impl Into<ImmutableString>,
        signature: host::signature::Definition,
        identity: host::stream::Version,
    ) -> Result<Self, host::Error> {
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::StreamOperation,
            receiver: None,
            signature,
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: Some(identity),
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Declare a compiler-provided function in a module namespace.
    pub fn intrinsic_function(
        path: impl Into<ImmutableString>,
        signature: host::signature::Definition,
    ) -> Result<Self, host::Error> {
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::IntrinsicFunction,
            receiver: None,
            signature,
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Declare a compiler-provided bracketed list-builder macro.
    ///
    /// The signature must declare exactly one positional type. That type is
    /// repeated for every comma-separated element inside `![...]`.
    pub fn intrinsic_macro(
        path: impl Into<ImmutableString>,
        signature: host::signature::Definition,
    ) -> Result<Self, host::Error> {
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::IntrinsicMacro,
            receiver: None,
            signature,
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Declare a receiver-free function associated with a host type.
    pub fn associated_function(
        path: impl Into<ImmutableString>,
        signature: host::signature::Definition,
    ) -> Result<Self, host::Error> {
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::AssociatedFunction,
            receiver: None,
            signature,
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Declare a method and its receiver mode.
    pub fn method(
        path: impl Into<ImmutableString>,
        receiver: receiver::Mode,
        signature: host::signature::Definition,
    ) -> Result<Self, host::Error> {
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::Method,
            receiver: Some(receiver),
            signature,
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Declare a compiler-provided method and its receiver mode.
    pub fn intrinsic_method(
        path: impl Into<ImmutableString>,
        receiver: receiver::Mode,
        signature: host::signature::Definition,
    ) -> Result<Self, host::Error> {
        Ok(Self {
            path: host::path::Path::parse(path)?,
            kind: Kind::IntrinsicMethod,
            receiver: Some(receiver),
            signature,
            outcome: host::usage::Outcome::Conservative,
            evaluation: host::usage::Evaluation::Required,
            stream_identity: None,
            imported: None,
            guard: None,
            opens: false,
            application: None,
            rules: None,
        })
    }

    /// Return the canonical qualified item path.
    #[must_use]
    pub fn path(&self) -> String {
        self.path.to_string()
    }

    /// Return this item's semantic kind.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// Return the receiver mode when this item is a method.
    #[must_use]
    pub const fn receiver(&self) -> Option<receiver::Mode> {
        self.receiver
    }

    /// Return the complete typed signature.
    #[must_use]
    pub const fn signature(&self) -> &host::signature::Definition {
        &self.signature
    }

    /// Permit discarding the exact value produced by this item.
    #[must_use]
    pub fn discardable(mut self) -> Self {
        self.outcome = host::usage::Outcome::Discardable;
        self
    }

    /// Permit omitting this exact producer when its output is discarded.
    #[must_use]
    pub fn elidable(mut self) -> Self {
        self.evaluation = host::usage::Evaluation::Elidable;
        self
    }

    /// Return this item's exact produced-value usage outcome.
    #[must_use]
    pub const fn outcome(&self) -> host::usage::Outcome {
        self.outcome
    }

    /// Return whether this exact producer may be omitted when discarded.
    #[must_use]
    pub const fn evaluation(&self) -> host::usage::Evaluation {
        self.evaluation
    }

    /// Return the explicit canonical identity protocol for a stream operation.
    #[must_use]
    pub const fn stream_identity(&self) -> Option<host::stream::Version> {
        self.stream_identity
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported(mut self, selector: rawr_import::surface::identity::Selector) -> Self {
        self.imported = Some(selector);
        self
    }

    pub(crate) const fn imported_identity(&self) -> Option<&ImportedSelector> {
        self.imported.as_ref()
    }

    /// This imported method as one proof-backed application.
    #[cfg(feature = "rust-import")]
    pub(crate) fn applied(mut self, identity: [u8; 32]) -> Self {
        self.application = Some(identity);
        self
    }

    /// The proof-backed application this item stands for, when it is one.
    #[cfg(feature = "rust-import")]
    pub(crate) const fn application(&self) -> Option<&[u8; 32]> {
        self.application.as_ref()
    }

    /// The item's output is a borrow guard reached as `guard` says.
    #[cfg(feature = "rust-import")]
    pub(crate) fn guarded(mut self, guard: Option<Guard>) -> Self {
        self.guard = guard;
        self
    }

    /// How the item's borrow guard output is reached, when it gives one.
    pub(crate) const fn guard(&self) -> Option<Guard> {
        self.guard
    }

    /// The item's output borrows what the call takes (D558 tier 1).
    #[cfg(feature = "rust-import")]
    pub(crate) fn opening(mut self, opens: bool) -> Self {
        self.opens = opens;
        self
    }

    /// Whether the call opens a scope frame (D558 tier 1).
    pub(crate) const fn opens(&self) -> bool {
        self.opens
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn imported_path_len(&self) -> Option<usize> {
        self.imported.as_ref()?;
        self.path.display_len()
    }

    /// Produce this item's deterministic exact contract proof.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        #[cfg(feature = "rust-import")]
        let imported = self
            .imported
            .as_ref()
            .map(|selector| selector.fingerprint());
        #[cfg(not(feature = "rust-import"))]
        let imported = None;
        Fingerprint {
            path: self.path.to_string().into(),
            kind: self.kind,
            receiver: self.receiver,
            signature: self.signature.fingerprint(),
            outcome: self.outcome,
            evaluation: self.evaluation,
            stream_identity: self.stream_identity,
            imported,
            guard: self.guard,
            opens: self.opens,
            application: self.application,
            rules: self.rules.as_deref().map(MacroRules::digest),
        }
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn try_fingerprint(&self) -> Result<Fingerprint, rawr_import::surface::Error> {
        Ok(Fingerprint {
            path: self.path.to_string().into(),
            kind: self.kind,
            receiver: self.receiver,
            signature: self.signature.try_fingerprint()?,
            outcome: self.outcome,
            evaluation: self.evaluation,
            stream_identity: self.stream_identity,
            imported: self.imported.as_ref().map(ImportedSelector::fingerprint),
            guard: self.guard,
            opens: self.opens,
            application: self.application,
            rules: self.rules.as_deref().map(MacroRules::digest),
        })
    }

    pub(crate) fn resolve(
        &self,
        root: &host::path::Path,
        modules: &[host::path::Path],
        types: &[host::type_def::Definition],
        supports: &[host::support::SignatureContract],
        imported: &host::type_ref::ImportedTypes,
    ) -> Result<Self, host::Error> {
        let path = host::path::Path::qualify(root, &self.path);
        let parent = path.parent().unwrap_or_else(|| root.clone());
        let parent_is_root = &parent == root;

        match self.kind {
            Kind::Value
            | Kind::Function
            | Kind::Operation
            | Kind::StreamOperation
            | Kind::IntrinsicFunction => {
                if !parent_is_root && modules.binary_search(&parent).is_err() {
                    return Err(host::Error::MissingFacadeModule {
                        path: path.to_string().into(),
                        module: parent.to_string().into(),
                    });
                }
            }
            Kind::IntrinsicMacro | Kind::MacroRules => {
                if !parent_is_root
                    && modules.binary_search(&parent).is_err()
                    && types
                        .binary_search_by(|definition| definition.path_ref().cmp(&parent))
                        .is_err()
                {
                    return Err(host::Error::MissingFacadeModule {
                        path: path.to_string().into(),
                        module: parent.to_string().into(),
                    });
                }
            }
            Kind::AssociatedValue
            | Kind::AssociatedFunction
            | Kind::Method
            | Kind::IntrinsicMethod => {
                if types
                    .binary_search_by(|definition| definition.path_ref().cmp(&parent))
                    .is_err()
                {
                    return Err(host::Error::InvalidAssociatedOwner {
                        item: path.to_string().into(),
                        owner: parent.to_string().into(),
                    });
                }
            }
        }

        if self.kind == Kind::IntrinsicMacro && self.signature.parameters().len() != 1 {
            return Err(host::Error::InvalidIntrinsicMacroSignature {
                item: path.to_string().into(),
                actual: self.signature.parameters().len(),
            });
        }

        if self.evaluation == host::usage::Evaluation::Elidable
            && (self.outcome != host::usage::Outcome::Discardable
                || self.receiver.is_some()
                || !self.signature.parameters().is_empty())
        {
            return Err(host::Error::InvalidElidableSignature {
                item: path.to_string().into(),
            });
        }

        Ok(Self {
            signature: self
                .signature
                .resolve(root, types, supports, imported, &path)?,
            path,
            kind: self.kind,
            receiver: self.receiver,
            outcome: self.outcome,
            evaluation: self.evaluation,
            stream_identity: self.stream_identity,
            #[cfg(feature = "rust-import")]
            imported: self
                .imported
                .as_ref()
                .map(rawr_import::surface::identity::Selector::try_clone)
                .transpose()
                .map_err(|_| host::Error::Allocation)?,
            #[cfg(not(feature = "rust-import"))]
            imported: self.imported.clone(),
            guard: self.guard,
            opens: self.opens,
            application: self.application,
            rules: self.rules.clone(),
        })
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn resolve_imported<A: host::ImportAllocation + ?Sized>(
        mut self,
        root: &host::path::Path,
        modules: &[host::path::Path],
        types: &[host::type_def::Definition],
        aliases: &[host::path::Path],
        supports: &[host::support::SignatureContract],
        imported: &host::type_ref::ImportedTypes,
        allocation: &mut A,
    ) -> Result<Self, host::Error> {
        let relative = self.path;
        let path = host::path::Path::qualify_imported(root, relative, allocation)?;
        let parent = path.parent().unwrap_or_else(|| root.clone());
        let parent_is_root = &parent == root;
        // An alias that passes its parameters on to a type has that type's
        // associated functions, as iced's `widget::Text::new`.
        let alias_function = matches!(self.kind, Kind::AssociatedFunction | Kind::AssociatedValue)
            && aliases.contains(&parent);

        match self.kind {
            Kind::Value
            | Kind::Function
            | Kind::Operation
            | Kind::StreamOperation
            | Kind::IntrinsicFunction => {
                if !parent_is_root && modules.binary_search(&parent).is_err() {
                    return Err(host::Error::MissingFacadeModule {
                        path: path.to_string().into(),
                        module: parent.to_string().into(),
                    });
                }
            }
            Kind::IntrinsicMacro | Kind::MacroRules => {
                if !parent_is_root
                    && modules.binary_search(&parent).is_err()
                    && types
                        .binary_search_by(|definition| definition.path_ref().cmp(&parent))
                        .is_err()
                {
                    return Err(host::Error::MissingFacadeModule {
                        path: path.to_string().into(),
                        module: parent.to_string().into(),
                    });
                }
            }
            Kind::AssociatedValue
            | Kind::AssociatedFunction
            | Kind::Method
            | Kind::IntrinsicMethod => {
                if !alias_function
                    && types
                        .binary_search_by(|definition| definition.path_ref().cmp(&parent))
                        .is_err()
                {
                    return Err(host::Error::InvalidAssociatedOwner {
                        item: path.to_string().into(),
                        owner: parent.to_string().into(),
                    });
                }
            }
        }

        if self.kind == Kind::IntrinsicMacro && self.signature.parameters().len() != 1 {
            return Err(host::Error::InvalidIntrinsicMacroSignature {
                item: path.to_string().into(),
                actual: self.signature.parameters().len(),
            });
        }
        if self.evaluation == host::usage::Evaluation::Elidable
            && (self.outcome != host::usage::Outcome::Discardable
                || self.receiver.is_some()
                || !self.signature.parameters().is_empty())
        {
            return Err(host::Error::InvalidElidableSignature {
                item: path.to_string().into(),
            });
        }

        self.signature = self
            .signature
            .resolve_imported(root, types, supports, imported, &path, allocation)?;
        self.path = path;
        Ok(self)
    }

    pub(crate) const fn path_ref(&self) -> &host::path::Path {
        &self.path
    }
}

impl Fingerprint {
    #[cfg(feature = "rust-import")]
    pub(crate) fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> usize {
        self.signature.normalize_profile(profile)
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        let signature = self
            .signature
            .retained_bytes()?
            .checked_sub(std::mem::size_of::<host::signature::Fingerprint>())?;
        std::mem::size_of::<Self>()
            .checked_add(self.path.len())?
            .checked_add(signature)
    }
}

impl crate::preview::digest::Write for Fingerprint {
    fn write(
        &self,
        writer: &mut crate::preview::digest::Writer,
    ) -> Result<(), crate::preview::digest::Error> {
        use crate::preview::digest::write_receiver;

        writer.tag(0x2500);
        writer.tag(0x2501);
        writer.text(&self.path);
        writer.tag(0x2502);
        writer.tag(match self.kind {
            Kind::Value => 0x2511,
            Kind::AssociatedValue => 0x2512,
            Kind::Function => 0x2513,
            Kind::IntrinsicFunction => 0x2514,
            Kind::IntrinsicMacro => 0x2515,
            Kind::IntrinsicMethod => 0x2516,
            Kind::AssociatedFunction => 0x2517,
            Kind::Method => 0x2518,
            Kind::Operation => 0x2519,
            Kind::StreamOperation => 0x251a,
            Kind::MacroRules => 0x251b,
        });
        writer.tag(0x2503);
        write_receiver(writer, self.receiver);
        writer.tag(0x2504);
        self.signature.write(writer)?;
        writer.tag(0x2505);
        writer.tag(match self.outcome {
            host::usage::Outcome::Conservative => 0x2521,
            host::usage::Outcome::Discardable => 0x2522,
        });
        writer.tag(0x2506);
        writer.tag(match self.evaluation {
            host::usage::Evaluation::Required => 0x2531,
            host::usage::Evaluation::Elidable => 0x2532,
        });
        writer.tag(0x2507);
        match self.stream_identity {
            Some(version) => {
                writer.tag(0x2541);
                writer.u32(version.get());
            }
            None => writer.tag(0x2542),
        }
        writer.tag(0x2508);
        match self.imported {
            Some(identity) => {
                writer.tag(0x2551);
                writer.bytes(&identity);
            }
            None => writer.tag(0x2552),
        }
        // A borrow guard's output, only when there is one, so other items
        // keep their digests.
        if let Some(guard) = self.guard {
            writer.tag(0x2509);
            writer.tag(match guard {
                Guard::Shared => 0x2561,
                Guard::Mutable => 0x2562,
                Guard::OptionShared => 0x2563,
                Guard::OptionMutable => 0x2564,
            });
        }
        if self.opens {
            writer.tag(0x2563);
        }
        if let Some(rules) = self.rules {
            writer.tag(0x250a);
            writer.bytes(&rules);
        }
        Ok(())
    }
}
