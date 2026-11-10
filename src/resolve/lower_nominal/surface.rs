//! A crate item's surface types: the types its signature, impls and
//! instances name, as the program sees them.

#[cfg(feature = "rust-import")]
use super::inference::Inference;
#[cfg(feature = "rust-import")]
use super::models::Models;
use super::ty::Ty;
#[cfg(feature = "rust-import")]
use super::ty::{Counter, Float, Int, Time};
#[cfg(feature = "rust-import")]
use super::{location, sequence, ImportedContext, ParseError};
#[cfg(feature = "rust-import")]
use crate::capability;
use crate::resolve::Error;
use crate::{ImmutableString, Span, INT};
use std::any::TypeId;

/// A lifetime's name as Rust writes it, with its `'`.
#[cfg(feature = "rust-import")]
pub(super) fn lifetime_text(name: &ImmutableString) -> Box<str> {
    if name.starts_with('\'') {
        name.as_str().into()
    } else {
        format!("'{name}").into()
    }
}

/// `ty`, a type a script impl's header or signature writes, as an
/// instance's surface type, with each of the impl's own parameters
/// `generics` a [`Type::Parameter`] at its index.
#[cfg(feature = "rust-import")]
pub(super) fn impl_surface_type(
    models: &Models,
    ty: &Ty,
    generics: &[ImmutableString],
) -> Option<rawr_import::classify::representability::convert::Type> {
    use rawr_import::classify::representability::convert::Type;
    let all = |types: &[Ty]| {
        types
            .iter()
            .map(|ty| impl_surface_type(models, ty, generics))
            .collect::<Option<Box<[_]>>>()
    };
    Some(match ty {
        Ty::Param(name) => Type::Parameter(
            u32::try_from(generics.iter().position(|generic| generic == name)?).ok()?,
        ),
        Ty::Unit => Type::Unit,
        Ty::Reference { mutable, target } => Type::Borrowed {
            mutable: *mutable,
            ty: Box::new(impl_surface_type(models, target, generics)?),
        },
        Ty::Option(element) => {
            Type::Option(Box::new(impl_surface_type(models, element, generics)?))
        }
        Ty::Result(ok, err) => Type::Result(
            Box::new(impl_surface_type(models, ok, generics)?),
            Box::new(impl_surface_type(models, err, generics)?),
        ),
        Ty::Array {
            element,
            kind: sequence::Kind::Vec,
        } => Type::Sequence {
            element: Box::new(impl_surface_type(models, element, generics)?),
            owned: true,
            length: None,
        },
        // `[T]`, as `visit_bytes(self, v: &[u8])` takes it.
        Ty::Array {
            element,
            kind: sequence::Kind::Slice,
        } => Type::Sequence {
            element: Box::new(impl_surface_type(models, element, generics)?),
            owned: false,
            length: None,
        },
        Ty::Tuple(members) => Type::Tuple(all(members)?),
        Ty::Script(definition, arguments) => Type::Script {
            path: models.script_path(*definition),
            arguments: all(arguments)?,
            holder: models.holder(*definition),
        },
        Ty::Host { proof, arguments } if proof.definition.identity().imported().is_some() => {
            Type::Nominal {
                definition: rawr_import::surface::identity::Definition::authenticated(
                    *proof.definition.identity().imported()?.as_bytes(),
                ),
                arguments: all(arguments)?,
            }
        }
        ty => instance_surface_type(&Inference::default(), models, ty)?,
    })
}

#[cfg(feature = "rust-import")]
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ImportedUse {
    pub(crate) source: crate::program::source::Id,
    pub(crate) path: ImmutableString,
    pub(crate) selector: rawr_import::surface::identity::Selector,
    pub(crate) types: Box<[rawr_import::classify::representability::convert::Type]>,
    /// Its const parameters' values, in declared order, as an array's
    /// length gives `N` (#1330).
    pub(crate) constants: Box<[rawr_import::classify::representability::convert::Constant]>,
    pub(crate) functions: Box<[ImportedFunctionUse]>,
    pub(crate) mappers: Box<[ImportedMapperUse]>,
    /// The item takes a script closure for its `F: FnOnce` parameter.
    pub(crate) callable: bool,
    /// The script functions that fill its callable parameters as slots
    /// (D594), as its instance keys them ([`Slot::served`]).
    ///
    /// [`Slot::served`]: rawr_import::surface::instance::Slot::served
    pub(crate) slots: Box<[rawr_import::surface::instance::Slot]>,
    /// The same slots by the functions that fill them.
    pub(crate) fillers: Box<[rawr_import::surface::instance::Slot]>,
    /// The proof-backed application this use calls, when it is one.
    pub(crate) application: Option<[u8; 32]>,
    pub(crate) span: Span,
}

#[cfg(feature = "rust-import")]
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ImportedFunctionUse {
    pub(crate) position: u32,
    /// A generic function's instance, in its declared parameter order.
    pub(crate) types: Box<[rawr_import::classify::representability::convert::Type]>,
    pub(crate) path: ImmutableString,
    pub(crate) selector: rawr_import::surface::identity::Selector,
    pub(crate) span: Span,
}

#[cfg(feature = "rust-import")]
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ImportedMapperUse {
    pub(crate) position: u32,
    pub(crate) seal: [u8; 16],
    pub(crate) span: Span,
}

#[cfg(feature = "rust-import")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ImportedFunctionProof {
    pub(super) item: crate::host::item::Definition,
    pub(super) owner: crate::host::facade::Dependency,
    pub(super) span: Span,
    /// A generic function's type parameters, in its declared order, with
    /// the types the use infers for them, as `window::close::<T>` passed
    /// where `and_then` takes `Fn(Id) -> Task<A>` takes `A`.
    pub(super) parameters: Box<[(ImmutableString, Ty)]>,
}

