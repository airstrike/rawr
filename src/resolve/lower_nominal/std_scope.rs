//! Whether a std trait is in scope where its method is called, as rustc's
//! probe asks of a trait (#1427): a prelude trait everywhere, any other
//! where a `use` binds it or a glob of a module that holds it, in this
//! module or in a block around the call. rustc's E0599 for a method that
//! only an out-of-scope std trait gives the receiver's std type.

use super::*;
use crate::std_table;

impl Parser<'_> {
    /// Whether the stable std trait `definition` may be in scope at `at`. A
    /// `use` of a path outside std that ends in the trait's name, or a glob
    /// of one, may reach it through a re-export, so it counts.
    pub(super) fn std_trait_in_scope(&self, definition: &str, at: Position) -> bool {
        if crate::resolve::std_trait::in_prelude(definition) {
            return true;
        }
        let Some(name) = definition.rsplit("::").next() else {
            return true;
        };
        let Some(imports) = self.models.lexical.get(&self.models.context) else {
            return true;
        };
        let std =
            |path: &[ImmutableString]| path.first().is_some_and(|root| std_table::is_root(root));
        let names = |path: &[ImmutableString]| {
            let path = path
                .iter()
                .map(ImmutableString::as_str)
                .collect::<Vec<_>>()
                .join("::");
            std_table::stable_trait(&path) == Some(definition)
        };
        imports
            .iter()
            .filter(|used| used.visible_at(at))
            .any(|used| {
                if let crate::item::UseTree::Glob { prefix, .. } = &used.tree {
                    let mut path = prefix
                        .segments
                        .iter()
                        .map(|segment| segment.name.text.clone())
                        .collect::<Vec<_>>();
                    if !std(&path) {
                        return true;
                    }
                    path.push(name.into());
                    return names(&path);
                }
                used.bindings().iter().any(|(_, path)| match &path[..] {
                    [.., last] if last != name => false,
                    path if std(path) => names(path),
                    _ => true,
                })
            })
    }

    /// rustc's E0599 for the method `member` at `at` of `receiver`, a value
    /// of the std type the facade holds at `facade` (`rawr_std::io::Cursor`),
    /// when std gives the type the method only through traits none of
    /// which is in scope: `stdout().flush()` with no `use std::io::Write;`.
    /// The facade binds such a method as the type's own, so its call is
    /// checked here. `None` for an inherent method, or one a trait in
    /// scope gives.
    pub(super) fn unscoped_facade_method(
        &self,
        facade: &str,
        receiver: &Ty,
        member: &ImmutableString,
        at: Position,
    ) -> Option<Error> {
        let path = facade.strip_prefix(crate::std_facade::ROOT)?;
        let owner = std_table::path(&format!("std{path}"), std_table::Namespace::Type)?;
        if std_table::item(&format!("{}::{member}", owner.definition)).is_some() {
            return None;
        }
        let traits = std_table::trait_functions(member)
            .iter()
            .filter(|function| takes_self(function.declaration()))
            .filter_map(|function| {
                function
                    .definition
                    .strip_suffix(member.as_str())?
                    .strip_suffix("::")
            })
            .filter(|definition| {
                std_table::impls(definition, owner.definition)
                    .next()
                    .is_some()
            })
            .collect::<Vec<_>>();
        if traits
            .iter()
            .any(|definition| self.std_trait_in_scope(definition, at))
        {
            return None;
        }
        let path = std_table::public_path(traits.first()?)?;
        // rustc names a generic type that has no inherent method of the
        // name and no `Deref` by its declaration: `std::io::Cursor<T>`.
        let mut name = without_references(receiver).name(&self.models);
        if let Some((head, _)) = name.split_once('<')
            && std_table::impls("core::ops::deref::Deref", owner.definition)
                .next()
                .is_none()
            && let Some(item) = std_table::item(owner.definition)
        {
            let parameters = item
                .generics()
                .filter(|parameter| parameter.stability == std_table::Stability::Stable)
                .map(|parameter| parameter.name)
                .collect::<Vec<_>>();
            name = format!("{head}<{}>", parameters.join(", ")).into();
        }
        Some(Error::UnscopedMethod {
            kind: match owner.kind {
                "enum" => "enum",
                _ => "struct",
            },
            owner: name,
            member: member.clone(),
            path: path.into(),
            at: Span::new(at, text_end(at, member)),
            placement: self.models.use_placement(),
        })
    }

    /// rustc's E0599 for `error`, an unknown method of a receiver whose std
    /// type implements a std trait that has the method, where that trait is
    /// out of scope at the call and no trait in scope gives the method:
    /// `v.write_all(..)` of a `Vec<u8>` with no `use std::io::Write;`,
    /// which rustc suggests importing. Any other error as it is.
    pub(super) fn unscoped_std_method(&self, error: Error) -> Error {
        let Error::UnknownMember {
            member, reference, ..
        } = &error
        else {
            return error;
        };
        let Some(receiver) = self.std_probed.get(&reference.start()) else {
            return error;
        };
        let at = reference.start();
        let implemented = std_table::trait_functions(member)
            .iter()
            .filter(|function| takes_self(function.declaration()))
            .filter_map(|function| {
                let definition = function
                    .definition
                    .strip_suffix(member.as_str())?
                    .strip_suffix("::")?;
                std_impl::select(
                    &mut std_impl::Probe(self),
                    definition,
                    receiver,
                    &[],
                    false,
                    0,
                )
                .is_ok()
                .then_some(definition)
            })
            .collect::<Vec<_>>();
        // A trait in scope that gives the method is rawr's to run.
        if implemented
            .iter()
            .any(|definition| self.std_trait_in_scope(definition, at))
        {
            return error;
        }
        let Some(path) = implemented
            .first()
            .and_then(|definition| std_table::public_path(definition))
        else {
            return error;
        };
        let kind = std_impl::std_definition(receiver)
            .and_then(|(definition, _)| std_table::item(definition))
            .map_or("type", |item| match item.kind {
                "struct" => "struct",
                "enum" => "enum",
                _ => "type",
            });
        Error::UnscopedMethod {
            kind,
            owner: receiver.name(&self.models),
            member: member.clone(),
            path: path.into(),
            at: Span::new(at, text_end(at, member)),
            placement: self.models.use_placement(),
        }
    }
}

/// Whether the trait function `declaration` (`fn write_all(&mut self, buf:
/// &[u8]) -> ..`) takes a receiver, so a method call may name it.
fn takes_self(declaration: &str) -> bool {
    declaration
        .split_once('(')
        .and_then(|(_, parameters)| parameters.split([',', ')']).next())
        .is_some_and(|first| {
            let first = first.trim();
            first == "self"
                || first.ends_with(" self")
                || first.ends_with("&self")
                || first.starts_with("self:")
                || first.starts_with("mut self")
        })
}
