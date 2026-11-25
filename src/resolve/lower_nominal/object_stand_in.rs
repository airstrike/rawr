//! A program's value where a crate's generic item takes a type parameter
//! bounded by an object-safe crate trait, as iced's `Element::new(widget:
//! impl Widget<..>)` takes a program's widget (#2698). Rustc instantiates
//! the item at the value's type. The runner can't hold an instance per
//! program type with no rebuild, so the parameter is the runner's one
//! stand-in for the trait (`object::Pointer::StandIn`), which owns the
//! value and calls the member of its impl each method names, as the
//! trait's object does (`crossing::Object`). A parameter only a shared
//! borrow takes, as `&dyn Trait` read as `&impl Trait` is, is the trait's
//! object itself, lent for the call.
//!
//! Which impl members the value calls is known only once the body's types
//! are: each argument passes through an op that [`Parser::finish_stand_ins`]
//! fills, and the call's parameter and operand are rewritten there.

use super::op::{IntrinsicOperation, Op};
use super::ty::{Auto, Ty};
use super::{counted, invariant, ParsedExpr, Parser};
use crate::func::FnCallArgs;
use crate::host::type_ref::Shape;
use crate::resolve::Error;
use crate::{Dynamic, ImmutableString, RawrResult, Span};
use std::collections::BTreeMap;

/// An argument of a crate call that may fill its parameter as the
/// trait's stand-in, decided once the body's types are known.
pub(super) struct Pending {
    /// The op the argument passes through.
    op: usize,
    /// The crate call's op.
    call: usize,
    /// The argument's index among the call's operands.
    operand: usize,
    /// The parameter it fills, and the type the call infers for it.
    parameter: (ImmutableString, Ty),
    /// The trait the parameter is bounded by, at its arguments.
    bound: (ImmutableString, Box<[Ty]>),
    /// Whether the call takes the value itself, not a shared borrow of it.
    owned: bool,
    span: Span,
}

/// What the op of a [`Pending`] argument does: passes the value on as it
/// is, or as the trait's object, calling each method's member of its impl
/// by the method's name.
#[derive(Clone, Debug, Default, Hash, serde::Deserialize, serde::Serialize)]
pub(super) struct Coercion {
    members: Option<Box<[(ImmutableString, ImmutableString)]>>,
}

impl Coercion {
    /// The private function's name, one per set of members.
    pub(super) fn name(&self) -> String {
        format!(
            "\0rawr.object.stand-in.{:016x}",
            crate::program::schema::symbol_id(self)
        )
    }

    /// The value `arguments[0]`, as the trait's object when the members
    /// are known.
    pub(super) fn invoke(&self, arguments: &mut FnCallArgs) -> RawrResult {
        let value = arguments
            .first_mut()
            .ok_or_else(|| invariant("a stand-in's argument lost its operand"))?
            .take();
        let Some(members) = &self.members else {
            return Ok(value);
        };
        let value = counted::value(&value).unwrap_or(value);
        Ok(Dynamic::from(crate::object::Program::coerced(
            value,
            members.iter().cloned().collect(),
        )))
    }
}

