//! Generic bodies lowered per instance (D1115): a program's generic
//! function whose body uses its parameters' types is a template, lowered
//! once for each list of types its callers give those parameters, as rustc
//! monomorphises it. Each instance reads the template's body at those
//! types, so its arithmetic, formatting, parsing and impl choices are the
//! concrete code's. The template's own body is still read once as the
//! generic body rustc checks, and stays the erased body a call whose types
//! hold a parameter of its caller runs.

use super::imported::named_type;
use super::models::Models;
use super::naming::{function_name, method_key_name, INSTANCE_SENTINEL, METHOD_INSTANCE_SENTINEL};
use super::ty::{contains, Float, Int, Ty};
use super::Parser;
use crate::capability::Kind;
use crate::resolve::{item, type_ref};
use crate::tokenizer::Token;
use crate::{ImmutableString, Position, Span};
use std::collections::BTreeMap;

/// The traits a bound names that tell a body nothing of its parameter's
/// type: markers, the callable and iterator traits whose values rawr runs
/// by what they are at run time (D399, D1114), and the borrowing
/// conversions, whose view the bound's target types.
const UNDIRECTED: &[&str] = &[
    "AsRef",
    "AsMut",
    "Borrow",
    "BorrowMut",
    "Sized",
    "Send",
    "Sync",
    "Unpin",
    "Copy",
    "Fn",
    "FnMut",
    "FnOnce",
    "Iterator",
    "IntoIterator",
    "DoubleEndedIterator",
    "ExactSizeIterator",
];

/// Whether a body reads values of `generic`'s type by what the type is: a
/// type parameter of the program's bounded by a trait whose methods,
/// operators or impls differ by type. A const parameter, a callable one, a
/// crate's (D629) and a `where` bound's lifted type are not.
pub(super) fn directs(generic: &item::Generic) -> bool {
    if generic.constant.is_some()
        || generic.host
        || generic.fixed.is_some()
        || !generic.crate_bounds.is_empty()
        || generic
            .traits
            .iter()
            .any(|name| matches!(name.text.as_str(), "Fn" | "FnMut" | "FnOnce"))
    {
        return false;
    }
    generic.bounds.iter().any(|kind| {
        matches!(
            kind,
            Kind::Default
                | Kind::Debug
                | Kind::Clone
                | Kind::PartialEq
                | Kind::PartialOrd
                | Kind::Ord
                | Kind::Hash
        )
    }) || generic.traits.iter().any(|name| {
        let last = name.text.rsplit("::").next().unwrap_or_default();
        !UNDIRECTED.contains(&last)
    })
}

/// The template `function` at `types`, the types of its `parameters`: its
/// signature names them where it names the parameters, which it no longer
/// declares, as D629's instances are written.
pub(super) fn definition(
    function: &item::Function,
    parameters: &[ImmutableString],
    types: &[type_ref::Type],
) -> item::Function {
    let substituted = parameters
        .iter()
        .cloned()
        .zip(types.iter().cloned())
        .collect::<Box<[_]>>();
    let substitute = |ty: &type_ref::Type| type_ref::substitute_parameters(ty, &substituted);
    let mut instance = function.clone();
    instance.generics = function
        .generics
        .iter()
        .filter(|generic| !parameters.contains(&generic.name.text))
        .cloned()
        .map(|mut generic| {
            generic.trait_arguments = generic
                .trait_arguments
                .iter()
                .map(|(name, arguments)| (name.clone(), arguments.iter().map(substitute).collect()))
                .collect();
            generic.projections = generic
                .projections
                .iter()
                .map(|(bound, name, ty)| (bound.clone(), name.clone(), substitute(ty)))
                .collect();
            generic.fixed = generic.fixed.as_ref().map(substitute);
            generic
        })
        .collect();
    // The signature says what each parameter borrows as the template
    // writes it, and elision reads the template's output, as rustc reads
    // the generic signature (`Lifetimes::written_parameters`).
    if instance.lifetimes.written_parameters.is_none() {
        instance.lifetimes.written_parameters = Some(
            function
                .parameters
                .iter()
                .map(|parameter| parameter.ty.clone())
                .collect(),
        );
    }
    if instance.lifetimes.written_output.is_none() {
        instance.lifetimes.written_output = function.output.clone().map(Box::new);
    }
    for parameter in instance.parameters.iter_mut() {
        parameter.ty = substitute(&parameter.ty);
    }
    instance.output = instance.output.as_ref().map(substitute);
    instance.lifetimes.bounds = instance
        .lifetimes
        .bounds
        .iter()
        .filter(|(bounded, _)| !parameters.contains(bounded))
        .cloned()
        .collect();
    instance
}

