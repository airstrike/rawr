//! Exact private code-generation plans retained by sealed demand.

use std::mem;

use serde::{Deserialize, Serialize};

use crate::api::{identity as api, model};
use crate::classify::representability::{self, convert, ownership, signature};

use super::identity;
use super::Error;

/// One exact demanded operation, complete enough to generate a rooted thunk.
#[derive(Deserialize, Serialize)]
pub(crate) struct Plan {
    identity: EntryIdentity,
    selector: Option<[u8; 32]>,
    export: api::Export,
    name: Box<str>,
    origin: representability::origin::Set,
    operation: Operation,
}

/// Stable private identity for one generated table entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct EntryIdentity([u8; 32]);

/// The closed Phase-3 operation payload after selecting one structural member.
#[derive(Deserialize, Serialize)]
pub(crate) enum Operation {
    Declaration {
        kind: Declaration,
        parameters: Box<[signature::Parameter]>,
    },
    StructConstructor {
        parameters: Box<[signature::Parameter]>,
        form: model::FieldForm,
        fields: Box<[Field]>,
    },
    StructField {
        parameters: Box<[signature::Parameter]>,
        field: Field,
    },
    EnumVariant {
        parameters: Box<[signature::Parameter]>,
        name: Box<str>,
        form: model::FieldForm,
        fields: Box<[Field]>,
    },
    Function {
        signature: signature::Signature,
        substitutions: Box<[convert::Type]>,
        constants: Box<[convert::Constant]>,
        functions: Box<[FunctionItem]>,
        mappers: Box<[EventMapper]>,
        container_parameters: usize,
        associated: bool,
    },
    Method {
        receiver: ownership::Receiver,
        signature: signature::Signature,
        substitutions: Box<[convert::Type]>,
        constants: Box<[convert::Constant]>,
        container_parameters: usize,
    },
    Constant {
        ty: convert::Type,
        value: Option<convert::Constant>,
        associated: bool,
    },
    Builder {
        steps: Box<[BuilderStep]>,
    },
}

/// One fully resolved native call inside a single-invocation builder replay.
#[derive(Deserialize, Serialize)]
pub(crate) struct BuilderStep {
    plan: Plan,
    rawr: super::Origin,
    arguments: Box<[u32]>,
    construct: bool,
}

/// One compile-time imported function item rooted inside its containing call.
#[derive(Deserialize, Serialize)]
pub(crate) struct FunctionItem {
    position: u32,
    plan: Box<Plan>,
}

/// One non-authoritative profile event mapper selected at an exact formal.
#[derive(Clone, Copy, Deserialize, Serialize)]
pub(crate) struct EventMapper {
    position: u32,
    seal: [u8; 16],
    projection: super::instance::EventProjection,
}

/// The exact declaration family for a type-only demanded entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum Declaration {
    Struct,
    Enum,
}

/// One selected structural field without the rest of its parent declaration.
#[derive(Deserialize, Serialize)]
pub(crate) struct Field {
    name: Box<str>,
    index: u32,
    ty: convert::Type,
    span: model::Span,
}

impl Plan {
    pub(super) fn retained_builder_bytes(
        contract: &crate::contract::Contract<crate::contract::Classified>,
        builder: &super::builder::Plan,
    ) -> Result<Option<usize>, Error> {
        let mut retained = mem::size_of::<Self>();
        for step in builder.steps() {
            let outcome = contract
                .outcome(step.entry().identity())
                .ok_or(Error::ForeignIdentity)?;
            let nested = Self::retained_bytes(contract, outcome, step.entry().identity())?.ok_or(
                Error::Limit {
                    resource: "builder adapter retained bytes",
                    limit: usize::MAX,
                    actual: usize::MAX,
                },
            )?;
            retained = retained
                .checked_add(mem::size_of::<BuilderStep>())
                .and_then(|bytes| bytes.checked_add(nested))
                .and_then(|bytes| bytes.checked_add(step.entry().origin().source().len()))
                .and_then(|bytes| {
                    bytes.checked_add(step.arguments().len().checked_mul(mem::size_of::<u32>())?)
                })
                .ok_or(Error::Limit {
                    resource: "builder adapter retained bytes",
                    limit: usize::MAX,
                    actual: usize::MAX,
                })?;
        }
        Ok(Some(retained))
    }

