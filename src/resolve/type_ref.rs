//! Resolved item-header type references.

use super::Error;
use crate::item;
use crate::{host, type_def, ImmutableString, Span};
use std::collections::BTreeMap;

// Large through a path's `Target::Host`; see `Target` for why it stays unboxed.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Type {
    Unit(Span),
    Tuple {
        members: Box<[Type]>,
        span: Span,
    },
    Path(Path),
    Reference {
        mutable: bool,
        lifetime: Option<Span>,
        target: Box<Type>,
        span: Span,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Path {
    pub name: ImmutableString,
    pub target: Target,
    pub arguments: Box<[Argument]>,
    pub span: Span,
}

// Boxing `Host` would rewrite its ~70 construction and match sites; header
// types are built while resolving, not per call.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Target {
    Builtin,
    Host {
        definition: host::type_def::Definition,
        owner: Option<host::facade::Dependency>,
    },
    Script {
        definition: type_def::Id,
        identity: type_def::Identity,
    },
    /// A generic type parameter of the enclosing item: any type, erased at
    /// runtime.
    Parameter,
    /// The associated type named by the path's name of this type, which is a
    /// generic parameter or a script type: `T::Output`, `Self::Output`, and
    /// the trait declaring it: written (`<T as Shape>::Output`) or found
    /// once every item is resolved.
    Projection(Box<Type>, Option<ImmutableString>),
    /// A type alias, which a header reads as its target.
    Alias(Alias),
}

/// A type alias's parameters and its target, resolved once in the module
/// that declares it. The target is `None` until it resolves.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Alias {
    pub params: Box<[ImmutableString]>,
    pub target: Option<Box<Type>>,
}

impl Alias {
    /// The alias's target with `arguments` for its parameters, written at
    /// `span`.
    pub(crate) fn expand(&self, arguments: &[Type], span: Span) -> Option<Type> {
        let mut ty = (**self.target.as_ref()?).clone();
        substitute(&mut ty, &self.params, arguments, span);
        Some(ty)
    }
}

/// Replace each parameter of `params` in `ty` with its argument, and give
/// every node the span where the alias is written.
fn substitute(ty: &mut Type, params: &[ImmutableString], arguments: &[Type], at: Span) {
    match ty {
        Type::Unit(span) => *span = at,
        Type::Tuple { members, span } => {
            *span = at;
            for member in members.iter_mut() {
                substitute(member, params, arguments, at);
            }
        }
        Type::Reference { target, span, .. } => {
            *span = at;
            substitute(target, params, arguments, at);
        }
        Type::Path(path) => {
            if path.target == Target::Parameter
                && path.arguments.is_empty()
                && let Some(index) = params.iter().position(|param| *param == path.name)
                && let Some(argument) = arguments.get(index)
            {
                *ty = argument.clone();
                return;
            }
            path.span = at;
            if let Target::Projection(base, _) = &mut path.target {
                substitute(base, params, arguments, at);
            }
            for argument in path.arguments.iter_mut() {
                match argument {
                    Argument::Type(ty) => substitute(ty, params, arguments, at),
                    Argument::Lifetime(span) => *span = at,
                }
            }
        }
    }
}

// Large through a path's `Target::Host`; see `Target` for why it stays unboxed.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Argument {
    Type(Type),
    Lifetime(Span),
}

/// The builtin path name of `dyn Trait` is `dyn Trait`: a trait object,
/// which at runtime is the value itself.
pub(crate) const DYN_PREFIX: &str = "dyn ";

