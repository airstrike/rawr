//! Function and method writers: the generated dispatchers of trait, instance,
//! `Into` and static calls, and the text of each lowered function and method.

use super::models::{Models, Nominal};
use super::op::{builtin_code, owner_id, owner_key, ExecutableOp, PreludeOp, COUNTED_KEY};
use super::ty::Ty;
use super::{
    closure, default_value, drops, instance_dispatcher_name, instance_text, item_generics,
    location, mentions_parameter, method_name, origin, retain_locations, static_dispatcher_name,
    static_instance_dispatcher_name, trait_dispatcher_name, witness_generics, witness_variable,
    Lowered, ParsedExpr, Parser, CLOSURE_SENTINEL, DEFAULT_DISPATCHER, FALLBACK_INSTANCE, SENTINEL,
};
use crate::resolve::{item, type_ref, Error, Program};
use crate::{capability, method, ImmutableString, Span};
use std::collections::BTreeMap;
use std::fmt::Write;

/// Write each lifted closure as its own generated function.
pub(super) fn write_closures(
    output: &mut location::Generated,
    retained: &mut Vec<location::Pending>,
    source: crate::program::source::Id,
    lifted: Vec<closure::Lifted>,
    closures: &mut Vec<closure::Callable>,
    rewrites: &mut BTreeMap<ImmutableString, ImmutableString>,
) {
    for closure in lifted {
        let Some(value) = closure.value else {
            continue;
        };
        let generated = format!("{CLOSURE_SENTINEL}{}", closures.len());
        let dispatch: ImmutableString = format!("\0rawr.closure.{}", closure.key).into();
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
        });
    }
}

/// One function per trait method with a receiver, running the impl of the
/// receiver's runtime type: how a call on a value of a generic type bounded
/// by the trait reaches its implementation.
pub(super) fn write_trait_dispatchers(
    output: &mut location::Generated,
    models: &Models,
    ops: &mut Vec<ExecutableOp>,
) {
    // A trait without methods, such as std's `Error`, which every program
    // knows, needs no dispatcher and so no type index.
    if models
        .traits
        .values()
        .all(|(_, declared)| declared.functions.is_empty())
    {
        return;
    }
    let type_index = format!("{SENTINEL}{}", ops.len());
    ops.push(ExecutableOp::Prelude(PreludeOp::TypeIndex));
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
                    trait_name,
                    *source,
                    declared,
                    function,
                    &parameters,
                    &type_index,
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
            let mut blanket = None;
            for ((owner, _), definitions) in models
                .trait_methods
                .iter()
                .filter(|((_, name), _)| *name == function.name.text)
            {
                for definition in definitions
                    .iter()
                    .filter(|definition| definition.trait_name() == Some(trait_name))
                {
                    if owner.builtin_code() == Some(crate::resolve::BLANKET_OWNER) {
                        blanket = Some(definition.id());
                        continue;
                    }
                    let _ = writeln!(
                        text,
                        "if __rawr_type == {} {{ return self.{}({parameters}); }}",
                        owner_key(*owner),
                        method_name(definition.id())
                    );
                }
            }
            // A std trait's dispatcher falls back to what Rawr does for a
            // type without a hand-written impl: a derive, or a builtin.
            let fallback = match (models.std_traits.contains(trait_name), trait_name.as_str()) {
                _ if let Some(id) = blanket => {
                    format!("return self.{}({parameters});", method_name(id))
                }
                (true, "Clone") => "return self;".to_string(),
                // An `Rc` or `Arc` writes what it points to, as std's
                // `impl<T: Display> Display for Rc<T>` does.
                (true, "Display") => {
                    let get = format!("{SENTINEL}{}", ops.len());
                    ops.push(ExecutableOp::Prelude(PreludeOp::CountedGet));
                    format!(
                        "if __rawr_type == {COUNTED_KEY} {{ return {get}(self).{}({parameters}); }}\n\
                         return \"\" + self;",
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
    trait_name: &ImmutableString,
    source: crate::program::source::Id,
    declared: &item::Trait,
    function: &item::Function,
    parameters: &str,
    type_index: &str,
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
        for (owner, definition) in &definitions {
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
                blanket = Some(definition.id());
                continue;
            }
            let _ = writeln!(
                text,
                "if __rawr_type == {} {{ return self.{}({parameters}); }}",
                owner_key(*owner),
                method_name(definition.id())
            );
        }
        if let Some(id) = blanket {
            let _ = writeln!(text, "return self.{}({parameters});", method_name(id));
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
        // A builtin owner's conversions are into each of its Self types:
        // `From<Roar> for f32` is not into `f64`.
        for conversion in owner_id(&target)
            .and_then(|id| models.conversions.get(&id))
            .into_iter()
            .flatten()
            .filter(|conversion| {
                conversion
                    .builtin_owner()
                    .is_none_or(|owner| Ty::from_resolved(owner) == target)
            })
        {
            let from =
                Ty::from_resolved(conversion.conversion().expect("conversions have a source"));
            if let Some(id) = owner_id(&from) {
                let _ = writeln!(
                    text,
                    "if __rawr_type == {} {{ return {}(self); }}",
                    owner_key(id),
                    method_name(conversion.id())
                );
            }
        }
        for (from, op) in builtin_conversions_into(&target) {
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
/// `From<char> for String`. (`From<&str> for String` is the value itself.)
pub(super) const BUILTIN_CONVERSIONS: [(crate::INT, crate::INT, PreludeOp); 3] = [
    (-2, -5, PreludeOp::IntFromBool),
    (-3, -5, PreludeOp::FloatFromBool),
    (-4, -6, PreludeOp::StringFromChar),
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
                            method_name(definition.id())
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
                    method_name(definition.id())
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
         if __rawr_witness == -2 {{ return 0; }}\n\
         if __rawr_witness == -3 {{ return 0.0; }}\n\
         if __rawr_witness == -4 {{ return \"\"; }}\n\
         if __rawr_witness == -5 {{ return false; }}\n\
         if __rawr_witness == -6 {{ return '\\x00'; }}\n\
         if __rawr_witness == -7 {{ return {none}(); }}\n\
         if __rawr_witness == -8 {{ return []; }}\n\
         if __rawr_witness == -9 {{ return (); }}\n"
    );
    for (id, model) in &models.by_definition {
        if !model.generics().is_empty() || !model.capabilities().contains(capability::Kind::Default)
        {
            continue;
        }
        let call = if let Some(definition) = models.std_impl(*id, "Default", "default") {
            format!("{}()", method_name(definition.id()))
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
            id.index()
        );
    }
    text.push_str("__rawr_throw \"no `Default` for this type witness\";\n}\n");
    output.declaration(shell.source, shell.span, &text);
}

pub(super) fn write_method(
    output: &mut location::Generated,
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
    output.declaration(
        source,
        declaration,
        &format!("{}(", method_name(definition.id())),
    );
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
    let copied = copied_parameters(
        &definition.parameters,
        receiver,
        definition.formatter.as_ref(),
    )
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
                Ty::FnItem {
                    key: Box::new(key.clone()),
                    inputs: inputs.clone(),
                    output: output.clone(),
                },
                arguments[index].span,
            );
            let call = self.finish_host_method(
                receiver.clone(),
                definition,
                items,
                owner.clone(),
                scoped,
                member_span,
                candidate,
                call,
            )?;
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
/// `formatter`, whose text the body gives back (`wrap_formatter`).
pub(super) fn copied_parameters(
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
    copied_parameters(definition.parameters(), None, definition.formatter())
}
