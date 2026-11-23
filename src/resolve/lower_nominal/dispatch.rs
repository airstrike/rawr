//! Function and method writers: the generated dispatchers of trait, instance,
//! `Into` and static calls, and the text of each lowered function and method.

use super::models::{Models, Nominal};
use super::naming::PRIVATE_CLOSURE;
use super::op::{
    builtin_code, owner_id, owner_key, witness_code, witness_key_statement, ExecutableOp,
    PreludeOp, COUNTED_KEY,
};
use super::ty::{Float, Int, Ty};
use super::{
    closure, default_value, drops, instance_dispatcher_name, instance_text, item_generics,
    kind_code, location, mentions_parameter, method_name, origin, retain_locations,
    static_dispatcher_name, static_instance_dispatcher_name, std_impl, trait_dispatcher_name,
    trait_object, witness_generics, witness_variable, Lowered, ParsedExpr, Parser,
    CLOSURE_SENTINEL, DEFAULT_DISPATCHER, FALLBACK_INSTANCE, SENTINEL, UNKNOWN_WITNESS,
};
use crate::resolve::{item, type_ref, Error, Program};
use crate::type_def;
use crate::{capability, method, ImmutableString, Span};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

/// The names one part gives its lifted closures' generated functions: its
/// numbering (`Parser::numbering`) and how many it named before, so a
/// part's text names none of another part's closures and a kept part's
/// text stays as it was written (#1877).
pub(super) struct Naming<'a> {
    numbering: &'a str,
    named: usize,
}

impl<'a> Naming<'a> {
    /// The names of the part numbered `numbering`, none given yet.
    pub(super) const fn new(numbering: &'a str) -> Self {
        Self {
            numbering,
            named: 0,
        }
    }

    fn next(&mut self) -> String {
        let name = format!("{CLOSURE_SENTINEL}{}{}", self.numbering, self.named);
        self.named += 1;
        name
    }
}

/// Write each lifted closure as its own generated function, named by
/// `naming`.
pub(super) fn write_closures(
    output: &mut location::Generated,
    retained: &mut Vec<location::Pending>,
    source: crate::program::source::Id,
    naming: &mut Naming<'_>,
    lifted: Vec<closure::Lifted>,
    closures: &mut Vec<closure::Callable>,
    rewrites: &mut BTreeMap<ImmutableString, ImmutableString>,
) {
    for closure in lifted {
        let Some(value) = closure.value else {
            continue;
        };
        let generated = naming.next();
        let dispatch: ImmutableString = format!("{PRIVATE_CLOSURE}{}", closure.key).into();
        output.declaration(
            source,
            closure.span,
            &format!("fn {generated}({}) {{\n", closure.parameters.join(", ")),
        );
        let (body, locations) = closure.body.finish();
        let offset = output.body(source, closure.span, &body, &locations);
        // A runtime error in the closure's body, as in an async body the
        // host runs, is placed at the statement that failed.
        retain_locations(retained, offset, Some(&closure.key), source, locations);
        output.declaration(source, closure.span, "\n}\n");
        rewrites.insert(generated.into(), dispatch.clone());
        closures.push(closure::Callable {
            arity: closure.parameters.len(),
            value,
            dispatch,
            source,
            frames: closure.frames,
        });
    }
}