    pub(super) fn try_builder(
        contract: &crate::contract::Contract<crate::contract::Classified>,
        builder: &super::builder::Plan,
    ) -> Result<Self, Error> {
        let mut steps = Vec::new();
        steps
            .try_reserve_exact(builder.steps().len())
            .map_err(|_| Error::Allocation)?;
        for step in builder.steps() {
            let outcome = contract
                .outcome(step.entry().identity())
                .ok_or(Error::ForeignIdentity)?;
            let nested = Self::try_new(contract, outcome, step.entry().identity())?;
            let arity = match nested.operation() {
                Operation::StructConstructor { fields, .. } => fields.len(),
                Operation::Function {
                    signature,
                    functions,
                    mappers,
                    associated: true,
                    ..
                } => signature
                    .inputs()
                    .len()
                    .checked_sub(functions.len())
                    .and_then(|arity| arity.checked_sub(mappers.len()))
                    .ok_or(Error::BuilderShape)?,
                Operation::Method { signature, .. } => signature.inputs().len(),
                _ => return Err(Error::BuilderShape),
            };
            if arity != step.arguments().len()
                || (step.is_construct()
                    && !matches!(
                        nested.operation(),
                        Operation::StructConstructor { .. }
                            | Operation::Function {
                                associated: true,
                                ..
                            }
                    ))
                || (!step.is_construct()
                    && !matches!(
                        nested.operation(),
                        Operation::Method {
                            receiver: ownership::Receiver::Owned,
                            ..
                        }
                    ))
            {
                return Err(Error::BuilderShape);
            }
            let mut arguments = Vec::new();
            arguments
                .try_reserve_exact(step.arguments().len())
                .map_err(|_| Error::Allocation)?;
            arguments.extend(step.arguments().iter().map(|value| value.position()));
            steps.push(BuilderStep {
                plan: nested,
                rawr: step.entry().origin().clone(),
                arguments: arguments.into_boxed_slice(),
                construct: step.is_construct(),
            });
        }
        let last = steps.last().ok_or(Error::BuilderShape)?;
        Ok(Self {
            identity: EntryIdentity(*builder.as_bytes()),
            selector: None,
            export: clone_export(last.plan.export())?,
            name: clone_str(last.plan.name())?,
            origin: clone_origin(SelectedOrigin {
                source: last.plan.origin().source(),
                declaration: last.plan.origin().declaration(),
            })?,
            operation: Operation::Builder {
                steps: steps.into_boxed_slice(),
            },
        })
    }

    /// Measure all retained ownership before any plan allocation begins.
    pub(crate) fn retained_bytes(
        contract: &crate::contract::Contract<crate::contract::Classified>,
        outcome: crate::classify::Outcome<'_>,
        selected: &identity::Identity,
    ) -> Result<Option<usize>, Error> {
        reject_unresolved_structural_parameters(outcome.representability())?;
        let origin = selected_origin(outcome, selected.selector())?;
        let operation = operation_bytes(contract, outcome.representability(), selected)?;
        let Some(export) = outcome.export().retained_bytes() else {
            return Ok(None);
        };
        let Some(operation) = operation else {
            return Ok(None);
        };
        Ok(mem::size_of::<Self>()
            .checked_add(export)
            .and_then(|bytes| bytes.checked_add(outcome.name().len()))
            .and_then(|bytes| bytes.checked_add(origin.source().retained_bytes()))
            .and_then(|bytes| bytes.checked_add(origin.declaration().retained_bytes()))
            .and_then(|bytes| bytes.checked_add(operation)))
    }

    /// Fallibly retain one already-charged exact adapter plan.
    pub(super) fn try_new(
        contract: &crate::contract::Contract<crate::contract::Classified>,
        outcome: crate::classify::Outcome<'_>,
        selected: &identity::Identity,
    ) -> Result<Self, Error> {
        reject_unresolved_structural_parameters(outcome.representability())?;
        let origin = selected_origin(outcome, selected.selector())?;
        Ok(Self {
            identity: EntryIdentity(*selected.as_bytes()),
            selector: Some(selected.selector().fingerprint()),
            export: clone_export(outcome.export())?,
            name: clone_str(outcome.name())?,
            origin: clone_origin(origin)?,
            operation: clone_operation(contract, outcome.representability(), selected)?,
        })
    }

