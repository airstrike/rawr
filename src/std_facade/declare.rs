//! The std facade's declarations, read from std's own in the std table
//! (D711 step 6).
//!
//! The facade states only its binds: the std item each callback runs
//! (`super::Bind`), and how rawr holds each std type it represents
//! ([`Held`]). The rest is std's. A bound item's signature is its
//! declaration: a type's inherent function, then a trait's that std
//! implements for the type, or a free function. A held type's capabilities
//! are the traits std implements for it that rawr runs. A bound variant is
//! its enum's, and a served module's type aliases are std's.

mod reader;
mod solve;

use std::collections::BTreeSet;

use crate::host::{self, facade, item, type_ref::Type};
use crate::std_facade::held::{self, Held};
use crate::std_table::{self, Written};
use crate::{capability, ImmutableString};

/// Declare in `builder` the held types and the items at the facade paths
/// `bound` (each relative to the facade's root, as `path::PathBuf::join`).
pub(super) fn declare(
    builder: &mut facade::Builder,
    held: &[Held],
    bound: &[String],
) -> Result<(), host::Error> {
    let reader = reader::Reader::new(held);
    let values = held
        .iter()
        .filter_map(|held| match held {
            Held::Value(value) => Some(*value),
            Held::Text(_) => None,
        })
        .collect::<Vec<_>>();
    let module = |path: &str| path.split_once("::").map(|(module, _)| module.to_owned());
    let modules = values
        .iter()
        .flat_map(|value| value.forms())
        .filter_map(|form| module(held::Value::facade(form)))
        .chain(bound.iter().filter_map(|path| module(path)))
        .collect::<BTreeSet<_>>();
    for module in &modules {
        builder.module(module.as_str())?;
    }
    for value in &values {
        let path = held::Value::facade(value.std);
        (value.register)(builder, path, &capabilities(value))?;
        for form in value.forms().skip(1) {
            builder.alias(held::Value::facade(form), path)?;
        }
    }
    for module in &modules {
        aliases(builder, &reader, module)?;
    }
    for path in bound {
        let (owner, name) = path.rsplit_once("::").unwrap_or(("", path));
        match values
            .iter()
            .find(|value| held::Value::facade(value.std) == owner)
        {
            Some(value) => member(builder, &reader, value, path, name)?,
            None => function(builder, &reader, path)?,
        }
    }
    Ok(())
}

/// The capabilities of `value` that std implements and rawr runs. `Send`
/// and `'static` are rawr's facts about its own value.
fn capabilities(value: &held::Value) -> Vec<capability::Kind> {
    use capability::Kind;
    let own = solve::read(value.std);
    value
        .runs
        .iter()
        .copied()
        .filter(|kind| {
            let (trait_, with_self) = match kind {
                Kind::Default => ("core::default::Default", false),
                Kind::Debug => ("core::fmt::Debug", false),
                Kind::Clone => ("core::clone::Clone", false),
                Kind::Copy => ("core::marker::Copy", false),
                Kind::PartialEq => ("core::cmp::PartialEq", true),
                Kind::Eq => ("core::cmp::Eq", false),
                Kind::PartialOrd => ("core::cmp::PartialOrd", true),
                Kind::Ord => ("core::cmp::Ord", false),
                Kind::Hash => ("core::hash::Hash", false),
                Kind::Send | Kind::Static | Kind::Iterator => return true,
            };
            let arguments = if with_self {
                vec![own.clone()]
            } else {
                Vec::new()
            };
            solve::holds(trait_, &own, &arguments)
        })
        .collect()
}

/// std's stable type aliases in `module` whose types rawr represents, as
/// `io::Result<T>`.
fn aliases(
    builder: &mut facade::Builder,
    reader: &reader::Reader<'_>,
    module: &str,
) -> Result<(), host::Error> {
    for path in std_table::children(&format!("std::{module}")) {
        if path.kind != "type" || path.stability != std_table::Stability::Stable {
            continue;
        }
        let Some(item) = std_table::item(path.definition) else {
            continue;
        };
        let parameters = item
            .generics()
            .filter(|parameter| parameter.kind == std_table::ParameterKind::Type)
            .map(|parameter| parameter.name)
            .collect::<Vec<_>>();
        let Some(target) = reader
            .with_parameters(&parameters)
            .host(&solve::read(item.declaration()))
        else {
            continue;
        };
        let name = path.path.rsplit("::").next().unwrap_or(path.path);
        builder.type_alias(
            &format!("{module}::{name}"),
            parameters
                .iter()
                .map(|name| ImmutableString::from(*name))
                .collect(),
            Box::default(),
            target,
        )?;
    }
    Ok(())
}

/// Declare the member `name` of a held type at `path`: a variant of its
/// enum, or each form of the functions its types declare.
fn member(
    builder: &mut facade::Builder,
    reader: &reader::Reader<'_>,
    value: &held::Value,
    path: &str,
    name: &str,
) -> Result<(), host::Error> {
    let own = Type::registered(held::Value::facade(value.std))?;
    let variant = std_table::item(&format!("{}::{name}", solve::definition(value.std)))
        .is_some_and(|item| item.kind == "variant");
    if variant {
        builder.item(item::Definition::associated_value(path, own)?)?;
        return Ok(());
    }
    let mut forms = Vec::<reader::Form>::new();
    for (declaration, scoped) in declarations(reader, value, name) {
        for form in scoped.forms(declaration) {
            let repeated = forms.iter().any(|known| {
                known.receiver.is_some() == form.receiver.is_some()
                    && known.signature.same_inputs(&form.signature).unwrap_or(true)
            });
            if !repeated {
                forms.push(form);
            }
        }
    }
    for form in forms {
        builder.item(match form.receiver {
            Some(mode) => item::Definition::method(path, mode, form.signature)?,
            None => item::Definition::associated_function(path, form.signature)?,
        })?;
    }
    Ok(())
}