/// One function per trait method with a receiver, running the impl of the
/// receiver's runtime type: how a call on a value of a generic type bounded
/// by the trait reaches its implementation. A trait object's value keyed by
/// its concrete type runs the impl `arms` give that type (D1115.3).
pub(super) fn write_trait_dispatchers(
    output: &mut location::Generated,
    models: &Models,
    ops: &mut Vec<ExecutableOp>,
    arms: &BTreeSet<trait_object::Arm>,
) {
    // A trait without methods, such as std's `Error`, which every program
    // knows, needs no dispatcher and so no type index.
    // So does a std trait whose functions reach no value (`unreached`).
    if models.traits.iter().all(|(trait_name, (_, declared))| {
        declared
            .functions
            .iter()
            .all(|function| unreached(models, trait_name, declared, function))
    }) {
        return;
    }
    let type_index = format!("{SENTINEL}{}", ops.len());
    ops.push(ExecutableOp::Prelude(PreludeOp::TypeIndex));
    let object_key = format!("{SENTINEL}{}", ops.len());
    ops.push(ExecutableOp::Prelude(PreludeOp::ObjectKey));
    for (trait_name, (source, declared)) in &models.traits {
        // `Drop` is run by the drop glue where Rust drops a value.
        if trait_name == "Drop" && models.std_traits.contains(trait_name) {
            drops::write_glue(
                output,
                models,
                ops,
                *source,
                declared.name.span,
                &type_index,
            );
            continue;
        }
        for function in &declared.functions {
            if function.receiver.is_none() {
                if !models.std_traits.contains(trait_name) {
                    write_static_dispatcher(
                        output,
                        models,
                        trait_name,
                        *source,
                        declared,
                        function,
                        &type_index,
                        ops,
                    );
                }
                continue;
            }
            // `Into<T>` is every `From<S> for T` read from `S`.
            if trait_name == "Into" && declared.name.span == Span::NONE {
                write_into_dispatchers(output, models, *source, declared, &type_index, ops);
                continue;
            }
            let parameters = (0..function.parameters.len())
                .map(|index| format!("__rawr_parameter_{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            // A generic trait has one dispatcher per instance.
            if !declared.generics.is_empty() {
                write_instance_dispatchers(
                    output,
                    models,
                    (trait_name, *source, declared),
                    function,
                    &parameters,
                    (&type_index, &object_key, arms),
                    ops,
                );
                continue;
            }
            let mut text = format!(
                "__rawr_private fn {}({parameters}) {{\nlet __rawr_type = {type_index}(self);\n",
                trait_dispatcher_name(trait_name, &function.name.text)
            );
            // A blanket impl (`impl<T: Bound> Trait for T`) runs for a
            // value no other impl is for, as coherence leaves it the only
            // impl that can apply.
            if unreached(models, trait_name, declared, function) {
                continue;
            }
            let mut blanket = None;
            for ((owner, _), definitions) in models
                .trait_methods
                .iter()
                .filter(|((_, name), _)| *name == function.name.text)
            {
                let implemented = definitions
                    .iter()
                    .filter(|definition| definition.trait_name() == Some(trait_name))
                    .collect::<Vec<_>>();
                let owner_arms = arms
                    .iter()
                    .filter(|arm| arm.of(trait_name, &function.name.text, None, &owner_key(*owner)))
                    .collect::<Vec<_>>();
                if !owner_arms.is_empty() {
                    write_keyed(&mut text, *owner, &object_key, &owner_arms, &parameters);
                }
                if self_types(models, implemented.iter().copied()) > 1
                    && owner.builtin_code() != Some(crate::resolve::BLANKET_OWNER)
                {
                    write_untold(&mut text, *owner, trait_name, &function.name.text);
                    continue;
                }
                for definition in implemented {
                    if owner.builtin_code() == Some(crate::resolve::BLANKET_OWNER) {
                        blanket = Some(definition);
                        continue;
                    }
                    let _ = writeln!(
                        text,
                        "if __rawr_type == {} {{ return self.{}({}); }}",
                        owner_key(*owner),
                        method_name(definition),
                        unknown_witnesses(&parameters, definition, models)
                    );
                }
            }
            // A std trait's dispatcher falls back to what Rawr does for a
            // type without a hand-written impl: a derive, or a builtin.
            let fallback = match (models.std_traits.contains(trait_name), trait_name.as_str()) {
                _ if let Some(blanket) = blanket => {
                    format!(
                        "return self.{}({});",
                        method_name(blanket),
                        unknown_witnesses(&parameters, blanket, models)
                    )
                }
                (true, "Clone") => "return self;".to_string(),
                // An `Rc` or `Arc` writes what it points to, as std's
                // `impl<T: Display> Display for Rc<T>` does.
                // A builtin writes itself under the formatter's spec.
                (true, "Display") => {
                    let get = format!("{SENTINEL}{}", ops.len());
                    ops.push(ExecutableOp::Prelude(PreludeOp::CountedGet));
                    let display = format!("{SENTINEL}{}", ops.len());
                    ops.push(ExecutableOp::Prelude(PreludeOp::FormatSpecDisplay));
                    format!(
                        "if __rawr_type == {COUNTED_KEY} {{ return {get}(self).{}({parameters}); }}\n\
                         return {display}(self, __rawr_parameter_0, 0);",
                        trait_dispatcher_name(trait_name, &function.name.text)
                    )
                }
                (true, "Debug") => {
                    let generic = format!("{SENTINEL}{}", ops.len());
                    ops.push(ExecutableOp::Prelude(PreludeOp::GenericDebug));
                    format!("return {generic}(self);")
                }
                (true, "PartialEq") => {
                    let generic = format!("{SENTINEL}{}", ops.len());
                    ops.push(ExecutableOp::Prelude(PreludeOp::GenericEqual));
                    format!("return {generic}(self, __rawr_parameter_0);")
                }
                (true, "PartialOrd" | "Ord") => {
                    let compare = format!("{SENTINEL}{}", ops.len());
                    ops.push(ExecutableOp::Prelude(PreludeOp::Compare));
                    let compared = format!("{compare}(self, __rawr_parameter_0)");
                    if trait_name == "Ord" {
                        let unwrap = format!("{SENTINEL}{}", ops.len());
                        ops.push(ExecutableOp::Prelude(PreludeOp::Unwrap));
                        format!("return {unwrap}({compared});")
                    } else {
                        format!("return {compared};")
                    }
                }
                // A std iterator is a runtime one (`lazy.rs`), which steps
                // itself.
                (true, "Iterator") if function.name.text == "next" => {
                    let lazy = format!("{SENTINEL}{}", ops.len());
                    ops.push(ExecutableOp::Prelude(PreludeOp::Lazy));
                    format!("return self.{lazy}({}, ());", super::lazy::NEXT)
                }
                _ if let Some(borrowed) = native_borrow(declared, function) => borrowed.to_string(),
                _ => format!(
                    "__rawr_throw \"`{trait_name}::{}` has no implementation for this value\";",
                    function.name.text
                ),
            };
            let _ = write!(text, "{fallback}\n}}\n");
            output.declaration(*source, declared.name.span, &text);
        }
    }
}

/// The arms of a dispatcher by runtime type for `owner`'s values keyed as
/// trait objects (`trait_object.rs`), which `object_key` reads the key of.
fn write_keyed(
    text: &mut String,
    owner: type_def::Id,
    object_key: &str,
    arms: &[&trait_object::Arm],
    parameters: &str,
) {
    let _ = writeln!(
        text,
        "if __rawr_type == {} {{\nlet __rawr_key = {object_key}(self);",
        owner_key(owner)
    );
    for arm in arms {
        text.push_str(&arm.text(parameters));
    }
    text.push_str("}\n");
}

/// The arm of a dispatcher by runtime type for `owner`, whose impls of
/// `trait_name` for two of its Self types each give `function`
/// (`Foo<i32>`'s and `Foo<i64>`'s, or `f32`'s and `f64`'s): a value no
/// trait object's coercion keyed holds no type arguments, so the runtime
/// type can't tell which is its impl, and rawr refuses rather than run the
/// wrong one.
fn write_untold(text: &mut String, owner: type_def::Id, trait_name: &str, function: &str) {
    let _ = writeln!(
        text,
        "if __rawr_type == {} {{ __rawr_throw \"`{trait_name}::{function}` of a value whose \
         type has impls of `{trait_name}` for two of its Self types, reached by its runtime \
         type, is not supported yet\"; }}",
        owner_key(owner)
    );
}

/// How many Self types `definitions`' impls are for.
fn self_types<'a>(
    models: &Models,
    definitions: impl Iterator<Item = &'a method::Definition>,
) -> usize {
    definitions
        .map(|definition| super::op::impl_self_ty(definition, &BTreeMap::new()).name(models))
        .collect::<BTreeSet<_>>()
        .len()
}

/// The arguments a dispatcher by runtime type passes the impl method
/// `definition`: `parameters`, then `UNKNOWN_WITNESS` for each witness its
/// generics take, which a value no trait object's coercion keyed doesn't
/// name (`impl<const N: usize> Display for Buf<N>` of a `Buf` a body whose
/// types hold a parameter boxed as `dyn Display`).
fn unknown_witnesses(parameters: &str, definition: &method::Definition, models: &Models) -> String {
    let witnesses = witness_generics(definition.generics(), models).count();
    std::iter::once(parameters)
        .filter(|parameters| !parameters.is_empty())
        .map(str::to_string)
        .chain(std::iter::repeat_n(UNKNOWN_WITNESS.to_string(), witnesses))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether `function` of the std trait `trait_name` reaches no value: no
/// impl of the program's has it and no builtin runs it, as `Error::source`
/// of a program that implements no `Error`. It needs no dispatcher.
fn unreached(
    models: &Models,
    trait_name: &str,
    declared: &item::Trait,
    function: &item::Function,
) -> bool {
    let builtin = matches!(
        trait_name,
        "Clone" | "Display" | "Debug" | "PartialEq" | "PartialOrd" | "Ord" | "Drop" | "Into"
    ) || native_borrow(declared, function).is_some();
    models.std_traits.contains(trait_name)
        && function.receiver.is_some()
        && declared.generics.is_empty()
        && !builtin
        && !models.trait_methods.iter().any(|((_, name), definitions)| {
            *name == function.name.text
                && definitions.iter().any(|definition| {
                    definition
                        .trait_name()
                        .is_some_and(|name| name == trait_name)
                })
        })
}

/// What a dispatcher gives for a std trait method that only borrows its
/// receiver (`as_ref`, `as_mut`, `borrow`, `borrow_mut`, `deref_mut`) on a
/// value of rawr's native types. std's impls on them borrow the value as it
/// is at runtime: a `String` is its `str`, a `Box` its value, a `Vec` its
/// slice. A shared borrow is the value, and a mutable one is the
/// receiver's place itself, the empty path below it, as a `&mut` a
/// program's method gives is a path below the place it was called on.
pub(super) fn native_borrow(
    declared: &item::Trait,
    function: &item::Function,
) -> Option<&'static str> {
    if declared.name.span != Span::NONE || !function.parameters.is_empty() {
        return None;
    }
    let Some(crate::resolve::type_ref::Type::Reference { mutable, .. }) = &function.output else {
        return None;
    };
    match (&function.receiver, mutable) {
        (Some(crate::item::Receiver::Shared(_)), false) => Some("return self;"),
        (Some(crate::item::Receiver::Mutable(_)), true) => Some("return [];"),
        _ => None,
    }
}

/// The dispatchers of a generic trait's method: one per instance of the
/// trait's arguments among its impls, running the impl of the receiver's
/// runtime type for that instance (or a generic impl, whose arguments can
/// be any), and a fallback by the receiver's type alone.
#[allow(clippy::too_many_arguments)]
pub(super) fn write_instance_dispatchers(
    output: &mut location::Generated,
    models: &Models,
    (trait_name, source, declared): (&ImmutableString, crate::program::source::Id, &item::Trait),
    function: &item::Function,
    parameters: &str,
    (type_index, object_key, arms): (&str, &str, &BTreeSet<trait_object::Arm>),
    ops: &mut Vec<ExecutableOp>,
) {
    let instances = models
        .trait_instances
        .get(trait_name)
        .cloned()
        .unwrap_or_default();
    let definitions = models
        .trait_methods
        .iter()
        .filter(|((_, name), _)| *name == function.name.text)
        .flat_map(|((owner, _), definitions)| {
            definitions
                .iter()
                .filter(|definition| definition.trait_name() == Some(trait_name))
                .map(move |definition| (*owner, definition))
        })
        .collect::<Vec<_>>();
    let instance_texts = instances
        .iter()
        .map(|instance| Some((instance, instance_text(instance))))
        .chain(std::iter::once(None));
    for (index, written) in instance_texts.enumerate() {
        let (arguments, instance) = written.unzip();
        let index = if instance.is_some() {
            index
        } else {
            FALLBACK_INSTANCE
        };
        let mut text = format!(
            "__rawr_private fn {}({parameters}) {{\nlet __rawr_type = {type_index}(self);\n",
            instance_dispatcher_name(trait_name, &function.name.text, index)
        );
        let mut blanket = None;
        let mut keyed = BTreeSet::new();
        let mut told = BTreeMap::<type_def::Id, Vec<&method::Definition>>::new();
        for (owner, definition) in &definitions {
            let arguments = definition.trait_arguments();
            let matches = match &instance {
                Some(instance) => {
                    arguments.iter().any(mentions_parameter)
                        || instance_text(arguments) == *instance
                }
                None => true,
            };
            if matches {
                told.entry(*owner).or_default().push(definition);
            }
        }
        let untold = told
            .into_iter()
            .filter(|(owner, implemented)| {
                owner.builtin_code() != Some(crate::resolve::BLANKET_OWNER)
                    && self_types(models, implemented.iter().copied()) > 1
            })
            .map(|(owner, _)| owner)
            .collect::<BTreeSet<_>>();
        for (owner, _) in &definitions {
            let owner_arms = arms
                .iter()
                .filter(|arm| {
                    arm.of(
                        trait_name,
                        &function.name.text,
                        instance.as_deref(),
                        &owner_key(*owner),
                    )
                })
                .collect::<Vec<_>>();
            if instance.is_none() || owner_arms.is_empty() || !keyed.insert(*owner) {
                continue;
            }
            write_keyed(&mut text, *owner, object_key, &owner_arms, parameters);
        }
        for owner in &untold {
            write_untold(&mut text, *owner, trait_name, &function.name.text);
        }
        for (owner, definition) in &definitions {
            if untold.contains(owner) {
                continue;
            }
            let arguments = definition.trait_arguments();
            let matches = match &instance {
                Some(instance) => {
                    arguments.iter().any(mentions_parameter)
                        || instance_text(arguments) == *instance
                }
                None => true,
            };
            if !matches {
                continue;
            }
            // A blanket impl runs for a value no other impl is for.
            if owner.builtin_code() == Some(crate::resolve::BLANKET_OWNER) {
                blanket = Some(definition);
                continue;
            }
            let _ = writeln!(
                text,
                "if __rawr_type == {} {{ return self.{}({parameters}); }}",
                owner_key(*owner),
                method_name(definition)
            );
        }
        if let Some(blanket) = blanket {
            let _ = writeln!(text, "return self.{}({parameters});", method_name(blanket));
        }
        // Rust's own operator impls on builtins: the runtime's operator.
        if let Some((symbol, ..)) = models
            .std_traits
            .contains(trait_name)
            .then(|| {
                crate::resolve::OPERATOR_TRAITS
                    .iter()
                    .find(|(_, operator, _)| operator == trait_name)
            })
            .flatten()
        {
            let _ = match *symbol {
                "neg" => writeln!(text, "return -self;"),
                "!" => writeln!(text, "return !self;"),
                assign if assign.ends_with('=') => {
                    writeln!(text, "self {assign} __rawr_parameter_0; return;")
                }
                binary => writeln!(text, "return self {binary} __rawr_parameter_0;"),
            };
        }
        if let Some(borrowed) = native_borrow(declared, function) {
            // The view the instance names, where a native value's runtime
            // form isn't it: a text's `[u8]` is its bytes.
            if let (Some(crate::item::Receiver::Shared(_)), Some([argument])) = (
                &function.receiver,
                arguments.map(|arguments| &arguments[..]),
            ) {
                for (key, op) in native_views(&Ty::from_resolved(argument)) {
                    let helper = format!("{SENTINEL}{}", ops.len());
                    ops.push(ExecutableOp::Prelude(op));
                    let _ = writeln!(text, "if __rawr_type == {key} {{ return {helper}(self); }}");
                }
            }
            let _ = writeln!(text, "{borrowed}");
        }
        let _ = write!(
            text,
            "__rawr_throw \"`{trait_name}::{}` has no implementation for this value\";\n}}\n",
            function.name.text
        );
        output.declaration(source, declared.name.span, &text);
    }
}

/// The runtime type key of each native type whose view `borrowed` a shared
/// borrow lends through an op rather than as the value is, with the op: a
/// text's `[u8]` is `str.bytes` (`lending::runs_lending`). Of the native
/// types a runtime key tells apart, only a text has such a view; a `Vec`'s
/// and an array's views are the array they are.
fn native_views(borrowed: &Ty) -> Vec<(crate::INT, PreludeOp)> {
    [Ty::String]
        .iter()
        .filter_map(|source| {
            let op = super::lending::runs_lending(source, borrowed)??;
            Some((builtin_code(source)?, op))
        })
        .collect()
}

/// Whether Rust implements the operator trait `trait_name` for the builtin
/// `ty`: the arithmetic ones and `Neg` for numbers, `Not` for `bool` and
/// integers, `Add`/`AddAssign` (of `&str`) for `String`.
pub(super) fn builtin_operator(ty: &Ty, trait_name: &str) -> bool {
    let Some((symbol, ..)) = crate::resolve::OPERATOR_TRAITS
        .iter()
        .find(|(_, operator, _)| *operator == trait_name)
    else {
        return false;
    };
    let bits = matches!(*symbol, "&" | "|" | "^" | "&=" | "|=" | "^=");
    let shift = matches!(*symbol, "<<" | ">>" | "<<=" | ">>=");
    match ty {
        Ty::Int(kind) => *symbol != "neg" || kind.signed(),
        Ty::Float(_) => !matches!(*symbol, "!") && !bits && !shift,
        Ty::Bool => *symbol == "!" || bits,
        Ty::String => matches!(*symbol, "+" | "+="),
        _ => false,
    }
}

/// The dispatchers of the prelude's `Into<T>::into`: one per conversion
/// target `T`, running the `From<S> for T` of the receiver's runtime type
/// `S`, else the value itself (`impl<T> From<T> for T`); the fallback is
/// the value itself.
pub(super) fn write_into_dispatchers(
    output: &mut location::Generated,
    models: &Models,
    source: crate::program::source::Id,
    declared: &item::Trait,
    type_index: &str,
    ops: &mut Vec<ExecutableOp>,
) {
    let targets = models
        .trait_instances
        .get("Into")
        .cloned()
        .unwrap_or_default();
    for (index, target) in targets.iter().enumerate() {
        let [target] = &target[..] else {
            continue;
        };
        let target = Ty::from_resolved(target);
        let mut text = format!(
            "__rawr_private fn {}() {{\nlet __rawr_type = {type_index}(self);\n",
            instance_dispatcher_name("Into", "into", index)
        );
        for conversion in models.conversions_into(&target) {
            let from =
                Ty::from_resolved(conversion.conversion().expect("conversions have a source"));
            if let Some(id) = owner_id(&from) {
                let _ = writeln!(
                    text,
                    "if __rawr_type == {} {{ return {}(self); }}",
                    owner_key(id),
                    method_name(conversion)
                );
            }
        }
        // A text's `From` into a sequence, `Vec<u8>`'s of its bytes, which
        // the bound names; a text's others are `text_into`'s.
        let text_bytes = matches!(target, Ty::Array { .. })
            .then(|| std_impl::runs_from(&target, &Ty::Str).flatten())
            .flatten()
            .and_then(|op| Some((builtin_code(&Ty::Str)?, op)));
        for (from, op) in builtin_conversions_into(&target)
            .into_iter()
            .chain(text_bytes)
        {
            let helper = format!("{SENTINEL}{}", ops.len());
            ops.push(ExecutableOp::Prelude(op));
            let _ = writeln!(
                text,
                "if __rawr_type == {from} {{ return {helper}(self); }}"
            );
        }
        text.push_str("return self;\n}\n");
        output.declaration(source, declared.name.span, &text);
    }
    let text = format!(
        "__rawr_private fn {}() {{\nreturn self;\n}}\n",
        instance_dispatcher_name("Into", "into", FALLBACK_INSTANCE)
    );
    output.declaration(source, declared.name.span, &text);
}

/// Rust's prelude `From` impls among Rawr's builtin types, as
/// `(target, source, op)` witness codes: `From<bool> for i64` and `f64`,
/// `From<u8> for f64` and the other integers a float holds exactly (which
/// of them std has is checked where the bound is), `From<char> for
/// String`, `From<char> for u32` and its wider kin, and `From<u8> for
/// char`. (`From<&str> for String` is the value itself.)
pub(super) const BUILTIN_CONVERSIONS: [(crate::INT, crate::INT, PreludeOp); 6] = [
    (-2, -5, PreludeOp::IntFromBool),
    (-3, -5, PreludeOp::FloatFromBool),
    (-3, -2, PreludeOp::FloatFromInt),
    (-4, -6, PreludeOp::StringFromChar),
    (-2, -6, PreludeOp::IntFromChar),
    (-6, -2, PreludeOp::CharFromByte),
];

/// The builtin conversions from `source`: each target's code and op.
pub(super) fn builtin_conversions_from(source: &Ty) -> Vec<(crate::INT, PreludeOp)> {
    let Some(code) = builtin_code(source) else {
        return Vec::new();
    };
    BUILTIN_CONVERSIONS
        .iter()
        .filter(|(_, from, _)| *from == code)
        .map(|(target, _, op)| (*target, *op))
        .collect()
}

/// The builtin conversions into `target`: each source's code and op.
pub(super) fn builtin_conversions_into(target: &Ty) -> Vec<(crate::INT, PreludeOp)> {
    let Some(code) = builtin_code(target) else {
        return Vec::new();
    };
    BUILTIN_CONVERSIONS
        .iter()
        .filter(|(to, _, _)| *to == code)
        .map(|(_, source, op)| (*source, *op))
        .collect()
}

/// One function per trait function without `self`, running the impl of the
/// type a witness names: how `T::new()` reaches its implementation for a
/// generic `T` bounded by the trait.
pub(super) fn write_static_dispatcher(
    output: &mut location::Generated,
    models: &Models,
    trait_name: &ImmutableString,
    source: crate::program::source::Id,
    declared: &item::Trait,
    function: &item::Function,
    type_index: &str,
    ops: &mut Vec<ExecutableOp>,
) {
    let prelude_from = trait_name == "From" && declared.name.span == Span::NONE;
    let std_from_str = trait_name == "FromStr" && declared.name.span == Span::NONE;
    // The function parameter typed by each trait parameter (`value: T` of
    // `From<T>::from`): the fallback reads that argument's runtime type to
    // choose the impl, for instances inference leaves generic.
    let keyed_parameters = declared
        .generics
        .iter()
        .map(|generic| {
            function.parameters.iter().position(|parameter| {
                matches!(&parameter.ty, type_ref::Type::Path(path)
                    if path.target == type_ref::Target::Parameter
                        && path.name == generic.name.text
                        && path.arguments.is_empty())
            })
        })
        .collect::<Vec<_>>();
    let parameters = (0..function.parameters.len())
        .map(|index| format!("__rawr_parameter_{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let separator = if parameters.is_empty() { "" } else { ", " };
    // A generic trait has one per instance of its arguments among its
    // impls, and a fallback by the witness alone.
    let instances: Vec<Option<&[type_ref::Type]>> = if declared.generics.is_empty() {
        vec![None]
    } else {
        models
            .trait_instances
            .get(trait_name)
            .into_iter()
            .flatten()
            .map(|instance| Some(&instance[..]))
            .chain(std::iter::once(None))
            .collect()
    };
    for (index, instance) in instances.iter().enumerate() {
        let name = if declared.generics.is_empty() {
            static_dispatcher_name(trait_name, &function.name.text)
        } else if instance.is_some() {
            static_instance_dispatcher_name(trait_name, &function.name.text, index)
        } else {
            static_instance_dispatcher_name(trait_name, &function.name.text, FALLBACK_INSTANCE)
        };
        let mut text =
            format!("__rawr_private fn {name}(__rawr_witness{separator}{parameters}) {{\n");
        if std_from_str {
            write_builtin_parses(&mut text, ops);
        }
        // `T::make()` of a `T` that is `u8` runs `u8`'s impl, not `i64`'s:
        // each builtin impl by its own type's witness (`witness_code`),
        // before the arms by runtime type.
        if declared.generics.is_empty() {
            for definition in models
                .trait_methods
                .iter()
                .filter(|((_, name), _)| *name == function.name.text)
                .flat_map(|(_, definitions)| definitions)
                .filter(|definition| definition.trait_name() == Some(trait_name))
            {
                if let Some(code) = definition
                    .builtin_owner()
                    .and_then(|owner| witness_code(&Ty::from_resolved(owner)))
                {
                    let _ = writeln!(
                        text,
                        "if __rawr_witness == {code} {{ return {}({parameters}); }}",
                        method_name(definition)
                    );
                }
            }
        }
        text.push_str(&witness_key_statement());
        let instance_types = *instance;
        let fallback = !declared.generics.is_empty() && instance_types.is_none();
        let instance = instance_types.map(instance_text);
        // The fallback first tries each impl whose arguments the call's
        // arguments' runtime types name, then each impl by witness alone.
        if fallback {
            for ((owner, _), definitions) in models
                .trait_methods
                .iter()
                .filter(|((_, name), _)| *name == function.name.text)
            {
                for definition in definitions
                    .iter()
                    .filter(|definition| definition.trait_name() == Some(trait_name))
                {
                    let conditions = definition
                        .trait_arguments()
                        .iter()
                        .zip(&keyed_parameters)
                        .filter_map(|(argument, parameter)| {
                            let parameter = (*parameter)?;
                            if mentions_parameter(argument) {
                                return None;
                            }
                            let id = owner_id(&Ty::from_resolved(argument))?;
                            Some(format!(
                                " && {type_index}(__rawr_parameter_{parameter}) == {}",
                                owner_key(id)
                            ))
                        })
                        .collect::<String>();
                    if !conditions.is_empty() {
                        let _ = writeln!(
                            text,
                            "if __rawr_witness == {}{conditions} {{ return {}({parameters}); }}",
                            owner_key(*owner),
                            method_name(definition)
                        );
                    }
                }
            }
            if prelude_from {
                for (target, from, op) in BUILTIN_CONVERSIONS {
                    let helper = format!("{SENTINEL}{}", ops.len());
                    ops.push(ExecutableOp::Prelude(op));
                    let _ = writeln!(
                        text,
                        "if __rawr_witness == {target} && {type_index}(__rawr_parameter_0) == \
                         {from} {{ return {helper}(__rawr_parameter_0); }}"
                    );
                }
                let _ = writeln!(
                    text,
                    "if __rawr_witness == {type_index}(__rawr_parameter_0) {{ return \
                     __rawr_parameter_0; }}"
                );
            }
        }
        for ((owner, _), definitions) in models
            .trait_methods
            .iter()
            .filter(|((_, name), _)| *name == function.name.text)
        {
            for definition in definitions
                .iter()
                .filter(|definition| definition.trait_name() == Some(trait_name))
            {
                let arguments = definition.trait_arguments();
                if let Some(instance) = &instance
                    && !arguments.iter().any(mentions_parameter)
                    && instance_text(arguments) != *instance
                {
                    continue;
                }
                let _ = writeln!(
                    text,
                    "if __rawr_witness == {} {{ return {}({parameters}); }}",
                    owner_key(*owner),
                    method_name(definition)
                );
            }
        }
        // The prelude's own impls: `From<bool> for i64`, and `From<T> for
        // T`.
        if prelude_from && let Some([from]) = instance_types {
            let from = Ty::from_resolved(from);
            for (target, op) in builtin_conversions_from(&from) {
                let helper = format!("{SENTINEL}{}", ops.len());
                ops.push(ExecutableOp::Prelude(op));
                let _ = writeln!(
                    text,
                    "if __rawr_witness == {target} {{ return {helper}(__rawr_parameter_0); }}"
                );
            }
            if let Some(id) = owner_id(&from) {
                let _ = writeln!(
                    text,
                    "if __rawr_witness == {} {{ return __rawr_parameter_0; }}",
                    owner_key(id)
                );
            }
        }
        let _ = write!(
            text,
            "__rawr_throw \"`{trait_name}::{}` has no implementation for this type\";\n}}\n",
            function.name.text
        );
        output.declaration(source, declared.name.span, &text);
    }
}

/// The arms of std's `FromStr::from_str` dispatcher for the builtin types
/// std implements it for, each by its kind's witness (`witness_code`): the
/// parse `s.parse::<T>()` runs for that type, `"300"` failing for `u8`.
fn write_builtin_parses(text: &mut String, ops: &mut Vec<ExecutableOp>) {
    let builtins = Int::KINDS.iter().map(|(_, kind)| Ty::Int(*kind)).chain([
        Ty::Float(Float::F32),
        Ty::Float(Float::F64),
        Ty::Bool,
        Ty::Char,
        Ty::String,
    ]);
    for ty in builtins {
        let Some(code) = witness_code(&ty) else {
            continue;
        };
        let kind = kind_code(ty);
        let helper = format!("{SENTINEL}{}", ops.len());
        ops.push(ExecutableOp::Kinded {
            op: PreludeOp::Parse,
            kinds: [kind, kind],
        });
        let _ = writeln!(
            text,
            "if __rawr_witness == {code} {{ return {helper}(__rawr_parameter_0); }}"
        );
    }
}

/// The function from a type witness to that type's default value, which
/// `T::default()` calls for a generic `T: Default`.
pub(super) fn write_default_dispatcher(
    output: &mut location::Generated,
    program: &Program,
    models: &Models,
    ops: &mut Vec<ExecutableOp>,
) {
    let Some(shell) = program.items.iter().find(|shell| {
        item_generics(&shell.kind)
            .into_iter()
            .any(|generic| generic.bounds.contains(capability::Kind::Default))
    }) else {
        return;
    };
    let none = format!("{SENTINEL}{}", ops.len());
    ops.push(ExecutableOp::Prelude(PreludeOp::None));
    let mut text = format!(
        "__rawr_private fn {DEFAULT_DISPATCHER}(__rawr_witness) {{\n\
         {}\
         if __rawr_witness == -2 {{ return 0; }}\n\
         if __rawr_witness == -3 {{ return 0.0; }}\n\
         if __rawr_witness == -4 {{ return \"\"; }}\n\
         if __rawr_witness == -5 {{ return false; }}\n\
         if __rawr_witness == -6 {{ return '\\x00'; }}\n\
         if __rawr_witness == -7 {{ return {none}(); }}\n\
         if __rawr_witness == -8 {{ return []; }}\n\
         if __rawr_witness == -9 {{ return (); }}\n",
        witness_key_statement()
    );
    for (id, model) in &models.by_definition {
        if !model.generics().is_empty() || !model.capabilities().contains(capability::Kind::Default)
        {
            continue;
        }
        let call = if let Some(definition) = models.std_impl(*id, "Default", "default") {
            format!("{}()", method_name(definition))
        } else if !models.default_folds(&model.ty(), &mut Vec::new()) {
            continue;
        } else if let Some(value) = default_value(model, models, &mut Vec::new()) {
            let helper = format!("{SENTINEL}{}", ops.len());
            ops.push(ExecutableOp::Default {
                value,
                definition: model.definition().clone(),
            });
            format!("{helper}()")
        } else {
            continue;
        };
        let _ = writeln!(
            text,
            "if __rawr_witness == {} {{ return {call}; }}",
            id.number()
        );
    }
    text.push_str("__rawr_throw \"no `Default` for this type witness\";\n}\n");
    output.declaration(shell.source, shell.span, &text);
}

pub(super) fn write_method(
    output: &mut location::Generated,
    name: &str,
    definition: &method::Definition,
    body: &Lowered,
    models: &Models,
) -> usize {
    let source = definition.source();
    let declaration = definition.name_span();
    output.declaration(source, declaration, "__rawr_private fn ");
    // A builtin owner's methods take any receiver: their callers are
    // checked, and dispatchers select them by runtime type key.
    if definition.receiver().is_some() && definition.builtin_owner().is_none() {
        let this_type = match models.definition(definition.owner()) {
            Nominal::Struct { .. } => "Rawr struct",
            Nominal::Enum { .. } => "Rawr enum",
        };
        output.declaration(
            source,
            definition.receiver_span().unwrap_or(declaration),
            &format!("{this_type:?}."),
        );
    }
    output.declaration(source, declaration, &format!("{name}("));
    for (index, parameter) in definition.parameters().iter().enumerate() {
        if index > 0 {
            output.declaration(source, declaration, ", ");
        }
        output.declaration(source, parameter.name.span, &parameter.name.text);
    }
    for (index, generic) in witness_generics(definition.generics(), models).enumerate() {
        if index > 0 || !definition.parameters().is_empty() {
            output.declaration(source, declaration, ", ");
        }
        output.declaration(source, declaration, &witness_variable(&generic.name.text));
    }
    output.declaration(source, declaration, ") {\n");
    let copied = method_copied(definition, models)
        .into_iter()
        .map(|index| definition.parameters()[index].name.text.as_str())
        .collect::<Vec<_>>();
    let body_offset = write_copied_body(
        output,
        source,
        declaration,
        definition.body(),
        body,
        &copied,
    );
    output.declaration(source, declaration, "\n}\n");
    body_offset
}

/// The body of a function copying `copied` back out: its value and the
/// parameters' final values, as each `return` gives them.
pub(super) fn write_copied_body(
    output: &mut location::Generated,
    source: crate::program::source::Id,
    declaration: Span,
    span: Span,
    body: &Lowered,
    copied: &[&str],
) -> usize {
    if copied.is_empty() {
        return output.body(source, span, &body.source, &body.locations);
    }
    output.declaration(source, declaration, "let __rawr_returned = {\n");
    let offset = output.body(source, span, &body.source, &body.locations);
    // The parameters' final values move out, as the body's locals end.
    let moved = copied
        .iter()
        .map(|name| format!("{}({name})", crate::engine::KEYWORD_MOVE))
        .collect::<Vec<_>>();
    output.declaration(
        source,
        declaration,
        &format!("\n}};\n[__rawr_returned, {}]", moved.join(", ")),
    );
    offset
}

pub(super) fn write_function(
    output: &mut location::Generated,
    source: crate::program::source::Id,
    generated_name: &str,
    definition: &item::Function,
    body: &Lowered,
    models: &Models,
) -> usize {
    let declaration = definition.name.span;
    output.declaration(source, declaration, &format!("fn {generated_name}("));
    let receiver = this_parameter(definition);
    let mut written = 0;
    for (index, parameter) in definition.parameters.iter().enumerate() {
        if Some(index) == receiver {
            continue;
        }
        if written > 0 {
            output.declaration(source, declaration, ", ");
        }
        written += 1;
        output.declaration(source, parameter.name.span, &parameter.name.text);
    }
    for (index, generic) in witness_generics(&definition.generics, models).enumerate() {
        if index > 0 || written > 0 {
            output.declaration(source, declaration, ", ");
        }
        output.declaration(source, declaration, &witness_variable(&generic.name.text));
    }
    output.declaration(source, declaration, ") {\n");
    let copied = function_copied(definition)
        .into_iter()
        .map(|index| definition.parameters[index].name.text.as_str())
        .collect::<Vec<_>>();
    let body_offset = write_copied_body(
        output,
        source,
        declaration,
        definition.body.span,
        body,
        &copied,
    );
    output.declaration(source, declaration, "\n}\n");
    body_offset
}

impl<'a> Parser<'a> {
    /// A host method whose callable argument is a `fn` pointer choice: one
    /// exact call per candidate function, chosen at runtime by the pointer,
    /// as iced takes one style function per widget.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn dispatch_host_method(
        &mut self,
        receiver: ParsedExpr,
        definition: &crate::host::type_def::Definition,
        items: &[crate::host::item::Definition],
        owner: Option<crate::host::facade::Dependency>,
        scoped: bool,
        member_span: Span,
        arguments: Vec<ParsedExpr>,
        index: usize,
        call: Span,
    ) -> Result<ParsedExpr, Error> {
        self.dispatch_choice(arguments, index, call, |this, candidate| {
            this.finish_host_method(
                receiver.clone(),
                definition,
                items,
                owner.clone(),
                scoped,
                member_span,
                candidate,
                call,
            )
        })
    }

    /// A host function whose callable argument is a `fn` pointer choice, as
    /// a host method's is.
    pub(super) fn dispatch_host_function(
        &mut self,
        items: &[crate::host::item::Definition],
        owner: Option<crate::host::facade::Dependency>,
        arguments: Vec<ParsedExpr>,
        index: usize,
        call: Span,
    ) -> Result<ParsedExpr, Error> {
        self.dispatch_choice(arguments, index, call, |this, candidate| {
            this.call_host_function(items, owner.clone(), candidate, call)
        })
    }

    /// `finish` of each candidate of the choice `arguments[index]` holds,
    /// run as the one the pointer names.
    fn dispatch_choice(
        &mut self,
        arguments: Vec<ParsedExpr>,
        index: usize,
        call: Span,
        mut finish: impl FnMut(&mut Self, Vec<ParsedExpr>) -> Result<ParsedExpr, Error>,
    ) -> Result<ParsedExpr, Error> {
        let Ty::FnChoice {
            keys,
            inputs,
            output,
        } = self.inference.resolve(&arguments[index].ty)
        else {
            unreachable!("the dispatched argument is a fn pointer choice");
        };
        let chosen = self.temporary("choice");
        let mut source = origin::Fragment::plain(format!("{{ const {chosen} = "));
        source.push(arguments[index].rendered_source());
        source.push_text("[0]; ");
        let mut result = None::<ParsedExpr>;
        for (position, key) in keys.iter().enumerate() {
            let mut candidate = arguments.clone();
            candidate[index] = self.operator_expr(
                origin::Fragment::plain("()"),
                self.inference.resolve(&key.ty(&inputs, &output)),
                arguments[index].span,
            );
            let call = finish(self, candidate)?;
            if position + 1 < keys.len() {
                source.push_text(format!("if {chosen} == {position} {{ "));
                source.push(call.rendered_source());
                source.push_text(" } else ");
            } else {
                source.push_text("{ ");
                source.push(call.rendered_source());
                source.push_text(" }");
            }
            match &result {
                Some(first) => self.require_type(&first.ty, &call.ty, call.span)?,
                None => result = Some(call),
            }
        }
        source.push_text(" }");
        let first = result.expect("a fn pointer choice has candidates");
        Ok(ParsedExpr {
            prelude: origin::Fragment::default(),
            source,
            ty: first.ty,
            span: call,
            lvalue: None,
            outcome: first.outcome,
            evaluation: first.evaluation,
        }
        .retain_origin())
    }
}

/// A free function's first `&mut` parameter, which a call passes as its
/// receiver's place. Not its `&mut fmt::Formatter`, whose text the body
/// gives back (`wrap_formatter`).
/// `names` with each of `from`'s names also in `to`, where `to` names
/// nothing of its own by that name.
pub(super) fn inherit_names<V: Clone>(
    names: &mut BTreeMap<(crate::program::source::Id, ImmutableString), V>,
    from: crate::program::source::Id,
    to: crate::program::source::Id,
) {
    let inherited = names
        .iter()
        .filter(|((source, _), _)| *source == from)
        .map(|((_, name), value)| (name.clone(), value.clone()))
        .collect::<Vec<_>>();
    for (name, value) in inherited {
        names.entry((to, name)).or_insert(value);
    }
}

/// What the blocks of `from` around `derive` bind, as names of `to`, the
/// derive's expansion: the innermost block's first, and the expansion's
/// own names over them.
pub(super) fn inherit_block_names<V: Clone>(
    blocks: &super::models::Blocks<V>,
    names: &mut BTreeMap<(crate::program::source::Id, ImmutableString), V>,
    from: crate::program::source::Id,
    derive: Span,
    to: crate::program::source::Id,
) {
    for ((source, name), bound) in blocks {
        if *source != from {
            continue;
        }
        let innermost = bound
            .iter()
            .filter(|(block, _)| crate::resolve::block::encloses(*block, derive))
            .max_by_key(|(block, _)| block.start());
        if let Some((_, value)) = innermost {
            names
                .entry((to, name.clone()))
                .or_insert_with(|| value.clone());
        }
    }
}

pub(crate) fn this_parameter(definition: &item::Function) -> Option<usize> {
    // An async function's future holds its `&mut` parameters after the call
    // returns, so each is lent by the caller (`lent_parameters`), none the
    // call's own receiver.
    if definition.body.asynchronous {
        return None;
    }
    definition.parameters.iter().position(|parameter| {
        matches!(
            parameter.ty,
            type_ref::Type::Reference { mutable: true, .. }
        ) && !crate::resolve::direct_mutable_invocation(&parameter.ty)
            && definition.formatter.as_ref() != Some(&parameter.name.text)
    })
}

/// The `&mut` parameters a call copies in and back out: a free
/// function's after its receiver (`this_parameter`), an inherent method's
/// or associated function's all. Rust's borrow rules keep them apart, so
/// the copies change the caller's places as the references would. Not the
/// `formatter`, whose text the body gives back (`wrap_formatter`). An
/// async function's are lent instead (`lent_parameters`).
pub(super) fn copied_parameters(
    parameters: &[item::Parameter],
    receiver: Option<usize>,
    formatter: Option<&ImmutableString>,
    asynchronous: bool,
) -> Vec<usize> {
    if asynchronous {
        return Vec::new();
    }
    further_mutable(parameters, receiver, formatter)
}

/// The `&mut` parameters an async function's call lends, the ones another
/// function's copies in and back out (`copied_parameters`). Its future
/// changes them after the call returns, for as long as it lives, so each
/// argument is the caller's place itself (`KEYWORD_LEND`), as Rust's
/// future holds the borrow.
pub(super) fn lent_parameters(
    parameters: &[item::Parameter],
    receiver: Option<usize>,
    formatter: Option<&ImmutableString>,
    asynchronous: bool,
) -> Vec<usize> {
    if !asynchronous {
        return Vec::new();
    }
    further_mutable(parameters, receiver, formatter)
}

/// `copied_parameters` of a function: not the ones it keeps
/// (`function_kept`), which its call lends.
pub(super) fn function_copied(definition: &item::Function) -> Vec<usize> {
    let kept = function_kept(definition);
    copied_parameters(
        &definition.parameters,
        this_parameter(definition),
        definition.formatter.as_ref(),
        definition.body.asynchronous,
    )
    .into_iter()
    .filter(|index| !kept.contains(index))
    .collect()
}

/// The `&mut` parameters a function's signature lets it keep past its
/// call (`kept_parameters`), which the call lends as references to the
/// caller's places (D1113).
pub(super) fn function_kept(definition: &item::Function) -> Vec<usize> {
    if definition.body.asynchronous {
        return Vec::new();
    }
    let receiver = this_parameter(definition);
    let mutable = further_mutable(
        &definition.parameters,
        receiver,
        definition.formatter.as_ref(),
    );
    kept_parameters(
        &definition.parameters,
        &definition.lifetimes,
        definition.output.as_ref(),
        &mutable,
    )
}

/// `function_kept` of a method: an inherent method's or associated
/// function's. A trait's methods copy every `&mut` parameter, as each of
/// their impls does (`method_copied`).
pub(super) fn method_kept(definition: &method::Definition) -> Vec<usize> {
    if definition.asynchronous() || definition.trait_name().is_some() {
        return Vec::new();
    }
    let mutable = further_mutable(definition.parameters(), None, definition.formatter());
    kept_parameters(
        definition.parameters(),
        definition.lifetimes(),
        Some(definition.output()),
        &mutable,
    )
}

/// The `&mut` parameters among `mutable` whose borrow the signature lets
/// outlive the call: the lifetime of each is named again below a
/// reference, by the output (`fn new(a: &'a mut i32) -> S<'a>`), another
/// parameter (`v: &mut Vec<&'a mut i32>`) or the impl's owner (`S<'a>` of
/// `self.a = a`), so the body may keep it in a value, as rustc's borrow
/// may live as long. An elided one is kept where it is the signature's
/// only reference and the output names an elided lifetime below its own
/// reference (`-> Option<&mut i32>`), as rustc's elision gives the output
/// its lifetime. A `&mut` output's own lifetime keeps none: the output is
/// a place below the call's receiver (`places::returned_place`).
fn kept_parameters(
    parameters: &[item::Parameter],
    lifetimes: &item::Lifetimes,
    output: Option<&type_ref::Type>,
    mutable: &[usize],
) -> Vec<usize> {
    let name = |span: &Span| lifetimes.names.get(span).map(ImmutableString::as_str);
    mutable
        .iter()
        .copied()
        .filter(|&index| {
            let type_ref::Type::Reference { lifetime, .. } = &parameters[index].ty else {
                return false;
            };
            let mut below = Vec::new();
            if let Some(output) = output {
                below_lifetimes(output, &mut below);
            }
            match lifetime.as_ref().and_then(name) {
                Some("static") => false,
                Some(own) => {
                    for (other, parameter) in parameters.iter().enumerate() {
                        if other != index {
                            below_lifetimes(&parameter.ty, &mut below);
                        }
                    }
                    if let Some(owner) = &lifetimes.owner {
                        lifetime_spans(owner, &mut below);
                    }
                    below.iter().any(|span| name(span) == Some(own))
                }
                None => {
                    let references = parameters
                        .iter()
                        .filter(|parameter| {
                            let mut spans = Vec::new();
                            lifetime_spans(&parameter.ty, &mut spans);
                            matches!(parameter.ty, type_ref::Type::Reference { .. })
                                || !spans.is_empty()
                        })
                        .count();
                    references == 1
                        && lifetimes.owner.is_none()
                        && below
                            .iter()
                            .any(|span| matches!(name(span), None | Some("_")))
                }
            }
        })
        .collect()
}

/// The lifetimes `ty` names below its own reference, if it is one (a
/// `&str` too, a `String` that keeps its lifetime): each lifetime of a
/// reference or a path's argument inside it.
fn below_lifetimes(ty: &type_ref::Type, spans: &mut Vec<Span>) {
    match ty {
        type_ref::Type::Reference { target, .. } => lifetime_spans(target, spans),
        type_ref::Type::Path(path) if ty.is_str() => {
            for argument in &path.arguments {
                if let type_ref::Argument::Type(ty) = argument {
                    lifetime_spans(ty, spans);
                }
            }
        }
        ty => lifetime_spans(ty, spans),
    }
}

/// Each lifetime `ty` names, by where it is written: a reference's (`None`
/// written is its own span, which no name has) and a path's arguments'.
fn lifetime_spans(ty: &type_ref::Type, spans: &mut Vec<Span>) {
    match ty {
        type_ref::Type::Unit(_) => {}
        type_ref::Type::Tuple { members, .. } => {
            members
                .iter()
                .for_each(|member| lifetime_spans(member, spans));
        }
        type_ref::Type::Reference {
            lifetime,
            target,
            span,
            ..
        } => {
            spans.push(lifetime.unwrap_or(*span));
            lifetime_spans(target, spans);
        }
        type_ref::Type::Path(path) => {
            for argument in &path.arguments {
                match argument {
                    type_ref::Argument::Type(ty) => lifetime_spans(ty, spans),
                    type_ref::Argument::Lifetime(span) => spans.push(*span),
                }
            }
        }
    }
}

/// The `&mut` parameters other than the receiver and the formatter.
fn further_mutable(
    parameters: &[item::Parameter],
    receiver: Option<usize>,
    formatter: Option<&ImmutableString>,
) -> Vec<usize> {
    parameters
        .iter()
        .enumerate()
        .filter(|(index, parameter)| {
            Some(*index) != receiver
                && matches!(
                    parameter.ty,
                    type_ref::Type::Reference { mutable: true, .. }
                )
                && !crate::resolve::direct_mutable_invocation(&parameter.ty)
                && formatter != Some(&parameter.name.text)
        })
        .map(|(index, _)| index)
        .collect()
}

/// `copied_parameters` of a method, a trait's too (every impl of a trait
/// function copies the same parameters, so a dispatcher passes the copies
/// through); not the formatter of a `Debug` or `Display` `fmt`, nor any
/// method's `&mut fmt::Formatter`, whose text the body gives back.
pub(super) fn method_copied(definition: &method::Definition, _models: &Models) -> Vec<usize> {
    if definition
        .trait_name()
        .is_some_and(|name| matches!(name.as_str(), "Debug" | "Display"))
    {
        return Vec::new();
    }
    let kept = method_kept(definition);
    copied_parameters(
        definition.parameters(),
        None,
        definition.formatter(),
        definition.asynchronous(),
    )
    .into_iter()
    .filter(|index| !kept.contains(index))
    .collect()
}

/// `lent_parameters` of a method: an async method's `&mut` parameters.
pub(super) fn method_lent(definition: &method::Definition) -> Vec<usize> {
    lent_parameters(
        definition.parameters(),
        None,
        definition.formatter(),
        definition.asynchronous(),
    )
}