    pub(crate) const fn identity(&self) -> EntryIdentity {
        self.identity
    }

    pub(crate) const fn selector(&self) -> Option<&[u8; 32]> {
        self.selector.as_ref()
    }

    pub(crate) const fn export(&self) -> &api::Export {
        &self.export
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) const fn origin(&self) -> &representability::origin::Set {
        &self.origin
    }

    pub(crate) const fn operation(&self) -> &Operation {
        &self.operation
    }

    pub(crate) fn builder_steps(&self) -> Option<&[BuilderStep]> {
        match &self.operation {
            Operation::Builder { steps } => Some(steps),
            _ => None,
        }
    }
}

fn reject_unresolved_structural_parameters(
    plan: &representability::Representability,
) -> Result<(), Error> {
    if super::has_unresolved_structural_parameters(plan) {
        return Err(Error::AdapterPlan {
            reason: "generic structural adapter requires concrete substitution proof",
        });
    }
    Ok(())
}

impl EntryIdentity {
    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Field {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    #[cfg(test)]
    pub(crate) const fn index(&self) -> u32 {
        self.index
    }

    pub(crate) const fn ty(&self) -> &convert::Type {
        &self.ty
    }

    #[cfg(test)]
    pub(crate) const fn span(&self) -> &model::Span {
        &self.span
    }
}

impl BuilderStep {
    pub(crate) const fn plan(&self) -> &Plan {
        &self.plan
    }

    pub(crate) const fn rawr(&self) -> &super::Origin {
        &self.rawr
    }

    pub(crate) const fn arguments(&self) -> &[u32] {
        &self.arguments
    }

    pub(crate) const fn is_construct(&self) -> bool {
        self.construct
    }
}

impl FunctionItem {
    pub(crate) const fn position(&self) -> u32 {
        self.position
    }

    pub(crate) const fn plan(&self) -> &Plan {
        &self.plan
    }
}

impl EventMapper {
    pub(crate) const fn position(&self) -> u32 {
        self.position
    }

    pub(crate) const fn seal(&self) -> &[u8; 16] {
        &self.seal
    }

    pub(crate) const fn projection(&self) -> &super::instance::EventProjection {
        &self.projection
    }
}

fn selected_origin<'a>(
    outcome: crate::classify::Outcome<'a>,
    selector: &identity::Selector,
) -> Result<SelectedOrigin<'a>, Error> {
    let origin = outcome.origin().ok_or(Error::AdapterPlan {
        reason: "demanded outcome has no Rust origin",
    })?;
    let declaration = match (outcome.representability(), selector) {
        (representability::Representability::Struct(_), identity::Selector::Declaration)
        | (
            representability::Representability::Struct(_),
            identity::Selector::StructConstructor(_),
        )
        | (representability::Representability::Enum(_), identity::Selector::Declaration)
        | (
            representability::Representability::Function(_),
            identity::Selector::Function { .. } | identity::Selector::AssociatedFunction { .. },
        )
        | (representability::Representability::Method(_), identity::Selector::Method { .. })
        | (
            representability::Representability::FunctionItem(_),
            identity::Selector::Function { .. }
            | identity::Selector::AssociatedFunction { .. }
            | identity::Selector::Method { .. },
        )
        | (
            representability::Representability::Invocation(_),
            identity::Selector::AssociatedFunction { .. } | identity::Selector::Method { .. },
        )
        | (
            representability::Representability::Constant(_),
            identity::Selector::Constant { .. } | identity::Selector::AssociatedConstant { .. },
        ) => origin.declaration(),
        (representability::Representability::Struct(plan), selector) => plan
            .fields()
            .iter()
            .find(|field| field.selector() == selector)
            .map(representability::structure::Field::span)
            .ok_or(Error::AdapterPlan {
                reason: "struct selector is absent from its exact plan",
            })?,
        (representability::Representability::Enum(plan), selector) => plan
            .variants()
            .iter()
            .find(|variant| variant.selector() == selector)
            .map(representability::enumeration::Variant::span)
            .ok_or(Error::AdapterPlan {
                reason: "enum selector is absent from its exact plan",
            })?,
        (
            representability::Representability::Value(_)
            | representability::Representability::Excluded(_)
            | representability::Representability::Rejected(_),
            _,
        )
        | (representability::Representability::Function(_), _)
        | (representability::Representability::FunctionItem(_), _)
        | (representability::Representability::Invocation(_), _)
        | (representability::Representability::Method(_), _)
        | (representability::Representability::Constant(_), _) => {
            return Err(Error::AdapterPlan {
                reason: "selector and representability plan disagree",
            });
        }
    };
    Ok(SelectedOrigin {
        source: origin.source(),
        declaration,
    })
}