/// The parameters of the method `definition`, its impl's and its own,
/// that its instances give types, when it is a template: each of its type
/// parameters, when one is bounded by a trait that directs its body and
/// none is a const, callable or crate parameter, which an instance's
/// signature can't name (`method::Definition::instance`). A trait impl's
/// method is reached through a trait object by its instance too, which the
/// object's key selects (D1115.3). A blanket impl's function without
/// `self` is one too: no value's runtime type can say which type it is
/// for, so only the call's types do.
pub(super) fn method_template_parameters(
    definition: &crate::method::Definition,
) -> Option<Box<[ImmutableString]>> {
    let generics = definition.generics();
    let blanket_function = definition.receiver().is_none()
        && definition.owner().builtin_code() == Some(crate::resolve::BLANKET_OWNER);
    if definition.builtin_owner().is_some() && !blanket_function
        || !generics.iter().any(directs)
        || generics.iter().any(|generic| {
            generic.constant.is_some()
                || generic.host
                || generic.fixed.is_some()
                || !generic.crate_bounds.is_empty()
                || generic
                    .traits
                    .iter()
                    .any(|name| matches!(name.text.as_str(), "Fn" | "FnMut" | "FnOnce"))
        })
    {
        return None;
    }
    Some(
        generics
            .iter()
            .map(|generic| generic.name.text.clone())
            .collect(),
    )
}

/// The parameters of `function` its instances give types, in the order it
/// declares them, when it is a template.
pub(super) fn template_parameters(function: &item::Function) -> Option<Box<[ImmutableString]>> {
    let mut instanced = function.generics.iter().map(directs).collect::<Vec<_>>();
    // A parameter a bound of one of them names is the type that bound's
    // impl gives it: `T` of `C: Conv<T>`.
    loop {
        let named = function
            .generics
            .iter()
            .enumerate()
            .filter(|(index, generic)| !instanced[*index] && generic.constant.is_none())
            .filter(|(_, generic)| {
                function
                    .generics
                    .iter()
                    .zip(&instanced)
                    .filter(|(_, instanced)| **instanced)
                    .any(|(bounded, _)| {
                        bounded
                            .trait_arguments
                            .iter()
                            .flat_map(|(_, arguments)| arguments.iter())
                            .chain(bounded.projections.iter().map(|(_, _, ty)| ty))
                            .any(|ty| super::call::mentions(ty, &generic.name.text))
                    })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if named.is_empty() {
            break;
        }
        for index in named {
            instanced[index] = true;
        }
    }
    let parameters = function
        .generics
        .iter()
        .zip(instanced)
        .filter(|(_, instanced)| *instanced)
        .map(|(generic, _)| generic.name.text.clone())
        .collect::<Box<[_]>>();
    (!parameters.is_empty()).then_some(parameters)
}

/// An instance a body's call binds: its template, and the type each of
/// the template's parameters takes (`Models::templates`).
#[derive(Clone, Debug)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Demand {
    pub(super) template: crate::item::Key,
    pub(super) types: Box<[Ty]>,
    /// The call that binds it.
    pub(super) call: Span,
    /// Whether the template is a method, which an instance names as one.
    pub(super) method: bool,
}

impl Demand {
    /// The instance's key: its template's, named with its types as rustc
    /// names an instance, `twice::<u8>`.
    pub(super) fn key(&self, models: &Models) -> crate::item::Key {
        self.instance_key(models)
    }

    /// Whether the template is a method.
    pub(super) const fn method(&self) -> bool {
        self.method
    }

    /// The instance's generated name, which its calls write.
    pub(super) fn name(&self, models: &Models) -> String {
        let key = self.key(models);
        if self.method() {
            method_key_name(&key)
        } else {
            function_name(&key)
        }
    }

    fn instance_key(&self, models: &Models) -> crate::item::Key {
        let types = self
            .types
            .iter()
            .map(|ty| ty.name(models).to_string())
            .collect::<Vec<_>>()
            .join(", ");
        self.template
            .renamed(format!("{}::<{types}>", self.template.name()).into())
    }
}

/// What reading a body found of instances: those its calls bind, and the
/// templates it calls by their erased bodies. A template's own reading
/// also says whether its erased body runs: one that needs its parameters'
/// types where the generic read has none to give (`Parser::gap`) is only
/// its instances.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Uses {
    pub(super) demands: Vec<Demand>,
    pub(super) erased: Vec<(crate::item::Key, Span)>,
    pub(super) gap: bool,
    /// Each method a template's generic read calls on a value of one of
    /// its parameters' types, through that parameter's bound.
    pub(super) bound: Vec<Bound>,
    /// The dispatcher arms the body's trait objects reach (D1115.3).
    pub(super) objects: Vec<super::trait_object::Arm>,
}