#[cfg(feature = "rust-import")]
pub(crate) fn imported_range(source: &str, span: Span) -> Option<(u32, u32)> {
    let start = u32::try_from(location::byte_at(source, span.start())?).ok()?;
    // A span of tokens a macro expansion made from one written token can run
    // past the written one's line, as a report's label reads it: the use is
    // where they were written.
    let end = location::byte_at(source, span.end())
        .and_then(|end| u32::try_from(end).ok())
        .unwrap_or(start);
    // A use a macro's expansion makes may end at the macro's own token, at
    // its call, before a token the caller passed in: it is the use's start.
    Some((start, end.max(start)))
}

/// Rust import paths in one call's lexical scope, each by its segments.
#[cfg(feature = "rust-import")]
pub(super) type ScopePaths = Box<[Box<[Box<str>]>]>;

#[cfg(feature = "rust-import")]
pub(super) fn imported_surface_type(
    inference: &Inference,
    models: &Models,
    ty: &Ty,
    operation: Span,
    imported: &mut ImportedContext<'_>,
) -> Result<rawr_import::classify::representability::convert::Type, Error> {
    use rawr_import::classify::representability::convert::{Builtin, Type};
    Ok(match inference.resolved_ref(ty) {
        Ty::Unit => Type::Unit,
        Ty::Bool => Type::Builtin(Builtin::Bool),
        Ty::Char => Type::Builtin(Builtin::Char),
        // `format_args!`, as `&format_args!(..)` fills a `&dyn Trait` input
        // read as `&impl Trait` (#1592).
        Ty::Arguments => Type::Builtin(Builtin::Arguments),
        // An unbound literal is `i32` or `f64`, as in Rust.
        Ty::Float(Float::F32) => Type::Builtin(Builtin::F32),
        Ty::Float(_) => Type::Builtin(Builtin::F64),
        Ty::Int(kind) => Type::Builtin(match kind {
            Int::I8 => Builtin::I8,
            Int::I16 => Builtin::I16,
            Int::I32 | Int::Var(_) => Builtin::I32,
            Int::I64 => Builtin::I64,
            Int::Isize => Builtin::Isize,
            Int::U8 => Builtin::U8,
            Int::U16 => Builtin::U16,
            Int::U32 => Builtin::U32,
            Int::U64 => Builtin::U64,
            Int::Usize => Builtin::Usize,
            Int::I128 | Int::U128 => {
                return Err(unsupported_type_argument(
                    if matches!(kind, Int::I128) {
                        "i128"
                    } else {
                        "u128"
                    }
                    .into(),
                    operation,
                ));
            }
        }),
        // `a..=b` of `i64`, or of literals rustc types from the call, is
        // Rawr's own range. One of another integer is a range of it, as
        // rustc fills a type parameter with it.
        Ty::InclusiveRange(kind) => match inference.resolve(&Ty::Int(*kind)) {
            Ty::Int(Int::I64 | Int::Var(_)) => Type::Builtin(Builtin::InclusiveRange),
            element => {
                imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
                Type::Nominal {
                    definition: rawr_import::surface::identity::Definition::range_inclusive(),
                    arguments: Box::new([imported_surface_type(
                        inference, models, &element, operation, imported,
                    )?]),
                }
            }
        },
        Ty::InclusiveOf(element) => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Nominal {
                definition: rawr_import::surface::identity::Definition::range_inclusive(),
                arguments: Box::new([imported_surface_type(
                    inference, models, element, operation, imported,
                )?]),
            }
        }
        Ty::Range(element) => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Range(Box::new(imported_surface_type(
                inference, models, element, operation, imported,
            )?))
        }
        Ty::String => Type::Builtin(Builtin::String),
        // A `&str`, as a string literal is: a crate's type parameter it
        // fills is `&'static str` in Rust, not `String`.
        Ty::Str => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Borrowed {
                mutable: false,
                ty: Box::new(Type::Builtin(Builtin::Str)),
            }
        }
        // `str` itself, as `F` of a `&F` a `&str` fills.
        Ty::UnsizedStr => Type::Builtin(Builtin::Str),
        // A script closure a crate value holds, as iced's `Map<W, F, A>`
        // from `Widget::map` does: the `Box<dyn Fn(..) -> ..>` the bridge
        // boxed it as (T417).
        Ty::Callable { inputs, output, .. }
        | Ty::Fn { inputs, output }
        | Ty::FnItem { inputs, output, .. }
        | Ty::FnChoice { inputs, output, .. } => {
            let mut converted = imported.entries(inputs.len() + 1)?;
            for input in inputs.iter() {
                converted.push(imported_surface_type(
                    inference, models, input, operation, imported,
                )?);
            }
            converted.push(imported_surface_type(
                inference, models, output, operation, imported,
            )?);
            Type::Nominal {
                definition: rawr_import::surface::identity::Definition::callable(),
                arguments: converted.into_boxed_slice(),
            }
        }
        Ty::ProfileMessage { payload, seal, .. } => {
            // The mapper is inline in the outer Type; its payload owns one
            // complete boxed Type in addition to any nested allocations.
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::ProfileMessage {
                mapper: *seal,
                payload: Box::new(imported_surface_type(
                    inference, models, payload, operation, imported,
                )?),
            }
        }
        Ty::Host { proof, arguments } => {
            let definition = host_surface_definition(&proof.definition)
                .ok_or_else(|| unsupported_type_argument(ty.name(models), operation))?;
            let mut converted = imported.entries(arguments.len())?;
            for argument in arguments.iter() {
                converted.push(imported_surface_type(
                    inference, models, argument, operation, imported,
                )?);
            }
            Type::Nominal {
                definition,
                arguments: converted.into_boxed_slice(),
            }
        }
        // A program's iterator a crate value holds, as itertools' `Product`
        // holds the two `cartesian_product` took: the native iterator that
        // pulls from it (`future::Items`).
        Ty::Iter(item, _) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::iterator(),
            arguments: Box::new([imported_surface_type(
                inference, models, item, operation, imported,
            )?]),
        },
        // A crate's trait object (D872): `dyn Trait` itself where a `&T`
        // takes it, as the `&dyn Trait` input a call reads as `&impl
        // Trait` is, and `Box<dyn Trait>`.
        Ty::Dyn(name, bindings) if crate::resolve::type_ref::is_native_trait(name) => {
            native_object(inference, models, name, bindings, operation, imported)?
        }
        Ty::Boxed(inner) | Ty::Counted(_, inner)
            if matches!(inference.resolved_ref(inner),
                Ty::Dyn(name, _) if crate::resolve::type_ref::is_native_trait(name)) =>
        {
            let Type::Object(object) =
                imported_surface_type(inference, models, inner, operation, imported)?
            else {
                return Err(unsupported_type_argument(ty.name(models), operation));
            };
            Type::Object(Box::new(object.behind(owning_pointer(ty))))
        }
        // A `Box<T>`: alloc's, which a crate's signature names (D711
        // step 5).
        Ty::Boxed(inner) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::boxed(),
            arguments: Box::new([imported_surface_type(
                inference, models, inner, operation, imported,
            )?]),
        },
        // A map or set, which crosses as std's of its kind (#777).
        Ty::Keyed(kind, members) => {
            let mut converted = imported.entries(members.len())?;
            for member in members.iter() {
                converted.push(imported_surface_type(
                    inference, models, member, operation, imported,
                )?);
            }
            Type::Nominal {
                definition: rawr_import::surface::identity::Definition::keyed(kind.name()),
                arguments: converted.into_boxed_slice(),
            }
        }
        // An `Rc<T>` or `Arc<T>` of a crate's type, which stands for the
        // crate's own pointer (#775).
        Ty::Counted(counter, inner) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::counted(
                *counter == Counter::Arc,
            ),
            arguments: Box::new([imported_surface_type(
                inference, models, inner, operation, imported,
            )?]),
        },
        Ty::Time(Time::Duration) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::duration(),
            arguments: Box::default(),
        },
        Ty::Time(Time::Instant) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::instant(),
            arguments: Box::default(),
        },
        // std's `Infallible`, iced's `Never`: the message of a view that
        // sends none, as `impl Widget<Never>` fixes it.
        Ty::ParseError(ParseError::Infallible) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::infallible(),
            arguments: Box::default(),
        },
        // A type argument `&[u8]` or `&str`, as `Reader<&'a [u8]>` names
        // (D558 tier 1).
        Ty::Reference { mutable: false, .. }
            if let Some(converted @ Type::Borrowed { .. }) =
                instance_surface_type(inference, models, ty) =>
        {
            imported.retain_checked(Some(2), Some(2 * std::mem::size_of::<Type>()))?;
            converted
        }
        // A tuple of Rust's own types is that tuple, as Freya's
        // `background((35, 35, 35))` takes `(u8, u8, u8)`, and so is one
        // holding a crate value (`instance_surface_type`).
        Ty::Tuple(members)
            if instance_surface_type(inference, models, ty)
                .is_some_and(|ty| !is_erased(&ty) || holds_crate_value(&ty)) =>
        {
            let mut converted = imported.entries(members.len())?;
            for member in members.iter() {
                converted.push(imported_surface_type(
                    inference, models, member, operation, imported,
                )?);
            }
            Type::Tuple(converted.into_boxed_slice())
        }
        // An array of Rust's own types is a `Vec` of them, as ratatui's
        // `List::new(rows)` takes `impl IntoIterator<Item: Into<ListItem>>`,
        // and so is one of script values (`instance_surface_type`).
        Ty::Array { element, .. }
            if instance_surface_type(inference, models, ty)
                .is_some_and(|ty| matches!(ty, Type::Sequence { .. })) =>
        {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Sequence {
                element: Box::new(imported_surface_type(
                    inference, models, element, operation, imported,
                )?),
                owned: true,
                length: None,
            }
        }
        // A type parameter no script value fixes, as iced's `Theme` that the
        // view's widget gives: rustc infers it where the bridge calls the
        // item (D594).
        Ty::Infer(_) => Type::Inferred,
        // A shared borrow of a script value, as `Canvas::new(self)` takes
        // `&Arc` for its `P: Program`: the proxy shares the value's handle,
        // so it is the borrow, and the impls the program writes for the
        // script type are the proxy's (D594).
        Ty::Reference {
            mutable: false,
            target,
        } if matches!(inference.resolved_ref(target), Ty::Script(..)) => {
            imported_surface_type(inference, models, target, operation, imported)?
        }
        // A script type is its proxy in the runner, named as the script
        // names it, so Rust's bounds, `TypeId` and `type_name` are its own
        // (D594).
        Ty::Script(definition, arguments) => {
            let mut converted = imported.entries(arguments.len())?;
            for argument in arguments.iter() {
                converted.push(imported_surface_type(
                    inference, models, argument, operation, imported,
                )?);
            }
            Type::Script {
                path: models.script_path(*definition),
                arguments: converted.into_boxed_slice(),
                holder: models.holder(*definition),
            }
        }
        // An `Option` or a `Result` holding a crate value is Rust's own
        // (`instance_surface_type`).
        Ty::Option(element)
            if instance_surface_type(inference, models, ty)
                .is_some_and(|ty| matches!(ty, Type::Option(_))) =>
        {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Option(Box::new(imported_surface_type(
                inference, models, element, operation, imported,
            )?))
        }
        Ty::Result(ok, err)
            if instance_surface_type(inference, models, ty)
                .is_some_and(|ty| matches!(ty, Type::Result(..))) =>
        {
            imported.retain_checked(Some(2), Some(2 * std::mem::size_of::<Type>()))?;
            Type::Result(
                Box::new(imported_surface_type(
                    inference, models, ok, operation, imported,
                )?),
                Box::new(imported_surface_type(
                    inference, models, err, operation, imported,
                )?),
            )
        }
        ty if erased(ty) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::value(),
            arguments: Box::default(),
        },
        // A type still unknown here is one rustc can't infer either.
        Ty::Unknown => {
            return Err(Error::InvalidHostOperation {
                item: "_".into(),
                reason: "type annotations needed: a crate's type argument is unknown (E0282)"
                    .into(),
                operation,
            });
        }
        other => return Err(unsupported_type_argument(other.name(models), operation)),
    })
}