struct SelectedOrigin<'a> {
    source: &'a model::Span,
    declaration: &'a model::Span,
}

impl SelectedOrigin<'_> {
    const fn source(&self) -> &model::Span {
        self.source
    }

    const fn declaration(&self) -> &model::Span {
        self.declaration
    }
}

fn operation_bytes(
    contract: &crate::contract::Contract<crate::contract::Classified>,
    plan: &representability::Representability,
    selected: &identity::Identity,
) -> Result<Option<usize>, Error> {
    let selector = selected.selector();
    Ok(match (plan, selector) {
        (representability::Representability::Struct(plan), identity::Selector::Declaration) => {
            parameter_slice_bytes(plan.parameters())
        }
        (
            representability::Representability::Struct(plan),
            selected @ identity::Selector::StructConstructor(_),
        ) if plan.constructor() == Some(selected) => parameter_slice_bytes(plan.parameters())
            .and_then(|bytes| bytes.checked_add(field_slice_bytes(plan.fields())?)),
        (representability::Representability::Struct(plan), selected) => {
            let field = plan
                .fields()
                .iter()
                .find(|field| field.selector() == selected)
                .ok_or(Error::AdapterPlan {
                    reason: "struct selector is absent from its exact plan",
                })?;
            parameter_slice_bytes(plan.parameters())
                .and_then(|bytes| bytes.checked_add(field_dynamic_bytes(field)?))
        }
        (representability::Representability::Enum(plan), identity::Selector::Declaration) => {
            parameter_slice_bytes(plan.parameters())
        }
        (representability::Representability::Enum(plan), selected) => {
            let variant = plan
                .variants()
                .iter()
                .find(|variant| variant.selector() == selected)
                .ok_or(Error::AdapterPlan {
                    reason: "enum selector is absent from its exact plan",
                })?;
            parameter_slice_bytes(plan.parameters())
                .and_then(|bytes| bytes.checked_add(variant.name().len()))
                .and_then(|bytes| bytes.checked_add(field_slice_bytes(variant.fields())?))
        }
        (
            representability::Representability::Function(plan),
            identity::Selector::Function { .. } | identity::Selector::AssociatedFunction { .. },
        ) => signature_dynamic_bytes(plan.signature())
            .and_then(|bytes| bytes.checked_add(function_items_bytes(contract, selected)?))
            .and_then(|bytes| bytes.checked_add(event_mappers_bytes(selected)?)),
        (representability::Representability::Method(plan), identity::Selector::Method { .. }) => {
            signature_dynamic_bytes(plan.signature())
        }
        (
            representability::Representability::FunctionItem(plan),
            identity::Selector::Function { .. }
            | identity::Selector::AssociatedFunction { .. }
            | identity::Selector::Method { .. },
        ) => signature_dynamic_bytes(plan.signature())
            .and_then(|bytes| bytes.checked_add(type_slice_bytes(selected.types())?))
            .and_then(|bytes| bytes.checked_add(constant_slice_bytes(selected.constants())?))
            .and_then(|bytes| bytes.checked_add(function_items_bytes(contract, selected)?))
            .and_then(|bytes| bytes.checked_add(event_mappers_bytes(selected)?)),
        (
            representability::Representability::Invocation(plan),
            identity::Selector::AssociatedFunction { .. } | identity::Selector::Method { .. },
        ) => signature_dynamic_bytes(plan.signature())
            .and_then(|bytes| bytes.checked_add(type_slice_bytes(selected.types())?))
            .and_then(|bytes| bytes.checked_add(constant_slice_bytes(selected.constants())?))
            .and_then(|bytes| bytes.checked_add(function_items_bytes(contract, selected)?))
            .and_then(|bytes| bytes.checked_add(event_mappers_bytes(selected)?)),
        (
            representability::Representability::Constant(plan),
            identity::Selector::Constant { .. } | identity::Selector::AssociatedConstant { .. },
        ) => type_dynamic_bytes(plan.ty()).and_then(|bytes| {
            plan.value().map_or(Some(bytes), |value| {
                bytes
                    .checked_add(value.ty().len())?
                    .checked_add(value.value().len())
            })
        }),
        (
            representability::Representability::Value(_)
            | representability::Representability::Excluded(_)
            | representability::Representability::Rejected(_),
            _,
        )
        | (representability::Representability::Function(_), _)
        | (representability::Representability::FunctionItem(_), _)
        | (representability::Representability::Invocation(_), _)
        | (representability::Representability::Method(_), _)
        | (representability::Representability::Constant(_), _) => {
            return Err(Error::AdapterPlan {
                reason: "selector and representability plan disagree",
            });
        }
    })
}