/// A method call the generic read of a template resolves through a bound
/// of a parameter's: `x.len()` of `x: &T` under `T: Tr` is `Tr::len`, which
/// each instance calls, whatever inherent `len` its type has.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Bound {
    /// Where the receiver starts.
    receiver: Position,
    /// Where the method's name is.
    member: Position,
    /// The parameter the receiver's type is.
    parameter: ImmutableString,
    adjustment: Adjustment,
}

/// What a method's receiver is taken as, from the value written, as
/// rustc's autoref and autoderef take it.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
enum Adjustment {
    /// The value itself: a `&T` for `&self`, a `T` for `self`.
    Value,
    /// `&` of a `T` for `&self`.
    Borrow,
    /// `&mut` of a `T` for `&mut self`.
    BorrowMut,
    /// `*` of a `&T` for `self`, or of a `&&T` for `&self`.
    Deref,
}

impl Adjustment {
    /// The text written before the receiver.
    fn text(self) -> &'static str {
        match self {
            Self::Value => "",
            Self::Borrow => "&",
            Self::BorrowMut => "&mut ",
            Self::Deref => "*",
        }
    }
}

/// A call of a template whose instance its types choose once the calling
/// body is read, as rustc's monomorphisation knows them only then.
pub(super) struct Pending {
    placeholder: String,
    template: crate::item::Key,
    types: Box<[Ty]>,
    call: Span,
    method: bool,
}

/// Whether `ty` names one type at run time: no parameter, projection or
/// type inference left open.
pub(super) fn concrete(ty: &Ty) -> bool {
    !contains(ty, &|ty| {
        matches!(
            ty,
            Ty::Param(..)
                | Ty::Projection(..)
                | Ty::Infer(_)
                | Ty::Unknown
                | Ty::Int(Int::Var(_))
                | Ty::Float(Float::Var(_))
        )
    })
}

/// The tokens of `text`, each at `at`.
fn lexed(engine: &crate::Engine, text: &str, at: Position) -> Vec<(Token, Position)> {
    super::lex_rust(engine, text)
        .into_iter()
        .filter(|(token, _)| !matches!(token, Token::EOF))
        .map(|(token, _)| (token, at))
        .collect()
}

/// The placeholder a template's call is written with until its instance
/// is known: as long as a function's generated name, or a method's.
fn placeholder(index: usize, method: bool) -> String {
    if method {
        format!("{METHOD_INSTANCE_SENTINEL}{index:010}")
    } else {
        format!("{INSTANCE_SENTINEL}{index:012}")
    }
}

