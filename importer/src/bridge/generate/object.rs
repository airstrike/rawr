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
use crate::classify::representability::object::{Object, Pointer};

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

/// The Rust type of `object` behind its pointer.
pub(super) fn rust_type(object: &Object) -> Option<String> {
    let dynamic = object_type(object)?;
    Some(match object.pointer() {
        Pointer::Lasting => format!("&'static ({dynamic})"),
        Pointer::Shared => format!("&({dynamic})"),
        Pointer::Boxed => format!("Box<{dynamic}>"),
    })
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
    write!(
        output,
        "{{ let value = {value}; match __rawr_internal::object::Coerced::of(&value) {{ Some((inner, tag)) => match tag.as_str() {{ {arms}_ => Err(Error::TypeMismatch) }}, None => value.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take::<{rust}>(handle)) }} }}"
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
        // A program's number lives in no static.
        (Kind::Primitive(_), Pointer::Lasting) => {
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
            format!("({converted}).map(|made| -> {rust} {{ Box::new(made) }})")
        }
        // A static's place, of a body or of native code (D862).
        (Kind::Native(path), Pointer::Lasting) => format!(
            "__rawr_internal::place::statics::lasting::<{path}>(&inner).map(|lasting| -> {rust} {{ lasting }}).ok_or_else(|| Error::Value(\"borrowed value does not live long enough (E0597)\".to_owned()))"
        ),
        // The value the box holds, moved out of its slot.
        (Kind::Native(path), _) => format!(
            "inner.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take::<{path}>(handle)).map(|made| -> {rust} {{ Box::new(made) }})"
        ),
    })
}

/// The statements that make `argument_{local}`, the `&dyn Trait` input
/// `object` of the call's argument `index`: an object native code gave, a
/// static's place, a crate value lent from its slot, or a program's number
/// made into its Rust value for the call.
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
    writeln!(
        output,
        "    let argument_{local}_value = (**arguments.values.get_mut({index}).ok_or(Error::MissingArgument)?).take();
    #[allow(unused_variables)]
    let argument_{local}_made: {boxed};
{kept}    let argument_{local}_lasting: {lasting};
    let argument_{local}: {rust} = match __rawr_internal::object::Coerced::of(&argument_{local}_value) {{
        Some((inner, tag)) => match tag.as_str() {{ {arms}_ => return Err(Error::TypeMismatch) }},
        None => {{
            argument_{local}_lasting = argument_{local}_value.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take::<{lasting}>(handle))?;
            argument_{local}_lasting
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

/// The statements that lend `argument_{local}_made` (`render_held_slice`)
/// as `argument_{local}`, the `&[E]` of `element`s the call takes.
pub(super) fn lent_slice(local: usize, element: &convert::Type) -> Result<String, request::Error> {
    let mut lent = String::new();
    lent_element(element, "held", &mut lent)?;
    let rust = concrete_type(element).ok_or_else(|| request::drift("a trait object's type"))?;
    Ok(format!(
        "let argument_{local}_lent: Vec<{rust}> = argument_{local}_made.iter().map(|held| {lent}).collect();
    let argument_{local}: &[{rust}] = &argument_{local}_lent;"
    ))
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
/// what `held_element` made, lends.
fn lent_element(
    element: &convert::Type,
    held: &str,
    output: &mut String,
) -> Result<(), request::Error> {
    match element {
        convert::Type::Option(inner) => {
            let mut lent = String::new();
            lent_element(inner, "held", &mut lent)?;
            write!(output, "{held}.as_ref().map(|held| {lent})").map_err(request::unwritable)
        }
        convert::Type::Object(_) => write!(output, "{held}.get()").map_err(request::unwritable),
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
            };
            write!(arms, "{:?} => {made}, ", implementor.tag).map_err(request::unwritable)?;
        }
        Ok(())
    })?;
    write!(
        output,
        "{{ let value = {value}; match __rawr_internal::object::Coerced::of(&value) {{ Some((inner, tag)) => match tag.as_str() {{ {arms}_ => Err(Error::TypeMismatch) }}, None => value.try_cast_result::<NativeHandle>().map_err(|_| Error::TypeMismatch).and_then(|handle| arguments.arena.take::<{lasting}>(handle)).map({held}::Lasting) }} }}"
    )
    .map_err(request::unwritable)
}
