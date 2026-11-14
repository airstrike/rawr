//! Trait objects of a crate's traits where an item takes or gives one
//! (D872), as tracing-core's `Identifier(&'static dyn Callsite)` holds one.
//!
//! Native code makes every object. A program's value coerced to one
//! crosses as a `Coerced` value, tagged with the type it was coerced from
//! (`object::tag`); the adapter finds that type among the facade's impls of
//! the trait and coerces the value of it, as rustc coerces it. An object
//! native code gave crosses as itself, in its slot.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{concrete_type, render_from_dynamic, request, TYPE_PATHS};
use crate::classify::representability::convert;
use crate::classify::representability::object::{Object, Pointer, ARGUMENTS, CALLABLE};

/// A type the facade implements a trait for that the adapter coerces a
/// program's value of to the trait's object.
struct Implementor {
    /// The tag a value coerced from it crosses with.
    tag: String,
    /// What a program's value of it is.
    kind: Kind,
}

enum Kind {
    /// A number, `bool` or `char`: Rawr's own value, made into the Rust
    /// value of this type.
    Primitive(convert::Builtin),
    /// A crate type with no parameters, by its Rust path: a native value.
    Native(String),
    /// core's `fmt::Arguments<'_>`: the text of the program's
    /// `format_args!`, which native code formats where the call runs, in a
    /// `let` of its own, as rustc makes one (#1561).
    Arguments,
}

thread_local! {
    /// Each trait's implementors, by the trait's identity, for one
    /// generation.
    static IMPLEMENTORS: RefCell<BTreeMap<[u8; 32], Vec<Implementor>>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// Read each implementor of each trait the facade's impls are of, once
/// `TYPE_PATHS` names the facade's types.
pub(super) fn install(facade: Option<&crate::surface::facade::Plan>) {
    IMPLEMENTORS.with_borrow_mut(|implementors| {
        implementors.clear();
        let headers = facade
            .into_iter()
            .flat_map(crate::surface::facade::Plan::facades)
            .flat_map(crate::surface::facade::Facade::impls)
            .filter(|header| header.parameters().len() == 0 && header.arguments().is_empty());
        for header in headers {
            let Some(implementor) = implementor(header.self_type()) else {
                continue;
            };
            let found = implementors.entry(*header.trait_identity()).or_default();
            if !found.iter().any(|known| known.tag == implementor.tag) {
                found.push(implementor);
            }
        }
    });
}

pub(super) fn clear() {
    IMPLEMENTORS.with_borrow_mut(BTreeMap::clear);
}

/// The implementor an impl's self type is, when the adapter coerces a
/// program's value of it.
fn implementor(ty: &crate::surface::facade::Type) -> Option<Implementor> {
    use crate::surface::facade::Type;
    let builtin = match ty {
        Type::Bool => Some(convert::Builtin::Bool),
        Type::Char => Some(convert::Builtin::Char),
        Type::F64 => Some(convert::Builtin::F64),
        Type::I64 => Some(convert::Builtin::I64),
        Type::Primitive(name) => convert::Builtin::primitive(name),
        _ => None,
    };
    if let Some(builtin) = builtin {
        let converted = convert::Type::Builtin(builtin);
        return Some(Implementor {
            tag: crate::classify::representability::object::tag(&converted)?,
            kind: Kind::Primitive(builtin),
        });
    }
    if matches!(ty, Type::Arguments) {
        return Some(Implementor {
            tag: ARGUMENTS.to_owned(),
            kind: Kind::Arguments,
        });
    }
    let Type::Nominal {
        identity,
        arguments,
    } = ty
    else {
        return None;
    };
    if !arguments.is_empty() {
        return None;
    }
    let path = TYPE_PATHS.with_borrow(|paths| paths.get(identity).cloned())?;
    let converted = convert::Type::Nominal {
        definition: crate::surface::identity::Definition::authenticated(*identity),
        arguments: Box::default(),
    };
    Some(Implementor {
        tag: crate::classify::representability::object::tag(&converted)?,
        kind: Kind::Native(path),
    })
}

/// `(dyn Trait<..> + auto)`, as the object names its trait where a type
/// path names it, as `<(dyn Shape) as Shape>::area` does.
pub(super) fn dyn_type(object: &Object) -> Option<String> {
    object_type(object).map(|dynamic| format!("({dynamic})"))
}

/// `dyn Trait<..> + auto`, as the object names its trait.
fn object_type(object: &Object) -> Option<String> {
    let mut text = String::from("dyn ");
    text.push_str(
        &TYPE_PATHS.with_borrow(|paths| paths.get(object.principal().as_bytes()).cloned())?,
    );
    if !object.arguments().is_empty() {
        let arguments = object
            .arguments()
            .iter()
            .map(concrete_type)
            .collect::<Option<Vec<_>>>()?;
        write!(text, "<{}>", arguments.join(", ")).ok()?;
    }
    for auto in object.auto() {
        write!(text, " + {}", auto.rust_path()).ok()?;
    }
    Some(text)
}

/// The function that puts a value behind the owning pointer `pointer`, as
/// `Box::new` does.
fn owner(pointer: Pointer) -> &'static str {
    match pointer {
        Pointer::Rc => "std::rc::Rc::new",
        Pointer::Arc => "std::sync::Arc::new",
        Pointer::Boxed | Pointer::Lasting | Pointer::Shared => "Box::new",
    }
}