impl Parser<'_> {
    /// The name a call of `template` is written with, given the types
    /// `substitution` gives its generic parameters: a placeholder its
    /// instance's name is written over once the body is read
    /// (`finish_template_calls`). `None` when it is no template.
    pub(super) fn template_call(
        &mut self,
        template: &crate::item::Key,
        substitution: &BTreeMap<ImmutableString, Ty>,
        call: Span,
        method: bool,
    ) -> Option<ImmutableString> {
        let parameters = self.models.templates.get(template)?;
        let types = parameters
            .iter()
            .map(|name| substitution.get(name).cloned().unwrap_or(Ty::Unknown))
            .collect();
        let placeholder = placeholder(self.pending_template_calls.len(), method);
        self.pending_template_calls.push(Pending {
            placeholder: placeholder.clone(),
            template: template.clone(),
            types,
            call,
            method,
        });
        Some(placeholder.into())
    }

    /// The name a call of the method `definition` is written with, given
    /// the types `substitution` gives its impl's and its own parameters
    /// (`template_call`); its own name when it is no template.
    pub(super) fn method_call_name(
        &mut self,
        definition: &crate::method::Definition,
        substitution: &BTreeMap<ImmutableString, Ty>,
        call: Span,
    ) -> String {
        self.template_call(definition.key(), substitution, call, true)
            .map_or_else(|| super::method_name(definition), |name| name.to_string())
    }

    /// Whether the template call at `call` binds an instance: its types
    /// are each one type, so its body runs at them and never selects an
    /// impl by a value's runtime type.
    pub(super) fn binds_instance(&self, call: Span) -> bool {
        self.pending_template_calls.iter().any(|pending| {
            pending.call == call
                && pending
                    .types
                    .iter()
                    .all(|ty| self.names_instance_type(&self.inference.resolve(ty)))
        })
    }

    /// Whether an instance can be at `ty`: one type its signature names.
    fn names_instance_type(&self, ty: &Ty) -> bool {
        concrete(ty) && named_type(ty, &self.models, Span::NONE).is_some()
    }

    /// The name of the instance of the template method `template` at the
    /// types `substitution` gives its parameters, which `call` binds:
    /// `None` when it is no template, or a type holds a parameter of this
    /// body's, where its erased body runs.
    pub(super) fn instance_name(
        &mut self,
        template: &crate::item::Key,
        substitution: &BTreeMap<ImmutableString, Ty>,
        call: Span,
    ) -> Option<String> {
        let types = self
            .models
            .templates
            .get(template)?
            .iter()
            .map(|name| {
                substitution
                    .get(name)
                    .map_or(Ty::Unknown, |ty| self.inference.resolve(ty))
            })
            .collect::<Box<[_]>>();
        if !types.iter().all(|ty| self.names_instance_type(ty)) {
            return None;
        }
        let demand = Demand {
            template: template.clone(),
            types,
            call,
            method: true,
        };
        let name = demand.name(&self.models);
        self.instance_uses.demands.push(demand);
        Some(name)
    }

    /// Write each template call's instance over its placeholder, now that
    /// the body's types are known: the instance of its types when each is
    /// one type, else the template's erased body.
    pub(super) fn finish_template_calls(&mut self) {
        for pending in std::mem::take(&mut self.pending_template_calls) {
            let types = pending
                .types
                .iter()
                .map(|ty| self.inference.resolve(ty))
                .collect::<Box<[_]>>();
            let named = types.iter().all(|ty| self.names_instance_type(ty));
            let name = if named {
                let demand = Demand {
                    template: pending.template,
                    types,
                    call: pending.call,
                    method: pending.method,
                };
                let name = demand.name(&self.models);
                self.instance_uses.demands.push(demand);
                name
            } else {
                let name = if pending.method {
                    method_key_name(&pending.template)
                } else {
                    function_name(&pending.template)
                };
                self.instance_uses
                    .erased
                    .push((pending.template, pending.call));
                name
            };
            self.output.replace_same_length(&pending.placeholder, &name);
            for closure in &mut self.lifted {
                closure
                    .body
                    .replace_same_length(&pending.placeholder, &name);
            }
        }
    }

    /// Read the body being parsed, `instance`'s, as its template's at
    /// `types`, the types of the template's `parameters`: each is that type
    /// here, as an impl's body reads its trait's arguments.
    pub(super) fn enter_instance(
        &mut self,
        (parameters, generics): (&[item::Parameter], &[item::Generic]),
        instanced: &[ImmutableString],
        types: &[Ty],
    ) {
        let substitution = instanced
            .iter()
            .cloned()
            .zip(types.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        // `x.into()` of a parameter `x: T` under `T: Into<U>` converts into
        // `U`, as the generic read takes it.
        for parameter in parameters {
            let type_ref::Type::Path(type_ref::Path {
                name,
                target: type_ref::Target::Parameter,
                arguments,
                ..
            }) = &parameter.ty
            else {
                continue;
            };
            let Some(generic) = generics
                .iter()
                .find(|generic| generic.name.text == *name && substitution.contains_key(name))
            else {
                continue;
            };
            if !arguments.is_empty() {
                continue;
            }
            if let Some((_, [target])) = generic
                .trait_arguments
                .iter()
                .find(|(trait_name, _)| trait_name.as_str() == "Into")
                .map(|(name, arguments)| (name, &arguments[..]))
            {
                let target = super::instantiate(&Ty::from_resolved(target), &substitution);
                self.instance_conversions
                    .insert(parameter.name.span, target);
            }
        }
        // The bounds of its own opaque outputs name the template's
        // parameters, which are the instance's types here.
        for (ty, _, arguments, ..) in &mut self.pending_traits {
            *ty = super::instantiate(ty, &substitution);
            for argument in arguments.iter_mut() {
                *argument = super::instantiate(argument, &substitution);
            }
        }
        for (ty, ..) in &mut self.pending_bounds {
            *ty = super::instantiate(ty, &substitution);
        }
        self.trait_parameters.extend(substitution);
    }

    /// Keep the generic read's call of `function`, a method of the bound
    /// of the parameter `bounded` on the receiver `receiver` of type
    /// `written`, for the template's instances (D1115).
    pub(super) fn keep_bound_method(
        &mut self,
        receiver: Span,
        member: Span,
        written: &Ty,
        bounded: &Ty,
        function: &item::Function,
    ) {
        let (true, Ty::Param(parameter, super::ty::Parameter::Declared)) =
            (self.template_read, bounded)
        else {
            return;
        };
        let mut references = 0;
        let mut ty = written;
        while let Ty::Reference { target, .. } = ty {
            references += 1;
            ty = target;
        }
        let adjustment = match (&function.receiver, references) {
            (Some(crate::item::Receiver::Shared(_)), 0) => Adjustment::Borrow,
            (Some(crate::item::Receiver::Mutable(_)), 0) => Adjustment::BorrowMut,
            (Some(crate::item::Receiver::Shared(_) | crate::item::Receiver::Mutable(_)), 1)
            | (Some(crate::item::Receiver::Owned(_) | crate::item::Receiver::OwnedMutable(_)), 0) => {
                Adjustment::Value
            }
            (Some(crate::item::Receiver::Owned(_) | crate::item::Receiver::OwnedMutable(_)), 1)
            | (Some(crate::item::Receiver::Shared(_) | crate::item::Receiver::Mutable(_)), 2) => {
                Adjustment::Deref
            }
            _ => return,
        };
        self.instance_uses.bound.push(Bound {
            receiver: receiver.start(),
            member: member.start(),
            parameter: parameter.clone(),
            adjustment,
        });
    }

    /// Write each call `bound` keeps as its bound's function, called on the
    /// receiver as the generic read takes it, where the instance's type has
    /// an inherent function of the name its probe would find first:
    /// `x.len()` as `<T as Tr>::len(x)`, which the instance's `T` then
    /// selects the impl of.
    pub(super) fn qualify_bound_methods(
        &mut self,
        engine: &crate::Engine,
        generics: &[item::Generic],
        bound: &[Bound],
        text: &str,
    ) {
        let mut tokens = std::mem::replace(&mut self.input, Vec::new().into_iter().peekable())
            .collect::<Vec<_>>();
        let mut bound = bound.iter().collect::<Vec<_>>();
        bound.sort_by_key(|call| std::cmp::Reverse(call.member));
        for call in bound {
            let Some(generic) = generics
                .iter()
                .find(|generic| generic.name.text == call.parameter)
            else {
                continue;
            };
            let Some(member) = tokens.iter().position(|(_, at)| *at == call.member) else {
                continue;
            };
            let Some(receiver) = tokens.iter().position(|(_, at)| *at == call.receiver) else {
                continue;
            };
            let Token::Identifier(name) = &tokens[member].0 else {
                continue;
            };
            let Some(ty) = self.trait_parameters.get(&call.parameter) else {
                continue;
            };
            let inherent = super::std_method::inherent(ty, name).is_some()
                || super::owner_id(ty).is_some_and(|owner| {
                    self.models
                        .methods
                        .contains_key(&(owner, name.as_str().into()))
                });
            if !inherent {
                continue;
            }
            let Some(declaring) = self.declaring_bound(generic, name, text) else {
                continue;
            };
            if member == 0
                || receiver >= member - 1
                || !matches!(tokens[member - 1].0, Token::Period)
            {
                continue;
            }
            // The arguments, to the `)` that closes them.
            let (arguments, end) = match tokens.get(member + 1) {
                Some((Token::Unit, _)) => (Vec::new(), member + 2),
                Some((Token::LeftParen, _)) => {
                    let mut depth = 0usize;
                    let mut close = None;
                    for (index, (token, _)) in tokens.iter().enumerate().skip(member + 1) {
                        match token {
                            Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
                            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                                depth -= 1;
                                if depth == 0 {
                                    close = Some(index);
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    let Some(close) = close else {
                        continue;
                    };
                    (tokens[member + 2..close].to_vec(), close + 1)
                }
                _ => continue,
            };
            let at = tokens[member].1;
            let mut written = vec![
                (Token::LessThan, at),
                (
                    Token::Identifier(Box::new(call.parameter.as_str().into())),
                    at,
                ),
                (Token::As, at),
            ];
            written.extend(lexed(engine, &declaring, at));
            written.push((Token::GreaterThan, at));
            written.push((Token::DoubleColon, at));
            written.push(tokens[member].clone());
            written.push((Token::LeftParen, at));
            written.extend(lexed(engine, call.adjustment.text(), at));
            written.extend_from_slice(&tokens[receiver..member - 1]);
            if !arguments.is_empty() {
                written.push((Token::Comma, at));
                written.extend(arguments);
            }
            written.push((Token::RightParen, at));
            tokens.splice(receiver..end, written);
        }
        self.input = tokens.into_iter().peekable();
    }

    /// Write each `T::item` of the body being read, `T` one of the
    /// template's `parameters`, as `<T as Trait>::item` of the bound that
    /// declares `item`, as the generic read resolves it through `T`'s
    /// bounds: the instance's type may have an inherent item of that name,
    /// which the generic body never reaches. `text` is the template's
    /// source, where each bound is written.
    pub(super) fn qualify_parameter_paths(
        &mut self,
        engine: &crate::Engine,
        generics: &[item::Generic],
        parameters: &[ImmutableString],
        text: &str,
    ) {
        let tokens = std::mem::replace(&mut self.input, Vec::new().into_iter().peekable())
            .collect::<Vec<_>>();
        let mut written = Vec::with_capacity(tokens.len());
        for (index, (token, at)) in tokens.iter().enumerate() {
            let qualified = match (token, tokens.get(index + 1), tokens.get(index + 2)) {
                (
                    Token::Identifier(name),
                    Some((Token::DoubleColon, _)),
                    Some((Token::Identifier(member), _)),
                ) if parameters
                    .iter()
                    .any(|parameter| parameter.as_str() == name.as_str())
                    && !matches!(written.last(), Some((Token::DoubleColon, _)))
                    && self.reaches_inherent(name, member) =>
                {
                    generics
                        .iter()
                        .find(|generic| generic.name.text.as_str() == name.as_str())
                        .and_then(|generic| self.declaring_bound(generic, member, text))
                }
                _ => None,
            };
            let Some(bound) = qualified else {
                written.push((token.clone(), *at));
                continue;
            };
            written.push((Token::LessThan, *at));
            written.push((self.forwarded_self(token, &bound), *at));
            written.push((Token::As, *at));
            written.extend(lexed(engine, &bound, *at));
            written.push((Token::GreaterThan, *at));
        }
        self.input = written.into_iter().peekable();
    }

    /// The Self type `<T as bound>::item` names for the parameter `token`:
    /// `T` itself, or where `T` is a reference whose impl of the bound's
    /// trait forwards each item to its referent's (`item::ReferenceForward`),
    /// the referent, under a name only the instance reads.
    fn forwarded_self(&mut self, token: &Token, bound: &str) -> Token {
        let Token::Identifier(name) = token else {
            return token.clone();
        };
        let trait_name = bound.split('<').next().unwrap_or(bound);
        let Some(Ty::Reference { mutable, target }) = self.trait_parameters.get(name.as_str())
        else {
            return token.clone();
        };
        let (mutable, target) = (*mutable, (**target).clone());
        let forwards = self
            .models
            .traits
            .get(trait_name)
            .is_some_and(|(_, declared)| {
                declared
                    .reference_forwards
                    .iter()
                    .any(|forward| forward.mutable == mutable)
            });
        if !forwards {
            return token.clone();
        }
        let referent: ImmutableString = format!("__rawr_referent_{name}").into();
        self.trait_parameters.insert(referent.clone(), target);
        Token::Identifier(Box::new(referent.as_str().into()))
    }

    /// Whether `T::member`, `T` the instance's type for `parameter`, would
    /// reach other than its bound's item: an inherent item of that name,
    /// or a builtin type's, which a path reaches only through its trait.
    fn reaches_inherent(&self, parameter: &str, member: &str) -> bool {
        let Some(ty) = self.trait_parameters.get(parameter) else {
            return false;
        };
        match super::owner_id(ty) {
            // `T::Output` is the bound's associated type of the type.
            Some(owner) if matches!(ty, Ty::Script(..)) => {
                self.models.methods.contains_key(&(owner, member.into()))
                    || self.models.traits.values().any(|(_, declared)| {
                        declared
                            .associated
                            .iter()
                            .any(|name| name.text.as_str() == member)
                    })
            }
            _ => true,
        }
    }

    /// The bound of `generic` that declares `member`, as the text of its
    /// trait and arguments: `From<u8>` of `T: From<u8>` for `from`.
    fn declaring_bound(&self, generic: &item::Generic, member: &str, text: &str) -> Option<String> {
        let declares = |trait_name: &str| {
            if let Some((_, declared)) = self.models.traits.get(trait_name) {
                return declared
                    .functions
                    .iter()
                    .any(|function| function.name.text.as_str() == member)
                    || declared
                        .associated
                        .iter()
                        .any(|name| name.text.as_str() == member);
            }
            crate::resolve::std_trait::anywhere(trait_name).is_some_and(|definition| {
                crate::std_table::item(&format!("{definition}::{member}")).is_some()
            })
        };
        let capabilities = generic.bounds.iter().map(|kind| format!("{kind:?}"));
        let traits = generic.traits.iter().map(|name| name.text.to_string());
        let trait_name = traits.chain(capabilities).find(|name| declares(name))?;
        let arguments = generic
            .trait_arguments
            .iter()
            .find(|(name, _)| name.as_str() == trait_name)
            .map(|(_, arguments)| {
                arguments
                    .iter()
                    .map(|argument| super::source_span(text, argument.span()))
                    .collect::<Vec<_>>()
                    .join(", ")
            });
        Some(match arguments {
            Some(arguments) => format!("{trait_name}<{arguments}>"),
            None => trait_name,
        })
    }

    /// The generic read of a template meets what only its instances can
    /// lower: its erased body then never runs, and `error` is what a call
    /// that needs it reports.
    pub(super) fn gap(
        &mut self,
        error: crate::resolve::Error,
    ) -> Result<(), crate::resolve::Error> {
        if self.template_read {
            self.instance_uses.gap = true;
            Ok(())
        } else {
            Err(error)
        }
    }
}
