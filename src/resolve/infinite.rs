//! Types that hold themselves by value, which have no size: rustc's E0072.

use super::{item, type_ref, Error, LocatedError, Program};
use crate::{program, type_def, ImmutableString, Span};
use std::collections::{BTreeMap, BTreeSet};

/// The program's structs and enums, as what each holds by value.
pub(super) struct Layouts<'a> {
    types: BTreeMap<type_def::Id, Layout<'a>>,
    /// Declaration order, which rustc names a cycle's types in.
    order: Vec<type_def::Id>,
}

/// One struct or enum: what it is called, where it is declared, its type
/// parameters and its fields' (or payloads') types.
struct Layout<'a> {
    name: &'a ImmutableString,
    header: Span,
    source: program::source::Id,
    parameters: Vec<&'a ImmutableString>,
    fields: Vec<&'a type_ref::Type>,
}

impl<'a> Layouts<'a> {
    pub(super) fn of(program: &'a Program) -> Self {
        let mut types = BTreeMap::new();
        let mut order = Vec::new();
        for shell in program.items() {
            let (definition, header, name, generics, fields): (_, _, _, _, Vec<_>) =
                match &shell.kind {
                    item::Kind::Struct(item) => (
                        item.definition,
                        &item.header,
                        &item.name,
                        &item.generics,
                        item.fields.iter().map(|field| &field.ty).collect(),
                    ),
                    item::Kind::Enum(item) => (
                        item.definition,
                        &item.header,
                        &item.name,
                        &item.generics,
                        item.variants
                            .iter()
                            .flat_map(|variant| variant.payload.iter())
                            .collect(),
                    ),
                    _ => continue,
                };
            order.push(definition);
            types.insert(
                definition,
                Layout {
                    name: &name.text,
                    // From `pub`, else the name: the keyword's place isn't kept.
                    header: Span::new(
                        header.visibility.unwrap_or(name.span).start(),
                        name.span.end(),
                    ),
                    source: shell.source,
                    parameters: generics.iter().map(|generic| &generic.name.text).collect(),
                    fields,
                },
            );
        }
        Self { types, order }
    }

    /// rustc's E0072 for the first type, in declaration order, that holds
    /// itself by value, through any of the program's types.
    pub(super) fn reject(&self) -> Result<(), LocatedError> {
        for &start in &self.order {
            let Some(cycle) = self.cycle(start) else {
                continue;
            };
            let mut names = cycle
                .iter()
                .map(|id| (self.order.iter().position(|other| other == id), *id))
                .collect::<Vec<_>>();
            names.sort();
            let names = names
                .iter()
                .map(|(_, id)| format!("`{}`", self.types[id].name))
                .collect::<Vec<_>>();
            let message = match names.as_slice() {
                [one] => format!("recursive type {one} has infinite size"),
                [rest @ .., last] => {
                    format!(
                        "recursive types {} and {last} have infinite size",
                        rest.join(", ")
                    )
                }
                [] => unreachable!("a cycle has a type"),
            };
            let layout = &self.types[&start];
            return Err(LocatedError::primary(
                layout.source,
                Error::StdPath {
                    code: "E0072",
                    message: message.into(),
                    label: None,
                    note: Some(
                        "insert some indirection (e.g., a `Box`, `Rc`, or `&`) to break the cycle"
                            .into(),
                    ),
                    reference: layout.header,
                },
            ));
        }
        Ok(())
    }

    /// The types on a path from `start` back to itself by value, if one
    /// exists.
    fn cycle(&self, start: type_def::Id) -> Option<Vec<type_def::Id>> {
        let mut path = vec![start];
        let mut seen = BTreeSet::from([start]);
        self.reach(start, start, &mut path, &mut seen)
            .then_some(path)
    }

    fn reach(
        &self,
        from: type_def::Id,
        goal: type_def::Id,
        path: &mut Vec<type_def::Id>,
        seen: &mut BTreeSet<type_def::Id>,
    ) -> bool {
        let mut held = Vec::new();
        for field in &self.types[&from].fields {
            self.held(field, &mut held);
        }
        for next in held {
            if next == goal {
                return true;
            }
            if seen.insert(next) {
                path.push(next);
                if self.reach(next, goal, path, seen) {
                    return true;
                }
                path.pop();
            }
        }
        false
    }