pub(super) fn resolve(
    parsed: item::type_ref::Type,
    types: &BTreeMap<ImmutableString, Target>,
    host: &host::Registry,
    #[cfg(feature = "rust-import")] native: Option<super::native::Site<'_>>,
) -> Result<Type, Error> {
    match parsed {
        item::type_ref::Type::Unit(span) => Ok(Type::Unit(span)),
        item::type_ref::Type::Tuple { members, span } => Ok(Type::Tuple {
            members: members
                .into_vec()
                .into_iter()
                .map(|member| {
                    resolve(
                        member,
                        types,
                        host,
                        #[cfg(feature = "rust-import")]
                        native,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
            span,
        }),
        item::type_ref::Type::Path(path) => {
            let written = path
                .segments
                .iter()
                .map(|segment| segment.name.text.as_str())
                .collect::<Vec<_>>()
                .join("::");
            // `PhantomData<T>` holds nothing: the unit type.
            if super::std_phantom(&written) && !types.contains_key("PhantomData") {
                return Ok(Type::Unit(path.span));
            }
            if let Some(Target::Alias(alias)) = types.get(written.as_str()) {
                let span = path.span;
                let mut arguments = Vec::new();
                for segment in path.segments {
                    for argument in segment.arguments {
                        if let item::type_ref::Argument::Type(ty) = argument {
                            arguments.push(resolve(
                                ty,
                                types,
                                host,
                                #[cfg(feature = "rust-import")]
                                native,
                            )?);
                        }
                    }
                }
                if arguments.len() != alias.params.len() {
                    return Err(Error::InvalidTypeArguments {
                        ty: written.into(),
                        expected: alias.params.len(),
                        actual: arguments.len(),
                        reference: span,
                    });
                }
                return alias.expand(&arguments, span).ok_or(Error::UnknownType {
                    name: written.into(),
                    reference: span,
                });
            }
            resolve_path(
                path,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )
            .map(Type::Path)
        }
        // `&str` is Rawr's `String`, which every string value is. The path
        // keeps the reference's lifetime, at its `&` when none is written,
        // which marks the text borrowed (`Type::is_str`): a `&str` is
        // `Copy`, where a `String` moves.
        item::type_ref::Type::Reference {
            mutable: false,
            lifetime,
            target,
            span,
        } if matches!(&*target, item::type_ref::Type::Path(path)
            if path.segments.len() == 1
                && path.segments[0].name.text == "str"
                && path.segments[0].arguments.is_empty()) =>
        {
            Ok(Type::Path(Path {
                name: "String".into(),
                target: Target::Builtin,
                arguments: Box::new([Argument::Lifetime(
                    lifetime.map_or(span, |lifetime| lifetime.span),
                )]),
                span,
            }))
        }
        item::type_ref::Type::Reference {
            mutable,
            lifetime,
            target,
            span,
        } => Ok(Type::Reference {
            mutable,
            lifetime: lifetime.map(|lifetime| lifetime.span),
            target: Box::new(resolve(
                *target,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?),
            span,
        }),
    }
}

fn resolve_path(
    parsed: item::type_ref::Path,
    types: &BTreeMap<ImmutableString, Target>,
    host: &host::Registry,
    #[cfg(feature = "rust-import")] native: Option<super::native::Site<'_>>,
) -> Result<Path, Error> {
    let span = parsed.span;
    // `::image::ImageError` names the crate `image`, never a type or
    // generic parameter of this program.
    if parsed.global
        && let Some(first) = parsed.segments.first()
        && (matches!(
            first.name.text.as_str(),
            "crate" | "self" | "super" | "Self"
        ) || parsed.segments.len() == 1
            || matches!(
                types.get(first.name.text.as_str()),
                Some(Target::Script { .. } | Target::Parameter | Target::Alias(_))
            ))
    {
        let first = &first.name.text;
        return Err(super::not_a_crate(first, span));
    }
    if let Some(path) = std_fmt_type(&parsed) {
        return Ok(path);
    }
    if let [segment] = &parsed.segments[..]
        && segment.name.text == item::type_ref::IMPL_TYPE
    {
        return resolve_impl(
            parsed,
            types,
            host,
            #[cfg(feature = "rust-import")]
            native,
        );
    }
    // `dyn Trait` parses as the path `dyn::Trait`; the trait is checked
    // against the crate's declarations once every item is resolved.
    if let [first, rest @ ..] = &parsed.segments[..]
        && first.name.text.as_str() == item::type_ref::DYN_SEGMENT
        && !rest.is_empty()
    {
        // Traits are crate-wide by name.
        let last = &rest[rest.len() - 1];
        let name = &last.name.text;
        // iced's `&mut dyn widget::Operation`, which a script widget's
        // `operate` lends on: the facade's operation handle.
        if name.as_str() == "Operation" && rest.len() > 1 {
            let mut handle = parsed.clone();
            handle.segments = rest.to_vec().into();
            let resolved = resolve_path(
                handle,
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            );
            if let Ok(path) = &resolved
                && matches!(&path.target, Target::Host { definition, .. }
                    if definition.path().as_str() == OPERATION)
            {
                return resolved;
            }
            // `widget::Operation` through a module `use`, which a type path
            // does not follow: iced's, when every segment is iced's.
            if rest[..rest.len() - 1].iter().all(|module| {
                matches!(
                    module.name.text.as_str(),
                    "iced" | "advanced" | "widget" | "core"
                )
            }) && let Some(definition) = host.type_(OPERATION)
            {
                return Ok(Path {
                    name: OPERATION.into(),
                    target: Target::Host {
                        definition: definition.clone(),
                        owner: host.type_owner(OPERATION).cloned(),
                    },
                    arguments: Box::default(),
                    span,
                });
            }
        }
        // `Output = i64` bindings, kept as the arguments `=Output<i64>`.
        let mut arguments = Vec::new();
        for argument in &last.arguments {
            // `dyn Convert<i64>`: a generic trait's argument, kept in order
            // before the bindings.
            let positional = match argument {
                item::type_ref::Argument::Type(item::type_ref::Type::Path(binding)) => {
                    !matches!(&binding.segments[..], [segment]
                        if segment.name.text.starts_with(item::type_ref::BINDING_PREFIX))
                }
                item::type_ref::Argument::Type(_) => true,
                item::type_ref::Argument::Lifetime(_) => false,
            };
            if positional && let item::type_ref::Argument::Type(ty) = argument {
                arguments.push(Argument::Type(resolve(
                    ty.clone(),
                    types,
                    host,
                    #[cfg(feature = "rust-import")]
                    native,
                )?));
                continue;
            }
            let item::type_ref::Argument::Type(item::type_ref::Type::Path(binding)) = argument
            else {
                continue;
            };
            let [segment] = &binding.segments[..] else {
                continue;
            };
            let (true, [item::type_ref::Argument::Type(ty)]) = (
                segment
                    .name
                    .text
                    .starts_with(item::type_ref::BINDING_PREFIX),
                &segment.arguments[..],
            ) else {
                continue;
            };
            let ty = resolve(
                ty.clone(),
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?;
            arguments.push(Argument::Type(Type::Path(Path {
                name: segment.name.text.clone(),
                target: Target::Builtin,
                arguments: Box::new([Argument::Type(ty)]),
                span: binding.span,
            })));
        }
        return Ok(Path {
            name: format!("{DYN_PREFIX}{name}").into(),
            target: Target::Builtin,
            arguments: arguments.into_boxed_slice(),
            span,
        });
    }
    // `<T as Trait>::Name`, parsed as `T::as<Trait>::Name`.
    if let Some(index) = parsed
        .segments
        .iter()
        .position(|segment| segment.name.text == item::type_ref::QUALIFIED_SEGMENT)
    {
        let segments = &parsed.segments;
        let trait_name = match &segments[index].arguments[..] {
            [item::type_ref::Argument::Type(item::type_ref::Type::Path(path))] => path
                .segments
                .last()
                .map(|segment| segment.name.text.clone()),
            _ => None,
        };
        let (Some(trait_name), [last]) = (trait_name, &segments[index + 1..]) else {
            return Err(Error::UnsupportedNominalSyntax {
                construct: "a qualified path other than `<T as Trait>::Name`".into(),
                source: span,
            });
        };
        let base = resolve_path(
            item::type_ref::Path {
                segments: segments[..index].to_vec().into_boxed_slice(),
                span: Span::new(span.start(), segments[index].span.start()),
                global: parsed.global,
            },
            types,
            host,
            #[cfg(feature = "rust-import")]
            native,
        )?;
        return Ok(Path {
            name: last.name.text.clone(),
            target: Target::Projection(Box::new(Type::Path(base)), Some(trait_name)),
            arguments: Box::default(),
            span,
        });
    }
    let name: ImmutableString = parsed
        .segments
        .iter()
        .map(|segment| segment.name.text.as_str())
        .collect::<Vec<_>>()
        .join("::")
        .into();

    // `T::Output`, `Self::Output` or `Shape::Output`: an associated type of a
    // generic parameter or a script type.
    if let [prefix @ .., last] = &parsed.segments[..]
        && !prefix.is_empty()
        && last.arguments.is_empty()
        && !types.contains_key(name.as_str())
    {
        let base_name = prefix
            .iter()
            .map(|segment| segment.name.text.as_str())
            .collect::<Vec<_>>()
            .join("::");
        if matches!(
            types.get(base_name.as_str()),
            Some(Target::Parameter | Target::Script { .. })
        ) {
            let base = resolve_path(
                item::type_ref::Path {
                    segments: prefix.to_vec().into_boxed_slice(),
                    span: Span::new(span.start(), prefix[prefix.len() - 1].span.end()),
                    global: parsed.global,
                },
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?;
            return Ok(Path {
                name: last.name.text.clone(),
                target: Target::Projection(Box::new(Type::Path(base)), None),
                arguments: Box::default(),
                span,
            });
        }
    }

    // `std::num::ParseIntError` and kin, unless the crate names the type.
    if let Some(error) = super::std_parse_error(&name)
        && !types.contains_key(error)
    {
        return Ok(Path {
            name: error.into(),
            target: Target::Builtin,
            arguments: Box::default(),
            span,
        });
    }
    // `std::time::Duration` names iced's where iced's `time` re-exports it,
    // and else Rawr's `Duration`, unless the crate names one.
    let name: ImmutableString = super::std_reexport(&name, host);
    if let Some(time) = super::std_time(&name)
        && !types.contains_key(name.as_str())
        && !types.contains_key(time)
    {
        return Ok(Path {
            name: time.into(),
            target: Target::Builtin,
            arguments: Box::default(),
            span,
        });
    }
    // `std::cmp::Ordering`, unless the crate names a type `Ordering`.
    if !types.contains_key(name.as_str()) && super::std_ordering(&name) {
        return Ok(Path {
            name: "Ordering".into(),
            target: Target::Builtin,
            arguments: Box::default(),
            span,
        });
    }
    // `std::collections::HashMap` and its kin, and `std::cmp::Reverse`,
    // unless the crate names one.
    let name = match super::std_collection(&name) {
        Some(collection) if !types.contains_key(name.as_str()) => collection.into(),
        None if super::std_reverse(&name) && !types.contains_key("Reverse") => "Reverse".into(),
        None if super::std_cell(&name) && !types.contains_key("RefCell") => "RefCell".into(),
        None if let Some(pointer) = super::std_pointer(&name)
            && !types.contains_key(pointer) =>
        {
            pointer.into()
        }
        None if !types.contains_key(name.as_str())
            && super::std_range(&name).is_some_and(|range| !types.contains_key(range)) =>
        {
            super::std_range(&name).expect("checked").into()
        }
        _ => name,
    };
    let target = if let Some(target) = types.get(name.as_str()) {
        target.clone()
    } else if host.is_builtin(name.as_str())
        || prelude_arity(&parsed).is_some()
        || super::std_collection(&name).is_some()
        || name.as_str() == "Reverse"
        || name.as_str() == "RefCell"
        || name.as_str() == "Rc"
        || name.as_str() == "Arc"
        || name.as_str() == "RangeInclusive"
        || name.as_str() == item::type_ref::ITER_TYPE
    {
        Target::Builtin
    } else if let Some(definition) = host.source_type(name.as_str()) {
        Target::Host {
            definition: definition.clone(),
            owner: host.type_owner(name.as_str()).cloned(),
        }
    } else {
        #[cfg(feature = "rust-import")]
        let missing =
            native.filter(|site| parsed.segments.len() > 1 && site.missing_binding(host, &name));
        let error = Error::UnknownType {
            name,
            reference: span,
        };
        #[cfg(feature = "rust-import")]
        if let Some(native) = missing {
            native.record(&error);
        }
        return Err(error);
    };

    let mut arguments = Vec::new();

    for segment in parsed.segments {
        for argument in segment.arguments {
            arguments.push(match argument {
                item::type_ref::Argument::Type(ty) => Argument::Type(resolve(
                    ty,
                    types,
                    host,
                    #[cfg(feature = "rust-import")]
                    native,
                )?),
                item::type_ref::Argument::Lifetime(lifetime) => Argument::Lifetime(lifetime.span),
            });
        }
    }

    let prelude = match name.as_str() {
        "Option" | "Vec" | "Box" => Some(1),
        "Result" | "HashMap" | "BTreeMap" => Some(2),
        "HashSet" | "BTreeSet" | "Reverse" | "RefCell" | "Rc" | "Arc" | "RangeInclusive" => Some(1),
        item::type_ref::ITER_TYPE | item::type_ref::INTO_ITER_TYPE => Some(1),
        name if item::type_ref::array_length(name).is_some() => Some(1),
        _ => None,
    };
    if let (Target::Builtin, Some(expected)) = (&target, prelude) {
        let actual = arguments
            .iter()
            .filter(|argument| matches!(argument, Argument::Type(_)))
            .count();
        if actual != expected {
            return Err(Error::InvalidTypeArguments {
                ty: name,
                expected,
                actual,
                reference: span,
            });
        }
    }

    if target == Target::Parameter && !arguments.is_empty() {
        return Err(Error::InvalidTypeArguments {
            ty: name,
            expected: 0,
            actual: arguments.len(),
            reference: span,
        });
    }

    // iced's `Size<Length>`, which `advanced::Widget::size` returns, is a
    // type of its own: the facade's `Size` is iced's `Size<f32>`.
    let target = match &target {
        Target::Host { definition, owner }
            if definition.path().as_str() == "iced::Size"
                && matches!(
                    &arguments[..],
                    [Argument::Type(Type::Path(Path { target: Target::Host { definition: length, .. }, .. }))]
                        if length.path().as_str() == "iced::Length"
                )
                && let Some(lengths) = host.type_(LENGTH_SIZE) =>
        {
            arguments.clear();
            Target::Host {
                definition: lengths.clone(),
                owner: owner.clone(),
            }
        }
        _ => target,
    };
    if let Target::Host { definition, owner } = &target {
        if definition.role() == host::type_def::Role::Trait {
            return Err(Error::UnsupportedNominalSyntax {
                construct: format!("the trait `{name}` as a type (write `impl {name}<..>`)").into(),
                source: span,
            });
        }
        content_messages(definition, &mut arguments, host);
        // iced's `Shell<'_, Message>`: the facade's Shell takes any message,
        // so a generic widget's `shell.request_redraw()` needs no witness.
        if matches!(
            definition.path().as_str(),
            "iced::advanced::Shell" | "iced::advanced::overlay::Element"
        ) {
            arguments.retain(|argument| !matches!(argument, Argument::Type(_)));
        }
        // iced's defaulted `Theme` and `Renderer` parameters after the
        // declared ones (`canvas::Geometry<Renderer>`) name the facade's own.
        while arguments
            .iter()
            .filter(|argument| matches!(argument, Argument::Type(_)))
            .count()
            > definition.parameters().len()
            && let Some(position) = arguments
                .iter()
                .rposition(|argument| matches!(argument, Argument::Type(_)))
            && matches!(
                &arguments[position],
                Argument::Type(Type::Path(Path { target: Target::Host { definition, .. }, .. }))
                    if matches!(definition.path().as_str(), "iced::widget::Renderer" | "iced::Theme")
            )
        {
            arguments.remove(position);
        }
        defaulted_arguments(definition, owner.as_ref(), &mut arguments, host, span);
        let actual = arguments
            .iter()
            .filter(|argument| matches!(argument, Argument::Type(_)))
            .count();
        if definition.exact_arity() && definition.parameters().len() != actual {
            return Err(Error::InvalidTypeArguments {
                ty: name,
                expected: definition.parameters().len(),
                actual,
                reference: span,
            });
        }
    }

    Ok(Path {
        name,
        target,
        arguments: arguments.into_boxed_slice(),
        span,
    })
}

/// The defaulted arguments a type is written without, as rustc fills them:
/// `palette::Hsl` is `Hsl<Srgb, f32>`. Nothing is filled unless every
/// missing argument has a default Rawr can name.
fn defaulted_arguments(
    definition: &host::type_def::Definition,
    owner: Option<&host::facade::Dependency>,
    arguments: &mut Vec<Argument>,
    host: &host::Registry,
    span: Span,
) {
    let mut filled = arguments
        .iter()
        .filter_map(|argument| match argument {
            Argument::Type(ty) => Some(ty.clone()),
            Argument::Lifetime(_) => None,
        })
        .collect::<Vec<_>>();
    let written = filled.len();
    for index in written..definition.parameters().len() {
        let Some(Some(default)) = definition.defaults().get(index) else {
            return;
        };
        let parameters = &definition.parameters()[..index];
        let Some(ty) = host_type(default, parameters, &filled, owner, definition, host, span)
        else {
            return;
        };
        filled.push(ty);
    }
    arguments.extend(filled.drain(written..).map(Argument::Type));
}

/// A host signature type as a header names it, with `parameters` read as
/// `arguments`. `None` when the header cannot name it.
fn host_type(
    ty: &host::type_ref::Type,
    parameters: &[ImmutableString],
    arguments: &[Type],
    owner: Option<&host::facade::Dependency>,
    originating: &host::type_def::Definition,
    host: &host::Registry,
    span: Span,
) -> Option<Type> {
    use host::type_ref::Shape;
    // Only an imported type reads the definition it is named from.
    #[cfg(not(feature = "rust-import"))]
    let _ = originating;
    let convert = |types: &[host::type_ref::Type], owner: Option<&host::facade::Dependency>| {
        types
            .iter()
            .map(|ty| host_type(ty, parameters, arguments, owner, originating, host, span))
            .map(|ty| ty.map(Argument::Type))
            .collect::<Option<Box<[_]>>>()
    };
    let path = |name: &str, target: Target, arguments: Box<[Argument]>| {
        Type::Path(Path {
            name: name.into(),
            target,
            arguments,
            span,
        })
    };
    Some(match ty.shape() {
        Shape::Unit => Type::Unit(span),
        Shape::Tuple(members) => Type::Tuple {
            members: members
                .iter()
                .map(|member| {
                    host_type(
                        member,
                        parameters,
                        arguments,
                        owner,
                        originating,
                        host,
                        span,
                    )
                })
                .collect::<Option<_>>()?,
            span,
        },
        Shape::Builtin(crate::host::type_ref::Builtin::Duration) => {
            path("Duration", Target::Builtin, Box::default())
        }
        Shape::Builtin(builtin) => path(builtin.name()?, Target::Builtin, Box::default()),
        Shape::Option(element) => path(
            "Option",
            Target::Builtin,
            convert(std::slice::from_ref(element.as_ref()), owner)?,
        ),
        Shape::Result(ok, err) => path(
            "Result",
            Target::Builtin,
            convert(&[ok.as_ref().clone(), err.as_ref().clone()], owner)?,
        ),
        Shape::Parameter(name) => {
            let name = name.to_string();
            let index = parameters.iter().position(|parameter| *parameter == name)?;
            arguments.get(index)?.clone()
        }
        Shape::Registered {
            path: registered,
            arguments: inner,
        } => {
            let name = registered.to_string();
            let definition = host.type_(&name)?.clone();
            let owner = host.type_owner(&name).cloned();
            let inner = convert(inner, owner.as_ref())?;
            path(&name, Target::Host { definition, owner }, inner)
        }
        #[cfg(feature = "rust-import")]
        Shape::Imported {
            identity,
            arguments: inner,
        } => {
            let imported = host.imported_type(identity, owner?, Some(originating))?;
            let definition = imported.definition.clone();
            let owner = imported.owner.cloned();
            let inner = convert(inner, owner.as_ref())?;
            path(
                &definition.path(),
                Target::Host { definition, owner },
                inner,
            )
        }
        _ => return None,
    })
}

/// iced's `widget::Operation`, which `operate` lends.
const OPERATION: &str = "iced::advanced::widget::Operation";

/// The facade's type for iced's `Size<Length>`.
pub(crate) const LENGTH_SIZE: &str = "iced::advanced::layout::LengthSize";

/// iced's widgets are generic over their content: `Column<Element<'_,
/// Message>>`, `Container<'a, W>`, `Button<'a, Message, &str>`. A facade
/// widget (a type projecting to what a facade trait erases to) is generic
/// over its Message only, so a content argument names its Message, and a
/// content without one (`&str`, `Text`) is dropped when the arguments
/// overflow; extra Messages (`Scrollable<'a, Message, W>`) are one.
fn content_messages(
    definition: &host::type_def::Definition,
    arguments: &mut Vec<Argument>,
    host: &host::Registry,
) {
    let erased = host
        .trait_types()
        .filter(|(definition, _)| !matches!(definition.shape(), host::type_def::Shape::Trait(_)))
        .flat_map(|(definition, _)| definition.projections().iter().map(|p| p.target()))
        .collect::<Vec<_>>();
    let widget = |definition: &host::type_def::Definition| {
        definition
            .projections()
            .iter()
            .any(|projection| erased.contains(&projection.target()))
    };
    if !widget(definition) {
        return;
    }
    // The content's Message: `Some(None)` for content without one.
    let content = |argument: &Argument| match argument {
        Argument::Type(Type::Path(Path {
            target: Target::Host { definition, .. },
            arguments,
            ..
        })) if widget(definition) || erased.contains(&definition.path()) => Some(
            definition
                .parameters()
                .iter()
                .position(|parameter| parameter == "Message")
                .and_then(|index| {
                    arguments
                        .iter()
                        .filter(|argument| matches!(argument, Argument::Type(_)))
                        .nth(index)
                        .cloned()
                }),
        ),
        Argument::Type(Type::Path(Path {
            target: Target::Builtin,
            name,
            ..
        })) if name == "String" => Some(None),
        _ => None,
    };
    let types = arguments
        .iter()
        .filter(|argument| matches!(argument, Argument::Type(_)))
        .count();
    if types == definition.parameters().len()
        && !arguments
            .iter()
            .any(|argument| content(argument).is_some_and(|message| message.is_some()))
    {
        return;
    }
    let mut kept = 0;
    *arguments = std::mem::take(arguments)
        .into_iter()
        .filter_map(|argument| match content(&argument) {
            Some(message) => message,
            None => Some(argument),
        })
        .filter(|argument| {
            let Argument::Type(_) = argument else {
                return true;
            };
            kept += 1;
            kept <= definition.parameters().len()
        })
        .collect();
}

/// The facade trait a bound names (iced's `Widget<Message>`), if any.
pub(super) fn host_trait(
    bound: &item::type_ref::Path,
    types: &BTreeMap<ImmutableString, Target>,
    host: &host::Registry,
) -> Option<host::type_def::Definition> {
    let name = bound
        .segments
        .iter()
        .map(|segment| segment.name.text.as_str())
        .collect::<Vec<_>>()
        .join("::");
    let definition = match types.get(name.as_str()) {
        Some(Target::Host { definition, .. }) => definition,
        Some(_) => return None,
        None => host.source_type(&name)?,
    };
    (definition.role() == host::type_def::Role::Trait).then(|| definition.clone())
}

/// `impl Trait<..> + 'a` of a facade trait is the one type the trait
/// projects to, taking the trait's leading arguments: `impl Widget<Message>`
/// is `Element<Message>`. Arguments after them (iced's defaulted `Theme` and
/// `Renderer`) name the facade's own and are not checked. Other `impl Trait`
/// types are a function parameter's anonymous generic (lifted when parsed),
/// `impl Fn*` or `impl Iterator`.
fn resolve_impl(
    parsed: item::type_ref::Path,
    types: &BTreeMap<ImmutableString, Target>,
    host: &host::Registry,
    #[cfg(feature = "rust-import")] native: Option<super::native::Site<'_>>,
) -> Result<Path, Error> {
    let span = parsed.span;
    if let Some(output) = future_output(&parsed) {
        // A future is iced's `Task` of its output, which runs it, and else
        // the core's future (D511.3).
        let task = if host.type_(TASK).is_some() {
            TASK
        } else {
            CORE_FUTURE
        };
        let definition = host
            .type_(task)
            .ok_or_else(|| Error::UnsupportedNominalSyntax {
                construct: "a future in a program without iced, whose `Task` runs it".into(),
                source: span,
            })?;
        let output = match output {
            Some(output) => resolve(
                output.clone(),
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )?,
            None => Type::Unit(span),
        };
        return Ok(Path {
            target: Target::Host {
                definition: definition.clone(),
                owner: host.type_owner(task).cloned(),
            },
            name: task.into(),
            arguments: Box::new([Argument::Type(output)]),
            span,
        });
    }
    let found = parsed.segments[0]
        .arguments
        .iter()
        .find_map(|argument| match argument {
            item::type_ref::Argument::Type(item::type_ref::Type::Path(bound)) => {
                host_trait(bound, types, host).map(|definition| (bound, definition))
            }
            _ => None,
        });
    let unsupported = || Error::UnsupportedNominalSyntax {
        construct: "`impl Trait` other than `impl Fn*`, `impl Iterator` or a facade trait \
                    (`impl Widget<Message>`) as a return type"
            .into(),
        source: span,
    };
    let Some((bound, definition)) = found else {
        return Err(unsupported());
    };
    let [projection] = definition.projections() else {
        return Err(unsupported());
    };
    let name = bound.segments[bound.segments.len() - 1].name.text.clone();
    let written = bound
        .segments
        .iter()
        .flat_map(|segment| segment.arguments.iter())
        .filter_map(|argument| match argument {
            item::type_ref::Argument::Type(ty) => Some(ty),
            item::type_ref::Argument::Lifetime(_) => None,
        })
        .collect::<Vec<_>>();
    let parameters = definition.parameters();
    if written.len() < parameters.len() {
        return Err(Error::InvalidTypeArguments {
            ty: name,
            expected: parameters.len(),
            actual: written.len(),
            reference: bound.span,
        });
    }
    let arguments = projection
        .arguments()
        .iter()
        .map(|argument| {
            let host::type_def::Argument::Source(parameter) = argument else {
                return Err(unsupported());
            };
            let index = parameters
                .iter()
                .position(|name| name == parameter)
                .ok_or_else(unsupported)?;
            resolve(
                written[index].clone(),
                types,
                host,
                #[cfg(feature = "rust-import")]
                native,
            )
            .map(Argument::Type)
        })
        .collect::<Result<Box<[_]>, _>>()?;
    let target = projection.target();
    let definition = host.source_type(&target).ok_or_else(unsupported)?;
    Ok(Path {
        target: Target::Host {
            definition: definition.clone(),
            owner: host.type_owner(&target).cloned(),
        },
        name: target.into(),
        arguments,
        span,
    })
}

/// The type that runs a future: iced's `Task`.
const TASK: &str = "iced::Task";
/// The core's future, where no host's type runs futures.
const CORE_FUTURE: &str = "rawr::future::Future";

/// `impl Future<Output = T>` (or `std::future::Future`): `Some(Some(T))`,
/// or `Some(None)` without its `Output`.
fn future_output(parsed: &item::type_ref::Path) -> Option<Option<&item::type_ref::Type>> {
    let arguments = &parsed.segments[0].arguments;
    let bound = arguments.iter().any(|argument| {
        matches!(argument, item::type_ref::Argument::Type(item::type_ref::Type::Path(bound))
            if bound.segments.last().is_some_and(|last| last.name.text == "Future")
                && matches!(
                    bound.segments.iter().map(|segment| segment.name.text.as_str()).collect::<Vec<_>>()[..],
                    ["Future"] | ["std" | "core", "future", "Future"]
                ))
    });
    if !bound {
        return None;
    }
    let binding = format!("{}Future=Output", item::type_ref::BINDING_PREFIX);
    Some(arguments.iter().find_map(|argument| match argument {
        item::type_ref::Argument::Type(item::type_ref::Type::Path(path)) => {
            match &path.segments[..] {
                [segment] if segment.name.text == binding.as_str() => {
                    match &segment.arguments[..] {
                        [item::type_ref::Argument::Type(ty)] => Some(ty),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }))
}

/// `fmt::Formatter` and `fmt::Result` (also under `std::` or
/// `core::`): a hand-written `Debug` or `Display` impl writes its text into
/// the formatter, which is Rawr's `String`, and returns `Result<(), ()>`.
fn std_fmt_type(parsed: &item::type_ref::Path) -> Option<Path> {
    let [prefix @ .., module, last] = &parsed.segments[..] else {
        return None;
    };
    if module.name.text != "fmt"
        || !module.arguments.is_empty()
        || !matches!(
            prefix,
            [] | [_] if prefix.iter().all(|root| matches!(root.name.text.as_str(), "std" | "core"))
        )
    {
        return None;
    }
    let types = |count: usize| {
        (0..count)
            .map(|_| Argument::Type(Type::Unit(parsed.span)))
            .collect::<Box<[_]>>()
    };
    let (name, arguments) = match last.name.text.as_str() {
        "Formatter"
            if last
                .arguments
                .iter()
                .all(|argument| matches!(argument, item::type_ref::Argument::Lifetime(_))) =>
        {
            ("String", types(0))
        }
        "Result" if last.arguments.is_empty() => ("Result", types(2)),
        _ => return None,
    };
    Some(Path {
        name: name.into(),
        target: Target::Builtin,
        arguments,
        span: parsed.span,
    })
}

/// `Option<T>` and `Result<T, E>`: Rust's prelude enums, builtin generics;
/// and `Vec<T>`, Rust's prelude vector; their type arity.
fn prelude_arity(parsed: &item::type_ref::Path) -> Option<usize> {
    match &parsed.segments[..] {
        [segment] if segment.name.text.as_str() == "Option" => Some(1),
        [segment] if segment.name.text.as_str() == "Vec" => Some(1),
        [segment] if segment.name.text.as_str() == "Box" => Some(1),
        // `[T; N]`.
        [segment] if item::type_ref::array_length(&segment.name.text).is_some() => Some(1),
        [segment] if segment.name.text.as_str() == "Result" => Some(2),
        // `impl Iterator<Item = T>`, `impl IntoIterator<Item = T>`.
        [segment]
            if matches!(
                segment.name.text.as_str(),
                item::type_ref::ITER_TYPE | item::type_ref::INTO_ITER_TYPE
            ) =>
        {
            Some(1)
        }
        // `fn(A, ..) -> B`: its inputs, then its output.
        [segment] if segment.name.text.as_str() == item::type_ref::FN_TYPE => {
            Some(segment.arguments.len())
        }
        _ => None,
    }
}

impl Type {
    /// Whether this is a written `&str`: Rawr's `String` path with the
    /// reference's lifetime (`resolve`).
    pub(crate) fn is_str(&self) -> bool {
        matches!(self, Type::Path(path)
            if path.target == Target::Builtin
                && path.name.as_str() == "String"
                && matches!(*path.arguments, [Argument::Lifetime(_)]))
    }

    /// `T` of `Option<T>`.
    pub(crate) fn option_element(&self) -> Option<&Type> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin || path.name.as_str() != "Option" {
            return None;
        }
        path.arguments.iter().find_map(|argument| match argument {
            Argument::Type(ty) => Some(ty),
            Argument::Lifetime(_) => None,
        })
    }

    /// `T` of `Vec<T>`.
    pub(crate) fn vec_element(&self) -> Option<&Type> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin || path.name.as_str() != "Vec" {
            return None;
        }
        path.arguments.iter().find_map(|argument| match argument {
            Argument::Type(ty) => Some(ty),
            Argument::Lifetime(_) => None,
        })
    }

    /// `T` and `N` of an array type `[T; N]`.
    pub(crate) fn array_element(&self) -> Option<(&Type, usize)> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin {
            return None;
        }
        let length = item::type_ref::array_length(&path.name)?;
        path.arguments.iter().find_map(|argument| match argument {
            Argument::Type(ty) => Some((ty, length)),
            Argument::Lifetime(_) => None,
        })
    }

    /// `T` of `Box<T>`, `Rc<T>` or `Arc<T>`, each the value itself at
    /// runtime.
    pub(crate) fn box_element(&self) -> Option<&Type> {
        self.pointer_element().map(|(_, element)| element)
    }

    /// The pointer's name and `T` of `Box<T>`, `Rc<T>` or `Arc<T>`.
    pub(crate) fn pointer_element(&self) -> Option<(&str, &Type)> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin || !matches!(path.name.as_str(), "Box" | "Rc" | "Arc") {
            return None;
        }
        path.arguments.iter().find_map(|argument| match argument {
            Argument::Type(ty) => Some((path.name.as_str(), ty)),
            Argument::Lifetime(_) => None,
        })
    }

    /// `T` of `std::cell::RefCell<T>`.
    pub(crate) fn cell_element(&self) -> Option<&Type> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin || path.name.as_str() != "RefCell" {
            return None;
        }
        path.arguments.iter().find_map(|argument| match argument {
            Argument::Type(ty) => Some(ty),
            Argument::Lifetime(_) => None,
        })
    }

    /// `T` of `std::cmp::Reverse<T>`.
    pub(crate) fn reverse_element(&self) -> Option<&Type> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin || path.name.as_str() != "Reverse" {
            return None;
        }
        path.arguments.iter().find_map(|argument| match argument {
            Argument::Type(ty) => Some(ty),
            Argument::Lifetime(_) => None,
        })
    }

    /// `T` of `impl Iterator<Item = T>` and `impl IntoIterator<Item = T>`,
    /// and whether it is the latter.
    pub(crate) fn iterator_item(&self) -> Option<(&Type, bool)> {
        let Type::Path(path) = self else {
            return None;
        };
        let into = match path.name.as_str() {
            item::type_ref::ITER_TYPE => false,
            item::type_ref::INTO_ITER_TYPE => true,
            _ => return None,
        };
        if path.target != Target::Builtin {
            return None;
        }
        path.arguments.iter().find_map(|argument| match argument {
            Argument::Type(ty) => Some((ty, into)),
            Argument::Lifetime(_) => None,
        })
    }

    /// `HashMap<K, V>`, `BTreeMap<K, V>`, `HashSet<K>`, `BTreeSet<K>`: the
    /// collection's name and its type arguments.
    pub(crate) fn keyed_arguments(&self) -> Option<(&str, Vec<&Type>)> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin || super::std_collection(&path.name).is_none() {
            return None;
        }
        let arguments = path
            .arguments
            .iter()
            .filter_map(|argument| match argument {
                Argument::Type(ty) => Some(ty),
                Argument::Lifetime(_) => None,
            })
            .collect();
        Some((path.name.as_str(), arguments))
    }

    /// The trait of a trait object type `dyn Trait`.
    pub(crate) fn dyn_trait(&self) -> Option<&str> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin {
            return None;
        }
        path.name.as_str().strip_prefix(DYN_PREFIX)
    }

    /// The associated type bindings of a trait object type
    /// `dyn Trait<Name = T>`: each name and type.
    pub(crate) fn dyn_bindings(&self) -> Vec<(ImmutableString, &Type)> {
        let Type::Path(path) = self else {
            return Vec::new();
        };
        path.arguments
            .iter()
            .filter_map(|argument| match argument {
                Argument::Type(Type::Path(binding)) => {
                    let name = binding.name.strip_prefix(item::type_ref::BINDING_PREFIX)?;
                    match &binding.arguments[..] {
                        [Argument::Type(ty)] => Some((ImmutableString::from(name), ty)),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect()
    }

    /// The type arguments of a trait object's generic trait:
    /// `i64` of `dyn Convert<i64>`.
    pub(crate) fn dyn_arguments(&self) -> Vec<&Type> {
        let Type::Path(path) = self else {
            return Vec::new();
        };
        path.arguments
            .iter()
            .filter_map(|argument| match argument {
                Argument::Type(Type::Path(binding))
                    if binding.name.starts_with(item::type_ref::BINDING_PREFIX) =>
                {
                    None
                }
                Argument::Type(ty) => Some(ty),
                Argument::Lifetime(_) => None,
            })
            .collect()
    }

    /// The inputs and output of a function type `fn(A, ..) -> B`.
    pub(crate) fn fn_signature(&self) -> Option<(Vec<&Type>, &Type)> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin || path.name.as_str() != item::type_ref::FN_TYPE {
            return None;
        }
        let mut types = path
            .arguments
            .iter()
            .filter_map(|argument| match argument {
                Argument::Type(ty) => Some(ty),
                Argument::Lifetime(_) => None,
            })
            .collect::<Vec<_>>();
        let output = types.pop()?;
        Some((types, output))
    }

    /// `T` and `E` of `Result<T, E>`.
    pub(crate) fn result_elements(&self) -> Option<(&Type, &Type)> {
        let Type::Path(path) = self else {
            return None;
        };
        if path.target != Target::Builtin || path.name.as_str() != "Result" {
            return None;
        }
        let mut types = path.arguments.iter().filter_map(|argument| match argument {
            Argument::Type(ty) => Some(ty),
            Argument::Lifetime(_) => None,
        });
        Some((types.next()?, types.next()?))
    }
}