/// The pointer a crate's trait object owned by `ty`, a `Box`, `Rc` or
/// `Arc` of it, is behind (#1304).
#[cfg(feature = "rust-import")]
fn owning_pointer(ty: &Ty) -> rawr_import::classify::representability::object::Pointer {
    use rawr_import::classify::representability::object::Pointer;
    match ty {
        Ty::Counted(Counter::Rc, _) => Pointer::Rc,
        Ty::Counted(Counter::Arc, _) => Pointer::Arc,
        _ => Pointer::Boxed,
    }
}

/// `dyn Trait<..>` of the crate trait `name` with `bindings`, as a crate's
/// type argument (D872): the trait by its identity, at its positional
/// arguments. One that binds an associated type isn't one yet.
#[cfg(feature = "rust-import")]
fn native_object(
    inference: &Inference,
    models: &Models,
    name: &ImmutableString,
    bindings: &[(ImmutableString, Ty)],
    operation: Span,
    imported: &mut ImportedContext<'_>,
) -> Result<rawr_import::classify::representability::convert::Type, Error> {
    let unsupported = || unsupported_type_argument(format!("dyn {name}").into(), operation);
    let identity = native_object_identity(models, name, bindings).ok_or_else(unsupported)?;
    let positional = super::dyn_arguments(bindings);
    let mut arguments = imported.entries(positional.len())?;
    for argument in &positional {
        arguments.push(imported_surface_type(
            inference, models, argument, operation, imported,
        )?);
    }
    Ok(shared_object(identity, arguments.into_boxed_slice()))
}