    /// The program's types `ty` holds by value: itself, the members of a
    /// tuple or an array, the payload of an `Option`, a `Result` or a cell,
    /// and a generic type's arguments for the parameters it holds by value.
    /// A pointer, a reference or a collection holds its items elsewhere.
    fn held(&self, ty: &type_ref::Type, held: &mut Vec<type_def::Id>) {
        if let Some(element) = ty
            .option_element()
            .or_else(|| ty.array_element().map(|(element, _)| element))
            .or_else(|| ty.cell_element().map(|(_, element)| element))
        {
            return self.held(element, held);
        }
        if let Some((ok, err)) = ty.result_elements() {
            self.held(ok, held);
            return self.held(err, held);
        }
        match ty {
            type_ref::Type::Tuple { members, .. } => {
                for member in members {
                    self.held(member, held);
                }
            }
            type_ref::Type::Path(path) => {
                let type_ref::Target::Script { definition, .. } = &path.target else {
                    return;
                };
                let Some(layout) = self.types.get(definition) else {
                    return;
                };
                held.push(*definition);
                let arguments = path.arguments.iter().filter_map(|argument| match argument {
                    type_ref::Argument::Type(ty) => Some(ty),
                    type_ref::Argument::Lifetime(_) => None,
                });
                for (parameter, argument) in layout.parameters.iter().zip(arguments) {
                    if self.holds_parameter(*definition, parameter, &mut BTreeSet::new()) {
                        self.held(argument, held);
                    }
                }
            }
            type_ref::Type::Unit(_) | type_ref::Type::Reference { .. } => {}
        }
    }

    /// Whether the type `owner` holds its type parameter `parameter` by
    /// value.
    fn holds_parameter(
        &self,
        owner: type_def::Id,
        parameter: &ImmutableString,
        seen: &mut BTreeSet<(type_def::Id, ImmutableString)>,
    ) -> bool {
        if !seen.insert((owner, parameter.clone())) {
            return false;
        }
        self.types[&owner]
            .fields
            .iter()
            .any(|field| self.holds_named(field, owner, parameter, seen))
    }

    fn holds_named(
        &self,
        ty: &type_ref::Type,
        owner: type_def::Id,
        parameter: &ImmutableString,
        seen: &mut BTreeSet<(type_def::Id, ImmutableString)>,
    ) -> bool {
        if let Some(element) = ty
            .option_element()
            .or_else(|| ty.array_element().map(|(element, _)| element))
            .or_else(|| ty.cell_element().map(|(_, element)| element))
        {
            return self.holds_named(element, owner, parameter, seen);
        }
        if let Some((ok, err)) = ty.result_elements() {
            return self.holds_named(ok, owner, parameter, seen)
                || self.holds_named(err, owner, parameter, seen);
        }
        match ty {
            type_ref::Type::Tuple { members, .. } => members
                .iter()
                .any(|member| self.holds_named(member, owner, parameter, seen)),
            type_ref::Type::Path(path) => match &path.target {
                type_ref::Target::Parameter => path.name == *parameter,
                type_ref::Target::Script { definition, .. } => {
                    let Some(layout) = self.types.get(definition) else {
                        return false;
                    };
                    let arguments = path.arguments.iter().filter_map(|argument| match argument {
                        type_ref::Argument::Type(ty) => Some(ty),
                        type_ref::Argument::Lifetime(_) => None,
                    });
                    layout
                        .parameters
                        .iter()
                        .zip(arguments)
                        .any(|(inner, argument)| {
                            self.holds_named(argument, owner, parameter, seen)
                                && self.holds_parameter(*definition, inner, seen)
                        })
                }
                _ => false,
            },
            type_ref::Type::Unit(_) | type_ref::Type::Reference { .. } => false,
        }
    }
}