/// Each declaration of `name` the types `value` stands for have, with the
/// reader of its scope, in the order rustc's method probe finds them: a
/// type's inherent function, then each trait impl's.
fn declarations<'a>(
    reader: &reader::Reader<'a>,
    value: &held::Value,
    name: &str,
) -> Vec<(&'static str, reader::Reader<'a>)> {
    let functions = std_table::trait_functions(name);
    let mut found = Vec::new();
    for form in value.forms() {
        let own = solve::read(form);
        if let Some(item) = std_table::item(&format!("{}::{name}", solve::definition(form)))
            && item.kind == "assoc-fn"
        {
            let this = item.impl_self().map_or_else(|| own.clone(), solve::read);
            found.push((
                item.declaration(),
                reader.scoped(vec![("Self", this)], Vec::new()),
            ));
        }
        for function in &functions {
            let trait_ = &function.definition[..function.definition.len() - name.len() - 2];
            let parameters = std_table::item(trait_)
                .map(|item| {
                    item.generics()
                        .filter(|parameter| parameter.kind == std_table::ParameterKind::Type)
                        .map(|parameter| parameter.name)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for implementation in std_table::trait_impls(trait_) {
                for binding in solve::instances(&implementation, &own) {
                    let arguments = implementation.arguments();
                    let mut names = vec![("Self", own.clone())];
                    for (index, parameter) in parameters.iter().enumerate() {
                        let argument = arguments.get(index).map_or_else(
                            || own.clone(),
                            |text| solve::substitute(&solve::read(text), &binding),
                        );
                        names.push((*parameter, argument));
                    }
                    let associated = implementation
                        .associated_types()
                        .map(|(bound, ty)| (bound, solve::substitute(&solve::read(ty), &binding)))
                        .collect::<Vec<(&'static str, Written)>>();
                    found.push((function.declaration(), reader.scoped(names, associated)));
                }
            }
        }
    }
    found
}

/// Declare the free function at `path`, std's `std::{path}`.
fn function(
    builder: &mut facade::Builder,
    reader: &reader::Reader<'_>,
    path: &str,
) -> Result<(), host::Error> {
    let Some(found) = std_table::path(&format!("std::{path}"), std_table::Namespace::Value) else {
        return Ok(());
    };
    let Some(item) = std_table::item(found.definition) else {
        return Ok(());
    };
    for form in reader.forms(item.declaration()) {
        builder.item(item::Definition::function(path, form.signature)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::capability::Kind;
    use crate::host::type_ref::Shape;
    use crate::receiver::Mode;

    fn forms(path: &str) -> Vec<crate::host::item::Definition> {
        crate::std_facade::definition()
            .overloads(&format!("rawr_std::{path}"))
            .map(<[_]>::to_vec)
            .unwrap_or_default()
    }

    #[test]
    fn std_facade_declarations_are_stds() {
        // `fs::read<P: AsRef<Path>>`: a `String`, an `OsString` and a
        // `PathBuf`, each by value or borrowed.
        assert_eq!(forms("fs::read").len(), 6);
        // `PathBuf::new()`, and `Path::new<S: AsRef<OsStr> + ?Sized>(s:
        // &S)` of the `Path` a `PathBuf` stands for.
        assert_eq!(forms("path::PathBuf::new").len(), 4);
        // `Stdin::read_line(&self, buf: &mut String)`.
        let read_line = forms("io::Stdin::read_line");
        assert_eq!(read_line.len(), 1);
        assert_eq!(read_line[0].receiver(), Some(Mode::Shared));
        assert!(matches!(
            read_line[0].signature().parameters()[0].shape(),
            Shape::Reference { mutable: true, .. }
        ));
        // `Write::flush(&mut self)` of `impl Write for Stdout`.
        assert_eq!(
            forms("io::Stdout::flush")[0].receiver(),
            Some(Mode::Mutable)
        );
        // `ToString::to_string` of `impl<T: Display> ToString for T`.
        assert_eq!(forms("io::Error::to_string").len(), 1);
    }

    #[test]
    fn std_facade_capabilities_are_the_traits_std_implements() {
        let definition = crate::std_facade::definition();
        let capabilities = |path: &str| {
            definition
                .types()
                .find(|ty| ty.path() == format!("rawr_std::{path}"))
                .map(|ty| ty.capabilities())
                .expect("a held type")
        };
        let path = capabilities("path::PathBuf");
        assert!(path.contains(Kind::Clone) && path.contains(Kind::PartialEq));
        // std's `Stdin` is no `Clone`, though rawr's value could be.
        let stdin = capabilities("io::Stdin");
        assert!(stdin.contains(Kind::Debug) && !stdin.contains(Kind::Clone));
        assert!(!capabilities("io::Error").contains(Kind::Clone));
    }
}
