//! A marked struct native code holds but never opens crosses as a handle
//! of the program value (D961): rawr's copy of the crate declares it as the
//! handle alone, whose layout no field edit changes, so a save that adds a
//! private field builds nothing native. Native code calls its methods as a
//! native trait object's stand-in calls its impl's (#1264): each receiver is
//! the handle, and the value stays the interpreter's.
//!
//! A struct is held by its handle only when the last check of rawr's copy
//! found native code holding it and never opening it (`Uses::opaque`): no
//! field read, struct literal or pattern, no `size_of`, `align_of` or
//! `transmute` of it; a type with no record keeps its real layout. It must
//! be one native code can't open either: it is visible, crosses, and has
//! fields, none visible to native code; it is neither generic nor `Copy`,
//! has no `repr`, derives only `Debug`, `Clone` and `Default`, which the
//! handle implements by asking the interpreter, and no other marked type
//! holds it, as such a type's value crosses whole, nor does a marked
//! function take `&mut` of it beside a receiver, which crosses as a copy
//! taken back.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::{item, path_of, slice, type_ref, Generator, Owner, Shape, Source, Std, MODULE};

impl Generator<'_> {
    /// The marked structs among `sources` native code holds by their
    /// handle, by their paths from the crate root: of `opaque`, the types
    /// the last check found native code holding but never opening, those
    /// it can't open.
    pub(super) fn handled(
        &self,
        sources: &[Source<'_>],
        opaque: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        // Each marked type another one holds, which crosses with that one's
        // value.
        let mut held = BTreeSet::new();
        for (owner, mirrored) in &self.types {
            for field in &mirrored.fields {
                let mut paths = Vec::new();
                paths_in(field, &mut paths);
                held.extend(
                    paths
                        .into_iter()
                        .filter_map(|path| self.marked(mirrored.source, path))
                        .filter(|marked| marked != owner)
                        .map(str::to_owned),
                );
            }
        }
        // And each a marked function takes `&mut` of beside its receiver,
        // alone or inside another type, which crosses as a copy taken back,
        // as a handle can't yet: native code calls it, so its trampoline
        // must cross.
        for source in sources {
            held.extend(self.lent_mutably(source));
        }
        let mut handled = BTreeSet::new();
        for source in sources {
            for item in source.items {
                let item::Parsed::Struct(declared) = item else {
                    continue;
                };
                let path = path_of(source.modules, &declared.name.text);
                if declared.header.visibility.is_some()
                    && opaque.contains(&path)
                    && self.crossing.contains(&path)
                    && !held.contains(&path)
                    && closed(source, declared)
                {
                    handled.insert(path);
                }
            }
        }
        handled
    }

    /// The marked types a function of `source` native code can call takes
    /// `&mut` of outside its receiver, as a free `update(state: &mut
    /// State, ..)` does, by their paths: the referent's own and each one it
    /// names, `Self` as its impl's type.
    fn lent_mutably(&self, source: &Source<'_>) -> BTreeSet<String> {
        let mut functions = Vec::new();
        for item in source.items {
            match item {
                item::Parsed::Function(declared) if declared.header.visibility.is_some() => {
                    functions.push((declared, None));
                }
                item::Parsed::Impl(declared) => {
                    let owner = match &declared.owner {
                        type_ref::Type::Path(owner) => self.marked(source, owner),
                        _ => None,
                    };
                    for function in &declared.functions {
                        if declared.trait_.is_some() || function.header.visibility.is_some() {
                            functions.push((function, owner));
                        }
                    }
                }
                _ => {}
            }
        }
        let mut lent = BTreeSet::new();
        for (function, owner) in functions {
            for parameter in &function.parameters {
                let type_ref::Type::Reference {
                    mutable: true,
                    target,
                    ..
                } = &parameter.ty
                else {
                    continue;
                };
                let mut paths = Vec::new();
                paths_in(target, &mut paths);
                for path in paths {
                    let named = match &path.segments[..] {
                        [only] if only.name.text == "Self" => owner,
                        _ => self.marked(source, path),
                    };
                    lent.extend(named.map(str::to_owned));
                }
            }
        }
        lent
    }

    /// The struct `declared` at `path`, held by its handle, as rawr's copy
    /// of the crate declares it, and its shape's text: its derives as
    /// written, which the shape keeps apart, and the rest, which names no
    /// field. Beside the handle, a marker of the fields' types makes it
    /// `Send`, `Sync`, `Unpin` and unwind-safe as they are, as cargo's
    /// build of the struct is (`PhantomData` has no layout); the shape
    /// keeps those types apart too ([`Shape::autos`]).
    pub(super) fn handle(&self, file: &Source<'_>, declared: &item::Struct, path: &str) -> Handle {
        let name = declared.name.text.as_str();
        let island = format!("crate::{MODULE}");
        let visibility = declared
            .header
            .visibility
            .map_or("", |visibility| slice(file.text, visibility));
        let mut derives = String::new();
        for attribute in &declared.header.attributes {
            if derive(attribute) {
                let _ = writeln!(derives, "{}", slice(file.text, attribute.span));
            }
        }
        let fields = declared
            .fields
            .iter()
            .map(|field| slice(file.text, field.ty.span()).trim())
            .collect::<Vec<_>>();
        let marker = format!("__RawrFields{name}");
        let phantom = "::std::marker::PhantomData";
        let text = format!(
            "/// Generated by Rawr: a `{name}` native code holds by the handle of the\n\
             /// program value, which keeps its fields.\n\
             {visibility} struct {name}({island}::Object, {phantom}<{marker}>);\n\
             impl {island}::Put for {name} {{\n    #[inline]\n    \
             fn put(&self, out: &mut {island}::Out) {{\n        \
             out.kept_lent(self.0.handle());\n    }}\n}}\n\
             impl {island}::Take for {name} {{\n    #[inline]\n    \
             fn take(input: &mut {island}::In<'_>) -> Self {{\n        \
             Self({island}::Object::new(input.handle()), {phantom})\n    }}\n}}\n\
             impl {island}::Mirror for {name} {{\n    #[inline]\n    \
             fn give(self, out: &mut {island}::Out) {{\n        \
             out.kept(self.0.give());\n    }}\n    #[inline]\n    \
             fn lend<'c>(&'c self, call: &mut {island}::Call<'c>) {{\n        \
             call.argument(self);\n    }}\n}}\n"
        );
        let mut items = format!(
            "/// Generated by Rawr: the types of `{name}`'s fields, which make its\n\
             /// handle `Send` and `Sync` as they are.\n\
             #[doc(hidden)]\nstruct {marker}({});\n{text}",
            fields.join(", ")
        );
        let full = format!("crate::{path}");
        let rest = format!(", {phantom}");
        for std in self.derived(declared) {
            items.push_str(&std.implementation(&full, path, &rest));
        }
        self.autos.borrow_mut().insert(
            path.to_owned(),
            fields.iter().map(|field| super::built(field)).collect(),
        );
        Handle {
            items,
            shape: format!("{derives}{text}"),
        }
    }

    /// Whether `ty`, written in `file`, is a type native code holds by its
    /// handle, by its path or as `Self` of `owner`.
    pub(super) fn handled_type(
        &self,
        file: &Source<'_>,
        ty: &type_ref::Type,
        owner: Owner<'_>,
    ) -> bool {
        let type_ref::Type::Path(path) = ty else {
            return false;
        };
        match (&path.segments[..], owner) {
            ([only], Owner::Marked(owner)) if only.name.text == "Self" => {
                self.handles.contains(owner)
            }
            _ => self
                .marked(file, path)
                .is_some_and(|marked| self.handles.contains(marked)),
        }
    }

    /// The std traits `declared` derives, each of which its handle
    /// implements by the program's own derived impl.
    fn derived(&self, declared: &item::Struct) -> BTreeSet<Std> {
        declared
            .header
            .attributes
            .iter()
            .filter(|attribute| derive(attribute))
            .flat_map(|attribute| attribute.arguments.iter())
            .filter_map(|derived| Std::named(&derived.segments.last()?.name.text))
            .filter(|std| !self.traits.contains(format!("{std:?}").as_str()))
            .collect()
    }
}