fn clone_operation(
    contract: &crate::contract::Contract<crate::contract::Classified>,
    plan: &representability::Representability,
    selected: &identity::Identity,
) -> Result<Operation, Error> {
    let selector = selected.selector();
    match (plan, selector) {
        (representability::Representability::Struct(plan), identity::Selector::Declaration) => {
            Ok(Operation::Declaration {
                kind: Declaration::Struct,
                parameters: clone_parameters(plan.parameters())?,
            })
        }
        (
            representability::Representability::Struct(plan),
            selected @ identity::Selector::StructConstructor(_),
        ) if plan.constructor() == Some(selected) => Ok(Operation::StructConstructor {
            parameters: clone_parameters(plan.parameters())?,
            form: plan.form(),
            fields: clone_fields(plan.fields())?,
        }),
        (representability::Representability::Struct(plan), selected) => {
            let field = plan
                .fields()
                .iter()
                .find(|field| field.selector() == selected)
                .ok_or(Error::AdapterPlan {
                    reason: "struct selector is absent from its exact plan",
                })?;
            Ok(Operation::StructField {
                parameters: clone_parameters(plan.parameters())?,
                field: clone_field(field)?,
            })
        }
        (representability::Representability::Enum(plan), identity::Selector::Declaration) => {
            Ok(Operation::Declaration {
                kind: Declaration::Enum,
                parameters: clone_parameters(plan.parameters())?,
            })
        }
        (representability::Representability::Enum(plan), selected) => {
            let variant = plan
                .variants()
                .iter()
                .find(|variant| variant.selector() == selected)
                .ok_or(Error::AdapterPlan {
                    reason: "enum selector is absent from its exact plan",
                })?;
            Ok(Operation::EnumVariant {
                parameters: clone_parameters(plan.parameters())?,
                name: clone_str(variant.name())?,
                form: variant.form(),
                fields: clone_fields(variant.fields())?,
            })
        }
        (
            representability::Representability::Function(plan),
            selector @ (identity::Selector::Function { .. }
            | identity::Selector::AssociatedFunction { .. }),
        ) => Ok(Operation::Function {
            signature: clone_signature(plan.signature())?,
            substitutions: Box::default(),
            constants: Box::default(),
            functions: clone_function_items(contract, selected)?,
            mappers: clone_event_mappers(selected)?,
            container_parameters: 0,
            associated: matches!(selector, identity::Selector::AssociatedFunction { .. }),
        }),
        (representability::Representability::Method(plan), identity::Selector::Method { .. }) => {
            Ok(Operation::Method {
                receiver: plan.receiver(),
                signature: clone_signature(plan.signature())?,
                substitutions: Box::default(),
                constants: Box::default(),
                container_parameters: 0,
            })
        }
        (
            representability::Representability::FunctionItem(plan),
            selector @ (identity::Selector::Function { .. }
            | identity::Selector::AssociatedFunction { .. }),
        ) => Ok(Operation::Function {
            signature: clone_signature(plan.signature())?,
            substitutions: clone_types(selected.types())?,
            constants: clone_constants(selected.constants())?,
            functions: clone_function_items(contract, selected)?,
            mappers: clone_event_mappers(selected)?,
            container_parameters: 0,
            associated: matches!(selector, identity::Selector::AssociatedFunction { .. }),
        }),
        (
            representability::Representability::Invocation(plan),
            identity::Selector::AssociatedFunction { .. },
        ) => Ok(Operation::Function {
            signature: clone_signature(plan.signature())?,
            substitutions: clone_types(selected.types())?,
            constants: clone_constants(selected.constants())?,
            functions: clone_function_items(contract, selected)?,
            mappers: clone_event_mappers(selected)?,
            container_parameters: plan.container_parameters(),
            associated: true,
        }),
        (
            representability::Representability::FunctionItem(plan),
            identity::Selector::Method { .. },
        ) => Ok(Operation::Method {
            receiver: plan.receiver().ok_or(Error::AdapterPlan {
                reason: "generic method has no receiver proof",
            })?,
            signature: clone_signature(plan.signature())?,
            substitutions: clone_types(selected.types())?,
            constants: clone_constants(selected.constants())?,
            container_parameters: 0,
        }),
        (
            representability::Representability::Invocation(plan),
            identity::Selector::Method { .. },
        ) => Ok(Operation::Method {
            receiver: plan.receiver().ok_or(Error::AdapterPlan {
                reason: "generic invocation method has no receiver proof",
            })?,
            signature: clone_signature(plan.signature())?,
            substitutions: clone_types(selected.types())?,
            constants: clone_constants(selected.constants())?,
            container_parameters: plan.container_parameters(),
        }),
        (
            representability::Representability::Constant(plan),
            selector @ (identity::Selector::Constant { .. }
            | identity::Selector::AssociatedConstant { .. }),
        ) => Ok(Operation::Constant {
            ty: clone_type(plan.ty())?,
            value: plan.value().map(clone_constant).transpose()?,
            associated: matches!(selector, identity::Selector::AssociatedConstant { .. }),
        }),
        (
            representability::Representability::Value(_)
            | representability::Representability::Excluded(_)
            | representability::Representability::Rejected(_),
            _,
        )
        | (representability::Representability::Function(_), _)
        | (representability::Representability::FunctionItem(_), _)
        | (representability::Representability::Invocation(_), _)
        | (representability::Representability::Method(_), _)
        | (representability::Representability::Constant(_), _) => Err(Error::AdapterPlan {
            reason: "selector and representability plan disagree",
        }),
    }
}

