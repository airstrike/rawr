//! Private executable proofs for authenticated generated Rust structure.
// Generated structural adapters are only admitted by the Rust importer; without
// `rust-import` the definitions stay empty but remain part of runtime tables.
#![cfg_attr(not(feature = "rust-import"), allow(dead_code))]

use crate::{host, Dynamic, EvalAltResult, ImmutableString, RawrResult, Span};

/// Exact generated structural operation family.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(crate) enum Kind {
    Constructor,
    Field {
        copy: bool,
    },
    Variant,
    /// A variant's fields when the value is that variant, else `None`.
    Match,
    /// `Some(())` when the value is the variant, else `None`: a pattern's
    /// test, which reads no field.
    Test,
    /// The fields of a value of the variant, moved out of it, as a
    /// by-value pattern binds them.
    Take,
    /// The fields of a value of the variant, lent for the rest of the
    /// scope that binds them, as a pattern through `&T` binds them.
    Lend,
    /// The fields of a value of the variant, lent mutably for the rest of
    /// the scope that binds them, as a pattern through `&mut T` or a `ref
    /// mut` binding binds them; or, under a field's selector, that field of
    /// a struct, as `&mut value.field` lends it.
    LendMut,
    /// One field of a value of the variant, copied, as a binding of a
    /// `Copy` field reads it.
    Read {
        field: u32,
    },
    /// One field of a value of the variant, lent for the rest of the scope
    /// that reads it, as a pattern below the variant's reads it; or, under
    /// a field's selector, that field of a struct, as `&value.field` reads
    /// it.
    Borrow {
        field: u32,
    },
    /// A field set in place to a value, through the place's handle or a
    /// value native code lends `&mut`.
    Store,
}

impl Kind {
    /// Whether one selector names this adapter beside another of its
    /// family, which `kind` tells apart: a variant's matchers beside its
    /// constructor, and a field's store beside its reader.
    pub(crate) const fn shares_selector(self) -> bool {
        matches!(
            self,
            Self::Match
                | Self::Test
                | Self::Take
                | Self::Lend
                | Self::LendMut
                | Self::Read { .. }
                | Self::Borrow { .. }
                | Self::Store
        )
    }

    /// Whether the adapter lends what it reads: it gives a scope frame,
    /// whose lent values are reached only through their own types'
    /// accessors and whose copies are Rawr's own values.
    pub(crate) const fn lends(self) -> bool {
        matches!(self, Self::Lend | Self::LendMut | Self::Borrow { .. })
    }
}

/// One generated-adapter admission retained by the compiler and runtime.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(crate) struct Definition {
    owner: host::facade::Dependency,
    path: ImmutableString,
    selector: [u8; 32],
    identity: [u8; 32],
    signature: [u8; 32],
    arguments: u32,
    /// The runner's carrier that holds the adapter; a stored one is the
    /// reading runner's (`stored::carrier`).
    #[cfg_attr(feature = "rust-import", serde(with = "crate::stored::carrier"))]
    manifest: [u8; 32],
    kind: Kind,
    instance: Option<[u8; 32]>,
}

/// One fully instantiated private structural operation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(crate) struct Proof {
    definition: Definition,
    operands: Box<[Operand]>,
    output: host::runtime::type_ref::Type,
    receiver: Option<crate::receiver::Mode>,
    span: Span,
}

/// One exact structural operand and its observable-use proof.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(crate) struct Operand {
    ty: host::runtime::type_ref::Type,
    span: Span,
    receiver: bool,
}

impl Definition {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        owner: host::facade::Dependency,
        path: ImmutableString,
        selector: [u8; 32],
        identity: [u8; 32],
        signature: [u8; 32],
        arguments: u32,
        manifest: [u8; 32],
        kind: Kind,
    ) -> Self {
        Self {
            owner,
            path,
            selector,
            identity,
            signature,
            arguments,
            manifest,
            kind,
            instance: None,
        }
    }

    pub(crate) const fn with_instance(mut self, instance: Option<[u8; 32]>) -> Self {
        self.instance = instance;
        self
    }

    pub(crate) const fn owner(&self) -> &host::facade::Dependency {
        &self.owner
    }

    pub(crate) fn path(&self) -> &str {
        self.path.as_str()
    }

    pub(crate) const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }

    pub(crate) const fn identity(&self) -> &[u8; 32] {
        &self.identity
    }

    pub(crate) const fn arguments(&self) -> u32 {
        self.arguments
    }

    pub(crate) const fn kind(&self) -> Kind {
        self.kind
    }

    pub(crate) const fn instance(&self) -> Option<[u8; 32]> {
        self.instance
    }

    pub(crate) fn retained_bytes(&self) -> Option<usize> {
        std::mem::size_of::<Self>().checked_add(self.path.len())
    }
}

impl Proof {
    pub(crate) fn transform(
        definition: Definition,
        operands: Box<[Operand]>,
        output: host::runtime::type_ref::Type,
        receiver: Option<crate::receiver::Mode>,
        span: Span,
    ) -> Option<Self> {
        let expected = usize::try_from(definition.arguments()).ok()?;
        (expected == operands.len()).then_some(Self {
            definition,
            operands,
            output,
            receiver,
            span,
        })
    }

