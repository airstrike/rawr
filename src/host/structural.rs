//! Private executable proofs for authenticated generated Rust structure.
// Generated structural adapters are only admitted by the Rust importer; without
// `rust-import` the definitions stay empty but remain part of runtime tables.
#![cfg_attr(not(feature = "rust-import"), allow(dead_code))]

use crate::{host, Dynamic, EvalAltResult, ImmutableString, RawrResult, Span};

/// Exact generated structural operation family.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum Kind {
    Constructor,
    Field { copy: bool },
    Variant,
    /// A variant's fields when the value is that variant, else `None`.
    Match,
}

/// One generated-adapter admission retained by the compiler and runtime.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Definition {
    owner: host::facade::Dependency,
    path: ImmutableString,
    selector: [u8; 32],
    identity: [u8; 32],
    signature: [u8; 32],
    arguments: u32,
    manifest: [u8; 32],
    kind: Kind,
}

/// One fully instantiated private structural operation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Proof {
    definition: Definition,
    operands: Box<[Operand]>,
    output: host::runtime::type_ref::Type,
    receiver: Option<crate::receiver::Mode>,
    span: Span,
}

/// One exact structural operand and its observable-use proof.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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
        }
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

        let result = runtime.invoke(requirement, args, None);

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