impl Shape {
    /// Why native code must be built again when a struct native code
    /// `reaches` and holds by its handle goes from `self`'s fields to
    /// `newer`'s, in one line: a new field's type may make it no longer
    /// `Send`, `Sync` or `Unpin`, which rustc must say where native code
    /// needs it to be. A type of a field it had, or one of std's plain
    /// types, which are all of them, changes none of those.
    pub(super) fn auto_change(
        &self,
        newer: &Self,
        reaches: impl Fn(&str) -> bool,
    ) -> Option<String> {
        newer.autos.iter().find_map(|(path, fields)| {
            let old = self.autos.get(path)?;
            let new = fields
                .iter()
                .find(|field| !old.contains(*field) && !plain(field))?;
            reaches(path).then(|| {
                format!(
                    "`{path}` holds a `{new}` now, which may change whether it is `Send` or `Sync`"
                )
            })
        })
    }
}

/// Whether the type `written` is made of std's plain types alone, each of
/// which is `Send`, `Sync`, `Unpin` and unwind-safe when what it holds is.
fn plain(written: &str) -> bool {
    written
        .split(|c: char| !(c == '_' || c.is_alphanumeric()))
        .filter(|word| !word.is_empty() && !word.starts_with(|c: char| c.is_ascii_digit()))
        .all(|word| {
            matches!(
                word,
                "bool"
                    | "char"
                    | "str"
                    | "static"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "isize"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "usize"
                    | "f32"
                    | "f64"
                    | "String"
                    | "Option"
                    | "Vec"
                    | "Box"
                    | "VecDeque"
                    | "BTreeMap"
                    | "BTreeSet"
                    | "std"
                    | "core"
                    | "alloc"
                    | "string"
                    | "vec"
                    | "option"
                    | "boxed"
                    | "collections"
            )
        })
}

