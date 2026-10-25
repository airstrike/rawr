//! Native islands (D501): a crate whose root is native Rust and whose
//! marked modules Rawr interprets.
//!
//! Rawr reads only the root's `mod` declarations, as the root of the marked
//! modules ([`root_text`]). The native build of the crate, the shadow crate
//! the runner links, holds the root and every native module as they are,
//! and in place of each marked file a generated one ([`Shadow`]): the types
//! native code can see, copied as declared, and for each function native
//! code can call, a trampoline with its signature whose body hands the
//! arguments to the interpreter and takes the result back. Values cross by
//! the small wire format of [`crate::island`], which both sides speak:
//! numbers, `bool`, `char`, strings, `Vec`, `Option` and the marked
//! crate's own structs and enums made of them. A function whose values
//! can't cross gets a trampoline that panics with a message naming it.
//!
//! The generated files are the island's shape: a save that changes them
//! (a signature, a struct's fields, a constant native code reads) needs a
//! new native build, so the program restarts. A save that leaves them
//! unchanged (a function body, a private item) swaps in the running
//! program at the next call through a trampoline.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::item::{self, type_ref};
use crate::{ImmutableString, Position, Span};

/// The module of the shadow crate's root that holds the island's runtime
/// support. The Rawr root calls its `main` to start the program.
pub(crate) const MODULE: &str = "__rawr_island";

/// The Rawr text of a native crate root: its `mod name;` declarations at
/// their places, everything else blanked (line breaks kept, so positions
/// stay the file's), and a `main` that starts the native program.
pub(crate) fn root_text(text: &str) -> String {
    let mut kept: Vec<std::ops::Range<usize>> = Vec::new();
    for declaration in super::polyglot::declarations(text) {
        if declaration.explicit_path || declaration.disabled || declaration.path.len() != 1 {
            continue;
        }
        let Some(keyword) = text[..declaration.range.start]
            .trim_end()
            .strip_suffix("mod")
        else {
            continue;
        };
        let Some(semicolon) = text[declaration.range.end..].find(';') else {
            continue;
        };
        kept.push(keyword.len()..declaration.range.end + semicolon + 1);
    }
    let mut blanked = String::with_capacity(text.len() + 64);
    for (index, character) in text.char_indices() {
        if matches!(character, '\n' | '\r') || kept.iter().any(|range| range.contains(&index)) {
            blanked.push(character);
        } else {
            blanked.push(' ');
        }
    }
    let _ = write!(
        blanked,
        "\nfn main() {{\n    crate::{MODULE}::main();\n}}\n"
    );
    blanked
}

/// The generated files of a native root's shadow crate, and their shape.
#[derive(Debug)]
pub(crate) struct Shadow {
    /// Each marked file's generated replacement, by its path relative to
    /// the crate root's directory.
    #[cfg(any(test, feature = "rust-import"))]
    pub(crate) files: Vec<(std::path::PathBuf, String)>,
    /// What native code sees of the marked modules, item by item.
    pub(crate) shape: Shape,
    /// Items native code can see that no trampoline can stand for, with
    /// why: an error before anything is built.
    #[cfg(any(test, feature = "rust-import"))]
    pub(crate) refused: Vec<String>,
}

/// What native code sees of a crate's marked modules: each item's path
/// and generated text. Two shapes are equal exactly when the generated
/// files are, so an unchanged shape needs no new native build.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Shape(BTreeMap<String, Entry>);

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    kind: Kind,
    text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Type,
    Function,
    Constant,
    Other,
}

impl Shape {
    /// Whether the crate has no marked item native code sees.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Why native code must be built again when the shape goes from `self`
    /// to `newer`, in one line; `None` when they are the same.
    #[must_use]
    pub fn change(&self, newer: &Self) -> Option<String> {
        if self == newer {
            return None;
        }
        let what = |kind: Kind| match kind {
            Kind::Type => "its fields",
            Kind::Function => "its signature",
            Kind::Constant => "its value, which native code reads",
            Kind::Other => "what native code sees of it",
        };
        for (path, entry) in &newer.0 {
            match self.0.get(path) {
                None => return Some(format!("native code can now see `{path}`")),
                Some(old) if old != entry => {
                    return Some(format!("`{path}` changed {}", what(entry.kind)));
                }
                Some(_) => {}
            }
        }
        self.0
            .keys()
            .find(|path| !newer.0.contains_key(*path))
            .map(|path| format!("native code can no longer see `{path}`"))
    }
}

/// One marked source: a file, or an inline module inside one.
struct Source<'a> {
    modules: &'a [ImmutableString],
    path: &'a Path,
    text: &'a str,
    items: &'a [item::Parsed],
}

/// How a value crosses for one parameter.
#[derive(Clone, Copy)]
enum Pass {
    /// By value: a copy goes to the interpreter.
    Value,
    /// `&T`: the referenced value is copied.
    Borrow,
    /// `&mut T`: the value is copied, and the interpreter's copy comes
    /// back into the reference after the call.
    Mutable,
}

/// The crate's own struct or enum that native code sees, by name.
struct Mirrored {
    path: String,
    generic: bool,
    fields: Vec<type_ref::Type>,
}