/// The identity of the crate trait `name` that an object of it with
/// `bindings` names, when the object binds no associated type (D872).
#[cfg(feature = "rust-import")]
fn native_object_identity(
    models: &Models,
    name: &str,
    bindings: &[(ImmutableString, Ty)],
) -> Option<[u8; 32]> {
    if bindings
        .iter()
        .any(|(binding, _)| binding.parse::<usize>().is_err())
    {
        return None;
    }
    let path = crate::host::path::Path::parse(name).ok()?;
    Some(models.host.trait_header(&path)?.identity)
}

/// `dyn Trait<arguments>` of the crate trait `identity`, where a `&T`
/// takes it (D872).
#[cfg(feature = "rust-import")]
fn shared_object(
    identity: [u8; 32],
    arguments: Box<[rawr_import::classify::representability::convert::Type]>,
) -> rawr_import::classify::representability::convert::Type {
    use rawr_import::classify::representability::{convert, object};
    convert::Type::Object(Box::new(object::Object::new(
        object::Pointer::Shared,
        rawr_import::surface::identity::Definition::authenticated(identity),
        arguments,
        Vec::new(),
    )))
}

/// A host type's identity in the runner: an imported crate's own, or one
/// of std's types Rawr's std holds as its Rust value, as `PathBuf` is
/// std's native `PathBuf` (D683.3).
#[cfg(feature = "rust-import")]
pub(super) fn host_surface_definition(
    definition: &crate::host::type_def::Definition,
) -> Option<rawr_import::surface::identity::Definition> {
    use rawr_import::surface::identity::Definition;
    if let Some(identity) = definition.identity().imported() {
        return Some(Definition::authenticated(*identity.as_bytes()));
    }
    let path = definition.path();
    let std = Definition::std(
        path.strip_prefix(crate::std_facade::ROOT)?
            .strip_prefix("::")?,
    );
    Definition::std_path(std.as_bytes())
        .is_some()
        .then_some(std)
}

/// A type the program gives a crate's generic item that Rawr can't pass
/// to Rust yet, named as the program wrote it.
#[cfg(feature = "rust-import")]
pub(super) fn unsupported_type_argument(ty: ImmutableString, operation: Span) -> Error {
    Error::InvalidHostOperation {
        reason: unsupported_type_argument_reason(&ty),
        item: ty,
        operation,
    }
}

/// Why a crate's generic item can't take `ty` as its type argument.
#[cfg(feature = "rust-import")]
pub(super) fn unsupported_type_argument_reason(ty: &str) -> ImmutableString {
    format!("`{ty}` is not supported in Rawr yet as a crate's type argument").into()
}

/// A value of type `ty` that the crate item `item` takes or gives, which
/// Rawr can't pass to Rust yet.
pub(super) fn unpassable(ty: ImmutableString, item: &str, operation: Span) -> Error {
    Error::InvalidHostOperation {
        item: item.into(),
        reason: format!("a `{ty}` passed to or from it is not supported in Rawr yet").into(),
        operation,
    }
}