fn function_items_bytes(
    contract: &crate::contract::Contract<crate::contract::Classified>,
    selected: &identity::Identity,
) -> Option<usize> {
    selected.function_items().iter().try_fold(
        selected
            .function_items()
            .len()
            .checked_mul(mem::size_of::<FunctionItem>())?,
        |bytes, function| {
            let outcome = contract.outcome(function.identity())?;
            bytes.checked_add(Plan::retained_bytes(contract, outcome, function.identity()).ok()??)
        },
    )
}

fn event_mappers_bytes(selected: &identity::Identity) -> Option<usize> {
    selected
        .event_mappers()
        .len()
        .checked_mul(mem::size_of::<EventMapper>())
}

fn clone_function_items(
    contract: &crate::contract::Contract<crate::contract::Classified>,
    selected: &identity::Identity,
) -> Result<Box<[FunctionItem]>, Error> {
    let mut functions = Vec::new();
    functions
        .try_reserve_exact(selected.function_items().len())
        .map_err(|_| Error::Allocation)?;
    for function in selected.function_items() {
        let outcome = contract
            .outcome(function.identity())
            .ok_or(Error::ForeignIdentity)?;
        let plan = Plan::try_new(contract, outcome, function.identity())?;
        if !matches!(
            plan.operation(),
            Operation::Function {
                functions,
                substitutions,
                ..
            } if functions.is_empty() && substitutions.is_empty()
        ) {
            return Err(Error::InstanceMismatch);
        }
        functions.push(FunctionItem {
            position: function.position(),
            plan: Box::new(plan),
        });
    }
    Ok(functions.into_boxed_slice())
}