/// Generate the shadow files of the marked sources of a crate whose root
/// is native. `module_root` is the directory of the crate root; `sources`
/// yields each marked source's module path, file, text and items.
pub(crate) fn shadow<'a>(
    module_root: &Path,
    sources: impl IntoIterator<Item = (&'a [ImmutableString], &'a Path, &'a str, &'a [item::Parsed])>,
) -> Shadow {
    let sources = sources
        .into_iter()
        .map(|(modules, path, text, items)| Source {
            modules,
            path,
            text,
            items,
        })
        .collect::<Vec<_>>();
    let generator = Generator::new(&sources);
    let mut files = Vec::new();
    let mut shape = BTreeMap::new();
    let mut refused = Vec::new();
    // An inline module is a source of its own with its file's path; the
    // file's own source has the shortest module path.
    let mut by_file: BTreeMap<&Path, Vec<&Source<'_>>> = BTreeMap::new();
    for source in &sources {
        by_file.entry(source.path).or_default().push(source);
    }
    for (path, mut group) in by_file {
        group.sort_by_key(|source| source.modules.len());
        let file = group[0];
        let Ok(relative) = path.strip_prefix(module_root) else {
            continue;
        };
        let mut text = format!(
            "// Generated by Rawr from `{}`, which starts with `// rawr`: what native code\n\
             // sees of it. Each function hands its arguments to the interpreter.\n\
             #![allow(unused_imports, dead_code, unused_variables, clippy::all)]\n",
            path.display()
        );
        text.push_str(&generator.body(file, &group, &mut shape, &mut refused));
        files.push((relative.to_path_buf(), text));
    }
    Shadow {
        #[cfg(any(test, feature = "rust-import"))]
        files,
        shape: Shape(shape),
        #[cfg(any(test, feature = "rust-import"))]
        refused,
    }
}

struct Generator<'a> {
    /// The marked crate's structs and enums native code sees, by name.
    types: BTreeMap<&'a str, Mirrored>,
    /// Those whose every field crosses.
    crossing: BTreeSet<String>,
    /// Traits declared in marked code: their impls are not mirrored.
    traits: BTreeSet<&'a str>,
}