/// A method named without its call, as rustc says it (E0615).
pub(super) fn method_without_call(member: &str) -> ImmutableString {
    format!("attempted to take value of method `{member}`; call it with `{member}(..)` (E0615)")
        .into()
}

/// A crate item whose signature names a type, `ty` when it has a path, that
/// the program's build doesn't reach, at the `operation` that uses the item.
/// Overload selection tries each form and passes over one that fails this
/// way.
pub(super) fn unnamed_signature_type(item: &str, operation: Span, ty: Option<&str>) -> Error {
    Error::InvalidHostOperation {
        item: item.into(),
        reason: match ty {
            Some(ty) => format!("its signature names `{ty}`, which is not supported in Rawr yet"),
            None => "its signature names a type that is not supported in Rawr yet".into(),
        }
        .into(),
        operation,
    }
}

/// What the program does with a crate's struct or enum, in Rust's words.
#[cfg(feature = "rust-import")]
pub(super) const fn structural_use(kind: crate::host::structural::Kind) -> &'static str {
    use crate::host::structural::Kind;
    match kind {
        Kind::Constructor => "constructing it",
        Kind::Field { .. } => "reading this field",
        Kind::Variant => "constructing this variant",
        Kind::Match
        | Kind::Test
        | Kind::Take
        | Kind::Lend
        | Kind::LendMut
        | Kind::Read { .. }
        | Kind::Borrow { .. } => "matching on this variant",
        Kind::Store => "assigning to this field",
    }
}

/// A type the crate fixes, as the associated type `<S as Future>::Output`
/// that the type filling `S` binds: Rust's own `Option`, `Result`, tuple or
/// `Vec` of what it holds, never `rawr::Value`, since native code gives or
/// takes it at that type. Anything else is as `imported_surface_type`
/// converts it.
#[cfg(feature = "rust-import")]
pub(super) fn fixed_surface_type(
    inference: &Inference,
    models: &Models,
    ty: &Ty,
    operation: Span,
    imported: &mut ImportedContext<'_>,
) -> Result<rawr_import::classify::representability::convert::Type, Error> {
    use rawr_import::classify::representability::convert::Type;
    Ok(match inference.resolved_ref(ty) {
        Ty::Option(element) => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Option(Box::new(fixed_surface_type(
                inference, models, element, operation, imported,
            )?))
        }
        Ty::Result(ok, err) => {
            imported.retain_checked(Some(2), Some(2 * std::mem::size_of::<Type>()))?;
            Type::Result(
                Box::new(fixed_surface_type(
                    inference, models, ok, operation, imported,
                )?),
                Box::new(fixed_surface_type(
                    inference, models, err, operation, imported,
                )?),
            )
        }
        Ty::Tuple(members) if !members.is_empty() => {
            let mut converted = imported.entries(members.len())?;
            for member in members.iter() {
                converted.push(fixed_surface_type(
                    inference, models, member, operation, imported,
                )?);
            }
            Type::Tuple(converted.into_boxed_slice())
        }
        Ty::Array { element, .. } => {
            imported.retain_checked(Some(1), Some(std::mem::size_of::<Type>()))?;
            Type::Sequence {
                element: Box::new(fixed_surface_type(
                    inference, models, element, operation, imported,
                )?),
                owned: true,
                length: None,
            }
        }
        _ => imported_surface_type(inference, models, ty, operation, imported)?,
    })
}

/// An instance type as `fixed_surface_type` converts it, without
/// allocation accounting: the key of a call's instance whose parameter an
/// associated type reads.
#[cfg(feature = "rust-import")]
pub(super) fn fixed_instance_surface_type(
    inference: &Inference,
    models: &Models,
    ty: &Ty,
) -> Option<rawr_import::classify::representability::convert::Type> {
    use rawr_import::classify::representability::convert::Type;
    let fixed = |ty| fixed_instance_surface_type(inference, models, ty);
    Some(match inference.resolved_ref(ty) {
        Ty::Option(element) => Type::Option(Box::new(fixed(element)?)),
        Ty::Result(ok, err) => Type::Result(Box::new(fixed(ok)?), Box::new(fixed(err)?)),
        Ty::Tuple(members) if !members.is_empty() => {
            Type::Tuple(members.iter().map(fixed).collect::<Option<_>>()?)
        }
        Ty::Array { element, .. } => Type::Sequence {
            element: Box::new(fixed(element)?),
            owned: true,
            length: None,
        },
        _ => instance_surface_type(inference, models, ty)?,
    })
}

/// The requirements that are an imported item's instance, in the order its
/// instance names them: the item's declared order, or the requirements' own
/// when a declared parameter has none. Its const lengths are its constants,
/// never its types (#1330).
#[cfg(feature = "rust-import")]
pub(super) fn instance_parameters<'r>(
    item: &crate::host::item::Definition,
    requirements: &'r [(ImmutableString, Ty, capability::Set)],
) -> Vec<&'r (ImmutableString, Ty, capability::Set)> {
    let lengths = item
        .signature()
        .declared_length_parameters()
        .collect::<Vec<_>>();
    let typed = requirements
        .iter()
        .filter(|(name, _, _)| !lengths.contains(name))
        .collect::<Vec<_>>();
    let ordered = item
        .signature()
        .declared_type_parameters()
        .filter_map(|name| {
            requirements
                .iter()
                .find(|(candidate, _, _)| *candidate == name)
        })
        .collect::<Vec<_>>();
    if ordered.len() == typed.len() {
        ordered
    } else {
        typed
    }
}