/// The Rust type of `object` behind its pointer.
pub(super) fn rust_type(object: &Object) -> Option<String> {
    let dynamic = object_type(object)?;
    Some(match object.pointer() {
        Pointer::Lasting => format!("&'static ({dynamic})"),
        Pointer::Shared => format!("&({dynamic})"),
        Pointer::Boxed => format!("Box<{dynamic}>"),
        Pointer::Rc => format!("std::rc::Rc<{dynamic}>"),
        Pointer::Arc => format!("std::sync::Arc<{dynamic}>"),
    })
}

/// The Rust type of a `&dyn Trait` native code lends a script closure for
/// the call (#28), and the scope family of it at each lifetime `'x`, which
/// the object's own lifetime is too. `None` for an object behind another
/// pointer.
pub(super) fn lent_family(object: &Object) -> Option<(String, String)> {
    if object.pointer() != Pointer::Shared {
        return None;
    }
    let dynamic = object_type(object)?;
    Some((format!("&({dynamic})"), format!("&'x ({dynamic} + 'x)")))
}

/// A generated `Result<T, Error>` expression making the object `object`, a
/// `&'static dyn` or a `Box<dyn>`, from the owned `Dynamic` named `value`:
/// an object native code gave, moved out of its slot, or a program's value
/// coerced from one of the trait's implementors.
pub(super) fn render_owned(
    object: &Object,
    value: &str,
    depth: usize,
    output: &mut String,
) -> Result<(), request::Error> {
    let rust = rust_type(object).ok_or_else(|| request::drift("a trait object's type"))?;
    let mut arms = String::new();
    let implementors = IMPLEMENTORS.with_borrow(|implementors| {
        implementors
            .get(object.principal().as_bytes())
            .map(|found| {
                found
                    .iter()
                    .map(|implementor| {
                        (
                            implementor.tag.clone(),
                            arm(object, implementor, &rust, depth),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    for (tag, made) in implementors {
        write!(arms, "{tag:?} => {}, ", made?).map_err(request::unwritable)?;
    }
    if let Some(made) = callable_arm(object, &rust)? {
        write!(arms, "{CALLABLE:?} => {made}, ").map_err(request::unwritable)?;
    }
    // An object native code gave, or a counted one made of the box a
    // program's own type is a native trait's object as (#1264, #1304).
    let given = match object.pointer() {
        Pointer::Rc | Pointer::Arc => {
            let boxed = rust_type(&object.behind(Pointer::Boxed))
                .ok_or_else(|| request::drift("a trait object's type"))?;
            format!(
                "if arguments.arena.index::<{boxed}>(&handle).is_ok() {{ arguments.arena.take::<{boxed}>(handle).map(<{rust}>::from) }} else {{ arguments.arena.take::<{rust}>(handle) }}"
            )
        }
        Pointer::Lasting | Pointer::Shared | Pointer::Boxed => {
            format!("arguments.arena.take::<{rust}>(handle)")
        }
    };
    write!(
        output,
        "{{ let value = {value}; match __rawr_internal::object::Coerced::of(&value) {{ Some((inner, tag)) => match tag.as_str() {{ {arms}_ => Err(Error::TypeMismatch) }}, None => value.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| {given}) }} }}"
    )
    .map_err(request::unwritable)
}

/// The arm of [`render_owned`] that makes `object` from a value of
/// `implementor`, named `inner`.
fn arm(
    object: &Object,
    implementor: &Implementor,
    rust: &str,
    depth: usize,
) -> Result<String, request::Error> {
    Ok(match (&implementor.kind, object.pointer()) {
        // A program's number lives in no static, and `format_args!`'s
        // `fmt::Arguments` borrows what it formats.
        (Kind::Primitive(_), Pointer::Lasting) | (Kind::Arguments, _) => {
            "Err(Error::Value(\"borrowed value does not live long enough (E0597)\".to_owned()))"
                .to_owned()
        }
        (Kind::Primitive(builtin), _) => {
            let mut converted = String::new();
            render_from_dynamic(
                &convert::Type::Builtin(*builtin),
                "inner",
                depth + 1,
                &mut converted,
            )?;
            let owner = owner(object.pointer());
            format!("({converted}).map(|made| -> {rust} {{ {owner}(made) }})")
        }
        // A static's place, of a body or of native code (D862).
        (Kind::Native(path), Pointer::Lasting) => format!(
            "__rawr_internal::place::statics::lasting::<{path}>(&inner).map(|lasting| -> {rust} {{ lasting }}).ok_or_else(|| Error::Value(\"borrowed value does not live long enough (E0597)\".to_owned()))"
        ),
        // The value the pointer holds, moved out of its slot.
        (Kind::Native(path), pointer) => {
            let owner = owner(pointer);
            format!(
                "inner.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take::<{path}>(handle)).map(|made| -> {rust} {{ {owner}(made) }})"
            )
        }
    })
}

/// The Rust closure that calls a program's callable, which fills an object
/// through its trait's impl for every callable (#1981).
struct Closure {
    /// The block that makes it.
    block: String,
    /// The `Box<dyn Fn(..) -> ..>` it can be held as.
    boxed: String,
}

/// The closure calling the program's callable `source` gives, which fills
/// `object`, its inputs attested by the witness of the coerced value the
/// `Dynamic` named `coerced` is. `None` for an object no callable fills.
fn closure(
    object: &Object,
    source: &str,
    coerced: &str,
) -> Result<Option<Closure>, request::Error> {
    use crate::classify::representability::{callable, signature};
    let Some(call) = object.call() else {
        return Ok(None);
    };
    let unwritten = || request::drift("a trait object's callable");
    let inputs = call
        .inputs()
        .iter()
        .map(concrete_type)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(unwritten)?;
    let output = concrete_type(call.output()).ok_or_else(unwritten)?;
    let made = callable::Call::new(0, call.inputs().into(), call.output().clone());
    let signature =
        signature::Signature::new(Box::default(), call.inputs().into(), call.output().clone());
    Ok(Some(Closure {
        block: super::render_callable_closure(
            super::Witnesses::Coerced(coerced),
            source,
            &made,
            &signature,
            &[],
            None,
        )?,
        boxed: format!("Box<dyn Fn({}) -> {output}>", inputs.join(", ")),
    }))
}

/// The arm of [`render_owned`] that makes `object`, of the Rust type
/// `rust`, from the program's callable `inner`: the closure that calls it,
/// behind an owning pointer, or kept for the runner's life behind a
/// `&'static`, one per function, as rustc promotes `&Code::highlight` to a
/// static. A closure that captures lives in no static (E0597).
fn callable_arm(object: &Object, rust: &str) -> Result<Option<String>, request::Error> {
    let Some(Closure { block: closure, .. }) = closure(object, "inner.clone()", "value")? else {
        return Ok(None);
    };
    let dynamic = object_type(object).ok_or_else(|| request::drift("a trait object's type"))?;
    Ok(Some(match object.pointer() {
        Pointer::Lasting | Pointer::Shared => format!(
            "match __rawr_internal::object::lasting::<{dynamic}, Error>(&inner, || -> Result<Box<{dynamic}>, Error> {{ let made = {closure}; Ok(Box::new(made)) }}) {{ Some(made) => made.map(|lasting| -> {rust} {{ lasting }}), None => Err(Error::Value(\"borrowed value does not live long enough (E0597)\".to_owned())) }}"
        ),
        pointer => {
            let owner = owner(pointer);
            format!("(|| -> Result<{rust}, Error> {{ let made = {closure}; Ok({owner}(made)) }})()")
        }
    }))
}

/// The statements that make `argument_{local}`, the `&dyn Trait` input
/// `object` of the call's argument `index`: an object native code gave (a
/// `&'static dyn`, or a `Box<dyn>` lent from its slot for the call), a
/// static's place, a crate value lent from its slot, a program's number
/// made into its Rust value for the call, or an object native code lent a
/// script closure, which the adapter reached around its body (#28).
pub(super) fn render_lent_input(
    local: usize,
    index: usize,
    object: &Object,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    use std::io::Write as _;
    let unwritten = || request::drift("a trait object's type");
    let rust = rust_type(object).ok_or_else(unwritten)?;
    let lasting = rust_type(&object.behind(Pointer::Lasting)).ok_or_else(unwritten)?;
    let boxed = rust_type(&object.behind(Pointer::Boxed)).ok_or_else(unwritten)?;
    let mut arms = String::new();
    // Each native implementor's value, shared out of its slot for as long
    // as the call borrows it.
    let mut kept = String::new();
    IMPLEMENTORS.with_borrow(|implementors| -> Result<(), request::Error> {
        let found = implementors.get(object.principal().as_bytes());
        for (position, implementor) in found.into_iter().flatten().enumerate() {
            let made = match &implementor.kind {
                // The text of the program's `format_args!`, formatted in a
                // `let` of its own before the object is made.
                Kind::Arguments => {
                    writeln!(
                        kept,
                        "    let argument_{local}_text = __rawr_internal::object::Coerced::of(&argument_{local}_value).filter(|(_, tag)| tag.as_str() == {ARGUMENTS:?}).and_then(|(inner, _)| inner.try_cast_result::<ImmutableString>().ok());
    let argument_{local}_str: &str = argument_{local}_text.as_ref().map_or(\"\", |text| text.as_str());
    let argument_{local}_arguments = ::core::format_args!(\"{{}}\", argument_{local}_str);"
                    )
                    .map_err(request::unwritable)?;
                    format!("&argument_{local}_arguments")
                }
                Kind::Primitive(builtin) => {
                    let mut converted = String::new();
                    render_from_dynamic(
                        &convert::Type::Builtin(*builtin),
                        "inner",
                        1,
                        &mut converted,
                    )?;
                    format!(
                        "{{ argument_{local}_made = Box::new(({converted})?); &*argument_{local}_made }}"
                    )
                }
                Kind::Native(path) => {
                    writeln!(
                        kept,
                        "    let argument_{local}_shared_{position}: std::rc::Rc<{path}>;"
                    )
                    .map_err(request::unwritable)?;
                    format!(
                        "match __rawr_internal::place::statics::lasting::<{path}>(&inner) {{ Some(lasting) => lasting, None => {{ argument_{local}_shared_{position} = inner.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.share::<{path}>(handle))?; &*argument_{local}_shared_{position} }} }}"
                    )
                }
            };
            write!(arms, "{:?} => {made}, ", implementor.tag).map_err(request::unwritable)?;
        }
        Ok(())
    })?;
    // The program's callable, a closure that calls it boxed for the call.
    if let Some(Closure {
        block: closure,
        boxed,
    }) = closure(object, "inner", &format!("argument_{local}_value"))?
    {
        writeln!(
            kept,
            "    #[allow(unused_variables)]\n    let argument_{local}_callable: {boxed};"
        )
        .map_err(request::unwritable)?;
        write!(
            arms,
            "{CALLABLE:?} => {{ argument_{local}_callable = Box::new({closure}); &argument_{local}_callable }}, "
        )
        .map_err(request::unwritable)?;
    }
    // An object native code lent the script closure that makes the call,
    // which the adapter reached around its body (#28).
    let lent = if super::lent_object_at(index) {
        format!("object_{index}")
    } else {
        "None".to_owned()
    };
    writeln!(
        output,
        "    let argument_{local}_value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take();
    #[allow(unused_variables)]
    let argument_{local}_made: {boxed};
{kept}    let argument_{local}_lasting: {lasting};
    #[allow(unused_variables, unused_mut)]
    let mut argument_{local}_held: PlaceHeld<{boxed}>;
    let argument_{local}: {rust} = match (__rawr_internal::object::Coerced::of(&argument_{local}_value), {lent}) {{
        (Some((inner, tag)), _) => match tag.as_str() {{ {arms}_ => return Err(Error::TypeMismatch) }},
        (None, Some(object)) => object,
        (None, None) => {{
            let handle = argument_{local}_value.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch)?;
            if arguments.arena.index::<{boxed}>(&handle).is_ok() {{
                argument_{local}_held = arguments.arena.lease_place::<{boxed}>(handle)?;
                &**argument_{local}_held.get_mut()
            }} else {{
                argument_{local}_lasting = arguments.arena.take::<{lasting}>(handle)?;
                argument_{local}_lasting
            }}
        }}
    }};"
    )
    .map_err(request::unwritable)
}

/// The element type of a `&[E]` input whose elements lend a crate trait's
/// objects, `E` a `&dyn Trait` or an `Option` of one, as tracing-core's
/// `value_set_all(&[Option<&dyn Value>])` takes (#1591). `None` for another
/// type.
pub(super) fn lent_slice_element(ty: &convert::Type) -> Option<&convert::Type> {
    let convert::Type::Borrowed { mutable: false, ty } = ty else {
        return None;
    };
    let convert::Type::Sequence { element, .. } = &**ty else {
        return None;
    };
    lends_objects(element).then_some(&**element)
}

/// Whether `element` is a `&dyn Trait` of a crate's trait, or an `Option`
/// of one.
pub(super) fn lends_objects(element: &convert::Type) -> bool {
    match element {
        convert::Type::Object(object) => object.pointer() == Pointer::Shared,
        convert::Type::Option(inner) => lends_objects(inner),
        _ => false,
    }
}

/// The statement that makes `argument_{local}_made`, each element of the
/// call's argument `index`, a program's array, with its objects held
/// (`HeldObject`): what a slice of `element`s lends, which a scope frame
/// keeps while its output borrows them. [`lent_slice`] lends it.
pub(super) fn render_held_slice(
    local: usize,
    index: usize,
    element: &convert::Type,
    output: &mut Vec<u8>,
) -> Result<(), request::Error> {
    use std::io::Write as _;
    let mut made = String::new();
    held_element(element, "value", &mut made)?;
    writeln!(
        output,
        "    let argument_{local}_made = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take().flatten().into_array().map_err(|_| Error::TypeMismatch)?.into_iter().map(|value| -> Result<_, Error> {{ {made} }}).collect::<Result<Vec<_>, Error>>()?;"
    )
    .map_err(request::unwritable)
}

/// The statements that lend a slice of objects, and what closes them.
pub(super) struct Lent {
    pub(super) open: String,
    pub(super) close: String,
}

/// The statements that lend `argument_{local}_made` (`render_held_slice`)
/// as `argument_{local}`, the `&[E]` of `element`s the call takes. Inside
/// a scope `frame`, each held text of the program's `format_args!` is
/// formatted in a `let` of its own as `with_arguments` nests them, and the
/// rest of the frame runs innermost, before what `close`s them: rustc's
/// `fmt::Arguments` lives only in the statement that makes it (#1561).
/// Outside a frame a held text is refused where it is lent.
pub(super) fn lent_slice(
    local: usize,
    element: &convert::Type,
    frame: bool,
) -> Result<Lent, request::Error> {
    let unwritten = || request::drift("a trait object's type");
    let mut lent = String::new();
    lent_element(local, element, "held", &mut lent)?;
    let rust = concrete_type(element).ok_or_else(unwritten)?;
    let object = element_object(element).ok_or_else(unwritten)?;
    let dynamic = object_type(object).ok_or_else(unwritten)?;
    let lend = format!(
        "let argument_{local}_lent: Vec<{rust}> = argument_{local}_made.iter().map(|held| -> Result<_, Error> {{ Ok({lent}) }}).collect::<Result<_, Error>>()"
    );
    let formats = IMPLEMENTORS.with_borrow(|implementors| {
        implementors
            .get(object.principal().as_bytes())
            .is_some_and(|found| {
                found
                    .iter()
                    .any(|implementor| matches!(implementor.kind, Kind::Arguments))
            })
    });
    if !frame {
        return Ok(Lent {
            open: format!(
                "let mut argument_{local}_next = std::iter::empty::<&({dynamic})>();
    {lend}?;
    let argument_{local}: &[{rust}] = &argument_{local}_lent;"
            ),
            close: String::new(),
        });
    }
    if !formats {
        return Ok(Lent {
            open: format!(
                "\n        let mut argument_{local}_next = std::iter::empty::<&({dynamic})>();
        {lend}.map_err(runtime_error)?;
        let argument_{local}: &[{rust}] = &argument_{local}_lent;"
            ),
            close: String::new(),
        });
    }
    let mut text = String::new();
    held_text(element, "held", &mut text)?;
    Ok(Lent {
        open: format!(
            "\n        let argument_{local}_texts: Vec<&str> = argument_{local}_made.iter().filter_map(|held| {text}).collect();
        with_arguments(&argument_{local}_texts, Vec::new(), |argument_{local}_arguments| -> Result<Dynamic, Box<__rawr_internal::EvalAltResult>> {{
        let argument_{local}_objects: Vec<&({dynamic})> = argument_{local}_arguments.iter().map(|arguments| *arguments as &({dynamic})).collect();
        let mut argument_{local}_next = argument_{local}_objects.iter().copied();
        {lend}.map_err(runtime_error)?;
        let argument_{local}: &[{rust}] = &argument_{local}_lent;"
        ),
        close: "\n        })".to_owned(),
    })
}

/// The object an element of a lent slice is, or holds in its `Option`s.
fn element_object(element: &convert::Type) -> Option<&Object> {
    match element {
        convert::Type::Option(inner) => element_object(inner),
        convert::Type::Object(object) => Some(object),
        _ => None,
    }
}

/// The `Option<&str>` of the held text of the program's `format_args!`
/// that `held`, a reference to what `held_element` made, holds.
fn held_text(
    element: &convert::Type,
    held: &str,
    output: &mut String,
) -> Result<(), request::Error> {
    match element {
        convert::Type::Option(inner) => {
            let mut text = String::new();
            held_text(inner, "held", &mut text)?;
            write!(output, "{held}.as_ref().and_then(|held| {text})").map_err(request::unwritable)
        }
        convert::Type::Object(_) => write!(output, "{held}.text()").map_err(request::unwritable),
        _ => Err(request::drift("a lent trait object's element")),
    }
}

/// A generated `Result<_, Error>` expression holding `element` of the
/// owned `Dynamic` named `value`: an `Option` of it as an `Option`, and an
/// object as its `HeldObject`.
fn held_element(
    element: &convert::Type,
    value: &str,
    output: &mut String,
) -> Result<(), request::Error> {
    match element {
        convert::Type::Option(inner) => {
            let mut held = String::new();
            held_element(inner, "inner", &mut held)?;
            write!(
                output,
                "{{ let mut value = {value}; match __rawr_internal::option::take(&mut value).ok_or(Error::TypeMismatch)? {{ None => Ok(None), Some(inner) => ({held}).map(Some) }} }}"
            )
            .map_err(request::unwritable)
        }
        convert::Type::Object(object) => held_object(object, value, output),
        _ => Err(request::drift("a lent trait object's element")),
    }
}

/// The `&dyn Trait`, or the `Option` of one, that `held`, a reference to
/// what `held_element` made, lends: a held text lends the next of
/// `argument_{local}_next`, the objects of the `fmt::Arguments` made of the
/// texts in order.
fn lent_element(
    local: usize,
    element: &convert::Type,
    held: &str,
    output: &mut String,
) -> Result<(), request::Error> {
    match element {
        convert::Type::Option(inner) => {
            let mut lent = String::new();
            lent_element(local, inner, "held", &mut lent)?;
            write!(
                output,
                "match {held}.as_ref() {{ None => None, Some(held) => Some({lent}) }}"
            )
            .map_err(request::unwritable)
        }
        convert::Type::Object(_) => write!(
            output,
            "match {held}.get() {{ Some(object) => object, None => argument_{local}_next.next().ok_or_else(|| Error::Value(\"`format_args!` lent in a slice to a call that doesn't keep it is not supported in Rawr yet\".to_owned()))? }}"
        )
        .map_err(request::unwritable),
        _ => Err(request::drift("a lent trait object's element")),
    }
}

/// A generated `Result<HeldObject<dyn Trait>, Error>` expression holding
/// `object` of the owned `Dynamic` named `value`: an object native code
/// gave, a static's place, a crate value its slot shares, or a program's
/// number made into its Rust value.
fn held_object(object: &Object, value: &str, output: &mut String) -> Result<(), request::Error> {
    let unwritten = || request::drift("a trait object's type");
    let dynamic = object_type(object).ok_or_else(unwritten)?;
    let lasting = rust_type(&object.behind(Pointer::Lasting)).ok_or_else(unwritten)?;
    let held = format!("HeldObject::<{dynamic}>");
    let mut arms = String::new();
    IMPLEMENTORS.with_borrow(|implementors| -> Result<(), request::Error> {
        let found = implementors.get(object.principal().as_bytes());
        for implementor in found.into_iter().flatten() {
            let made = match &implementor.kind {
                Kind::Primitive(builtin) => {
                    let mut converted = String::new();
                    render_from_dynamic(
                        &convert::Type::Builtin(*builtin),
                        "inner",
                        1,
                        &mut converted,
                    )?;
                    format!("({converted}).map(|made| {held}::Made(Box::new(made)))")
                }
                Kind::Native(path) => format!(
                    "match __rawr_internal::place::statics::lasting::<{path}>(&inner) {{ Some(lasting) => Ok({held}::Lasting(lasting)), None => inner.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.share::<{path}>(handle)).map(|shared| {held}::Shared(shared)) }}"
                ),
                Kind::Arguments => format!(
                    "inner.try_cast_result::<ImmutableString>().map_err(|_| Error::TypeMismatch).map({held}::Text)"
                ),
            };
            write!(arms, "{:?} => {made}, ", implementor.tag).map_err(request::unwritable)?;
        }
        Ok(())
    })?;
    if let Some(Closure { block, .. }) = closure(object, "inner", "value")? {
        write!(
            arms,
            "{CALLABLE:?} => (|| -> Result<_, Error> {{ Ok({held}::Made(Box::new({block}))) }})(), "
        )
        .map_err(request::unwritable)?;
    }
    write!(
        output,
        "{{ let value = {value}; match __rawr_internal::object::Coerced::of(&value) {{ Some((inner, tag)) => match tag.as_str() {{ {arms}_ => Err(Error::TypeMismatch) }}, None => value.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take::<{lasting}>(handle)).map({held}::Lasting) }} }}"
    )
    .map_err(request::unwritable)
}