impl Parser<'_> {
    /// Each of `arguments` the call of `item` may pass as its trait's
    /// stand-in, passed through an op of its own: one whose parameter only
    /// it fills, by value or shared borrow, which no output names, and
    /// whose one bound is a crate trait the runner has a stand-in for. A
    /// value whose type is already known not to be the program's is left
    /// as it is. Each is that argument's index and its [`Pending`], whose
    /// call is still to be pushed.
    pub(super) fn stand_in_arguments(
        &mut self,
        item: &crate::host::item::Definition,
        requirements: &[(ImmutableString, Ty, crate::capability::Set)],
        arguments: &mut [ParsedExpr],
        call: Span,
    ) -> Result<Vec<(usize, Pending)>, Error> {
        let signature = item.signature();
        let relations = signature.relations();
        if item.imported_identity().is_none()
            || relations
                .iter()
                .all(|relation| relation.trait_identity().is_none())
        {
            return Ok(Vec::new());
        }
        let item_path = item.path();
        let owner = item_path
            .rsplit_once("::")
            .and_then(|(path, _)| self.models.host.type_owner(path))
            .or_else(|| self.models.host.item_owner(&item_path))
            .cloned();
        let mut parameters = requirements
            .iter()
            .map(|(name, ty, _)| (name.clone(), ty.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut pending = Vec::new();
        for (position, declared) in signature.parameters().iter().enumerate() {
            let (name, owned) = match declared.shape() {
                Shape::Parameter(name) => (name.to_string(), true),
                Shape::Reference {
                    mutable: false,
                    target,
                } => match target.shape() {
                    Shape::Parameter(name) => (name.to_string(), false),
                    _ => continue,
                },
                _ => continue,
            };
            let mut bounds = relations
                .iter()
                .filter(|relation| relation.parameter() == name);
            let (Some(relation), None) = (bounds.next(), bounds.next()) else {
                continue;
            };
            let Some(identity) = relation.trait_identity() else {
                continue;
            };
            let Some(trait_name) = self.models.host.trait_name(&identity) else {
                continue;
            };
            let object = crate::host::path::Path::parse(&trait_name)
                .ok()
                .and_then(|path| self.models.host.trait_header(&path))
                .is_some_and(|header| header.object);
            let mentions = signature
                .parameters()
                .iter()
                .chain([signature.output()])
                .filter(|ty| mentions(ty, &name))
                .count();
            if !object || mentions != 1 || !self.models.native_trait(&trait_name) {
                continue;
            }
            let Some(argument) = arguments.get_mut(position) else {
                continue;
            };
            let filled = self.inference.resolve(&argument.ty);
            if !matches!(
                super::without_references(&filled),
                Ty::Script(..) | Ty::Infer(_)
            ) {
                continue;
            }
            let Some(ty) = parameters.get(name.as_str()).cloned() else {
                continue;
            };
            let mut trait_arguments = Vec::with_capacity(relation.arguments().len());
            for written in relation.arguments() {
                trait_arguments.push(self.pattern_type(
                    written,
                    &mut BTreeMap::new(),
                    &mut parameters,
                    (&item_path, call),
                    owner.as_ref(),
                )?);
            }
            let op = self.ops.len();
            let helper = self.push_op(Op::StandIn(Coercion::default()));
            if self.normalize_private_calls {
                let operand = self.materialize_in_place(argument, &format!("{helper}_arg_0"));
                argument.source = self.op_call(&helper, [operand]);
            } else {
                let source = std::mem::take(&mut argument.source);
                argument.source = self.op_call(&helper, [source]);
            }
            argument.source.cover(argument.span);
            pending.push((
                position,
                Pending {
                    op,
                    call: 0,
                    operand: 0,
                    parameter: (name.into(), ty),
                    bound: (trait_name, trait_arguments.into_boxed_slice()),
                    owned,
                    span: argument.span,
                },
            ));
        }
        Ok(pending)
    }

    /// Keep each of `pending`, the arguments [`Parser::stand_in_arguments`]
    /// passed through their ops, of the call pushed as the op `call`,
    /// whose operands start at `first` with the argument at index `0`.
    pub(super) fn keep_stand_ins(
        &mut self,
        pending: Vec<(usize, Pending)>,
        call: usize,
        first: usize,
    ) {
        for (position, mut kept) in pending {
            kept.call = call;
            kept.operand = first + position;
            self.pending_stand_ins.push(kept);
        }
    }

    /// Fill each pending argument's op, now that the body's types are
    /// known: a value of the program's own type with an impl of the trait
    /// becomes the trait's object, and its call's parameter the trait's
    /// stand-in, or the object itself where a shared borrow takes it. Any
    /// other value passes as it is.
    pub(super) fn finish_stand_ins(&mut self) -> Result<(), Error> {
        for pending in std::mem::take(&mut self.pending_stand_ins) {
            let Pending {
                op,
                call,
                operand,
                parameter: (parameter, ty),
                bound: (trait_name, arguments),
                owned,
                span,
            } = pending;
            let ty = self.inference.resolve(&ty);
            // A native island's type is the stand-in rawr's copy of its
            // crate declares for it (#1264).
            if !matches!(ty, Ty::Script(..)) || self.native_object_owner(&ty, &trait_name).is_some()
            {
                continue;
            }
            let arguments = arguments
                .iter()
                .map(|argument| self.inference.resolve(argument))
                .collect::<Vec<_>>();
            let Some(members) = self.native_object_members(&ty, (&trait_name, &arguments), span)?
            else {
                continue;
            };
            let object = Ty::Dyn(
                trait_name,
                arguments
                    .into_iter()
                    .enumerate()
                    .map(|(index, argument)| (index.to_string().into(), argument))
                    .collect(),
                Auto::Unknown,
            );
            let Some(Op::Intrinsic { operation }) = self.ops.get_mut(call) else {
                return Err(Error::LoweringUnavailable {
                    item: "<a stand-in's call>".into(),
                    body: span,
                });
            };
            let IntrinsicOperation {
                requirements,
                operands,
                stand_ins,
                ..
            } = &mut **operation;
            if let Some((_, required, _)) = requirements
                .iter_mut()
                .find(|(name, ..)| *name == parameter)
            {
                *required = object.clone();
            }
            if let Some((given, ..)) = operands.get_mut(operand) {
                *given = if owned {
                    Ty::Boxed(Box::new(object))
                } else {
                    Ty::Reference {
                        mutable: false,
                        target: Box::new(object),
                    }
                };
            }
            if owned {
                let mut named = std::mem::take(stand_ins).into_vec();
                named.push(parameter);
                *stand_ins = named.into_boxed_slice();
            }
            self.ops[op] = Op::StandIn(Coercion {
                members: Some(members.into_boxed_slice()),
            });
        }
        Ok(())
    }
}

/// `surface`, the crate type a call's parameter `name` is, as the trait's
/// stand-in where `stand_ins` names the parameter.
pub(super) fn standing_in(
    surface: rawr_import::classify::representability::convert::Type,
    name: &str,
    stand_ins: &[ImmutableString],
) -> rawr_import::classify::representability::convert::Type {
    use rawr_import::classify::representability::{convert, object};
    match surface {
        convert::Type::Object(made) if stand_ins.iter().any(|stand_in| stand_in == name) => {
            convert::Type::Object(Box::new(made.behind(object::Pointer::StandIn)))
        }
        surface => surface,
    }
}

/// Whether `ty` names the type parameter `name`, at any depth.
fn mentions(ty: &crate::host::type_ref::Type, name: &str) -> bool {
    let any = |types: &[crate::host::type_ref::Type]| types.iter().any(|ty| mentions(ty, name));
    match ty.shape() {
        Shape::Parameter(parameter) => parameter.to_string() == name,
        Shape::Unit | Shape::Builtin(_) => false,
        #[cfg(feature = "rust-import")]
        Shape::Constant(_) => false,
        Shape::Tuple(members) | Shape::Keyed(_, members) => any(members),
        Shape::Registered { arguments, .. } | Shape::External { arguments, .. } => any(arguments),
        #[cfg(feature = "rust-import")]
        Shape::Imported { arguments, .. } | Shape::Object { arguments, .. } => any(arguments),
        Shape::Array { element, .. }
        | Shape::Option(element)
        | Shape::Iter(element)
        | Shape::Range(element)
        | Shape::Boxed(element)
        | Shape::Counted(_, element)
        | Shape::JoinHandle(element)
        | Shape::RangeInclusive(element)
        | Shape::Reference {
            target: element, ..
        } => mentions(element, name),
        Shape::Result(ok, err) => mentions(ok, name) || mentions(err, name),
        Shape::Callable { input, output } => mentions(input, name) || mentions(output, name),
        #[cfg(feature = "rust-import")]
        Shape::ImportedFunctionItem { input, output } => {
            mentions(input, name) || mentions(output, name)
        }
    }
}