/// Whether an associated type a relation reads names `parameter`, as
/// `S::Output` of the type filling `S` does: that type is the crate's,
/// Rust's own (`fixed_surface_type`).
#[cfg(feature = "rust-import")]
pub(super) fn fixed_parameter(item: &crate::host::item::Definition, parameter: &str) -> bool {
    item.signature().relations().iter().any(|relation| {
        relation.associated().is_some()
            && matches!(relation.arguments(),
                [crate::host::signature::Pattern::Parameter(name)] if name == parameter)
    })
}

/// A type argument of a crate's generic item that keys none of its
/// instances, in rustc's words: one it can't infer, or one Rawr can't pass
/// to Rust yet.
#[cfg(feature = "rust-import")]
pub(super) fn keyless_type_argument(
    inference: &Inference,
    models: &Models,
    item: &crate::host::item::Definition,
    parameter: &str,
    ty: &Ty,
    operation: Span,
) -> Error {
    let reason = match inference.resolve(ty) {
        Ty::Unknown => "type annotations needed: a crate's type argument is unknown (E0282)".into(),
        ty => unsupported_type_argument_reason(&ty.name(models)),
    };
    Error::InvalidHostOperation {
        item: format!("{}'s `{parameter}`", item.path()).into(),
        reason,
        operation,
    }
}

/// A const argument of a crate's generic item rustc can't infer either.
#[cfg(feature = "rust-import")]
pub(super) fn unknown_const_argument(
    item: &crate::host::item::Definition,
    operation: Span,
) -> Error {
    Error::InvalidHostOperation {
        item: item.path().into(),
        reason: "type annotations needed: a const argument of it is unknown (E0282)".into(),
        operation,
    }
}

/// A value type Rust can't name in the runner, as a script struct or a
/// tuple holding one: a generic Rust item takes it as `rawr::Value`
/// (D513.2), which carries it as it is.
#[cfg(feature = "rust-import")]
pub(super) fn erased(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Script(..)
            | Ty::Array { .. }
            | Ty::Tuple(_)
            | Ty::Option(_)
            | Ty::Result(..)
            | Ty::Keyed(..)
            | Ty::Boxed(..)
            | Ty::Counted(..)
            | Ty::Weak(..)
            | Ty::Reverse(_)
            | Ty::Cell(..)
            | Ty::Ordering
            | Ty::Time(Time::Instant)
    )
}

/// An instance's array as the Rust array of `length`, where a bound's case
/// fixes it (`related_array_length`).
#[cfg(feature = "rust-import")]
pub(super) fn fix_array_length(
    surface: &mut rawr_import::classify::representability::convert::Type,
    length: Option<u32>,
) {
    if let rawr_import::classify::representability::convert::Type::Sequence {
        owned: true,
        length: fixed @ None,
        ..
    } = surface
    {
        *fixed = length;
    }
}

/// The const parameter whose length an input `[T; N]` or `&[T; N]` takes.
#[cfg(feature = "rust-import")]
pub(super) fn length_parameter(ty: &crate::host::type_ref::Type) -> Option<&ImmutableString> {
    match ty.shape() {
        crate::host::type_ref::Shape::Array {
            sequence: crate::host::type_ref::Sequence::Length(name),
            ..
        } => Some(name),
        crate::host::type_ref::Shape::Reference { target, .. } => length_parameter(target),
        _ => None,
    }
}

/// The length of an argument's array, or of the array it borrows.
#[cfg(feature = "rust-import")]
pub(super) fn array_length(ty: &Ty) -> Option<usize> {
    match ty {
        Ty::Array {
            kind: sequence::Kind::Array(length),
            ..
        } => Some(*length),
        Ty::Reference { target, .. } => array_length(target),
        _ => None,
    }
}

/// `T` of a crate's `RangeInclusive<T>` of a type parameter.
pub(super) fn range_inclusive_element(
    ty: &crate::host::type_ref::Type,
) -> Option<&crate::host::type_ref::Type> {
    match ty.shape() {
        crate::host::type_ref::Shape::RangeInclusive(element) => Some(element),
        _ => None,
    }
}