    pub(crate) const fn definition(&self) -> &Definition {
        &self.definition
    }

    pub(crate) const fn operands(&self) -> &[Operand] {
        &self.operands
    }

    pub(crate) const fn output(&self) -> &host::runtime::type_ref::Type {
        &self.output
    }

    pub(crate) const fn receiver(&self) -> Option<crate::receiver::Mode> {
        self.receiver
    }

    pub(crate) const fn span(&self) -> Span {
        self.span
    }

    pub(crate) fn dependencies(&self) -> std::collections::BTreeSet<host::facade::Dependency> {
        let mut dependencies = std::collections::BTreeSet::from([self.definition.owner().clone()]);
        for operand in &self.operands {
            operand.ty.collect_dependencies(&mut dependencies);
        }
        self.output.collect_dependencies(&mut dependencies);
        dependencies
    }

    pub(crate) fn symbol_id(&self) -> u64 {
        crate::program::schema::symbol_id(self)
    }

    /// This proof with each program type's schema as `schema` gives it
    /// (`type_ref::Type::renewed`).
    #[cfg(feature = "rust-import")]
    pub(crate) fn renewed(
        &self,
        schema: &dyn Fn(&crate::type_def::Identity) -> Option<crate::program::schema::Fingerprint>,
    ) -> Option<Self> {
        Some(Self {
            operands: self
                .operands
                .iter()
                .map(|operand| {
                    Some(Operand {
                        ty: operand.ty.renewed(schema)?,
                        ..operand.clone()
                    })
                })
                .collect::<Option<_>>()?,
            output: self.output.renewed(schema)?,
            ..self.clone()
        })
    }

    pub(crate) fn invoke_prepared(
        &self,
        runtime: &host::runtime::Registry,
        requirement: &host::runtime::Requirement,
        args: &mut [&mut Dynamic],
        model: &crate::program::model::Model,
    ) -> RawrResult {
        if args.len() != self.operands.len() {
            return Err(EvalAltResult::ErrorRuntime(
                format!(
                    "generated structural adapter for `{}` received {} operand(s), expected {}",
                    self.definition.path(),
                    args.len(),
                    self.operands.len()
                )
                .into(),
                self.span.start(),
            )
            .into());
        }
        for (index, (operand, value)) in self.operands.iter().zip(args.iter()).enumerate() {
            if let Err(error) = operand.ty.validate(value, model, runtime) {
                assert!(
                    !error.is_invariant(),
                    "internal error: `{}` was given operand {index} as a native value that is not its `{}`: {error}. This is a rawr bug",
                    self.definition.path(),
                    operand.ty
                );
                return Err(EvalAltResult::ErrorRuntime(
                    format!(
                        "generated structural adapter for `{}` received invalid operand {index}, expected `{}`: {error}",
                        self.definition.path(), operand.ty
                    )
                    .into(),
                    operand.span.start(),
                )
                .into());
            }
        }

        // A field a pattern borrows is lent to the call alone, but a lender
        // of its own fields projects through it.
        let result = if self.definition.kind().lends() {
            runtime.invoke(requirement, args, None, None, &crate::Access::NONE)
        } else {
            crate::scope::opened(args, &mut |args| {
                runtime.invoke(requirement, args, None, None, &crate::Access::NONE)
            })
        };

        for (index, (operand, value)) in self.operands.iter().zip(args.iter()).enumerate() {
            let observable =
                operand.receiver && !matches!(self.receiver, Some(crate::receiver::Mode::Owned));
            if observable && let Err(error) = operand.ty.validate(value, model, runtime) {
                return Err(EvalAltResult::ErrorRuntime(
                        format!(
                            "generated structural adapter for `{}` left observable operand {index} outside exact `{}`: {error}",
                            self.definition.path(), operand.ty
                        )
                        .into(),
                        operand.span.start(),
                    )
                    .into());
            }
        }

        let result = result?;
        // A scope frame a variant's lender made: the fields it lends are
        // reached only through their own types' accessors, and the copies
        // it makes are Rawr's own values.
        if self.definition.kind().lends() && crate::scope::is_opening(&result) {
            return Ok(result);
        }
        if let Err(error) = self.output.validate(&result, model, runtime) {
            return Err(EvalAltResult::ErrorRuntime(
                format!(
                    "generated structural adapter for `{}` returned `{}`, expected `{}`: {error}",
                    self.definition.path(),
                    result.type_name(),
                    self.output
                )
                .into(),
                self.span.start(),
            )
            .into());
        }
        Ok(result)
    }
}

impl Operand {
    pub(crate) const fn new(ty: host::runtime::type_ref::Type, span: Span, receiver: bool) -> Self {
        Self { ty, span, receiver }
    }

    pub(crate) const fn ty(&self) -> &host::runtime::type_ref::Type {
        &self.ty
    }
}