fn clone_event_mappers(selected: &identity::Identity) -> Result<Box<[EventMapper]>, Error> {
    selected
        .event_mappers()
        .iter()
        .map(|mapper| {
            Ok(EventMapper {
                position: mapper.position(),
                seal: *mapper.seal(),
                projection: *mapper.projection().ok_or(Error::InstanceMismatch)?,
            })
        })
        .collect()
}

fn clone_export(export: &api::Export) -> Result<api::Export, Error> {
    let mut path = Vec::new();
    path.try_reserve_exact(export.path().len())
        .map_err(|_| Error::Allocation)?;
    for segment in export.path() {
        path.push(clone_str(segment)?);
    }
    Ok(api::Export::new(
        clone_str(export.root_alias())?,
        path.into_boxed_slice(),
        export.namespace(),
    ))
}

fn clone_origin(origin: SelectedOrigin<'_>) -> Result<representability::origin::Set, Error> {
    Ok(representability::origin::Set::new(
        clone_span(origin.source())?,
        clone_span(origin.declaration())?,
    ))
}

fn clone_span(span: &model::Span) -> Result<model::Span, Error> {
    let ((line_start, column_start), (line_end, column_end)) = span.positions();
    let range = span.byte_range();
    Ok(model::Span::new(
        clone_str(span.file())?,
        range.start,
        range.end,
        line_start,
        column_start,
        line_end,
        column_end,
    ))
}

fn clone_fields(fields: &[representability::structure::Field]) -> Result<Box<[Field]>, Error> {
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(fields.len())
        .map_err(|_| Error::Allocation)?;
    for field in fields {
        retained.push(clone_field(field)?);
    }
    Ok(retained.into_boxed_slice())
}

fn clone_field(field: &representability::structure::Field) -> Result<Field, Error> {
    Ok(Field {
        name: clone_str(field.name())?,
        index: field.index(),
        ty: clone_type(field.ty())?,
        span: clone_span(field.span())?,
    })
}

fn clone_signature(source: &signature::Signature) -> Result<signature::Signature, Error> {
    Ok(signature::Signature::new(
        clone_parameters(source.parameters())?,
        clone_types(source.inputs())?,
        clone_type(source.output())?,
    ))
}

fn clone_parameters(source: &[signature::Parameter]) -> Result<Box<[signature::Parameter]>, Error> {
    let mut parameters = Vec::new();
    parameters
        .try_reserve_exact(source.len())
        .map_err(|_| Error::Allocation)?;
    for parameter in source {
        parameters.push(signature::Parameter::new(
            parameter.index(),
            clone_str(parameter.name())?,
            parameter.kind(),
        ));
    }
    Ok(parameters.into_boxed_slice())
}

fn clone_types(source: &[convert::Type]) -> Result<Box<[convert::Type]>, Error> {
    let mut types = Vec::new();
    types
        .try_reserve_exact(source.len())
        .map_err(|_| Error::Allocation)?;
    for ty in source {
        types.push(clone_type(ty)?);
    }
    Ok(types.into_boxed_slice())
}

fn clone_type(source: &convert::Type) -> Result<convert::Type, Error> {
    Ok(match source {
        convert::Type::Unit => convert::Type::Unit,
        convert::Type::Builtin(builtin) => convert::Type::Builtin(*builtin),
        convert::Type::Nominal {
            definition,
            arguments,
        } => convert::Type::Nominal {
            definition: *definition,
            arguments: clone_types(arguments)?,
        },
        convert::Type::FunctionItem { input, output } => convert::Type::FunctionItem {
            input: Box::new(clone_type(input)?),
            output: Box::new(clone_type(output)?),
        },
        convert::Type::ProfileMessage { mapper, payload } => convert::Type::ProfileMessage {
            mapper: *mapper,
            payload: Box::new(clone_type(payload)?),
        },
        convert::Type::Option(element) => convert::Type::Option(Box::new(clone_type(element)?)),
        convert::Type::Result(ok, err) => {
            convert::Type::Result(Box::new(clone_type(ok)?), Box::new(clone_type(err)?))
        }
        convert::Type::Sequence { element, owned } => convert::Type::Sequence {
            element: Box::new(clone_type(element)?),
            owned: *owned,
        },
        convert::Type::Tuple(members) => convert::Type::Tuple(clone_types(members)?),
        convert::Type::Parameter(index) => convert::Type::Parameter(*index),
    })
}