/// An instance type as `imported_surface_type` converts it, without
/// allocation accounting: the key of a call's instance.
#[cfg(feature = "rust-import")]
pub(super) fn instance_surface_type(
    inference: &Inference,
    models: &Models,
    ty: &Ty,
) -> Option<rawr_import::classify::representability::convert::Type> {
    use rawr_import::classify::representability::convert::{Builtin, Type};
    Some(match inference.resolved_ref(ty) {
        Ty::Unit => Type::Unit,
        Ty::Bool => Type::Builtin(Builtin::Bool),
        Ty::Char => Type::Builtin(Builtin::Char),
        Ty::Arguments => Type::Builtin(Builtin::Arguments),
        Ty::Float(Float::F32) => Type::Builtin(Builtin::F32),
        Ty::Float(_) => Type::Builtin(Builtin::F64),
        Ty::Int(kind) => Type::Builtin(match kind {
            Int::I8 => Builtin::I8,
            Int::I16 => Builtin::I16,
            Int::I32 | Int::Var(_) => Builtin::I32,
            Int::I64 => Builtin::I64,
            Int::Isize => Builtin::Isize,
            Int::U8 => Builtin::U8,
            Int::U16 => Builtin::U16,
            Int::U32 => Builtin::U32,
            Int::U64 => Builtin::U64,
            Int::Usize => Builtin::Usize,
            Int::I128 | Int::U128 => return None,
        }),
        Ty::InclusiveRange(kind) => match inference.resolve(&Ty::Int(*kind)) {
            Ty::Int(Int::I64 | Int::Var(_)) => Type::Builtin(Builtin::InclusiveRange),
            element => Type::Nominal {
                definition: rawr_import::surface::identity::Definition::range_inclusive(),
                arguments: Box::new([instance_surface_type(inference, models, &element)?]),
            },
        },
        Ty::InclusiveOf(element) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::range_inclusive(),
            arguments: Box::new([instance_surface_type(inference, models, element)?]),
        },
        Ty::Range(element) => {
            Type::Range(Box::new(instance_surface_type(inference, models, element)?))
        }
        Ty::String => Type::Builtin(Builtin::String),
        Ty::Str => Type::Borrowed {
            mutable: false,
            ty: Box::new(Type::Builtin(Builtin::Str)),
        },
        // The unsized `str` a literal fills `F` of `&'f F` with, as jiff's
        // `strftime` takes it: the key of the `strftime::<str>` adapter, as
        // `imported_surface_type` names it, never the item's other
        // instances' shared binding.
        Ty::UnsizedStr => Type::Builtin(Builtin::Str),
        Ty::ProfileMessage { payload, seal, .. } => Type::ProfileMessage {
            mapper: *seal,
            payload: Box::new(instance_surface_type(inference, models, payload)?),
        },
        Ty::Host { proof, arguments } => Type::Nominal {
            definition: host_surface_definition(&proof.definition)?,
            arguments: arguments
                .iter()
                .map(|argument| instance_surface_type(inference, models, argument))
                .collect::<Option<_>>()?,
        },
        // A crate's trait object, as `imported_surface_type` has it (D872).
        Ty::Dyn(name, bindings) if crate::resolve::type_ref::is_native_trait(name) => {
            shared_object(
                native_object_identity(models, name, bindings)?,
                super::dyn_arguments(bindings)
                    .iter()
                    .map(|argument| instance_surface_type(inference, models, argument))
                    .collect::<Option<_>>()?,
            )
        }
        Ty::Boxed(inner) | Ty::Counted(_, inner)
            if matches!(inference.resolved_ref(inner),
                Ty::Dyn(name, _) if crate::resolve::type_ref::is_native_trait(name)) =>
        {
            let Type::Object(object) = instance_surface_type(inference, models, inner)? else {
                return None;
            };
            Type::Object(Box::new(object.behind(owning_pointer(ty))))
        }
        Ty::Boxed(inner) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::boxed(),
            arguments: Box::new([instance_surface_type(inference, models, inner)?]),
        },
        // A program's iterator a crate value holds (`future::Items`).
        Ty::Iter(item, _) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::iterator(),
            arguments: Box::new([instance_surface_type(inference, models, item)?]),
        },
        Ty::Counted(counter, inner) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::counted(
                *counter == Counter::Arc,
            ),
            arguments: Box::new([instance_surface_type(inference, models, inner)?]),
        },
        Ty::Keyed(kind, members) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::keyed(kind.name()),
            arguments: members
                .iter()
                .map(|member| instance_surface_type(inference, models, member))
                .collect::<Option<_>>()?,
        },
        Ty::Time(Time::Duration) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::duration(),
            arguments: Box::default(),
        },
        Ty::Time(Time::Instant) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::instant(),
            arguments: Box::default(),
        },
        Ty::ParseError(ParseError::Infallible) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::infallible(),
            arguments: Box::default(),
        },
        // A script closure a crate value holds, as `Map<W, F, A>` does: the
        // boxed closure `imported_surface_type` names, so a call on the
        // value keys the adapter generated for it (T417).
        Ty::Callable { inputs, output, .. }
        | Ty::Fn { inputs, output }
        | Ty::FnItem { inputs, output, .. }
        | Ty::FnChoice { inputs, output, .. } => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::callable(),
            arguments: inputs
                .iter()
                .chain([&**output])
                .map(|ty| instance_surface_type(inference, models, ty))
                .collect::<Option<_>>()?,
        },
        // A shared borrow of a script value is its proxy, which shares it.
        Ty::Reference {
            mutable: false,
            target,
        } if matches!(inference.resolved_ref(target), Ty::Script(..)) => {
            instance_surface_type(inference, models, target)?
        }
        // A type argument `&[u8]` or `&str`, as `Reader<&'a [u8]>` names
        // (D558 tier 1).
        Ty::Reference {
            mutable: false,
            target,
        } => Type::Borrowed {
            mutable: false,
            ty: Box::new(match inference.resolve(target) {
                Ty::Array { element, .. } => Type::Sequence {
                    element: Box::new(instance_surface_type(inference, models, &element)?),
                    owned: false,
                    length: None,
                },
                Ty::Str | Ty::String => Type::Builtin(Builtin::Str),
                // A crate value the program holds, as `Some(&self.backend)`
                // fills iced's `pick_list`'s `V: Borrow<T>`.
                target @ Ty::Host { .. } => instance_surface_type(inference, models, &target)?,
                _ => return None,
            }),
        },
        // A tuple of Rust's own types is that tuple. So is one that holds
        // a crate value beside script values, as iced's `new` gives
        // `(Self, Task<Message>)`: native code takes the crate value as
        // itself and each script value as its proxy, where a `rawr::Value`
        // would carry a handle it can't use.
        Ty::Tuple(members)
            if let Some(converted) = members
                .iter()
                .map(|member| instance_surface_type(inference, models, member))
                .collect::<Option<Box<[_]>>>()
                .filter(|converted| {
                    !converted.iter().any(is_erased) || converted.iter().any(holds_crate_value)
                }) =>
        {
            Type::Tuple(converted)
        }
        // An `Option` or a `Result` that holds a crate value is Rust's own,
        // as iced's `font::list()` gives `Task<Result<Vec<Family>, Error>>`
        // and `.map(Result::ok)` takes it: native code holds the crate
        // value as itself.
        Ty::Option(element)
            if let Some(element) =
                instance_surface_type(inference, models, element).filter(holds_crate_value) =>
        {
            Type::Option(Box::new(element))
        }
        Ty::Result(ok, err)
            if let (Some(ok), Some(err)) = (
                instance_surface_type(inference, models, ok),
                instance_surface_type(inference, models, err),
            ) && (holds_crate_value(&ok) || holds_crate_value(&err)) =>
        {
            Type::Result(Box::new(ok), Box::new(err))
        }
        // An array of Rust's own types, or of a script type's proxies as a
        // slice of them is, is a `Vec` of them, as serde's
        // `serialize_field` takes `&Vec<Task>` for its `T: Serialize`.
        Ty::Array { element, .. }
            if let Some(element) =
                instance_surface_type(inference, models, element).filter(|element| {
                    !is_erased(element) || matches!(element, Type::Script { .. })
                }) =>
        {
            Type::Sequence {
                element: Box::new(element),
                owned: true,
                length: None,
            }
        }
        Ty::Infer(_) => Type::Inferred,
        Ty::Script(definition, arguments) => Type::Script {
            path: models.script_path(*definition),
            arguments: arguments
                .iter()
                .map(|argument| instance_surface_type(inference, models, argument))
                .collect::<Option<_>>()?,
            holder: models.holder(*definition),
        },
        ty if erased(ty) => Type::Nominal {
            definition: rawr_import::surface::identity::Definition::value(),
            arguments: Box::default(),
        },
        _ => return None,
    })
}