impl<'a> Generator<'a> {
    fn new(sources: &'a [Source<'a>]) -> Self {
        let mut types = BTreeMap::new();
        let mut traits = BTreeSet::new();
        for source in sources {
            for item in source.items {
                let (name, generics, fields) = match item {
                    item::Parsed::Struct(declared) if declared.header.visibility.is_some() => (
                        declared.name.text.as_str(),
                        !declared.generics.is_empty(),
                        declared
                            .fields
                            .iter()
                            .map(|field| field.ty.clone())
                            .collect(),
                    ),
                    item::Parsed::Enum(declared) if declared.header.visibility.is_some() => (
                        declared.name.text.as_str(),
                        !declared.generics.is_empty(),
                        declared
                            .variants
                            .iter()
                            .flat_map(|variant| variant.payload.iter().cloned())
                            .collect(),
                    ),
                    item::Parsed::Trait(declared) => {
                        traits.insert(declared.name.text.as_str());
                        continue;
                    }
                    _ => continue,
                };
                types.insert(
                    name,
                    Mirrored {
                        path: path_of(source.modules, name),
                        generic: generics,
                        fields,
                    },
                );
            }
        }
        // A type crosses when every field does; start from all and drop
        // those with a field that doesn't, until none drops.
        let mut generator = Self {
            types,
            crossing: BTreeSet::new(),
            traits,
        };
        generator.crossing = generator
            .types
            .iter()
            .filter(|(_, mirrored)| !mirrored.generic)
            .map(|(name, _)| (*name).to_owned())
            .collect();
        loop {
            let dropped = generator
                .crossing
                .iter()
                .filter(|name| {
                    generator.types[name.as_str()]
                        .fields
                        .iter()
                        .any(|field| generator.crosses(field, None).is_err())
                })
                .cloned()
                .collect::<Vec<_>>();
            if dropped.is_empty() {
                break;
            }
            for name in dropped {
                generator.crossing.remove(&name);
            }
        }
        generator
    }

    /// Whether a value of `ty` crosses by the wire format; `Err` says why
    /// not. `owner` is what `Self` names.
    fn crosses(&self, ty: &type_ref::Type, owner: Option<&str>) -> Result<(), String> {
        match ty {
            type_ref::Type::Unit(_) => Ok(()),
            type_ref::Type::Tuple { .. } => Err("tuples can't cross yet".into()),
            type_ref::Type::Reference { .. } => Err("a reference can only be a parameter".into()),
            type_ref::Type::Path(path) => {
                let Some(last) = path.segments.last() else {
                    return Err("an empty path".into());
                };
                let name = last.name.text.as_str();
                let arguments = last
                    .arguments
                    .iter()
                    .map(|argument| match argument {
                        type_ref::Argument::Type(ty) => Ok(ty),
                        type_ref::Argument::Lifetime(_) => {
                            Err(format!("`{name}` with a lifetime can't cross yet"))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                match (name, arguments.as_slice()) {
                    (
                        "bool" | "char" | "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16"
                        | "u32" | "u64" | "usize" | "f32" | "f64" | "String",
                        [],
                    ) => Ok(()),
                    ("Vec" | "Option", [inner]) => self.crosses(inner, owner),
                    ("Self", []) => match owner {
                        Some(owner) if self.crossing.contains(owner) => Ok(()),
                        _ => Err("`Self` can't cross here".into()),
                    },
                    (name, []) if self.crossing.contains(name) => Ok(()),
                    (name, _) if self.types.contains_key(name) => {
                        Err(format!("`{name}` has a field that can't cross yet"))
                    }
                    (name, _) => Err(format!("a `{name}` can't cross yet")),
                }
            }
        }
    }

    /// How a parameter of `ty` crosses, written as `text` in the source.
    fn pass(&self, ty: &type_ref::Type, text: &str, owner: Option<&str>) -> Result<Pass, String> {
        let type_ref::Type::Reference {
            mutable, target, ..
        } = ty
        else {
            return self.crosses(ty, owner).map(|()| Pass::Value);
        };
        // `&str` and `&[T]` are borrowed views: copied in, never written back.
        let referent = text.trim_start_matches('&').trim_start();
        let referent = referent
            .strip_prefix('\'')
            .map_or(referent, |rest| {
                rest.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_')
            })
            .trim_start()
            .trim_start_matches("mut ")
            .trim_start();
        let view = referent.starts_with('[') || referent == "str";
        if *mutable {
            if view {
                return Err(format!("`{}` can't cross yet", text.trim()));
            }
            return self.crosses(target, owner).map(|()| Pass::Mutable);
        }
        if referent == "str" {
            return Ok(Pass::Borrow);
        }
        self.crosses(target, owner).map(|()| Pass::Borrow)
    }

    /// The generated text of `file` with its inline modules `group`.
    fn body(
        &self,
        file: &Source<'_>,
        group: &[&Source<'_>],
        shape: &mut BTreeMap<String, Entry>,
        refused: &mut Vec<String>,
    ) -> String {
        let mut items = String::new();
        let mut uses = Vec::new();
        for item in file.items {
            match item {
                item::Parsed::Use(declared) => uses.push(declared),
                item::Parsed::Module(module) => {
                    let name = module.name.text.as_str();
                    match module.inline {
                        Some(_) => {
                            let mut modules = file.modules.to_vec();
                            modules.push(module.name.text.clone());
                            let Some(inner) = group.iter().find(|source| source.modules == modules)
                            else {
                                continue;
                            };
                            let text = self.body(inner, group, shape, refused);
                            let _ = write!(items, "pub mod {name} {{\n{text}}}\n");
                        }
                        None => {
                            let _ = writeln!(items, "pub mod {name};");
                            shape.insert(
                                path_of(file.modules, name),
                                Entry {
                                    kind: Kind::Other,
                                    text: format!("mod {name};"),
                                },
                            );
                        }
                    }
                }
                item::Parsed::Struct(declared) if declared.header.visibility.is_some() => {
                    self.mirror(file, &declared.name, declared.span, None, &mut items, shape);
                    if self.crossing.contains(declared.name.text.as_str()) {
                        let text = self.cross_struct(file, declared);
                        items.push_str(&text);
                        append(shape, &path_of(file.modules, &declared.name.text), &text);
                    }
                }
                item::Parsed::Enum(declared) if declared.header.visibility.is_some() => {
                    self.mirror(file, &declared.name, declared.span, None, &mut items, shape);
                    if self.crossing.contains(declared.name.text.as_str()) {
                        let text = self.cross_enum(file, declared);
                        items.push_str(&text);
                        append(shape, &path_of(file.modules, &declared.name.text), &text);
                    }
                }
                item::Parsed::TypeAlias(declared) if declared.header.visibility.is_some() => {
                    self.mirror(
                        file,
                        &declared.name,
                        declared.span,
                        Some(Kind::Type),
                        &mut items,
                        shape,
                    );
                }
                item::Parsed::Function(declared) if declared.header.visibility.is_some() => {
                    if declared.constant {
                        self.mirror(
                            file,
                            &declared.name,
                            declared.span,
                            Some(Kind::Constant),
                            &mut items,
                            shape,
                        );
                        continue;
                    }
                    let path = path_of(file.modules, &declared.name.text);
                    match self.stub(file, declared, None, &path) {
                        Ok(text) => {
                            items.push_str(&text);
                            shape.insert(
                                path,
                                Entry {
                                    kind: Kind::Function,
                                    text,
                                },
                            );
                        }
                        Err(reason) => refused.push(reason),
                    }
                }
                item::Parsed::Impl(declared) => {
                    self.implementation(file, declared, &mut items, shape, refused);
                }
                _ => {}
            }
        }
        // Only the imports the generated text names: a function's private
        // helpers are not generated, and an import of one would not resolve.
        let named = identifiers(&items);
        let mut text = String::new();
        for declared in uses {
            // A re-export is part of what native code sees.
            if declared.header.visibility.is_some() {
                let line = without_rawr_attributes(slice(file.text, declared.span));
                let _ = writeln!(text, "{line}");
                shape.insert(
                    format!("{} uses {}", path_of(file.modules, ""), line),
                    Entry {
                        kind: Kind::Other,
                        text: line,
                    },
                );
                continue;
            }
            if matches!(declared.tree, item::UseTree::Glob { .. }) {
                let _ = writeln!(text, "{}", slice(file.text, declared.span));
                continue;
            }
            for (name, path) in declared.bindings() {
                if !named.contains(name.text.as_str()) {
                    continue;
                }
                let path = path.iter().map(ImmutableString::as_str).collect::<Vec<_>>();
                let global = if declared.global() { "::" } else { "" };
                let line = if path.last() == Some(&name.text.as_str()) {
                    format!("use {global}{};", path.join("::"))
                } else {
                    format!("use {global}{} as {};", path.join("::"), name.text)
                };
                let _ = writeln!(text, "{line}");
                shape.insert(
                    format!("{} uses {}", path_of(file.modules, ""), name.text),
                    Entry {
                        kind: Kind::Other,
                        text: line,
                    },
                );
            }
        }
        text.push_str(&items);
        text
    }

    /// Copy one item as declared, its Rawr-only attributes dropped.
    fn mirror(
        &self,
        file: &Source<'_>,
        name: &item::Name,
        span: Span,
        kind: Option<Kind>,
        items: &mut String,
        shape: &mut BTreeMap<String, Entry>,
    ) {
        let text = format!("{}\n", without_rawr_attributes(slice(file.text, span)));
        items.push_str(&text);
        shape.insert(
            path_of(file.modules, &name.text),
            Entry {
                kind: kind.unwrap_or(Kind::Type),
                text,
            },
        );
    }

    fn implementation(
        &self,
        file: &Source<'_>,
        declared: &item::Impl,
        items: &mut String,
        shape: &mut BTreeMap<String, Entry>,
        refused: &mut Vec<String>,
    ) {
        let type_ref::Type::Path(owner) = &declared.owner else {
            return;
        };
        let Some(owner) = owner
            .segments
            .last()
            .map(|segment| segment.name.text.as_str())
        else {
            return;
        };
        let Some(mirrored) = self.types.get(owner) else {
            return;
        };
        let trait_ = declared
            .trait_
            .as_ref()
            .and_then(|path| path.segments.last())
            .map(|segment| segment.name.text.as_str());
        if trait_.is_some_and(|name| self.traits.contains(name)) {
            return;
        }
        // A trait's functions are all public; an inherent impl shows only
        // its visible ones.
        let functions = declared
            .functions
            .iter()
            .filter(|function| trait_.is_some() || function.header.visibility.is_some())
            .collect::<Vec<_>>();
        if functions.is_empty() {
            return;
        }
        let header = slice(
            file.text,
            Span::new(declared.span.start(), declared.header.span.end()),
        );
        let header = header.trim_end().trim_end_matches('{').trim_end();
        let mut text = format!("{header} {{\n");
        for (name, ty) in &declared.associated {
            let _ = writeln!(
                text,
                "    type {} = {};",
                name.text,
                slice(file.text, ty.span())
            );
        }
        let prefix = match trait_ {
            Some(name) => format!("{} as {name}", mirrored.path),
            None => mirrored.path.clone(),
        };
        for function in functions {
            let path = format!("{}::{}", mirrored.path, function.name.text);
            match self.stub(file, function, Some(owner), &path) {
                Ok(stub) => {
                    for line in stub.lines() {
                        let _ = writeln!(text, "    {line}");
                    }
                    shape.insert(
                        format!("{prefix}::{}", function.name.text),
                        Entry {
                            kind: Kind::Function,
                            text: format!("{header}\n{stub}"),
                        },
                    );
                }
                Err(reason) => refused.push(reason),
            }
        }
        text.push_str("}\n");
        items.push_str(&text);
    }

    /// One function's trampoline. `owner` is the type of an associated
    /// function; `path` is what the interpreter finds it by.
    fn stub(
        &self,
        file: &Source<'_>,
        function: &item::Function,
        owner: Option<&str>,
        path: &str,
    ) -> Result<String, String> {
        if let Some(type_ref::Type::Path(output)) = &function.output {
            let first = output
                .segments
                .first()
                .map_or("", |segment| segment.name.text.as_str());
            if first.starts_with("impl") || first == type_ref::FN_TYPE {
                return Err(format!(
                    "`{path}` returns `{}`, and native code can't call it through rawr yet. \
                     Return a named type, or make it private to the marked module.",
                    slice(file.text, output.span)
                ));
            }
        }
        if function.asynchronous {
            return Err(format!(
                "`{path}` is an `async fn`, and native code can't call it through rawr yet."
            ));
        }
        // The signature as written, each parameter renamed so a pattern
        // parameter binds one name.
        let start = function.span.start();
        let end = function.body_span.start();
        let mut signature = String::new();
        let mut cursor = start;
        let mut passes = Vec::new();
        let mut reasons = Vec::new();
        if !function.generics.is_empty() {
            reasons.push("generic functions can't cross yet".to_owned());
        }
        for (index, parameter) in function.parameters.iter().enumerate() {
            signature.push_str(slice(file.text, Span::new(cursor, parameter.span.start())));
            let written = slice(file.text, parameter.ty.span());
            let _ = write!(signature, "__a{index}: {written}");
            cursor = parameter.span.end();
            match self.pass(&parameter.ty, written, owner) {
                Ok(pass) => passes.push(pass),
                Err(reason) => {
                    reasons.push(format!("parameter `{}`: {reason}", parameter.name.text))
                }
            }
        }
        signature.push_str(slice(file.text, Span::new(cursor, end)));
        let signature = without_rawr_attributes(signature.trim_end());
        if let Some(output) = &function.output
            && let Err(reason) = self.crosses(output, owner)
        {
            reasons.push(format!("its result: {reason}"));
        }
        let receiver = match &function.receiver {
            None => None,
            Some(item::Receiver::Shared(_)) => Some(Pass::Borrow),
            Some(item::Receiver::Mutable(_)) => Some(Pass::Mutable),
            Some(item::Receiver::Owned(_) | item::Receiver::OwnedMutable(_)) => Some(Pass::Value),
        };
        if receiver.is_some() && owner.is_none_or(|owner| !self.crossing.contains(owner)) {
            reasons.push("its receiver can't cross yet".to_owned());
        }
        let island = format!("crate::{MODULE}");
        let mut text = format!("{signature} {{\n");
        if !reasons.is_empty() {
            let _ = writeln!(
                text,
                "    ::core::panic!({:?})\n}}",
                format!(
                    "rawr: native code can't call `{path}` yet: {}",
                    reasons.join("; ")
                )
            );
            return Ok(text);
        }
        let owned = owner.is_some();
        let _ = writeln!(
            text,
            "    let mut __call = {island}::Call::new({path:?}, {owned});"
        );
        let mut mutable = Vec::new();
        let mut argument = |name: &str, pass: Pass, text: &mut String| match pass {
            Pass::Value => {
                let _ = writeln!(text, "    __call.argument(&{name});");
            }
            Pass::Borrow => {
                let _ = writeln!(text, "    __call.argument({name});");
            }
            Pass::Mutable => {
                let _ = writeln!(text, "    __call.mutable(&*{name});");
                mutable.push(name.to_owned());
            }
        };
        if let Some(pass) = receiver {
            argument("self", pass, &mut text);
        }
        for (index, pass) in passes.into_iter().enumerate() {
            argument(&format!("__a{index}"), pass, &mut text);
        }
        text.push_str("    let mut __reply = __call.send();\n");
        // `Self` reads as the impl's own type, at its place in the header.
        let output = function
            .output
            .as_ref()
            .map_or("()", |ty| slice(file.text, ty.span()));
        let _ = writeln!(text, "    let __value: {output} = __reply.take();");
        for name in mutable {
            let _ = writeln!(text, "    *{name} = __reply.take();");
        }
        text.push_str("    __value\n}\n");
        Ok(text)
    }

    fn cross_struct(&self, file: &Source<'_>, declared: &item::Struct) -> String {
        let name = declared.name.text.as_str();
        let path = path_of(file.modules, name);
        let island = format!("crate::{MODULE}");
        let count = declared.fields.len();
        let mut put = String::new();
        let mut take = String::new();
        for field in &declared.fields {
            let _ = writeln!(
                put,
                "        {island}::Put::put(&self.{}, out);",
                field.name.text
            );
        }
        match declared.form {
            item::StructForm::Named => {
                take.push_str(&format!("{name} {{\n"));
                for field in &declared.fields {
                    let _ = writeln!(
                        take,
                        "            {}: {island}::Take::take(input),",
                        field.name.text
                    );
                }
                take.push_str("        }");
            }
            item::StructForm::Tuple => {
                take.push_str(&format!("{name}(\n"));
                for _ in &declared.fields {
                    let _ = writeln!(take, "            {island}::Take::take(input),");
                }
                take.push_str("        )");
            }
            item::StructForm::Unit => take.push_str(name),
        }
        format!(
            "impl {island}::Put for {name} {{\n    fn put(&self, out: &mut {island}::Out) {{\n        \
             out.structure({path:?}, {count});\n{put}    }}\n}}\n\
             impl {island}::Take for {name} {{\n    fn take(input: &mut {island}::In<'_>) -> Self {{\n        \
             input.structure({path:?}, {count});\n        {take}\n    }}\n}}\n"
        )
    }

    fn cross_enum(&self, file: &Source<'_>, declared: &item::Enum) -> String {
        let name = declared.name.text.as_str();
        let path = path_of(file.modules, name);
        let island = format!("crate::{MODULE}");
        let mut put = String::new();
        let mut take = String::new();
        for variant in &declared.variants {
            let variant_name = variant.name.text.as_str();
            let count = variant.payload.len();
            let bindings = (0..count)
                .map(|index| format!("__f{index}"))
                .collect::<Vec<_>>();
            let pattern = match &variant.fields {
                Some(fields) => format!(
                    "Self::{variant_name} {{ {} }}",
                    fields
                        .iter()
                        .zip(&bindings)
                        .map(|(field, binding)| format!("{}: {binding}", field.text))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None if count == 0 => format!("Self::{variant_name}"),
                None => format!("Self::{variant_name}({})", bindings.join(", ")),
            };
            let _ = writeln!(
                put,
                "            {pattern} => {{\n                out.variant({path:?}, {variant_name:?}, {count});"
            );
            for binding in &bindings {
                let _ = writeln!(put, "                {island}::Put::put({binding}, out);");
            }
            put.push_str("            }\n");
            let value = |()| format!("{island}::Take::take(input)");
            let built = match &variant.fields {
                Some(fields) => format!(
                    "Self::{variant_name} {{ {} }}",
                    fields
                        .iter()
                        .map(|field| format!("{}: {}", field.text, value(())))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None if count == 0 => format!("Self::{variant_name}"),
                None => format!(
                    "Self::{variant_name}({})",
                    (0..count).map(|_| value(())).collect::<Vec<_>>().join(", ")
                ),
            };
            let _ = writeln!(take, "            ({variant_name:?}, {count}) => {built},");
        }
        format!(
            "impl {island}::Put for {name} {{\n    fn put(&self, out: &mut {island}::Out) {{\n        \
             match self {{\n{put}        }}\n    }}\n}}\n\
             impl {island}::Take for {name} {{\n    fn take(input: &mut {island}::In<'_>) -> Self {{\n        \
             match input.variant({path:?}) {{\n{take}            (variant, count) => input.unknown({path:?}, variant, count),\n        }}\n    }}\n}}\n"
        )
    }
}

/// Append `text` to the shape entry of `path`.
fn append(shape: &mut BTreeMap<String, Entry>, path: &str, text: &str) {
    if let Some(entry) = shape.get_mut(path) {
        entry.text.push_str(text);
    }
}

/// `modules::name`, from the crate root.
fn path_of(modules: &[ImmutableString], name: &str) -> String {
    modules
        .iter()
        .map(ImmutableString::as_str)
        .chain((!name.is_empty()).then_some(name))
        .collect::<Vec<_>>()
        .join("::")
}

/// The text `span` covers: positions are lines and 1-based character
/// columns, the end exclusive.
fn slice(text: &str, span: Span) -> &str {
    let start = offset(text, span.start());
    let end = offset(text, span.end()).max(start);
    &text[start..end]
}

fn offset(text: &str, position: Position) -> usize {
    let (Some(line), Some(column)) = (position.line(), position.position()) else {
        return text.len();
    };
    let mut start = 0;
    for _ in 1..line {
        match text[start..].find('\n') {
            Some(at) => start += at + 1,
            None => return text.len(),
        }
    }
    text[start..]
        .char_indices()
        .nth(column.saturating_sub(1))
        .map_or(text.len(), |(at, _)| start + at)
}

/// `text` without the attributes only Rawr reads (`#[preview]`,
/// `#[widget]`, `#[rawr::…]`).
fn without_rawr_attributes(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("#[") {
        let (before, attribute) = rest.split_at(at);
        kept.push_str(before);
        let Some(close) = attribute.find(']') else {
            kept.push_str(attribute);
            return kept;
        };
        let inner = attribute[2..close].trim();
        let rawr = inner.starts_with("rawr::")
            || inner.starts_with("preview")
            || inner.starts_with("widget");
        if rawr {
            rest = attribute[close + 1..].trim_start_matches([' ', '\t']);
            rest = rest.strip_prefix('\n').unwrap_or(rest);
        } else {
            kept.push_str(&attribute[..=close]);
            rest = &attribute[close + 1..];
        }
    }
    kept.push_str(rest);
    kept
}

/// Every identifier in `text`.
fn identifiers(text: &str) -> BTreeSet<&str> {
    text.split(|c: char| !(c == '_' || c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect()
}

/// The runtime support of a native root's shadow crate, appended to its
/// root as the module [`MODULE`]. `main` is whether the crate root has a
/// `fn main` to start.
#[cfg(feature = "rust-import")]
pub(crate) fn support(main: bool) -> String {
    let entry = if main {
        "Exit::code(super::main())"
    } else {
        "{ ::std::eprintln!(\"rawr: the crate root has no `main` to run\"); 0 }"
    };
    SUPPORT.replace("ENTRY", entry)
}

/// The wire format matches [`crate::island`]'s.
#[cfg(feature = "rust-import")]
const SUPPORT: &str = r#"
/// Generated by Rawr: how the native code of this crate reaches the marked
/// modules, which Rawr interprets. `cargo build` never sees this module.
#[doc(hidden)]
#[allow(unsafe_code, dead_code, unused_imports, clippy::all)]
pub mod __rawr_island {
    // Defined by the `rawr` crate the runner links.
    unsafe extern "Rust" {
        fn __rawr_island_call_v1(path: &str, arguments: &[u8]) -> ::std::vec::Vec<u8>;
        fn __rawr_island_start_v1(entry: fn() -> i32);
    }

    /// Start the program: the runner calls the native `main` once this
    /// returns, on a thread of its own.
    pub fn main() {
        // SAFETY: the runner links `rawr`, which defines the function with
        // this exact signature.
        unsafe { __rawr_island_start_v1(entry) }
    }

    fn entry() -> i32 {
        ENTRY
    }

    trait Exit {
        fn code(self) -> i32;
    }
    impl Exit for () {
        fn code(self) -> i32 {
            0
        }
    }
    impl<E: ::std::fmt::Debug> Exit for ::std::result::Result<(), E> {
        fn code(self) -> i32 {
            match self {
                Ok(()) => 0,
                Err(error) => {
                    ::std::eprintln!("Error: {error:?}");
                    1
                }
            }
        }
    }

    const UNIT: u8 = 0;
    const FALSE: u8 = 1;
    const TRUE: u8 = 2;
    const INT: u8 = 3;
    const FLOAT: u8 = 4;
    const CHAR: u8 = 5;
    const STR: u8 = 6;
    const ARRAY: u8 = 7;
    const NONE: u8 = 10;
    const SOME: u8 = 11;
    const STRUCT: u8 = 15;
    const ENUM: u8 = 16;

    /// A value on its way to the interpreter.
    pub struct Out(::std::vec::Vec<u8>);

    impl Out {
        fn byte(&mut self, byte: u8) {
            self.0.push(byte);
        }
        fn count(&mut self, count: usize) {
            self.0.extend_from_slice(&(count as u64).to_le_bytes());
        }
        fn text(&mut self, text: &str) {
            self.count(text.len());
            self.0.extend_from_slice(text.as_bytes());
        }
        pub fn structure(&mut self, path: &str, fields: usize) {
            self.byte(STRUCT);
            self.text(path);
            self.count(fields);
        }
        pub fn variant(&mut self, path: &str, variant: &str, fields: usize) {
            self.byte(ENUM);
            self.text(path);
            self.text(variant);
            self.count(fields);
        }
    }

    /// A value on its way back from the interpreter.
    pub struct In<'a> {
        bytes: &'a [u8],
        path: &'a str,
    }

    impl<'a> In<'a> {
        fn fail(&self, what: &str) -> ! {
            ::std::panic!("rawr: the interpreter's answer to `{}` is malformed: {what}", self.path)
        }
        fn take(&mut self, length: usize) -> &'a [u8] {
            if self.bytes.len() < length {
                self.fail("it ended early");
            }
            let (head, tail) = self.bytes.split_at(length);
            self.bytes = tail;
            head
        }
        fn byte(&mut self) -> u8 {
            self.take(1)[0]
        }
        fn expect(&mut self, tag: u8, what: &str) {
            if self.byte() != tag {
                self.fail(&::std::format!("expected {what}"));
            }
        }
        fn eight(&mut self) -> [u8; 8] {
            let mut bytes = [0; 8];
            bytes.copy_from_slice(self.take(8));
            bytes
        }
        fn count(&mut self) -> usize {
            u64::from_le_bytes(self.eight()) as usize
        }
        fn text(&mut self) -> &'a str {
            let length = self.count();
            match ::std::str::from_utf8(self.take(length)) {
                Ok(text) => text,
                Err(_) => self.fail("a string is not UTF-8"),
            }
        }
        pub fn structure(&mut self, path: &str, fields: usize) {
            self.expect(STRUCT, path);
            let (found, count) = (self.text(), self.count());
            if found != path || count != fields {
                self.fail(&::std::format!(
                    "expected `{path}` with {fields} fields, found `{found}` with {count}; \
                     restart `rawr run` if the struct changed"
                ));
            }
        }
        pub fn variant(&mut self, path: &str) -> (&'a str, usize) {
            self.expect(ENUM, path);
            let found = self.text();
            if found != path {
                self.fail(&::std::format!("expected `{path}`, found `{found}`"));
            }
            (self.text(), self.count())
        }
        pub fn unknown(&self, path: &str, variant: &str, count: usize) -> ! {
            self.fail(&::std::format!("`{path}` has no variant `{variant}` with {count} fields"))
        }
    }

    /// A value that goes to the interpreter.
    pub trait Put {
        fn put(&self, out: &mut Out);
    }

    /// A value that comes back from the interpreter.
    pub trait Take: Sized {
        fn take(input: &mut In<'_>) -> Self;
    }

    impl Put for () {
        fn put(&self, out: &mut Out) {
            out.byte(UNIT);
        }
    }
    impl Take for () {
        fn take(input: &mut In<'_>) -> Self {
            input.expect(UNIT, "`()`");
        }
    }
    impl Put for bool {
        fn put(&self, out: &mut Out) {
            out.byte(if *self { TRUE } else { FALSE });
        }
    }
    impl Take for bool {
        fn take(input: &mut In<'_>) -> Self {
            match input.byte() {
                TRUE => true,
                FALSE => false,
                _ => input.fail("expected a `bool`"),
            }
        }
    }
    impl Put for char {
        fn put(&self, out: &mut Out) {
            out.byte(CHAR);
            out.0.extend_from_slice(&u32::from(*self).to_le_bytes());
        }
    }
    impl Take for char {
        fn take(input: &mut In<'_>) -> Self {
            input.expect(CHAR, "a `char`");
            let mut bytes = [0; 4];
            bytes.copy_from_slice(input.take(4));
            match char::from_u32(u32::from_le_bytes(bytes)) {
                Some(character) => character,
                None => input.fail("not a `char`"),
            }
        }
    }
    macro_rules! integers {
        ($($ty:ty),*) => {$(
            impl Put for $ty {
                fn put(&self, out: &mut Out) {
                    match i64::try_from(*self) {
                        Ok(value) => {
                            out.byte(INT);
                            out.0.extend_from_slice(&value.to_le_bytes());
                        }
                        Err(_) => ::std::panic!(
                            "rawr: {} is out of range for the interpreter's integers", self
                        ),
                    }
                }
            }
            impl Take for $ty {
                fn take(input: &mut In<'_>) -> Self {
                    input.expect(INT, ::std::stringify!($ty));
                    let value = i64::from_le_bytes(input.eight());
                    match <$ty>::try_from(value) {
                        Ok(value) => value,
                        Err(_) => input.fail(&::std::format!(
                            "{value} is out of range for `{}`", ::std::stringify!($ty)
                        )),
                    }
                }
            }
        )*};
    }
    integers!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
    impl Put for f64 {
        fn put(&self, out: &mut Out) {
            out.byte(FLOAT);
            out.0.extend_from_slice(&self.to_le_bytes());
        }
    }
    impl Take for f64 {
        fn take(input: &mut In<'_>) -> Self {
            input.expect(FLOAT, "an `f64`");
            f64::from_le_bytes(input.eight())
        }
    }
    impl Put for f32 {
        fn put(&self, out: &mut Out) {
            f64::from(*self).put(out);
        }
    }
    impl Take for f32 {
        fn take(input: &mut In<'_>) -> Self {
            f64::take(input) as f32
        }
    }
    impl Put for str {
        fn put(&self, out: &mut Out) {
            out.byte(STR);
            out.text(self);
        }
    }
    impl Put for ::std::string::String {
        fn put(&self, out: &mut Out) {
            self.as_str().put(out);
        }
    }
    impl Take for ::std::string::String {
        fn take(input: &mut In<'_>) -> Self {
            input.expect(STR, "a string");
            input.text().to_owned()
        }
    }
    impl<T: Put> Put for [T] {
        fn put(&self, out: &mut Out) {
            out.byte(ARRAY);
            out.count(self.len());
            for item in self {
                item.put(out);
            }
        }
    }
    impl<T: Put> Put for ::std::vec::Vec<T> {
        fn put(&self, out: &mut Out) {
            self.as_slice().put(out);
        }
    }
    impl<T: Take> Take for ::std::vec::Vec<T> {
        fn take(input: &mut In<'_>) -> Self {
            input.expect(ARRAY, "a `Vec`");
            let count = input.count();
            (0..count).map(|_| T::take(input)).collect()
        }
    }
    impl<T: Put> Put for ::std::option::Option<T> {
        fn put(&self, out: &mut Out) {
            match self {
                None => out.byte(NONE),
                Some(value) => {
                    out.byte(SOME);
                    value.put(out);
                }
            }
        }
    }
    impl<T: Take> Take for ::std::option::Option<T> {
        fn take(input: &mut In<'_>) -> Self {
            match input.byte() {
                NONE => None,
                SOME => Some(T::take(input)),
                _ => input.fail("expected an `Option`"),
            }
        }
    }
    impl<T: Put + ?Sized> Put for &T {
        fn put(&self, out: &mut Out) {
            (**self).put(out);
        }
    }

    /// One call of a marked function.
    pub struct Call {
        path: &'static str,
        owned: bool,
        count: u32,
        mutable: u64,
        values: Out,
    }

    impl Call {
        /// A call of the function at `path` from the crate root; `owned`
        /// when its last segment is a member of the type before it.
        pub fn new(path: &'static str, owned: bool) -> Self {
            Self {
                path,
                owned,
                count: 0,
                mutable: 0,
                values: Out(::std::vec::Vec::new()),
            }
        }
        pub fn argument<T: Put + ?Sized>(&mut self, value: &T) {
            value.put(&mut self.values);
            self.count += 1;
        }
        /// An argument the call may change, which comes back after it.
        pub fn mutable<T: Put + ?Sized>(&mut self, value: &T) {
            self.mutable |= 1 << self.count;
            self.argument(value);
        }
        pub fn send(self) -> Reply {
            let mut bytes = ::std::vec::Vec::with_capacity(13 + self.values.0.len());
            bytes.push(u8::from(self.owned));
            bytes.extend_from_slice(&self.count.to_le_bytes());
            bytes.extend_from_slice(&self.mutable.to_le_bytes());
            bytes.extend_from_slice(&self.values.0);
            // SAFETY: the runner links `rawr`, which defines the function
            // with this exact signature.
            let reply = unsafe { __rawr_island_call_v1(self.path, &bytes) };
            match reply.first() {
                Some(0) => Reply { bytes: reply, at: 1, path: self.path },
                Some(_) => {
                    let message = ::std::string::String::from_utf8_lossy(&reply[1..]).into_owned();
                    ::std::panic!("{message}")
                }
                None => ::std::panic!("rawr: no answer to `{}`", self.path),
            }
        }
    }

    /// What a call returned, then each argument it could change.
    pub struct Reply {
        bytes: ::std::vec::Vec<u8>,
        at: usize,
        path: &'static str,
    }

    impl Reply {
        pub fn take<T: Take>(&mut self) -> T {
            let mut input = In { bytes: &self.bytes[self.at..], path: self.path };
            let value = T::take(&mut input);
            self.at = self.bytes.len() - input.bytes.len();
            value
        }
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn generate(text: &str) -> Shadow {
        let unit = crate::Engine::new().parse_unit(text).unwrap();
        let modules = [ImmutableString::from("physics")];
        shadow(
            Path::new("/crate/src"),
            [(
                &modules[..],
                Path::new("/crate/src/physics.rs"),
                text,
                unit.items(),
            )],
        )
    }

    #[test]
    fn a_root_keeps_only_its_module_declarations() {
        let text = "//! Native.\nmod physics;\npub mod util;\n#[cfg(test)]\nmod tests;\n\
                    fn main() { physics::run(); }\n";
        let rawr = root_text(text);
        let lines = rawr.lines().collect::<Vec<_>>();
        assert_eq!(lines[0].trim(), "");
        assert_eq!(lines[1], "mod physics;");
        assert_eq!(lines[2], "    mod util;");
        assert!(
            lines[3..6].iter().all(|line| line.trim().is_empty()),
            "{rawr}"
        );
        assert!(rawr.ends_with("fn main() {\n    crate::__rawr_island::main();\n}\n"));
    }

    #[test]
    fn visible_items_get_trampolines_and_private_ones_nothing() {
        let shadow = generate(
            "// rawr\nuse std::collections::HashMap;\nuse crate::util::Scale;\n\
             const G: f64 = 9.8;\npub const LIMIT: f64 = 3.0;\n\
             #[derive(Clone, Copy, Debug)]\npub struct Ball {\n    pub x: f64,\n    pub moving: bool,\n}\n\
             impl Ball {\n    pub fn new(x: f64) -> Ball { Ball { x, moving: true } }\n    \
             pub fn advance(&mut self, (a, b): (f64, f64)) { self.x += a * G; }\n    \
             fn private(&self) -> f64 { self.x }\n}\n\
             pub fn render(ball: &Ball, width: usize, label: &str) -> String { String::new() }\n\
             pub fn scale(s: Scale) -> f64 { 1.0 }\n\
             fn helper(map: HashMap<String, f64>) {}\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(path, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        assert_eq!(path, Path::new("physics.rs"));
        // The private constant, method and function are not there, nor is
        // the import only the private function needs.
        assert!(!text.contains("G: f64"), "{text}");
        assert!(!text.contains("private"), "{text}");
        assert!(!text.contains("helper"), "{text}");
        assert!(!text.contains("HashMap"), "{text}");
        assert!(text.contains("use crate::util::Scale;\n"), "{text}");
        assert!(text.contains("pub const LIMIT: f64 = 3.0;\n"), "{text}");
        assert!(
            text.contains("#[derive(Clone, Copy, Debug)]\npub struct Ball {\n    pub x: f64,"),
            "{text}"
        );
        assert!(
            text.contains("out.structure(\"physics::Ball\", 2);"),
            "{text}"
        );
        assert!(
            text.contains(
                "    pub fn advance(&mut self, __a0: (f64, f64)) {\n        \
                 ::core::panic!(\"rawr: native code can't call `physics::Ball::advance` yet: \
                 parameter `__rawr_parameter_0`: tuples can't cross yet\")"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "pub fn render(__a0: &Ball, __a1: usize, __a2: &str) -> String {\n    \
                 let mut __call = crate::__rawr_island::Call::new(\"physics::render\", false);\n    \
                 __call.argument(__a0);\n    __call.argument(&__a1);\n    __call.argument(__a2);\n    \
                 let mut __reply = __call.send();\n    let __value: String = __reply.take();\n    __value\n}\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("::core::panic!(\"rawr: native code can't call `physics::scale` yet: parameter `s`: a `Scale` can't cross yet\")"),
            "{text}"
        );
        let paths = shadow.shape.0.keys().cloned().collect::<Vec<_>>();
        assert_eq!(
            paths,
            [
                "physics uses Scale",
                "physics::Ball",
                "physics::Ball::advance",
                "physics::Ball::new",
                "physics::LIMIT",
                "physics::render",
                "physics::scale",
            ]
        );
    }

    #[test]
    fn a_body_edit_keeps_the_shape_and_a_layout_edit_names_the_change() {
        let before = generate(
            "// rawr\npub struct Ball { pub x: f64 }\nconst G: f64 = 9.8;\n\
             pub fn fall(ball: &mut Ball) { ball.x -= G; }\n",
        );
        let body = generate(
            "// rawr\npub struct Ball { pub x: f64 }\nconst G: f64 = 1.6;\n\
             pub fn fall(ball: &mut Ball) { ball.x -= G * 2.0; }\n",
        );
        assert_eq!(before.files, body.files);
        assert_eq!(before.shape.change(&body.shape), None);

        let layout = generate(
            "// rawr\npub struct Ball { pub x: f64, pub y: f64 }\nconst G: f64 = 9.8;\n\
             pub fn fall(ball: &mut Ball) { ball.x -= G; }\n",
        );
        assert_ne!(before.files, layout.files);
        assert_eq!(
            before.shape.change(&layout.shape).as_deref(),
            Some("`physics::Ball` changed its fields")
        );
        let signature = generate(
            "// rawr\npub struct Ball { pub x: f64 }\nconst G: f64 = 9.8;\n\
             pub fn fall(ball: &mut Ball, by: f64) { ball.x -= by; }\n",
        );
        assert_eq!(
            before.shape.change(&signature.shape).as_deref(),
            Some("`physics::fall` changed its signature")
        );
    }

    #[test]
    fn enums_cross_by_variant_and_impl_trait_results_are_refused() {
        let shadow = generate(
            "// rawr\npub enum Shape {\n    Dot,\n    Circle(f64),\n    Box { width: f64, height: f64 },\n}\n\
             pub fn area(shape: Shape) -> f64 { 0.0 }\n\
             pub fn points() -> impl Iterator<Item = i64> { 0..3 }\n",
        );
        let text = &shadow.files[0].1;
        assert!(text.contains("Self::Circle(__f0) => {"), "{text}");
        assert!(
            text.contains("(\"Box\", 2) => Self::Box { width: crate::__rawr_island::Take::take(input), height: crate::__rawr_island::Take::take(input) },"),
            "{text}"
        );
        assert!(text.contains("__call.argument(&__a0);"), "{text}");
        assert_eq!(shadow.refused.len(), 1);
        assert!(
            shadow.refused[0].starts_with("`physics::points` returns `impl Iterator<Item = i64>`"),
            "{:?}",
            shadow.refused
        );
    }
}