fn clone_constant(source: &convert::Constant) -> Result<convert::Constant, Error> {
    Ok(convert::Constant::literal(
        clone_str(source.ty())?,
        clone_str(source.value())?,
    ))
}

fn clone_constants(source: &[convert::Constant]) -> Result<Box<[convert::Constant]>, Error> {
    let mut constants = Vec::new();
    constants
        .try_reserve_exact(source.len())
        .map_err(|_| Error::Allocation)?;
    for constant in source {
        constants.push(clone_constant(constant)?);
    }
    Ok(constants.into_boxed_slice())
}

fn clone_str(source: &str) -> Result<Box<str>, Error> {
    let mut retained = String::new();
    retained
        .try_reserve_exact(source.len())
        .map_err(|_| Error::Allocation)?;
    retained.push_str(source);
    Ok(retained.into_boxed_str())
}

fn parameter_slice_bytes(parameters: &[signature::Parameter]) -> Option<usize> {
    parameters
        .len()
        .checked_mul(mem::size_of::<signature::Parameter>())?
        .checked_add(parameters.iter().try_fold(0_usize, |bytes, parameter| {
            bytes.checked_add(parameter.name().len())
        })?)
}

fn field_slice_bytes(fields: &[representability::structure::Field]) -> Option<usize> {
    fields
        .len()
        .checked_mul(mem::size_of::<Field>())?
        .checked_add(fields.iter().try_fold(0_usize, |bytes, field| {
            bytes.checked_add(field_dynamic_bytes(field)?)
        })?)
}

fn field_dynamic_bytes(field: &representability::structure::Field) -> Option<usize> {
    field
        .name()
        .len()
        .checked_add(type_dynamic_bytes(field.ty())?)?
        .checked_add(field.span().retained_bytes())
}

fn signature_dynamic_bytes(signature: &signature::Signature) -> Option<usize> {
    parameter_slice_bytes(signature.parameters())?
        .checked_add(type_slice_bytes(signature.inputs())?)?
        .checked_add(type_dynamic_bytes(signature.output())?)
}

fn type_slice_bytes(types: &[convert::Type]) -> Option<usize> {
    types
        .len()
        .checked_mul(mem::size_of::<convert::Type>())?
        .checked_add(types.iter().try_fold(0_usize, |bytes, ty| {
            bytes.checked_add(type_dynamic_bytes(ty)?)
        })?)
}

fn constant_slice_bytes(constants: &[convert::Constant]) -> Option<usize> {
    constants.iter().try_fold(
        constants
            .len()
            .checked_mul(mem::size_of::<convert::Constant>())?,
        |bytes, constant| {
            bytes
                .checked_add(constant.ty().len())?
                .checked_add(constant.value().len())
        },
    )
}

fn type_dynamic_bytes(ty: &convert::Type) -> Option<usize> {
    match ty {
        convert::Type::Nominal { arguments, .. } => type_slice_bytes(arguments),
        convert::Type::FunctionItem { input, output } => type_dynamic_bytes(input)?
            .checked_add(type_dynamic_bytes(output)?)
            .and_then(|bytes| bytes.checked_add(2 * mem::size_of::<convert::Type>())),
        convert::Type::ProfileMessage { payload, .. } => type_dynamic_bytes(payload)?
            .checked_add(mem::size_of::<convert::Type>())
            .and_then(|bytes| bytes.checked_add(16)),
        convert::Type::Option(element) | convert::Type::Sequence { element, .. } => {
            type_dynamic_bytes(element)?.checked_add(mem::size_of::<convert::Type>())
        }
        convert::Type::Result(ok, err) => type_dynamic_bytes(ok)?
            .checked_add(type_dynamic_bytes(err)?)?
            .checked_add(mem::size_of::<convert::Type>().checked_mul(2)?),
        convert::Type::Tuple(members) => type_slice_bytes(members),
        convert::Type::Unit | convert::Type::Builtin(_) | convert::Type::Parameter(_) => Some(0),
    }
}