/// A `Range<T>` value's runtime type: a `Range<f64>` of floats, else a
/// `Range<i64>` of integers or chars' code points.
pub(super) fn range_type_id(element: &Ty) -> TypeId {
    if matches!(element, Ty::Float(_)) {
        TypeId::of::<std::ops::Range<crate::FLOAT>>()
    } else {
        TypeId::of::<std::ops::Range<INT>>()
    }
}

/// Whether Rust holds some part of `ty` as a bare script value. Unlike
/// [`is_erased`], a script type counts as its proxy, a Rust type of its own.
#[cfg(feature = "rust-import")]
pub(super) fn holds_value(ty: &rawr_import::classify::representability::convert::Type) -> bool {
    use rawr_import::classify::representability::convert::Type;
    match ty {
        Type::Nominal {
            definition,
            arguments,
        } => {
            *definition == rawr_import::surface::identity::Definition::value()
                || arguments.iter().any(holds_value)
        }
        Type::Script { arguments, .. } | Type::Tuple(arguments) => {
            arguments.iter().any(holds_value)
        }
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. }
        | Type::ConstArray { element, .. } => holds_value(element),
        Type::Result(ok, err) => holds_value(ok) || holds_value(err),
        Type::ProfileMessage { payload, .. } => holds_value(payload),
        Type::Object(object) => object.types().any(holds_value),
        Type::Inferred
        | Type::Unit
        | Type::Never
        | Type::Builtin(_)
        | Type::FunctionItem { .. }
        | Type::Parameter(_) => false,
    }
}

/// Whether an instance type holds a crate's own value, which native code
/// holds as itself and a `rawr::Value` could only carry as a handle.
#[cfg(feature = "rust-import")]
pub(super) fn holds_crate_value(
    ty: &rawr_import::classify::representability::convert::Type,
) -> bool {
    use rawr_import::classify::representability::convert::Type;
    match ty {
        Type::Nominal { definition, .. } => {
            *definition != rawr_import::surface::identity::Definition::value()
        }
        Type::Tuple(members) => members.iter().any(holds_crate_value),
        Type::Option(element)
        | Type::Sequence { element, .. }
        | Type::ConstArray { element, .. } => holds_crate_value(element),
        Type::Result(ok, err) => holds_crate_value(ok) || holds_crate_value(err),
        // An object native code gave is held as itself (D872).
        Type::Object(object) => {
            object.pointer() != rawr_import::classify::representability::object::Pointer::Shared
        }
        Type::Script { .. }
        | Type::Borrowed { .. }
        | Type::Range(_)
        | Type::ProfileMessage { .. }
        | Type::Inferred
        | Type::Unit
        | Type::Never
        | Type::Builtin(_)
        | Type::FunctionItem { .. }
        | Type::Parameter(_) => false,
    }
}

/// Whether an instance type holds a value Rust takes as `rawr::Value`.
#[cfg(feature = "rust-import")]
pub(super) fn is_erased(ty: &rawr_import::classify::representability::convert::Type) -> bool {
    use rawr_import::classify::representability::convert::Type;
    match ty {
        Type::Nominal {
            definition,
            arguments,
        } => {
            *definition == rawr_import::surface::identity::Definition::value()
                || arguments.iter().any(is_erased)
        }
        Type::Tuple(members) => members.iter().any(is_erased),
        // A script type's proxy holds a script value (D594).
        Type::Script { .. } => true,
        Type::Option(element)
        | Type::Borrowed { ty: element, .. }
        | Type::Range(element)
        | Type::Sequence { element, .. }
        | Type::ConstArray { element, .. } => is_erased(element),
        Type::Result(ok, err) => is_erased(ok) || is_erased(err),
        Type::ProfileMessage { payload, .. } => is_erased(payload),
        Type::Object(object) => object.types().any(is_erased),
        Type::Inferred => false,
        Type::Unit
        | Type::Never
        | Type::Builtin(_)
        | Type::FunctionItem { .. }
        | Type::Parameter(_) => false,
    }
}