/// A struct held by its handle, as rawr's copy declares it (`items`) and
/// as its shape has it (`shape`).
pub(super) struct Handle {
    pub(super) items: String,
    pub(super) shape: String,
}

/// Whether native code can't open `declared`: it has fields and none it
/// sees, no type parameters or lifetimes, no `repr`, no `Copy`, derived or
/// written, and derives only `Debug`, `Clone` and `Default`, which its
/// handle implements by asking the program.
fn closed(file: &Source<'_>, declared: &item::Struct) -> bool {
    let name = declared.name.text.as_str();
    let attributes = declared.header.attributes.iter().all(|attribute| {
        let named = |wanted: &str| {
            matches!(attribute.path.segments.as_ref(), [only] if only.name.text == wanted)
        };
        if named("repr") {
            return false;
        }
        // A value native code compares, hashes or orders, as a map's key,
        // is one it copies, each comparison native code's own.
        !derive(attribute)
            || attribute.arguments.iter().all(|derived| {
                derived.segments.last().is_some_and(|last| {
                    matches!(
                        Std::named(&last.name.text),
                        Some(Std::Debug | Std::Clone | Std::Default)
                    )
                })
            })
    });
    let copied = file.items.iter().any(|item| {
        matches!(item, item::Parsed::Impl(declared)
            if declared.trait_.as_ref().and_then(|path| path.segments.last())
                .is_some_and(|last| last.name.text == "Copy")
                && matches!(&declared.owner, type_ref::Type::Path(owner)
                    if owner.segments.last().is_some_and(|last| last.name.text == name)))
    });
    !declared.fields.is_empty()
        && declared.generics.is_empty()
        && declared.lifetimes.params.is_empty()
        && !super::visible_fields(file, declared)
        && attributes
        && !copied
}

/// Whether `attribute` is a `derive`.
fn derive(attribute: &item::Attribute) -> bool {
    matches!(attribute.path.segments.as_ref(), [only] if only.name.text == "derive")
}

/// Each path `ty` names, its arguments' too.
fn paths_in<'t>(ty: &'t type_ref::Type, paths: &mut Vec<&'t type_ref::Path>) {
    match ty {
        type_ref::Type::Unit(_) => {}
        type_ref::Type::Tuple { members, .. } => {
            for member in members {
                paths_in(member, paths);
            }
        }
        type_ref::Type::Reference { target, .. } => paths_in(target, paths),
        type_ref::Type::Path(path) => {
            paths.push(path);
            for segment in &path.segments {
                for argument in &segment.arguments {
                    if let type_ref::Argument::Type(argument) = argument {
                        paths_in(argument, paths);
                    }
                }
            }
        }
    }
}
