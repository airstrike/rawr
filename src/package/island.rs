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
//! numbers, `bool`, `char`, strings, `Vec`, `Option`, `Result`, tuples and
//! the marked crate's own structs and enums made of them or of native
//! values (#1466). A borrow of such a value lends the interpreter each
//! native part where it is, `&mut` too, a value moves its parts, and a
//! `&self` method's reference into a part comes back where the part is
//! (#1469, #1468), inside an `Option`, a `Vec`, a `Box` or a tuple too
//! (#1475): a part a `&mut` call left where it was stays, and an element
//! that holds one moves where the call put it.
//! Each part comes back as the value the interpreter moves. A value of a
//! native type, as the app's own `Message` or the `iced::Error` of an
//! `iced::Result`, moves itself:
//! back from the interpreter, bare or as a part of any of those, and in by
//! value, which the interpreter keeps. A borrow of one is lent for the
//! call. Every impl native code may rely on is there too: a marked type's,
//! and a trait's for a native type, as `impl From<&Theme> for Settings`. A
//! function whose values can't cross gets a trampoline that panics with a
//! message naming it.
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
use crate::{ImmutableString, Span};

/// The module of the shadow crate's root that holds the island's runtime
/// support. The Rawr root calls its `main` to start the program.
pub(crate) const MODULE: &str = "__rawr_island";

/// The name the runner links a package's native binary by, when the
/// program runs from the package's library (#1107). The binary's own
/// shadow crate is a library of this name, beside the library's.
pub(crate) const BIN: &str = "__rawr_bin";

/// The Rawr `main` that starts the native program: the crate root's own
/// `main`, or with `bin`, the native binary's.
fn start(bin: bool) -> String {
    let owner = if bin { BIN } else { "crate" };
    format!("\nfn main() {{\n    {owner}::{MODULE}::main();\n}}\n")
}

/// The Rawr text of a native crate root: its [`module_text`] and a `main`
/// that starts the native program, the binary's with `bin`.
pub(crate) fn root_text(
    text: &str,
    bin: bool,
    features: &crate::project::cargo::features::Enabled,
) -> String {
    let mut declarations = module_text(text, features);
    declarations.push_str(&start(bin));
    declarations
}

/// The Rawr text of a native module that declares marked modules, the crate
/// root or a native module on the way to a marked one (#1467): its `mod
/// name;` declarations that Cargo compiles under `features`, each with its
/// visibility, at their places, and everything else blanked (line breaks
/// kept, so positions stay the file's).
pub(crate) fn module_text(
    text: &str,
    features: &crate::project::cargo::features::Enabled,
) -> String {
    let mut kept: Vec<std::ops::Range<usize>> = Vec::new();
    for declaration in super::polyglot::declarations(text, features) {
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
        kept.push(visibility_start(keyword)..declaration.range.end + semicolon + 1);
    }
    let mut blanked = String::with_capacity(text.len() + 64);
    for (index, character) in text.char_indices() {
        if matches!(character, '\n' | '\r') || kept.iter().any(|range| range.contains(&index)) {
            blanked.push(character);
        } else {
            blanked.push(' ');
        }
    }
    blanked
}

/// Where the visibility that `before`, the text up to a `mod` keyword, ends
/// with starts (`pub`, `pub(crate)`, `pub(in crate::a)`): `before`'s end
/// when it ends with none.
fn visibility_start(before: &str) -> usize {
    let end = before.len();
    let trimmed = before.trim_end();
    let written = match trimmed.strip_suffix(')') {
        Some(inside) => match inside.rfind('(') {
            Some(open) => before[..open].trim_end(),
            None => return end,
        },
        None => trimmed,
    };
    match written.strip_suffix("pub") {
        Some(rest)
            if !rest
                .ends_with(|character: char| character == '_' || character.is_alphanumeric()) =>
        {
            rest.len()
        }
        _ => end,
    }
}

/// The generated files of a native root's shadow crate, and their shape.
#[derive(Debug)]
pub(crate) struct Shadow {
    /// Each marked file's generated replacement, by its path relative to
    /// the crate root's directory.
    #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
    pub(crate) files: Vec<(std::path::PathBuf, String)>,
    /// What native code sees of the marked modules, item by item.
    pub(crate) shape: Shape,
    /// Items native code can see that no trampoline can stand for, with
    /// why: an error before anything is built.
    #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
    pub(crate) refused: Vec<String>,
    /// Each stand-in's maker, by its type's path from the crate's root and
    /// the maker's from the shadow crate's (#1264), for [`support`] to
    /// register.
    #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
    pub(crate) objects: Vec<(String, String)>,
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
#[derive(Clone)]
enum Pass {
    /// By value: a copy goes to the interpreter.
    Value,
    /// `&T`: the referenced value is copied.
    Borrow,
    /// `&mut T`: the value is copied, and the interpreter's copy comes
    /// back into the reference after the call.
    Mutable,
    /// `&T` or `&mut T` of a type that doesn't cross, as ratatui's `&mut
    /// Frame<'_>`: lent to the interpreter for the call (D558.3), written
    /// with each named lifetime as `'_`.
    Lend { mutable: bool, referent: String },
    /// A value of a type the marked crate doesn't declare, as an app's own
    /// `Message`: moved to the interpreter, which keeps it (#1139).
    Give,
    /// A value of a type the marked crate doesn't declare whose type names
    /// a lifetime, written `ty`, as inboard's by-value `View<'a>` of
    /// borrows (#1225): moved to the interpreter for the call alone, as
    /// rustc lets the function keep nothing of it past the call but what
    /// its result borrows, which native code holds at those lifetimes.
    GiveForCall { ty: String },
    /// A borrow of a marked value that holds native parts ([`Generator::
    /// holding`]): each part lent where it is for the call, mutably for a
    /// `&mut`, and the other fields copied, a `&mut` one's copy taken back
    /// after the call (#1469).
    LendParts { mutable: bool },
    /// A marked value that holds native parts, by value: each part moved to
    /// the interpreter, as Rust moves the value (#1469).
    GiveParts,
    /// The receiver of a native trait object's stand-in: the handle of the
    /// program value it holds, which the call takes as its receiver (#1264).
    Object,
}

/// The crate's own struct or enum that native code sees.
struct Mirrored<'a> {
    generic: bool,
    fields: Vec<type_ref::Type>,
    /// The source that declares it, which its fields' types are written in.
    source: &'a Source<'a>,
}

/// What `Self` names in a signature.
#[derive(Clone, Copy)]
enum Owner<'o> {
    /// Nothing: a free function, or a stand-in's, whose `Self` doesn't cross.
    None,
    /// The marked type at this path.
    Marked(&'o str),
    /// A native type, as the crate's `Settings` of a marked `impl From<&Theme>
    /// for Settings` (#1466): its values move as native ones do.
    Native,
}

/// Each marked `pub static` whose type names none of the marked crate's
/// own types, by its path from the crate root, as `["component", "badge",
/// "BLOCKS"]` for a page's `pub static BLOCKS: &[page::Block]` (#1291).
/// The shadow crate holds it as written ([`Generator::body`]), so it is a
/// native place: marked code reads native code's item, by reference,
/// rather than build a value of its own at each read. `sources` are as
/// [`shadow`] takes them.
#[cfg(feature = "rust-import")]
pub(crate) fn native_statics<'a>(
    sources: impl IntoIterator<Item = (&'a [ImmutableString], &'a str, &'a [item::Parsed])>,
) -> Vec<Vec<ImmutableString>> {
    let sources = sources
        .into_iter()
        .map(|(modules, text, items)| Source {
            modules,
            path: Path::new(""),
            text,
            items,
        })
        .collect::<Vec<_>>();
    let generator = Generator::new(&sources);
    let mut statics = Vec::new();
    for source in &sources {
        for item in source.items {
            let item::Parsed::Function(declared) = item else {
                continue;
            };
            let keyword = slice(
                source.text,
                Span::new(declared.span.start(), declared.name.span.start()),
            );
            let Some(ty) = declared.output.as_ref() else {
                continue;
            };
            let marked = identifiers(slice(source.text, ty.span()))
                .into_iter()
                .any(|name| name == "Self" || generator.names_type(name));
            if declared.constant
                && declared.header.visibility.is_some()
                && keyword.split_whitespace().last() == Some("static")
                && !marked
            {
                let mut path = source.modules.to_vec();
                path.push(declared.name.text.clone());
                statics.push(path);
            }
        }
    }
    statics
}

/// The marked types the shadow crate mirrors so that a program value of one
/// crosses whole, by their paths from the crate root, as `["workspace",
/// "home", "Message"]`: each visible struct and enum whose every field
/// crosses. The runner holds a program value of one as its mirror where a
/// crate's generic takes it, as `Task<Message>` does (#1249). `sources` are
/// as [`native_statics`] takes them.
#[cfg(feature = "rust-import")]
pub(crate) fn mirrors<'a>(
    sources: impl IntoIterator<Item = (&'a [ImmutableString], &'a str, &'a [item::Parsed])>,
) -> Vec<Vec<ImmutableString>> {
    let sources = sources
        .into_iter()
        .map(|(modules, text, items)| Source {
            modules,
            path: Path::new(""),
            text,
            items,
        })
        .collect::<Vec<_>>();
    Generator::new(&sources)
        .crossing
        .iter()
        .map(|path| path.split("::").map(ImmutableString::from).collect())
        .collect()
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
    let mut objects = Vec::new();
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
        text.push_str(&generator.body(file, &group, &mut shape, &mut refused, &mut objects));
        files.push((relative.to_path_buf(), text));
    }
    Shadow {
        #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
        files,
        shape: Shape(shape),
        #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
        refused,
        #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
        objects,
    }
}

struct Generator<'a> {
    /// The marked crate's structs and enums native code sees, by their path
    /// from the crate root.
    types: BTreeMap<String, Mirrored<'a>>,
    /// Every struct, enum and alias the marked crate declares, visible or
    /// not, by its path: what an impl's owner names when it is marked.
    declared: BTreeSet<String>,
    /// Those of `types` whose every field crosses, by the wire format or as
    /// a native value, alone or in a container ([`Generator::held`]).
    crossing: BTreeSet<String>,
    /// Traits declared in marked code: their impls are not mirrored.
    traits: BTreeSet<&'a str>,
    /// The std traits each marked type implements, derived or written, by
    /// the type's name: what its stand-in implements too (#1301).
    implemented: BTreeMap<&'a str, BTreeSet<Std>>,
    /// Those of `crossing` that hold a native part, directly or in a field
    /// of such a type, as `enum Theme { Selected(themedef::Theme) }`: a
    /// borrow of one lends the interpreter its parts where they are, and a
    /// value moves them (#1469).
    holding: BTreeSet<String>,
}

/// A std trait a stand-in implements for the marked type it holds, as a
/// native trait's supertrait may require (#1301).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Std {
    Debug,
    Display,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Default,
    PartialOrd,
    Ord,
}

impl Std {
    /// The std trait named `name`, as a derive or an impl writes it last.
    fn named(name: &str) -> Option<Self> {
        Some(match name {
            "Debug" => Self::Debug,
            "Display" => Self::Display,
            "Clone" => Self::Clone,
            "PartialEq" => Self::PartialEq,
            "Eq" => Self::Eq,
            "Hash" => Self::Hash,
            "Default" => Self::Default,
            "PartialOrd" => Self::PartialOrd,
            "Ord" => Self::Ord,
            _ => return None,
        })
    }

    /// The stand-in `stand_in`'s impl of the trait, each function asking
    /// the interpreter for the program value's own (`island::Object`), the
    /// type at `owner`'s.
    fn implementation(self, stand_in: &str, owner: &str) -> String {
        let island = format!("crate::{MODULE}");
        let body = match self {
            Self::Debug | Self::Display => {
                let which = if self == Self::Debug { "DEBUG" } else { "DISPLAY" };
                format!(
                    "    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        \
                     f.write_str(&self.0.text({owner:?}, {island}::{which}))\n    }}\n"
                )
            }
            Self::Clone => format!(
                "    fn clone(&self) -> Self {{\n        Self(self.0.duplicate({owner:?}))\n    }}\n"
            ),
            Self::PartialEq => format!(
                "    fn eq(&self, other: &Self) -> bool {{\n        self.0.equal(&other.0, {owner:?})\n    }}\n"
            ),
            Self::Eq => String::new(),
            Self::Hash => format!(
                "    fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) {{\n        \
                 state.write_u64(self.0.hash({owner:?}))\n    }}\n"
            ),
            Self::Default => format!(
                "    fn default() -> Self {{\n        Self({island}::Object::default_of({owner:?}))\n    }}\n"
            ),
            Self::PartialOrd => format!(
                "    fn partial_cmp(&self, other: &Self) -> ::std::option::Option<::std::cmp::Ordering> {{\n        \
                 self.0.compare(&other.0, {owner:?}, false)\n    }}\n"
            ),
            Self::Ord => format!(
                "    fn cmp(&self, other: &Self) -> ::std::cmp::Ordering {{\n        \
                 self.0.compare(&other.0, {owner:?}, true).unwrap_or(::std::cmp::Ordering::Equal)\n    }}\n"
            ),
        };
        let path = match self {
            Self::Debug => "::std::fmt::Debug",
            Self::Display => "::std::fmt::Display",
            Self::Clone => "::std::clone::Clone",
            Self::PartialEq => "::std::cmp::PartialEq",
            Self::Eq => "::std::cmp::Eq",
            Self::Hash => "::std::hash::Hash",
            Self::Default => "::std::default::Default",
            Self::PartialOrd => "::std::cmp::PartialOrd",
            Self::Ord => "::std::cmp::Ord",
        };
        format!("impl {path} for {stand_in} {{\n{body}}}\n")
    }
}

impl<'a> Generator<'a> {
    fn new(sources: &'a [Source<'a>]) -> Self {
        let mut types = BTreeMap::new();
        let mut named = BTreeSet::new();
        let mut traits = BTreeSet::new();
        let mut implemented = BTreeMap::<&str, BTreeSet<Std>>::new();
        for source in sources {
            for item in source.items {
                for (name, std) in std_impls(item) {
                    implemented.entry(name).or_default().insert(std);
                }
                if let item::Parsed::Struct(item::Struct { name, .. })
                | item::Parsed::Enum(item::Enum { name, .. })
                | item::Parsed::TypeAlias(item::TypeAlias { name, .. }) = item
                {
                    named.insert(path_of(source.modules, &name.text));
                }
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
                    path_of(source.modules, name),
                    Mirrored {
                        generic: generics,
                        fields,
                        source,
                    },
                );
            }
        }
        // A marked trait named as a std one is the program's own.
        for implemented in implemented.values_mut() {
            implemented.retain(|std| !traits.contains(format!("{std:?}").as_str()));
        }
        // A type crosses when every field does; start from all and drop
        // those with a field that doesn't, until none drops.
        let mut generator = Self {
            types,
            declared: named,
            crossing: BTreeSet::new(),
            traits,
            implemented,
            holding: BTreeSet::new(),
        };
        generator.crossing = generator
            .types
            .iter()
            .filter(|(_, mirrored)| !mirrored.generic)
            .map(|(path, _)| path.clone())
            .collect();
        loop {
            let dropped = generator
                .crossing
                .iter()
                .filter(|path| {
                    let mirrored = &generator.types[path.as_str()];
                    mirrored.fields.iter().any(|field| {
                        generator
                            .held(mirrored.source, field, Owner::Marked(path))
                            .is_err()
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            if dropped.is_empty() {
                break;
            }
            for path in dropped {
                generator.crossing.remove(&path);
            }
        }
        // A type holds a native part when a field is one or is a type that
        // holds one; add those till none is added.
        loop {
            let added = generator
                .crossing
                .iter()
                .filter(|path| !generator.holding.contains(*path))
                .filter(|path| {
                    let mirrored = &generator.types[path.as_str()];
                    let owner = Owner::Marked(path);
                    mirrored.fields.iter().any(|field| {
                        !matches!(
                            generator.held(mirrored.source, field, owner),
                            Ok(Held::Wire)
                        )
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            if added.is_empty() {
                break;
            }
            generator.holding.extend(added);
        }
        generator
    }

    /// Whether `ty`, written in `file`, is a marked type that holds a
    /// native part ([`Generator::holding`]), by its path or as `Self`.
    fn holds(&self, file: &Source<'_>, ty: &type_ref::Type, owner: Owner<'_>) -> bool {
        let type_ref::Type::Path(path) = ty else {
            return false;
        };
        let Some(last) = path.segments.last() else {
            return false;
        };
        if !last.arguments.is_empty() {
            return false;
        }
        match (last.name.text.as_str(), owner) {
            ("Self", Owner::Marked(owner)) if path.segments.len() == 1 => {
                self.holding.contains(owner)
            }
            _ => self
                .marked(file, path)
                .is_some_and(|marked| self.holding.contains(marked)),
        }
    }

    /// The path from the crate root of the marked type `path` names, written
    /// in `file` (#1466): by its name in its own module or through a `use`,
    /// or by a path through `crate`, `self`, `super` or a child module. A
    /// type of a native module or of a dependency of the same name, as
    /// `themedef::Theme` or `iced::Theme` beside a marked `Theme`, is not
    /// it.
    fn marked(&self, file: &Source<'_>, path: &type_ref::Path) -> Option<&str> {
        if path.global {
            return None;
        }
        let names = path
            .segments
            .iter()
            .map(|segment| segment.name.text.as_str())
            .collect::<Vec<_>>();
        let found = |absolute: Vec<String>| {
            self.types
                .get_key_value(absolute.join("::").as_str())
                .map(|(path, _)| path.as_str())
        };
        if let Some(found) = absolute(file, &names, 0).and_then(found) {
            return Some(found);
        }
        // A bare name a glob brings in, `use crate::theme::*;`.
        let [name] = names[..] else {
            return None;
        };
        file.items.iter().find_map(|item| match item {
            item::Parsed::Use(used) if !used.global() && used.scope.is_none() => {
                let item::UseTree::Glob { prefix, .. } = &used.tree else {
                    return None;
                };
                let prefix = prefix
                    .segments
                    .iter()
                    .map(|segment| segment.name.text.as_str())
                    .collect::<Vec<_>>();
                let mut module = absolute(file, &prefix, 0)?;
                module.push(name.to_owned());
                found(module)
            }
            _ => None,
        })
    }

    /// Whether `path`, written in `file`, names a struct, enum or alias the
    /// marked crate declares, visible to native code or not.
    fn declares(&self, file: &Source<'_>, path: &type_ref::Path) -> bool {
        let names = path
            .segments
            .iter()
            .map(|segment| segment.name.text.as_str())
            .collect::<Vec<_>>();
        !path.global
            && absolute(file, &names, 0)
                .is_some_and(|absolute| self.declared.contains(&absolute.join("::")))
    }

    /// Whether a marked type is named `name` in some module, as a word of
    /// a signature's text may name it.
    #[cfg(feature = "rust-import")]
    fn names_type(&self, name: &str) -> bool {
        self.types
            .keys()
            .any(|path| path.rsplit("::").next() == Some(name))
    }

    /// How a field of `ty` crosses as part of its marked type's value
    /// ([`Held`]): by the wire format, as a native value of its own, as the
    /// `themedef::Theme` of a marked `enum Theme { Selected(themedef::Theme)
    /// }` (#1466), as a marked type that holds such values, or as an
    /// `Option`, `Vec`, `Box` or tuple of any of them, as the
    /// `Option<wizard::State>` of inboard's `home::State` (#1475). `Err`
    /// says why it can't.
    fn held<'f>(
        &self,
        file: &Source<'f>,
        ty: &type_ref::Type,
        owner: Owner<'_>,
    ) -> Result<Held<'f>, String> {
        if self.holds(file, ty, owner) {
            return Ok(Held::Marked);
        }
        let crossed = self.crosses(file, ty, owner);
        match ty {
            type_ref::Type::Tuple { members, .. } => {
                let members = members
                    .iter()
                    .map(|member| self.held(file, member, owner))
                    .collect::<Result<Vec<_>, _>>()?;
                if members.iter().all(|member| matches!(member, Held::Wire)) {
                    return crossed.map(|()| Held::Wire);
                }
                Ok(Held::Tuple(members))
            }
            type_ref::Type::Path(path) if let Some((name, inner)) = container(path) => {
                let inner = self.held(file, inner, owner)?;
                if matches!(inner, Held::Wire) && crossed.is_ok() {
                    return Ok(Held::Wire);
                }
                let inner = Box::new(inner);
                Ok(match name {
                    Container::Option => Held::Option(inner),
                    Container::Vec => Held::Vec(inner),
                    Container::Box => Held::Boxed(inner),
                })
            }
            _ => {
                let Err(reason) = crossed else {
                    return Ok(Held::Wire);
                };
                let written = slice(file.text, ty.span());
                if self.lendable(file, ty, owner) && static_lifetimes(written) == written {
                    Ok(Held::Native(written))
                } else {
                    Err(reason)
                }
            }
        }
    }

    /// Whether a value of `ty` crosses by the wire format; `Err` says why
    /// not. `owner` is what `Self` names.
    fn crosses(
        &self,
        file: &Source<'_>,
        ty: &type_ref::Type,
        owner: Owner<'_>,
    ) -> Result<(), String> {
        match ty {
            type_ref::Type::Unit(_) => Ok(()),
            type_ref::Type::Tuple { members, .. } => members
                .iter()
                .try_for_each(|member| self.crosses(file, member, owner)),
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
                // A std type by its own name or by its path, as
                // `std::string::String`.
                let single = path.segments.len() == 1
                    || matches!(
                        path.segments[0].name.text.as_str(),
                        "std" | "core" | "alloc"
                    );
                match (name, arguments.as_slice()) {
                    (
                        "bool" | "char" | "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16"
                        | "u32" | "u64" | "usize" | "f32" | "f64" | "String",
                        [],
                    ) if single => Ok(()),
                    ("Vec" | "Option" | crate::item::type_ref::SLICE_TYPE, [inner]) if single => {
                        self.crosses(file, inner, owner)
                    }
                    ("Result", [ok, err]) if single => {
                        self.crosses(file, ok, owner)?;
                        self.crosses(file, err, owner)
                    }
                    ("Self", []) if path.segments.len() == 1 => match owner {
                        Owner::Marked(owner) if self.crossing.contains(owner) => Ok(()),
                        _ => Err("`Self` can't cross here".into()),
                    },
                    _ => match self.marked(file, path) {
                        Some(marked) if arguments.is_empty() && self.crossing.contains(marked) => {
                            Ok(())
                        }
                        Some(_) => Err(format!("`{name}` has a field that can't cross yet")),
                        None => Err(format!("a `{name}` can't cross yet")),
                    },
                }
            }
        }
    }

    /// How a parameter of `ty` crosses, written as `text` in the source.
    fn pass(
        &self,
        file: &Source<'_>,
        ty: &type_ref::Type,
        text: &str,
        owner: Owner<'_>,
    ) -> Result<Pass, String> {
        let type_ref::Type::Reference {
            mutable, target, ..
        } = ty
        else {
            return match self.crosses(file, ty, owner) {
                Ok(()) if self.holds(file, ty, owner) => Ok(Pass::GiveParts),
                Ok(()) => Ok(Pass::Value),
                // The interpreter keeps it in its arena as long as it
                // likes, unless it borrows something: then for the call
                // alone (#1225).
                Err(_) if self.lendable(file, ty, owner) && static_lifetimes(text) == text => {
                    Ok(Pass::Give)
                }
                Err(_) if self.lendable(file, ty, owner) => Ok(Pass::GiveForCall {
                    ty: text.trim().to_owned(),
                }),
                Err(reason) => Err(reason),
            };
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
        if *mutable && view {
            return Err(format!("`{}` can't cross yet", text.trim()));
        }
        if referent == "str" {
            return Ok(Pass::Borrow);
        }
        match self.crosses(file, target, owner) {
            Ok(()) if self.holds(file, target, owner) => Ok(Pass::LendParts { mutable: *mutable }),
            Ok(()) if *mutable => Ok(Pass::Mutable),
            Ok(()) => Ok(Pass::Borrow),
            // A type the marked crate doesn't declare, as ratatui's `Frame`,
            // is lent for the call (D558.3).
            Err(_) if self.lendable(file, target, owner) => Ok(Pass::Lend {
                mutable: *mutable,
                referent: unnamed_lifetimes(referent),
            }),
            Err(reason) => Err(reason),
        }
    }

    /// How a result of `ty` comes back, as an expression that reads it from
    /// `input`: by the wire format where its type crosses by it, and else
    /// part by part through `Vec`, `Option`, `Result` and tuples, each part
    /// whose type the marked crate doesn't declare as the native value the
    /// interpreter moves (#1118, #1203). An alias of a `Result`, as
    /// `iced::Result`, comes back by its parts. Each lifetime the type names
    /// is read as `'static`, as the interpreter's arena holds the value, and
    /// becomes the signature's by the type's variance. `Err` says why it
    /// can't come back.
    fn back(
        &self,
        file: &Source<'_>,
        ty: &type_ref::Type,
        owner: Owner<'_>,
    ) -> Result<String, String> {
        let island = format!("crate::{MODULE}");
        let Err(reason) = self.crosses(file, ty, owner) else {
            return Ok(format!("{island}::Take::take(input)"));
        };
        let path = match ty {
            type_ref::Type::Tuple { members, .. } => {
                let parts = members
                    .iter()
                    .map(|member| self.back(file, member, owner))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(format!(
                    "{{ input.tuple({}); ({},) }}",
                    parts.len(),
                    parts.join(", ")
                ));
            }
            type_ref::Type::Path(path) => path,
            _ => return Err(reason),
        };
        let Some(last) = path.segments.last() else {
            return Err(reason);
        };
        let name = last.name.text.as_str();
        let types = last
            .arguments
            .iter()
            .filter_map(|argument| match argument {
                type_ref::Argument::Type(ty) => Some(ty),
                type_ref::Argument::Lifetime(_) => None,
            })
            .collect::<Vec<_>>();
        let parts = || {
            types
                .iter()
                .map(|part| self.back(file, part, owner))
                .collect::<Result<Vec<_>, _>>()
        };
        let generic = matches!(owner, Owner::Marked(owner)
            if self.types.get(owner).is_some_and(|owner| owner.generic));
        match (name, types.len()) {
            ("Vec" | "Option", 1) => {
                let inner = parts()?.concat();
                Ok(format!("input.{}(|input| {inner})", name.to_lowercase()))
            }
            ("Result", 2) => {
                let [ok, err] = &parts()?[..] else {
                    return Err(reason);
                };
                Ok(format!("input.result(|input| {ok}, |input| {err})"))
            }
            _ if name != "Result" && !self.lendable(file, ty, owner) => Err(reason),
            // Outside a generic impl, each native part's way back is decided
            // by its type alone.
            _ if generic => Err(format!("a `{name}` of a generic type can't cross yet")),
            ("Result", _) => {
                let written = static_lifetimes(slice(file.text, ty.span()));
                let part = |part: &str| {
                    format!(
                        "|input| (&{island}::Back::<<{written} as {island}::Outcome>::{part}>::new()).take(input)"
                    )
                };
                Ok(format!("input.result({}, {})", part("Ok"), part("Err")))
            }
            _ => Ok(format!(
                "(&{island}::Back::<{}>::new()).take(input)",
                static_lifetimes(slice(file.text, ty.span()))
            )),
        }
    }

    /// Whether a borrow of `ty`, written in `file`, is lent for a call: a
    /// path to a type the marked crate doesn't declare, or `Self` in an
    /// impl for a native type.
    fn lendable(&self, file: &Source<'_>, ty: &type_ref::Type, owner: Owner<'_>) -> bool {
        let type_ref::Type::Path(path) = ty else {
            return false;
        };
        let Some(last) = path.segments.last() else {
            return false;
        };
        match last.name.text.as_str() {
            "Self" if path.segments.len() == 1 => matches!(owner, Owner::Native),
            "Vec" | "Option" | crate::item::type_ref::SLICE_TYPE => false,
            _ => self.marked(file, path).is_none(),
        }
    }

    /// For a stand-in's `&self` function whose result is a shared borrow of
    /// its receiver (#1305), as `fn label(&self) -> &str`, how the result
    /// comes back. `None` when the result is no such borrow, and `Err` says
    /// why it can't come back.
    fn kept_result(
        &self,
        file: &Source<'_>,
        function: &item::Function,
    ) -> Option<Result<Borrowed, String>> {
        let (
            Some(item::Receiver::Shared(receiver)),
            Some(type_ref::Type::Reference {
                mutable: false,
                lifetime,
                target,
                ..
            }),
        ) = (&function.receiver, &function.output)
        else {
            return None;
        };
        // An elided lifetime is the receiver's, as rustc elides it; a named
        // one is the receiver's only when `&'a self` names it.
        if let Some(lifetime) = lifetime
            && lifetime.name.text != "_"
            && !slice(file.text, *receiver).contains(&format!("'{}", lifetime.name.text))
        {
            return None;
        }
        let owned = match target.as_ref() {
            type_ref::Type::Path(path)
                if path.segments.len() == 1 && path.segments[0].name.text == "str" =>
            {
                return Some(Ok(Borrowed::Pinned));
            }
            type_ref::Type::Path(path)
                if path
                    .segments
                    .last()
                    .is_some_and(|last| last.name.text == crate::item::type_ref::SLICE_TYPE) =>
            {
                let element = path
                    .segments
                    .last()
                    .and_then(|last| match &last.arguments[..] {
                        [type_ref::Argument::Type(element)] => Some(element),
                        _ => None,
                    })?;
                if let Err(reason) = self.crosses(file, element, Owner::None) {
                    return Some(Err(reason));
                }
                format!("::std::vec::Vec<{}>", slice(file.text, element.span()))
            }
            // A native value the receiver holds is lent where it is.
            target
                if self.crosses(file, target, Owner::None).is_err()
                    && self.lendable(file, target, Owner::None) =>
            {
                return Some(Ok(Borrowed::InPlace));
            }
            target => {
                if let Err(reason) = self.crosses(file, target, Owner::None) {
                    return Some(Err(reason));
                }
                slice(file.text, target.span()).to_owned()
            }
        };
        Some(Ok(Borrowed::Kept(owned)))
    }

    /// The marked input whose borrow `function`'s result keeps, as rustc
    /// ties their lifetimes, by its name in the trampoline (#1473): an
    /// input whose type names a lifetime the result names, as `&'a self` of
    /// `view<'a>(&'a self, ..) -> Element<'a, Message>`, and else, as rustc
    /// elides it, `&self` or the one input that borrows. Only a marked
    /// value native code holds is an anchor: one lent by its parts or
    /// copied, `receiver` and `passes` say. `None` when there is none.
    fn anchor(
        &self,
        file: &Source<'_>,
        function: &item::Function,
        receiver: Option<&Pass>,
        passes: &[Pass],
    ) -> Option<String> {
        let output = function.output.as_ref()?;
        if matches!(output, type_ref::Type::Reference { .. }) {
            return None;
        }
        let named = lifetimes(slice(file.text, output.span()))
            .into_iter()
            .filter(|name| *name != "_")
            .collect::<Vec<_>>();
        let names = |text: &str| lifetimes(text).iter().any(|name| named.contains(name));
        let borrows = |ty: &type_ref::Type| {
            matches!(ty, type_ref::Type::Reference { .. })
                || !lifetimes(slice(file.text, ty.span())).is_empty()
        };
        let receiver = match (&function.receiver, receiver) {
            (Some(item::Receiver::Shared(span)), Some(pass)) => Some((*span, pass)),
            _ => None,
        };
        let tied_receiver = receiver.filter(|(span, _)| {
            if named.is_empty() {
                true
            } else {
                names(slice(file.text, *span))
            }
        });
        // An elided lifetime of a `&mut self` method is the receiver's,
        // which native code can't hold as an anchor.
        if named.is_empty() && matches!(function.receiver, Some(item::Receiver::Mutable(_))) {
            return None;
        }
        let tied = if named.is_empty() && tied_receiver.is_none() {
            let borrowing = function
                .parameters
                .iter()
                .enumerate()
                .filter(|(_, parameter)| borrows(&parameter.ty))
                .collect::<Vec<_>>();
            match &borrowing[..] {
                [(index, _)] => vec![*index],
                _ => Vec::new(),
            }
        } else if named.is_empty() {
            Vec::new()
        } else {
            function
                .parameters
                .iter()
                .enumerate()
                .filter(|(_, parameter)| names(slice(file.text, parameter.ty.span())))
                .map(|(index, _)| index)
                .collect()
        };
        let marked = |pass: &Pass| matches!(pass, Pass::LendParts { mutable: false });
        let held = |index: usize, ty: &type_ref::Type| {
            let type_ref::Type::Reference {
                mutable: false,
                target,
                ..
            } = ty
            else {
                return false;
            };
            let type_ref::Type::Path(path) = target.as_ref() else {
                return false;
            };
            marked(&passes[index])
                || (matches!(passes[index], Pass::Borrow) && self.marked(file, path).is_some())
        };
        if let Some((_, pass)) = tied_receiver
            && (marked(pass) || matches!(pass, Pass::Borrow))
        {
            return Some("self".to_owned());
        }
        tied.into_iter()
            .find(|index| held(*index, &function.parameters[*index].ty))
            .map(|index| format!("__a{index}"))
    }

    /// How a `&'static` result of a native value comes back (#1442): `None`
    /// when `function`'s result is no such reference.
    fn lasting_result(
        &self,
        file: &Source<'_>,
        function: &item::Function,
        owner: Owner<'_>,
    ) -> Option<Lasting> {
        let Some(type_ref::Type::Reference {
            mutable: false,
            lifetime: Some(lifetime),
            target,
            ..
        }) = &function.output
        else {
            return None;
        };
        if lifetime.name.text != "static" {
            return None;
        }
        let type_ref::Type::Path(path) = target.as_ref() else {
            return None;
        };
        let last = path.segments.last()?;
        if path.segments.len() == 1 && last.name.text == "str" {
            return Some(Lasting::Text);
        }
        if last.name.text == crate::item::type_ref::SLICE_TYPE {
            let [type_ref::Argument::Type(element)] = &last.arguments[..] else {
                return None;
            };
            return self
                .lendable(file, element, owner)
                .then_some(Lasting::Slice);
        }
        (self.crosses(file, target, owner).is_err() && self.lendable(file, target, owner))
            .then_some(Lasting::Value)
    }

    /// The generated text of `file` with its inline modules `group`, and
    /// each stand-in's maker it declares in `objects`.
    fn body(
        &self,
        file: &Source<'_>,
        group: &[&Source<'_>],
        shape: &mut BTreeMap<String, Entry>,
        refused: &mut Vec<String>,
        objects: &mut Vec<(String, String)>,
    ) -> String {
        let mut items = String::new();
        let mut uses = Vec::new();
        let mut stand_ins = BTreeSet::new();
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
                            let text = self.body(inner, group, shape, refused, objects);
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
                    if self
                        .crossing
                        .contains(&path_of(file.modules, &declared.name.text))
                    {
                        let text = self.cross_struct(file, declared);
                        items.push_str(&text);
                        append(shape, &path_of(file.modules, &declared.name.text), &text);
                    }
                }
                item::Parsed::Enum(declared) if declared.header.visibility.is_some() => {
                    self.mirror(file, &declared.name, declared.span, None, &mut items, shape);
                    if self
                        .crossing
                        .contains(&path_of(file.modules, &declared.name.text))
                    {
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
                    match self.stub(file, declared, Owner::None, false, &path) {
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
                    let stand_in = Place {
                        declared: &mut stand_ins,
                        items: &mut items,
                        shape,
                        refused,
                        objects,
                    };
                    self.stand_in(file, declared, stand_in);
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

    /// The marked impl `declared` as native code sees it: an impl of a
    /// visible marked type, or a trait's impl for a native type, as `impl
    /// From<&Theme> for Settings` (#1466), with a trampoline for each of its
    /// functions native code can call. An impl of a marked trait is the
    /// interpreter's alone, and one of a type native code doesn't see has
    /// nothing to stand for.
    fn implementation(
        &self,
        file: &Source<'_>,
        declared: &item::Impl,
        items: &mut String,
        shape: &mut BTreeMap<String, Entry>,
        refused: &mut Vec<String>,
    ) {
        let type_ref::Type::Path(written) = &declared.owner else {
            return;
        };
        let trait_ = declared.trait_.as_ref();
        if trait_
            .and_then(|path| path.segments.last())
            .is_some_and(|last| self.traits.contains(last.name.text.as_str()))
        {
            return;
        }
        let (owner, path) = match self.marked(file, written) {
            Some(path) => (Owner::Marked(path), path.to_owned()),
            None if self.declares(file, written) || trait_.is_none() => return,
            None => (Owner::Native, native_owner(written)),
        };
        // A trait's functions are all public; an inherent impl shows only
        // its visible ones.
        let functions = declared
            .functions
            .iter()
            .filter(|function| trait_.is_some() || function.header.visibility.is_some())
            .collect::<Vec<_>>();
        if functions.is_empty() && trait_.is_none() {
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
        // Each function is its owner's member, a trait's by the trait's
        // instance, `From<palette::Theme>::from`, as the interpreter keys
        // it.
        let instance = trait_.map(instance);
        let prefix = match &instance {
            Some(instance) => format!("{path}::{instance}"),
            None => path.clone(),
        };
        if functions.is_empty() {
            shape.insert(
                prefix.clone(),
                Entry {
                    kind: Kind::Other,
                    text: format!("{header} {{}}"),
                },
            );
        }
        for function in functions {
            let member = format!("{prefix}::{}", function.name.text);
            match self.stub(file, function, owner, false, &member) {
                Ok(stub) => {
                    for line in stub.lines() {
                        let _ = writeln!(text, "    {line}");
                    }
                    shape.insert(
                        member,
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

    /// For the marked impl `declared` of one of the crate's own native
    /// traits for a marked type, the type's stand-in and its impl of the
    /// trait (#1264): a struct holding the handle of the program value
    /// native code holds as the trait's object, whose functions each call
    /// the impl's own with that value as the receiver, in the generation the
    /// call runs in. Its maker, which the interpreter makes the object
    /// with, goes in the place's `objects`.
    fn stand_in(&self, file: &Source<'_>, declared: &item::Impl, place: Place<'_>) {
        let (Some(trait_), type_ref::Type::Path(written)) = (&declared.trait_, &declared.owner)
        else {
            return;
        };
        let (Some(last), Some(owner)) = (trait_.segments.last(), written.segments.last()) else {
            return;
        };
        // A trait of the crate's own native modules, as `page::Page` after
        // `use crate::page;`: rawr's copy of the crate declares it, and the
        // stand-in beside it. A dependency's trait crosses by the runner's
        // proxy (D594), and a marked trait is the interpreter's.
        let from_crate = |start: Option<&ImmutableString>| {
            start.is_some_and(|start| matches!(start.as_str(), "crate" | "self" | "super"))
        };
        let first = &trait_.segments[0].name.text;
        let crate_own = from_crate(Some(first))
            || file.items.iter().any(|item| {
                matches!(item, item::Parsed::Use(used) if !used.global()
                && used.bindings().iter().any(|(name, path)| {
                    name.text == *first && from_crate(path.first())
                }))
            });
        if !crate_own
            || trait_.global
            || self.traits.contains(last.name.text.as_str())
            || !declared.generics.is_empty()
            || !declared.lifetimes.params.is_empty()
            || !last.arguments.is_empty()
            || !owner.arguments.is_empty()
        {
            return;
        }
        let name = owner.name.text.as_str();
        let path = match self.marked(file, written) {
            Some(marked) if self.types[marked].generic => return,
            Some(marked) => marked.to_owned(),
            None => path_of(file.modules, name),
        };
        let stand_in = format!("__Rawr{name}");
        let full = format!("crate::{}", path_of(file.modules, &stand_in));
        let island = format!("crate::{MODULE}");
        let mut text = String::new();
        if place.declared.insert(stand_in.clone()) {
            let _ = writeln!(
                text,
                "/// Generated by Rawr: a `{name}` native code holds as a trait object,\n\
                 /// by the handle of the program value.\n\
                 #[doc(hidden)]\n\
                 pub struct {stand_in}(pub {island}::Object);"
            );
            // Each std trait the marked type implements, as a supertrait of
            // the native trait may require (#1301).
            for std in self.implemented.get(name).into_iter().flatten() {
                text.push_str(&std.implementation(&full, &path));
            }
        }
        let written = slice(file.text, trait_.span);
        let _ = writeln!(text, "impl {written} for {full} {{");
        for (associated, ty) in &declared.associated {
            let _ = writeln!(
                text,
                "    type {} = {};",
                associated.text,
                slice(file.text, ty.span())
            );
        }
        for function in &declared.functions {
            // The impl's function is the owner's member `Page::title`.
            let member = format!("{path}::{}::{}", last.name.text, function.name.text);
            match self.stub(file, function, Owner::None, true, &member) {
                Ok(stub) => {
                    for line in stub.lines() {
                        let _ = writeln!(text, "    {line}");
                    }
                }
                Err(reason) => place.refused.push(reason),
            }
        }
        text.push_str("}\n");
        let maker = format!("__rawr_object_{}", place.objects.len());
        let _ = writeln!(
            text,
            "#[doc(hidden)]\n\
             pub fn {maker}(trait_name: &str, handle: u64) -> ::std::option::Option<{island}::Given> {{\n    \
             {island}::object!(trait_name, handle, dyn {written}, {full})\n}}"
        );
        place.shape.insert(
            format!("{path} as dyn {written}"),
            Entry {
                kind: Kind::Other,
                text: text.clone(),
            },
        );
        place.items.push_str(&text);
        place
            .objects
            .push((path, format!("crate::{}", path_of(file.modules, &maker))));
    }

    /// One function's trampoline. `owner` is the type of an associated
    /// function; `path` is what the interpreter finds it by. A stand-in's
    /// function (`object`) has no `owner`: its `Self` is the stand-in, which
    /// doesn't cross, and its receiver is the program value's handle.
    fn stub(
        &self,
        file: &Source<'_>,
        function: &item::Function,
        owner: Owner<'_>,
        object: bool,
        path: &str,
    ) -> Result<String, String> {
        if let Some(type_ref::Type::Path(output)) = &function.output {
            let first = output
                .segments
                .first()
                .map_or("", |segment| segment.name.text.as_str());
            if first.starts_with("impl") || type_ref::is_fn_type(first) {
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
            match self.pass(file, &parameter.ty, written, owner) {
                Ok(pass) => passes.push(pass),
                Err(reason) => {
                    reasons.push(format!("parameter `{}`: {reason}", parameter.name.text))
                }
            }
        }
        signature.push_str(slice(file.text, Span::new(cursor, end)));
        // On a stand-in, `Self` is the stand-in, which doesn't cross.
        let names_self =
            |ty: &type_ref::Type| identifiers(slice(file.text, ty.span())).contains("Self");
        if object
            && function
                .parameters
                .iter()
                .map(|parameter| &parameter.ty)
                .chain(&function.output)
                .any(names_self)
        {
            reasons.push("a trait object's function can't name `Self` yet".to_owned());
        }
        let signature = without_rawr_attributes(signature.trim_end());
        // A result that doesn't cross by the wire format may still come
        // back part by part, its native parts moved.
        let mut back = None;
        let lasting = self.lasting_result(file, function, owner);
        // A `&self` method of a marked type that holds native parts lends
        // them where they are, so a reference its result gives into them
        // is one into the receiver, as `&self.0.colors` is (#1468).
        let holding = matches!(owner, Owner::Marked(owner) if self.holding.contains(owner));
        let lent = holding && matches!(function.receiver, Some(item::Receiver::Shared(_)));
        let kept = match self.kept_result(file, function).filter(|_| object || lent) {
            Some(Ok(Borrowed::Kept(_))) if !object => {
                reasons.push(
                    "its result: a reference to a value the interpreter copies can't come back yet"
                        .to_owned(),
                );
                None
            }
            Some(Ok(owned)) => Some(owned),
            Some(Err(reason)) => {
                reasons.push(format!("its result: {reason}"));
                None
            }
            None => None,
        };
        if let Some(output) = &function.output
            && kept.is_none()
            && lasting.is_none()
            && self.crosses(file, output, owner).is_err()
        {
            match self.back(file, output, owner) {
                Ok(read) => back = Some(read),
                Err(reason) => reasons.push(format!("its result: {reason}")),
            }
        }
        let receiver = match &function.receiver {
            None => None,
            Some(item::Receiver::Shared(_) | item::Receiver::Mutable(_)) if object => {
                Some(Pass::Object)
            }
            Some(item::Receiver::Shared(_)) if holding => Some(Pass::LendParts { mutable: false }),
            Some(item::Receiver::Mutable(_)) if holding => Some(Pass::LendParts { mutable: true }),
            Some(item::Receiver::Owned(_) | item::Receiver::OwnedMutable(_)) if holding => {
                Some(Pass::GiveParts)
            }
            Some(item::Receiver::Shared(_)) => Some(Pass::Borrow),
            Some(item::Receiver::Mutable(_)) => Some(Pass::Mutable),
            Some(item::Receiver::Owned(_) | item::Receiver::OwnedMutable(_)) => Some(Pass::Value),
        };
        if object && !matches!(receiver, Some(Pass::Object)) {
            reasons.push("a trait object's function takes `&self` or `&mut self`".to_owned());
        } else if !object
            && receiver.is_some()
            && !matches!(owner, Owner::Marked(owner) if self.crossing.contains(owner))
        {
            reasons.push("its receiver can't cross yet".to_owned());
        }
        // Native code calls each method of a trait object it holds, so a
        // reference one gives that can't come back is an error before
        // anything runs, not a panic at the call.
        if object
            && matches!(function.output, Some(type_ref::Type::Reference { .. }))
            && let Some(reason) = reasons
                .iter()
                .find(|reason| reason.starts_with("its result"))
        {
            let at = function
                .output
                .as_ref()
                .map_or(function.span, type_ref::Type::span)
                .start();
            return Err(format!(
                "error: native code can't borrow the result of `{path}` through rawr yet: {}\n  \
                 --> {}:{}:{}",
                reason.trim_start_matches("its result: "),
                file.path.display(),
                at.line().unwrap_or(1),
                at.position().unwrap_or(1)
            ));
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
        // A `&mut self` call ends every borrow of the receiver, as rustc
        // allows it only once none is left: what the stand-in kept for
        // them goes (#1305).
        if object && matches!(function.receiver, Some(item::Receiver::Mutable(_))) {
            text.push_str("    self.0.release();\n");
        }
        let owned = !matches!(owner, Owner::None) || object;
        let _ = writeln!(
            text,
            "    let mut __call = {island}::Call::new({path:?}, {owned});"
        );
        if matches!(kept, Some(Borrowed::Pinned)) {
            text.push_str("    __call.pinned();\n");
        }
        // A reference to a native value comes back where the value is
        // (#1442): a `&'static` only from a static.
        if lasting.is_some() || matches!(kept, Some(Borrowed::InPlace)) {
            let _ = writeln!(text, "    __call.in_place({});", lasting.is_some());
        }
        // A function of `()` gives `()`, whatever its body's last
        // statement left (#1475).
        if matches!(function.output, None | Some(type_ref::Type::Unit(_))) {
            text.push_str("    __call.unit();\n");
        }
        // Each `&mut` argument in argument order, as its change comes back:
        // a copy, or what a lent place was given.
        let mut changed = Vec::new();
        let mut argument = |name: &str, pass: Pass, text: &mut String| match pass {
            Pass::Value => {
                let _ = writeln!(text, "    __call.argument(&{name});");
            }
            Pass::Borrow => {
                let _ = writeln!(text, "    __call.argument({name});");
            }
            Pass::Mutable => {
                let _ = writeln!(text, "    __call.mutable(&*{name});");
                changed.push((name.to_owned(), Changed::Copied));
            }
            Pass::Lend { mutable, referent } => {
                lend(name, mutable, &referent, &island, text);
                if mutable {
                    changed.push((name.to_owned(), Changed::Lent(referent)));
                }
            }
            Pass::LendParts { mutable: false } => {
                let _ = writeln!(text, "    __call.lent({name});");
            }
            Pass::LendParts { mutable: true } => {
                let _ = writeln!(text, "    __call.lent_mut(&mut *{name});");
                changed.push((name.to_owned(), Changed::Parts));
            }
            Pass::GiveParts => {
                let _ = writeln!(text, "    __call.given({name});");
            }
            Pass::Give => {
                let _ = writeln!(text, "    {island}::give!(__call, {name});");
            }
            Pass::GiveForCall { ty } => {
                let _ = writeln!(
                    text,
                    "    // SAFETY: only its lifetimes change, which have no layout. The\n    \
                     // runner drops what is left of it once the call returns, and the\n    \
                     // function, which rustc checked, keeps nothing of it past the call\n    \
                     // but what its result borrows, which this signature types.\n    \
                     #[allow(unsafe_code)]\n    \
                     let {name} = unsafe {{ ::std::mem::transmute::<{}, {}>({name}) }};\n    \
                     __call.for_call();\n    \
                     {island}::give!(__call, {name});",
                    unnamed_lifetimes(&ty),
                    static_lifetimes(&ty)
                );
            }
            Pass::Object => {
                let _ = writeln!(text, "    __call.object(&{name}.0);");
            }
        };
        // A result read part by part that borrows a marked input, as
        // `view<'a>(&'a self, ..) -> Element<'a, Message>` borrows `self`,
        // is made in a region kept with that input (#1473).
        let anchor = back
            .as_ref()
            .and_then(|_| self.anchor(file, function, receiver.as_ref(), &passes));
        if let Some(pass) = receiver {
            argument("self", pass, &mut text);
        }
        // A native value lent `&'a`, as `inbox: &'a inbox::State` of
        // `view<'a>(&'a self, ..)`, is borrowed as long as the anchored
        // result, which may borrow it (#1413).
        let lends_kept = if anchor.is_some() {
            kept_lends(file, function, &passes)
        } else {
            Vec::new()
        };
        if let Some(anchor) = anchor {
            let _ = writeln!(text, "    __call.anchor({anchor});");
        }
        for (index, pass) in passes.into_iter().enumerate() {
            let name = format!("__a{index}");
            if lends_kept.contains(&index) {
                let _ = writeln!(text, "    let {name}_lends = __call.lends();");
                argument(&name, pass, &mut text);
                let _ = writeln!(text, "    __call.keep_lends({name}_lends);");
            } else {
                argument(&name, pass, &mut text);
            }
        }
        text.push_str("    let mut __reply = __call.send();\n");
        // `Self` reads as the impl's own type, at its place in the header.
        let output = function
            .output
            .as_ref()
            .map_or("()", |ty| slice(file.text, ty.span()));
        if let Some(lasting) = lasting {
            // A static's own place, which lives as long as the program.
            let read = match lasting {
                Lasting::Value => "lasting",
                Lasting::Slice => "lasting_slice",
                Lasting::Text => "lasting_text",
            };
            let _ = writeln!(
                text,
                "    let __value: {output} = __reply.read(|input| input.{read}());"
            );
        } else if let Some(Borrowed::InPlace) = kept
            && object
        {
            // The receiver's own native value, kept where it is for as
            // long as `&self` is borrowed.
            let _ = writeln!(
                text,
                "    let __value: {output} = self.0.lent(&mut __reply);"
            );
        } else if let Some(Borrowed::InPlace) = kept {
            // A place in a part the call lent of the receiver (#1468).
            let _ = writeln!(
                text,
                "    // SAFETY: the runner gave a place the call reached through `self`'s\n    \
                 // lent parts.\n    \
                 #[allow(unsafe_code)]\n    \
                 let __value: {output} = unsafe {{ __reply.lent(self) }};"
            );
        } else if let Some(Borrowed::Pinned) = kept
            && object
        {
            // The result is the program's own text, pinned with the value.
            let _ = writeln!(
                text,
                "    let __value: {output} = self.0.pinned(&mut __reply);"
            );
        } else if let Some(Borrowed::Pinned) = kept {
            // The text the receiver's lent parts hold (#1468).
            let _ = writeln!(
                text,
                "    // SAFETY: the text is in a part of `self`, which stays borrowed for as\n    \
                 // long as the result is, or pinned for as long as the runner lives.\n    \
                 #[allow(unsafe_code)]\n    \
                 let __value: {output} = unsafe {{ __reply.pinned() }};"
            );
        } else if let Some(Borrowed::Kept(kept)) = kept {
            // The result borrows the receiver: the stand-in keeps the
            // value it came back as for as long as `&self` is borrowed.
            let _ = writeln!(
                text,
                "    let __value: {output} = self.0.keep::<{kept}>(__reply.take());"
            );
        } else if let Some(read) = back {
            // Each native part comes back as the value the interpreter moves:
            // `Back`'s two `take`s, the first found for its type.
            let _ = writeln!(
                text,
                "    let __value: {output} = {{\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
                 __reply.read(|input| {read})\n    }};"
            );
        } else {
            let _ = writeln!(text, "    let __value: {output} = __reply.take();");
        }
        // A lent `&mut` the function gave a whole new value, `*place =
        // value`, takes it (#1139), and one of a type that crosses by the
        // wire format, as an alias of `usize`, takes its copy (`Hand`,
        // #1297). One whose type names a lifetime can't take a new value. A
        // marked value whose parts were lent takes its copied fields and
        // each part given a whole new value (#1469).
        for (name, changed) in changed {
            match changed {
                Changed::Copied => {
                    let _ = writeln!(text, "    *{name} = __reply.take();");
                }
                Changed::Parts => {
                    let _ = writeln!(
                        text,
                        "    __reply.read(|input| {island}::Parts::take_into(&mut *{name}, input));"
                    );
                }
                Changed::Lent(referent) if static_lifetimes(&referent) == referent => {
                    let _ = writeln!(
                        text,
                        "    {{ use {island}::{{ByCopy as _, ByLoan as _}}; \
                         (&{island}::hand(&*{name})).back(&mut __reply, {name}) }}"
                    );
                }
                Changed::Lent(referent) => {
                    let _ = writeln!(
                        text,
                        "    __reply.read(|input| input.unreplaced(\"{referent}\"));"
                    );
                }
            }
        }
        text.push_str("    __value\n}\n");
        Ok(text)
    }

    /// How one field of `ty` of the marked type at `path` goes to the
    /// interpreter from `value`, a `&` of it, and how it comes back: by the
    /// wire format, or each native part as a value of its own (#1466,
    /// #1475).
    fn field(
        &self,
        file: &Source<'_>,
        path: &str,
        ty: &type_ref::Type,
        value: &str,
    ) -> (String, String) {
        let held = self.field_held(file, path, ty);
        (held.code(value, Use::Put, 0), held.take())
    }

    /// [`Generator::held`] of a field of the crossing marked type at
    /// `path`, which holds what crosses, as [`Generator::crossing`] found.
    fn field_held<'f>(&self, file: &Source<'f>, path: &str, ty: &type_ref::Type) -> Held<'f> {
        match self.held(file, ty, Owner::Marked(path)) {
            Ok(held) => held,
            // Only a crossing type's fields come here, and each of them is
            // held.
            Err(_) => Held::Wire,
        }
    }

    /// One statement of a field's `use` in its type's `Parts` impl
    /// ([`Generator::parts`]), the field reached by `binding`: a native part
    /// lent or moved where it is, a field of a marked type that holds
    /// parts by that type's own impl, either inside an `Option`, a `Vec`,
    /// a `Box` or a tuple (#1475), and any other field by the wire format
    /// (#1469).
    fn part_use(
        &self,
        file: &Source<'_>,
        path: &str,
        ty: &type_ref::Type,
        binding: &str,
        purpose: Use,
    ) -> String {
        self.field_held(file, path, ty).code(binding, purpose, 0)
    }

    /// The `Parts` impl of the marked type `name` at `path`, which holds
    /// native parts (#1469), from each `Use`'s body: how a borrow of it
    /// lends its parts, a `&mut` one too, a value moves them, and a `&mut`
    /// one takes back what the call made of it.
    fn parts(name: &str, bodies: [String; 4]) -> String {
        let island = format!("crate::{MODULE}");
        let [lend, lend_mut, give, take_into] = bodies;
        format!(
            "impl {island}::Parts for {name} {{\n    \
             fn lend<'c>(&'c self, call: &mut {island}::Call<'c>) {{\n{lend}    }}\n    \
             fn lend_mut<'c>(&'c mut self, call: &mut {island}::Call<'c>) {{\n{lend_mut}    }}\n    \
             fn give(self, out: &mut {island}::Out) {{\n{give}    }}\n    \
             fn take_into(&mut self, input: &mut {island}::In<'_>) {{\n        \
             #[allow(unused_imports)]\n        use {island}::{{ByMove as _, ByWire as _}};\n\
             {take_into}    }}\n}}\n"
        )
    }

    /// [`Generator::parts`] of the struct `declared` at `path`.
    fn struct_parts(&self, file: &Source<'_>, declared: &item::Struct, path: &str) -> String {
        let count = declared.fields.len();
        let header = |out: &str| format!("        {out}.structure({path:?}, {count});\n");
        let mut bodies = [
            header("call.out()"),
            header("call.out()"),
            header("out"),
            format!("        input.structure({path:?}, {count});\n"),
        ];
        let bindings = (0..count)
            .map(|index| format!("__f{index}"))
            .collect::<Vec<_>>();
        let _ = writeln!(
            bodies[2],
            "        let Self {{ {} }} = self;",
            declared
                .fields
                .iter()
                .zip(&bindings)
                .map(|(field, binding)| format!("{}: {binding}", field.name.text))
                .collect::<Vec<_>>()
                .join(", ")
        );
        for (field, binding) in declared.fields.iter().zip(&bindings) {
            let name = &field.name.text;
            for (body, (binding, purpose)) in bodies.iter_mut().zip([
                (format!("&self.{name}"), Use::Lend),
                (format!("&mut self.{name}"), Use::LendMut),
                (binding.clone(), Use::Give),
                (format!("&mut self.{name}"), Use::TakeInto),
            ]) {
                let _ = writeln!(
                    body,
                    "        {}",
                    self.part_use(file, path, &field.ty, &binding, purpose)
                );
            }
        }
        Self::parts(&declared.name.text, bodies)
    }

    /// [`Generator::parts`] of the enum `declared` at `path`; `take` reads
    /// a variant's value whole, by its name and count, as its `Take` does.
    fn enum_parts(
        &self,
        file: &Source<'_>,
        declared: &item::Enum,
        path: &str,
        take: &str,
    ) -> String {
        let mut bodies = [
            "        match self {\n".to_owned(),
            "        match self {\n".to_owned(),
            "        match self {\n".to_owned(),
            format!(
                "        let (variant, count) = input.variant({path:?});\n        \
                 match (self, variant, count) {{\n"
            ),
        ];
        for variant in &declared.variants {
            let name = variant.name.text.as_str();
            let count = variant.payload.len();
            let bindings = (0..count)
                .map(|index| format!("__f{index}"))
                .collect::<Vec<_>>();
            let pattern = variant_pattern(variant, &bindings);
            for (index, (body, purpose)) in bodies
                .iter_mut()
                .zip([Use::Lend, Use::LendMut, Use::Give, Use::TakeInto])
                .enumerate()
            {
                let _ = match purpose {
                    Use::TakeInto => {
                        writeln!(body, "            ({pattern}, {name:?}, {count}) => {{")
                    }
                    _ => {
                        let out = if index == 2 { "out" } else { "call.out()" };
                        writeln!(
                            body,
                            "            {pattern} => {{\n                \
                             {out}.variant({path:?}, {name:?}, {count});"
                        )
                    }
                };
                for (binding, ty) in bindings.iter().zip(&variant.payload) {
                    let _ = writeln!(
                        body,
                        "                {}",
                        self.part_use(file, path, ty, binding, purpose)
                    );
                }
                body.push_str("            }\n");
            }
        }
        for body in &mut bodies[..3] {
            body.push_str("        }\n");
        }
        // Another variant than the receiver's is a new value, read whole.
        let _ = writeln!(
            bodies[3],
            "            (this, variant, count) => {{\n                \
             *this = match (variant, count) {{\n{take}                    \
             (variant, count) => input.unknown({path:?}, variant, count),\n                \
             }};\n            }}\n        }}"
        );
        Self::parts(&declared.name.text, bodies)
    }

    fn cross_struct(&self, file: &Source<'_>, declared: &item::Struct) -> String {
        let name = declared.name.text.as_str();
        let path = path_of(file.modules, name);
        let island = format!("crate::{MODULE}");
        let count = declared.fields.len();
        let mut put = String::new();
        let mut taken = Vec::new();
        for field in &declared.fields {
            let value = format!("&self.{}", field.name.text);
            let (one, back) = self.field(file, &path, &field.ty, &value);
            let _ = writeln!(put, "        {one}");
            taken.push(back);
        }
        let mut take = String::new();
        match declared.form {
            item::StructForm::Named => {
                take.push_str(&format!("{name} {{\n"));
                for (field, back) in declared.fields.iter().zip(&taken) {
                    let _ = writeln!(take, "            {}: {back},", field.name.text);
                }
                take.push_str("        }");
            }
            item::StructForm::Tuple => {
                take.push_str(&format!("{name}(\n"));
                for back in &taken {
                    let _ = writeln!(take, "            {back},");
                }
                take.push_str("        )");
            }
            item::StructForm::Unit => take.push_str(name),
        }
        let mut text = format!(
            "impl {island}::Put for {name} {{\n    fn put(&self, out: &mut {island}::Out) {{\n        \
             out.structure({path:?}, {count});\n{put}    }}\n}}\n\
             impl {island}::Take for {name} {{\n    fn take(input: &mut {island}::In<'_>) -> Self {{\n        \
             #[allow(unused_imports)]\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
             input.structure({path:?}, {count});\n        {take}\n    }}\n}}\n"
        );
        if self.holding.contains(&path) {
            text.push_str(&self.struct_parts(file, declared, &path));
        }
        text.push_str(&self.mirror_impl(name, &path));
        text
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
            let pattern = variant_pattern(variant, &bindings);
            let _ = writeln!(
                put,
                "            {pattern} => {{\n                out.variant({path:?}, {variant_name:?}, {count});"
            );
            let mut taken = Vec::new();
            for (binding, ty) in bindings.iter().zip(&variant.payload) {
                let (one, back) = self.field(file, &path, ty, binding);
                let _ = writeln!(put, "                {one}");
                taken.push(back);
            }
            put.push_str("            }\n");
            let built = variant_pattern(variant, &taken);
            let _ = writeln!(take, "            ({variant_name:?}, {count}) => {built},");
        }
        let mut text = format!(
            "impl {island}::Put for {name} {{\n    fn put(&self, out: &mut {island}::Out) {{\n        \
             match self {{\n{put}        }}\n    }}\n}}\n\
             impl {island}::Take for {name} {{\n    fn take(input: &mut {island}::In<'_>) -> Self {{\n        \
             #[allow(unused_imports)]\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
             match input.variant({path:?}) {{\n{take}            (variant, count) => input.unknown({path:?}, variant, count),\n        }}\n    }}\n}}\n"
        );
        if self.holding.contains(&path) {
            text.push_str(&self.enum_parts(file, declared, &path, &take));
        }
        text.push_str(&self.mirror_impl(name, &path));
        text
    }

    /// The `Mirror` impl of the crossing type `name` at `path`, which the
    /// runner holds a program value as where a crate's generic takes one
    /// (#1249): a value moves its native parts, as Rust moves it.
    fn mirror_impl(&self, name: &str, path: &str) -> String {
        let island = format!("crate::{MODULE}");
        let give = if self.holding.contains(path) {
            format!("{island}::Parts::give(self, out);")
        } else {
            format!("{island}::Put::put(&self, out);")
        };
        format!(
            "impl {island}::Mirror for {name} {{\n    fn give(self, out: &mut {island}::Out) {{\n        \
             {give}\n    }}\n}}\n"
        )
    }
}

/// The pattern of `variant` of `Self` whose payload `parts` bind, or the
/// value they build: `Self::Selected(__f0)`, `Self::Named { colors: __f0 }`.
fn variant_pattern(variant: &item::Variant, parts: &[String]) -> String {
    let name = variant.name.text.as_str();
    match &variant.fields {
        Some(fields) => format!(
            "Self::{name} {{ {} }}",
            fields
                .iter()
                .zip(parts)
                .map(|(field, part)| format!("{}: {part}", field.text))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None if parts.is_empty() => format!("Self::{name}"),
        None => format!("Self::{name}({})", parts.join(", ")),
    }
}

/// Each std trait `item` gives a type of the marked crate, by the type's
/// name: a struct's or enum's derives, or an impl of one for a type with no
/// parameters, `impl fmt::Debug for Badge` (#1301).
fn std_impls(item: &item::Parsed) -> Vec<(&str, Std)> {
    fn derived<'i>(header: &item::Header, name: &'i item::Name) -> Vec<(&'i str, Std)> {
        header
            .attributes
            .iter()
            .filter(|attribute| {
                matches!(attribute.path.segments.as_ref(), [only] if only.name.text == "derive")
            })
            .flat_map(|attribute| attribute.arguments.iter())
            .filter_map(|derived| Std::named(&derived.segments.last()?.name.text))
            .map(|std| (name.text.as_str(), std))
            .collect()
    }
    match item {
        item::Parsed::Struct(declared) if declared.generics.is_empty() => {
            derived(&declared.header, &declared.name)
        }
        item::Parsed::Enum(declared) if declared.generics.is_empty() => {
            derived(&declared.header, &declared.name)
        }
        item::Parsed::Impl(declared) => {
            let (Some(trait_), type_ref::Type::Path(owner)) = (&declared.trait_, &declared.owner)
            else {
                return Vec::new();
            };
            let (Some(last), Some(owner)) = (trait_.segments.last(), owner.segments.last()) else {
                return Vec::new();
            };
            let own = matches!(
                trait_.segments[0].name.text.as_str(),
                "crate" | "self" | "super"
            );
            match Std::named(&last.name.text) {
                Some(std)
                    if !own
                        && last.arguments.is_empty()
                        && owner.arguments.is_empty()
                        && declared.generics.is_empty() =>
                {
                    vec![(owner.name.text.as_str(), std)]
                }
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// How a stand-in's `&self` result that borrows its receiver comes back
/// (`Generator::kept_result`).
enum Borrowed {
    /// A `&str`: the program's own text, read where it is, pinned with the
    /// value till it is next taken `&mut` (#1322).
    Pinned,
    /// A `&[T]` or a `&T`, whose program value is no native `[T]` or `T`:
    /// the owned type it comes back as, `Vec<T>` for `&[T]` and `T` for
    /// `&T`, which the stand-in keeps for as long as `&self` is borrowed.
    Kept(String),
    /// A `&T` of a native value the receiver holds: the value where it is,
    /// kept there for as long as `&self` is borrowed (#1442).
    InPlace,
}

/// How a `&mut` argument's change comes back after a call
/// (`Generator::stub`).
enum Changed {
    /// Its copy, by the wire format.
    Copied,
    /// What the lent place of this type was given as a whole, if anything.
    Lent(String),
    /// A marked value whose parts were lent: its copied fields, and each
    /// part given a whole new value (#1469).
    Parts,
}

/// What one field of a marked type that holds native parts does in its
/// [`Generator::parts`] impl, the field reached by a binding of the use's
/// kind.
#[derive(Clone, Copy)]
enum Use {
    /// Lend it to the call `call`, by a `&` binding.
    Lend,
    /// Lend it mutably to the call `call`, by a `&mut` binding.
    LendMut,
    /// Move it to the interpreter by `out`, by an owned binding.
    Give,
    /// Take back from `input` what the call made of it, by a `&mut`
    /// binding.
    TakeInto,
    /// Put it in `out` for its type's `Put`, by a `&` binding: a native
    /// part as a clone (#1466).
    Put,
}

/// How a field of a marked type crosses as part of its value
/// (`Generator::held`), and so how its `Parts` impl lends, moves and takes
/// it back (#1475).
enum Held<'t> {
    /// By the wire format, as a copy.
    Wire,
    /// A marked type that holds native parts, by its own `Parts` impl.
    Marked,
    /// A native part, of the type written as this text: lent where it is,
    /// moved as itself.
    Native(&'t str),
    /// An `Option` of what the inner one holds.
    Option(Box<Held<'t>>),
    /// A `Vec` of what the inner one holds.
    Vec(Box<Held<'t>>),
    /// A `Box` of what the inner one holds, which the interpreter holds as
    /// its value itself.
    Boxed(Box<Held<'t>>),
    /// A tuple of what each member holds.
    Tuple(Vec<Held<'t>>),
}

impl Held<'_> {
    /// One statement that does `purpose` with the field `binding` reaches,
    /// as [`Use`] says, its own bindings suffixed by `depth`.
    fn code(&self, binding: &str, purpose: Use, depth: usize) -> String {
        let island = format!("crate::{MODULE}");
        let out = match purpose {
            Use::Lend | Use::LendMut => "call.out()",
            Use::Give | Use::TakeInto | Use::Put => "out",
        };
        let inner = format!("__p{depth}");
        match self {
            Self::Wire => match purpose {
                Use::Lend | Use::LendMut => format!("call.part(&*{binding});"),
                Use::Give => format!("{island}::Put::put(&{binding}, out);"),
                Use::TakeInto => format!("*{binding} = {island}::Take::take(input);"),
                Use::Put => format!("{island}::Put::put({binding}, out);"),
            },
            Self::Marked => match purpose {
                Use::Lend => format!("{island}::Parts::lend({binding}, call);"),
                Use::LendMut => format!("{island}::Parts::lend_mut({binding}, call);"),
                Use::Give => format!("{island}::Parts::give({binding}, out);"),
                Use::TakeInto => format!("{island}::Parts::take_into({binding}, input);"),
                Use::Put => format!("{island}::Put::put({binding}, out);"),
            },
            Self::Native(written) => match purpose {
                Use::Lend => format!("{island}::lend_part!(call, {binding}, {written});"),
                Use::LendMut => format!("{island}::lend_part_mut!(call, {binding}, {written});"),
                Use::Give => format!("{island}::give_part!(out, {binding});"),
                Use::TakeInto => {
                    format!("(&{island}::Back::<{written}>::new()).take_into(input, {binding});")
                }
                Use::Put => format!("{island}::part!(out, {binding});"),
            },
            // A `None` the call left or made drops what was there, and a
            // `Some` where there was none is new.
            Self::Option(held) if matches!(purpose, Use::TakeInto) => format!(
                "if input.some() {{ match {binding} {{ \
                 ::std::option::Option::Some({inner}) => {{ {} }} \
                 {inner} => *{inner} = ::std::option::Option::Some({}), }} }} \
                 else {{ *{binding} = ::std::option::Option::None; }}",
                held.code(&inner, purpose, depth + 1),
                held.take(),
            ),
            Self::Option(held) => format!(
                "match {binding} {{ ::std::option::Option::None => {out}.none(), \
                 ::std::option::Option::Some({inner}) => {{ {out}.some(); {} }} }}",
                held.code(&inner, purpose, depth + 1)
            ),
            // Each element still holding a part it was lent is taken back
            // into, and moves where the call put it; any other is new.
            Self::Vec(held) if matches!(purpose, Use::TakeInto) => format!(
                "{island}::vec_into(input, {binding}, |input, {inner}| {{ {} }}, \
                 |input| {});",
                held.code(&inner, purpose, depth + 1),
                held.take(),
            ),
            Self::Vec(held) => format!(
                "{{ let {inner} = {binding}; {out}.array({inner}.len()); \
                 for __e{depth} in {inner} {{ {} }} }}",
                held.code(&format!("__e{depth}"), purpose, depth + 1)
            ),
            Self::Boxed(held) => {
                let unboxed = match purpose {
                    Use::Lend | Use::Put => format!("&**{binding}"),
                    Use::LendMut | Use::TakeInto => format!("&mut **{binding}"),
                    Use::Give => format!("*{binding}"),
                };
                held.code(&unboxed, purpose, depth + 1)
            }
            Self::Tuple(members) => {
                let names = (0..members.len())
                    .map(|index| format!("__t{depth}_{index}"))
                    .collect::<Vec<_>>();
                let start = match purpose {
                    Use::TakeInto => format!("input.tuple({});", members.len()),
                    _ => format!("{out}.array({});", members.len()),
                };
                let each = members
                    .iter()
                    .zip(&names)
                    .map(|(member, name)| member.code(name, purpose, depth + 1))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(
                    "{{ let ({},) = {binding}; {start} {each} }}",
                    names.join(", ")
                )
            }
        }
    }

    /// An expression that reads a whole new value of the field from
    /// `input`: by the wire format, or each native part as the value the
    /// interpreter moved.
    fn take(&self) -> String {
        let island = format!("crate::{MODULE}");
        match self {
            Self::Wire | Self::Marked => format!("{island}::Take::take(input)"),
            Self::Native(written) => format!("(&{island}::Back::<{written}>::new()).take(input)"),
            Self::Option(held) => format!("input.option(|input| {})", held.take()),
            Self::Vec(held) => format!("input.vec(|input| {})", held.take()),
            Self::Boxed(held) => format!("::std::boxed::Box::new({})", held.take()),
            Self::Tuple(members) => format!(
                "{{ input.tuple({}); ({},) }}",
                members.len(),
                members
                    .iter()
                    .map(|member| member.take())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// The std containers a marked type's field may hold native parts in
/// (`Generator::held`).
#[derive(Clone, Copy)]
enum Container {
    Option,
    Vec,
    Box,
}

/// The container `path` names and the type it holds: `Option<T>`,
/// `Vec<T>` or `Box<T>`, by its own name or its std path.
fn container(path: &type_ref::Path) -> Option<(Container, &type_ref::Type)> {
    let last = path.segments.last()?;
    let std = path.segments.len() == 1
        || matches!(
            path.segments[0].name.text.as_str(),
            "std" | "core" | "alloc"
        );
    let [type_ref::Argument::Type(inner)] = &last.arguments[..] else {
        return None;
    };
    let container = match last.name.text.as_str() {
        "Option" => Container::Option,
        "Vec" => Container::Vec,
        "Box" => Container::Box,
        _ => return None,
    };
    std.then_some((container, inner))
}

/// How a `&'static` result of a native value comes back
/// (`Generator::lasting_result`): a static's own place, as `fn meta(&self)
/// -> &'static Meta { &META }` gives (#1442).
#[derive(Clone, Copy)]
enum Lasting {
    /// A `&'static T`.
    Value,
    /// A `&'static [T]`, as a static slice of native values is.
    Slice,
    /// A `&'static str`: the program's own text, pinned for as long as the
    /// runner lives.
    Text,
}

/// Where a module's stand-ins go (`Generator::stand_in`): the names it
/// has declared already, its generated items, the shape, what can't cross
/// and the makers.
struct Place<'p> {
    declared: &'p mut BTreeSet<String>,
    items: &'p mut String,
    shape: &'p mut BTreeMap<String, Entry>,
    refused: &'p mut Vec<String>,
    objects: &'p mut Vec<(String, String)>,
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

/// The path from the crate root that `names`, written in `file`, names:
/// through `crate`, `self` or `super`, through a `use` of its first name,
/// and else from `file`'s module, as a child module or an item of it. A
/// `use` path is read the same way, `depth` deep. `None` for a path that
/// leaves the crate.
fn absolute(file: &Source<'_>, names: &[&str], depth: usize) -> Option<Vec<String>> {
    let mut at = file
        .modules
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let owned = |names: &[&str]| names.iter().map(ToString::to_string).collect::<Vec<_>>();
    let (first, rest) = names.split_first()?;
    match *first {
        "crate" => {
            at.clear();
            at.extend(owned(rest));
        }
        "self" => at.extend(owned(rest)),
        "super" => {
            let mut rest = names;
            while let Some((&"super", after)) = rest.split_first() {
                at.pop()?;
                rest = after;
            }
            at.extend(owned(rest));
        }
        first => {
            let used = file.items.iter().find_map(|item| match item {
                item::Parsed::Use(used) if used.scope.is_none() && depth < 8 => used
                    .bindings()
                    .into_iter()
                    .find(|(name, _)| name.text == first)
                    .map(|(_, path)| (used.global(), path)),
                _ => None,
            });
            match used {
                Some((true, _)) => return None,
                Some((false, path)) => {
                    let path = path.iter().map(ImmutableString::as_str).collect::<Vec<_>>();
                    let mut used = absolute(file, &path, depth + 1)?;
                    used.extend(owned(rest));
                    return Some(used);
                }
                None => at.extend(owned(names)),
            }
        }
    }
    Some(at)
}

/// The trait `trait_` an impl names, as the interpreter keys its
/// functions: its name, and each type argument as the impl writes it,
/// `From<palette::Theme>` or `From<&Theme>` (`resolve::full_type_name`).
fn instance(trait_: &type_ref::Path) -> String {
    let Some(last) = trait_.segments.last() else {
        return String::new();
    };
    let arguments = type_arguments(&last.arguments);
    if arguments.is_empty() {
        last.name.text.to_string()
    } else {
        format!("{}<{arguments}>", last.name.text)
    }
}

/// The owner of a marked impl for the native type `written`, as the
/// interpreter keys its functions: `::` and the type's name, as
/// `::Settings` for `impl From<&Theme> for markdown::Settings` (#1466).
fn native_owner(written: &type_ref::Path) -> String {
    let name = written
        .segments
        .last()
        .map_or("", |last| last.name.text.as_str());
    format!("::{name}")
}

/// `arguments`' types as the interpreter names them in a trait's instance
/// (`resolve::full_type_name`): each path's segments, each reference's
/// referent after `&` or `&mut `, lifetimes left out.
fn type_arguments(arguments: &[type_ref::Argument]) -> String {
    fn name(ty: &type_ref::Type) -> String {
        match ty {
            type_ref::Type::Unit(_) => "()".to_owned(),
            type_ref::Type::Tuple { members, .. } => format!(
                "({})",
                members.iter().map(name).collect::<Vec<_>>().join(", ")
            ),
            type_ref::Type::Reference {
                mutable, target, ..
            } => format!("&{}{}", if *mutable { "mut " } else { "" }, name(target)),
            type_ref::Type::Path(path) => {
                let written = path
                    .segments
                    .iter()
                    .map(|segment| segment.name.text.as_str())
                    .collect::<Vec<_>>()
                    .join("::");
                let arguments = path
                    .segments
                    .last()
                    .map(|last| type_arguments(&last.arguments))
                    .unwrap_or_default();
                if arguments.is_empty() {
                    written
                } else {
                    format!("{written}<{arguments}>")
                }
            }
        }
    }
    arguments
        .iter()
        .filter_map(|argument| match argument {
            type_ref::Argument::Type(ty) => Some(name(ty)),
            type_ref::Argument::Lifetime(_) => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The text `span` covers.
fn slice(text: &str, span: Span) -> &str {
    span.text(text)
}

/// `text` without the attributes only Rawr reads (`#[preview]`,
/// `#[rawr::…]`).
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
        let rawr = inner.starts_with("rawr::") || inner.starts_with("preview");
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

/// The statements that lend the parameter `name`, a `&mut referent` or a
/// `&referent`, for the call (D558.3). The lend's key is a type of its own,
/// and its accessors, which the generated runner reaches the value through,
/// claim that key and hand a visitor the value at every lifetime. Only
/// these accessors read the place, as the type it is.
fn lend(name: &str, mutable: bool, referent: &str, island: &str, text: &mut String) {
    let result = "::std::result::Result<(), u8>";
    let key = "::std::any::TypeId::of::<Key>()";
    let _ = writeln!(
        text,
        "    let {name}_lend = {{
        struct Key;"
    );
    if mutable {
        let _ = writeln!(
            text,
            "        #[allow(unsafe_code)]\n        \
             fn mutable(nonce: u64, visit: &mut dyn FnMut(&mut {referent})) -> {result} {{\n            \
             {island}::reach(nonce, {key}, false, |place| {{\n                \
             // SAFETY: `Key`'s lend is this call's `&mut {referent}`, claimed exclusively.\n                \
             visit(unsafe {{ &mut *place.cast::<{referent}>() }})\n            }})\n        }}\n        \
             static MUTABLE: fn(u64, &mut dyn FnMut(&mut {referent})) -> {result} = mutable;"
        );
    }
    let _ = writeln!(
        text,
        "        #[allow(unsafe_code)]\n        \
         fn shared(nonce: u64, visit: &mut dyn FnMut(&{referent})) -> {result} {{\n            \
         {island}::reach(nonce, {key}, true, |place| {{\n                \
         // SAFETY: `Key`'s lend is this call's `{referent}`, claimed shared.\n                \
         visit(unsafe {{ &*place.cast_const().cast::<{referent}>() }})\n            }})\n        }}\n        \
         static SHARED: fn(u64, &mut dyn FnMut(&{referent})) -> {result} = shared;\n        \
         {island}::Lend::new({key}, {}, &SHARED)\n    }};",
        if mutable { "Some(&MUTABLE)" } else { "None" }
    );
    // A borrow of a type that crosses by the wire format, as an alias of
    // `usize`, is copied instead (`Hand`), and a `&mut` one's copy comes
    // back (#1297). A type that names a lifetime is lent.
    let hand = |method: &str, value: &str| {
        format!(
            "{{ use {island}::{{ByCopy as _, ByLoan as _}}; (&{island}::hand({value})).{method}(&mut __call, {name}"
        )
    };
    let (call, close) = if !mutable {
        (hand("lend_ref", name), " }")
    } else if static_lifetimes(referent) == referent {
        (hand("lend", &format!("&*{name}")), " }")
    } else {
        (format!("__call.lend({name}"), "")
    };
    let _ = writeln!(
        text,
        "    // SAFETY: the lend's accessors read its key's place as `{referent}`.\n    \
         #[allow(unsafe_code)]\n    \
         let () = unsafe {{ {call}, {name}_lend, ::std::any::type_name::<{referent}>()){close} }};"
    );
}

/// `text` with each named lifetime but `'static` written `'_`, so a type
/// written in a signature names its `'static` form where it is inferred.
fn unnamed_lifetimes(text: &str) -> String {
    each_lifetime(text, "'_")
}

/// `text` with each lifetime written `'static`, as the interpreter's arena
/// holds a value whose type has one.
fn static_lifetimes(text: &str) -> String {
    each_lifetime(text, "'static")
}

/// `text` with each lifetime but `'static` written `with`.
fn each_lifetime(text: &str, with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('\'') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after
            .find(|c: char| !(c == '_' || c.is_alphanumeric()))
            .unwrap_or(after.len());
        if &after[..end] == "static" {
            out.push_str("'static");
        } else {
            out.push_str(with);
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// The parameters of `function` lent `&` for its call whose type names a
/// lifetime its result names, as `inbox: &'a inbox::State` of `view<'a>(
/// &'a self, inbox: &'a inbox::State) -> Element<'a, Message>` (#1413):
/// native code keeps each borrowed as long as the result.
fn kept_lends(file: &Source<'_>, function: &item::Function, passes: &[Pass]) -> Vec<usize> {
    let Some(output) = &function.output else {
        return Vec::new();
    };
    let named = lifetimes(slice(file.text, output.span()))
        .into_iter()
        .filter(|name| *name != "_")
        .collect::<Vec<_>>();
    function
        .parameters
        .iter()
        .zip(passes)
        .enumerate()
        .filter(|(_, (parameter, pass))| {
            matches!(pass, Pass::Lend { mutable: false, .. })
                && lifetimes(slice(file.text, parameter.ty.span()))
                    .iter()
                    .any(|name| named.contains(name))
        })
        .map(|(index, _)| index)
        .collect()
}

/// The name of each lifetime `text` names but `'static`, `_` for `'_`.
fn lifetimes(text: &str) -> Vec<&str> {
    text.split('\'')
        .skip(1)
        .map(|after| {
            let end = after
                .find(|c: char| !(c == '_' || c.is_alphanumeric()))
                .unwrap_or(after.len());
            &after[..end]
        })
        .filter(|name| *name != "static")
        .collect()
}

/// Every identifier in `text`.
fn identifiers(text: &str) -> BTreeSet<&str> {
    text.split(|c: char| !(c == '_' || c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect()
}

/// The runtime support of a native root's shadow crate, appended to its
/// root as the module [`MODULE`]. `main` is whether the crate root has a
/// `fn main` to start; `objects` are the crate's stand-ins' makers, each by
/// its type's path ([`Shadow::objects`]).
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
pub(crate) fn support(main: bool, objects: &[(String, String)]) -> String {
    let entry = if main {
        "Exit::code(super::main())"
    } else {
        "{ ::std::eprintln!(\"rawr: the crate root has no `main` to run\"); 0 }"
    };
    let makers = objects
        .iter()
        .map(|(owner, maker)| format!("({owner:?}, {maker} as Maker)"))
        .collect::<Vec<_>>()
        .join(", ");
    SUPPORT
        .replace("ENTRY", entry)
        .replace("__MAKER_LIST__", &format!("[{makers}]"))
}

/// The wire format matches [`crate::island`]'s.
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
const SUPPORT: &str = r#"
/// Generated by Rawr: how the native code of this crate reaches the marked
/// modules, which Rawr interprets. `cargo build` never sees this module.
#[doc(hidden)]
#[allow(unsafe_code, dead_code, unused_imports, clippy::all)]
pub mod __rawr_island {
    // Defined by the `rawr` crate the runner links.
    unsafe extern "Rust" {
        fn __rawr_island_call_v4(
            path: &str,
            arguments: &[u8],
            given: &mut [::std::option::Option<Given>],
            lent: &[(
                ::std::any::TypeId,
                &'static str,
                *mut (),
                ::std::option::Option<&'static (dyn ::std::any::Any + Sync)>,
                &'static (dyn ::std::any::Any + Sync),
            )],
            natives: &mut ::std::vec::Vec<::std::boxed::Box<dyn ::std::any::Any>>,
        ) -> ::std::vec::Vec<u8>;
        fn __rawr_scope_claim_v1(
            nonce: u64,
            key: ::std::any::TypeId,
            shared: bool,
        ) -> ::std::result::Result<*mut (), u8>;
        fn __rawr_scope_release_v1(nonce: u64, shared: bool);
        fn __rawr_island_start_v1(entry: fn() -> i32);
        fn __rawr_island_objects_v1(makers: &[(&'static str, Maker)]);
        fn __rawr_island_release_v1(handle: u64);
        fn __rawr_island_trait_v1(owner: &str, op: u8, handle: u64, other: u64) -> ::std::vec::Vec<u8>;
    }

    /// What makes a native trait's object of a program value: the
    /// trait's path below the crate's root and the value's handle give a
    /// box of the stand-in, or `None` for another trait
    /// (`rawr::island::Maker`).
    pub type Maker = fn(&str, u64) -> ::std::option::Option<Given>;

    /// Each stand-in's maker, by its type's path from the crate's root.
    static MAKERS: &[(&str, Maker)] = &__MAKER_LIST__;

    /// Register [`MAKERS`] once, before the first call into marked code,
    /// which is where a program value becomes a trait object.
    fn register() {
        static ONCE: ::std::sync::Once = ::std::sync::Once::new();
        // SAFETY: the runner links `rawr`, which defines the function with
        // this exact signature.
        ONCE.call_once(|| unsafe { __rawr_island_objects_v1(MAKERS) });
    }

    /// The handle of the program value a stand-in holds, let go of when
    /// the stand-in is dropped, and the results of its `&self` calls that
    /// borrow it, as `fn label(&self) -> &str`'s (#1305).
    pub struct Object(u64, ::std::sync::Mutex<::std::vec::Vec<Kept>>);

    /// A result an [`Object`] keeps for its borrows, by its own allocation.
    pub struct Kept(*mut (dyn ::std::any::Any + Send));

    // SAFETY: what a `Kept` points at is `Send`, and only its `Object`
    // reaches it.
    unsafe impl Send for Kept {}

    impl Drop for Kept {
        fn drop(&mut self) {
            // SAFETY: `Box::into_raw` made the pointer, and it is turned
            // back once, here.
            drop(unsafe { ::std::boxed::Box::from_raw(self.0) });
        }
    }

    impl Object {
        pub fn new(handle: u64) -> Self {
            Self(handle, ::std::sync::Mutex::new(::std::vec::Vec::new()))
        }

        /// `value`, a `&self` call's result that borrows the receiver,
        /// borrowed for as long as the object is.
        pub fn keep<T: Send + 'static>(&self, value: T) -> &T {
            let place = ::std::boxed::Box::into_raw(::std::boxed::Box::new(value));
            self.1
                .lock()
                .unwrap_or_else(::std::sync::PoisonError::into_inner)
                .push(Kept(place));
            // SAFETY: the allocation is freed only by `release`, which
            // takes `&mut self`, or by the object's drop, and neither can
            // happen while `&self` is borrowed; nothing else reaches or
            // moves it.
            unsafe { &*place }
        }

        /// The `&str` a `&self` call gave back ([`Call::pinned`]): the
        /// program's own text, which the runner keeps pinned with the
        /// program value, for as long as the object is borrowed (#1322).
        pub fn pinned(&self, reply: &mut Reply) -> &str {
            let (address, length) = reply.read(|input| {
                input.expect(PINNED, "a pinned `&str`");
                (input.count(), input.count())
            });
            // SAFETY: the runner pinned the text, valid UTF-8, with the
            // value of this object's handle, and lets it go only when the
            // value is next taken `&mut` or released, which a `&mut self`
            // call or the object's drop does, and neither can happen while
            // `&self` is borrowed. Nothing writes a pinned text in place.
            unsafe {
                ::std::str::from_utf8_unchecked(::std::slice::from_raw_parts(
                    ::std::ptr::with_exposed_provenance::<u8>(address),
                    length,
                ))
            }
        }

        /// The native value a `&self` call gave where the program value
        /// holds it ([`Call::in_place`]), as `fn state(&self) -> &State {
        /// &self.state }` gives it (#1442), borrowed for as long as the
        /// object is.
        pub fn lent<T: 'static>(&self, reply: &mut Reply) -> &T {
            let place = reply.read(|input| input.place::<T>());
            // SAFETY: the runner keeps the value where it is with the
            // program value's pins, which it lets go only when the value is
            // next taken `&mut` or released, which a `&mut self` call or
            // the object's drop does, and neither can happen while `&self`
            // is borrowed. Nothing writes the value while the pins hold it.
            unsafe { &*place }
        }

        /// Let go of the kept results, as a `&mut self` call may once
        /// rustc has ended every borrow of them.
        pub fn release(&mut self) {
            self.1
                .get_mut()
                .unwrap_or_else(::std::sync::PoisonError::into_inner)
                .clear();
        }
    }

    impl Drop for Object {
        fn drop(&mut self) {
            // SAFETY: as for `register`.
            unsafe { __rawr_island_release_v1(self.0) }
        }
    }

    // What a stand-in's std trait asks of the program value it holds, by
    // the program's own impl, derived or written (`rawr::island::Op`).
    pub const DEBUG: u8 = 0;
    pub const DISPLAY: u8 = 1;
    const CLONE: u8 = 2;
    const EQUAL: u8 = 3;
    const HASH: u8 = 4;
    const DEFAULT: u8 = 5;
    const PARTIAL_ORDER: u8 = 6;
    const ORDER: u8 = 7;

    /// The answer to `op` of the value at `handle` (and `other`), as the
    /// type at `owner` implements it; a failure panics, as a trampoline's.
    fn ask(owner: &str, op: u8, handle: u64, other: u64) -> ::std::vec::Vec<u8> {
        register();
        // SAFETY: as for `register`.
        let mut reply = unsafe { __rawr_island_trait_v1(owner, op, handle, other) };
        match reply.first() {
            ::std::option::Option::Some(0) => {
                reply.remove(0);
                reply
            }
            _ => ::std::panic!(
                "{}",
                ::std::string::String::from_utf8_lossy(reply.get(1..).unwrap_or_default())
            ),
        }
    }

    /// The first eight bytes of `reply` as a number.
    fn number(reply: &[u8]) -> u64 {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(&reply[..8]);
        u64::from_le_bytes(bytes)
    }

    impl Object {
        /// The value's `Debug` or `Display` text.
        pub fn text(&self, owner: &str, op: u8) -> ::std::string::String {
            ::std::string::String::from_utf8_lossy(&ask(owner, op, self.0, 0)).into_owned()
        }

        /// A clone of the value, under a handle of its own.
        pub fn duplicate(&self, owner: &str) -> Self {
            Self::new(number(&ask(owner, CLONE, self.0, 0)))
        }

        pub fn equal(&self, other: &Self, owner: &str) -> bool {
            ask(owner, EQUAL, self.0, other.0) == [1]
        }

        pub fn hash(&self, owner: &str) -> u64 {
            number(&ask(owner, HASH, self.0, 0))
        }

        /// The `Default` of the type at `owner`, under a handle of its own.
        pub fn default_of(owner: &str) -> Self {
            Self::new(number(&ask(owner, DEFAULT, 0, 0)))
        }

        /// `partial_cmp`, or with `total` `cmp`, of the two values.
        pub fn compare(
            &self,
            other: &Self,
            owner: &str,
            total: bool,
        ) -> ::std::option::Option<::std::cmp::Ordering> {
            let op = if total { ORDER } else { PARTIAL_ORDER };
            match ask(owner, op, self.0, other.0).as_slice() {
                [0] => ::std::option::Option::Some(::std::cmp::Ordering::Less),
                [1] => ::std::option::Option::Some(::std::cmp::Ordering::Equal),
                [2] => ::std::option::Option::Some(::std::cmp::Ordering::Greater),
                _ => ::std::option::Option::None,
            }
        }
    }

    /// Whether `object`, a trait object type's name, is `dyn` of the trait
    /// at `below`, its path below the crate's root.
    pub fn names(object: &str, below: &str) -> bool {
        let object = object.strip_prefix("dyn ").unwrap_or(object);
        let object = object.split_once("::").map_or(object, |(_, below)| below);
        object == below
    }

    /// The box of `dyn $object` a maker gives for the handle `$handle`, the
    /// stand-in `$stand` holding it, when `$trait_name` names `$object`'s
    /// trait, with what the box has of [`Traits`].
    macro_rules! object {
        ($trait_name:expr, $handle:expr, $object:ty, $stand:path) => {{
            #[allow(unused_imports)]
            use crate::__rawr_island::{
                HasClone as _, HasDebug as _, HasDisplay as _, HasEqual as _, HasHash as _,
                HasOrder as _, LacksClone as _, LacksDebug as _, LacksDisplay as _,
                LacksEqual as _, LacksHash as _, LacksOrder as _,
            };
            if !crate::__rawr_island::names(::std::any::type_name::<$object>(), $trait_name) {
                return ::std::option::Option::None;
            }
            let value: ::std::boxed::Box<$object> =
                ::std::boxed::Box::new($stand(crate::__rawr_island::Object::new($handle)));
            let probe = &crate::__rawr_island::probe(&value);
            let traits = (
                probe.debug_of(),
                probe.display_of(),
                probe.clone_of(),
                probe.equal_of(),
                probe.hash_of(),
                probe.order_of(),
            );
            ::std::option::Option::Some((
                ::std::boxed::Box::new(value) as ::std::boxed::Box<dyn ::std::any::Any>,
                ::std::any::type_name::<::std::boxed::Box<$object>>(),
                traits,
            ))
        }};
    }
    pub(crate) use object;

    type Any = dyn ::std::any::Any;

    /// What a trampoline finds of a value's type, for the runner to keep it
    /// as it keeps a value it made: its `Debug` and `Display` text, its
    /// `Clone`, `PartialEq`, `Hash` and `PartialOrd`, each when the type has
    /// it (`rawr::island::Traits`).
    pub type Traits = (
        ::std::option::Option<fn(&Any) -> ::std::option::Option<::std::string::String>>,
        ::std::option::Option<fn(&Any) -> ::std::option::Option<::std::string::String>>,
        ::std::option::Option<fn(&Any) -> ::std::option::Option<::std::boxed::Box<Any>>>,
        ::std::option::Option<fn(&Any, &Any) -> ::std::option::Option<bool>>,
        ::std::option::Option<fn(&Any) -> ::std::option::Option<u64>>,
        ::std::option::Option<fn(&Any, &Any) -> ::std::option::Option<::std::cmp::Ordering>>,
    );

    /// A native value a call gives the interpreter by value, with its
    /// type's name and [`Traits`] (`rawr::island::Given`).
    pub type Given = (::std::boxed::Box<Any>, &'static str, Traits);

    /// Finds one of a concrete `T`'s [`Traits`] when `T` has it, and
    /// nothing otherwise, by method resolution.
    pub struct Probe<T>(::std::marker::PhantomData<T>);
    pub fn probe<T>(_: &T) -> Probe<T> {
        Probe(::std::marker::PhantomData)
    }
    macro_rules! probes {
        ($($has:ident $lacks:ident $method:ident [$($bound:tt)*] $signature:ty => $found:expr;)*) => {$(
            pub trait $has {
                fn $method(&self) -> ::std::option::Option<$signature>;
            }
            impl<T: $($bound)* + 'static> $has for Probe<T> {
                fn $method(&self) -> ::std::option::Option<$signature> {
                    ::std::option::Option::Some($found)
                }
            }
            pub trait $lacks {
                fn $method(&self) -> ::std::option::Option<$signature>;
            }
            impl<T> $lacks for &Probe<T> {
                fn $method(&self) -> ::std::option::Option<$signature> {
                    ::std::option::Option::None
                }
            }
        )*};
    }
    probes! {
        HasDebug LacksDebug debug_of [::std::fmt::Debug] fn(&Any) -> ::std::option::Option<::std::string::String>
            => |value| value.downcast_ref::<T>().map(|value| ::std::format!("{value:?}"));
        HasDisplay LacksDisplay display_of [::std::fmt::Display] fn(&Any) -> ::std::option::Option<::std::string::String>
            => |value| value.downcast_ref::<T>().map(::std::string::ToString::to_string);
        HasClone LacksClone clone_of [::std::clone::Clone] fn(&Any) -> ::std::option::Option<::std::boxed::Box<Any>>
            => |value| value.downcast_ref::<T>().map(|value| ::std::boxed::Box::new(value.clone()) as ::std::boxed::Box<Any>);
        HasEqual LacksEqual equal_of [::std::cmp::PartialEq] fn(&Any, &Any) -> ::std::option::Option<bool>
            => |lhs, rhs| ::std::option::Option::Some(lhs.downcast_ref::<T>()? == rhs.downcast_ref::<T>()?);
        HasHash LacksHash hash_of [::std::hash::Hash] fn(&Any) -> ::std::option::Option<u64>
            => |value| {
                use ::std::hash::{Hash as _, Hasher as _};
                let mut hasher = ::std::hash::DefaultHasher::new();
                value.downcast_ref::<T>()?.hash(&mut hasher);
                ::std::option::Option::Some(hasher.finish())
            };
        HasOrder LacksOrder order_of [::std::cmp::PartialOrd] fn(&Any, &Any) -> ::std::option::Option<::std::cmp::Ordering>
            => |lhs, rhs| lhs.downcast_ref::<T>()?.partial_cmp(rhs.downcast_ref::<T>()?);
    }

    /// Give `$value` to the call `$call` by value, with what its concrete
    /// type has of [`Traits`].
    macro_rules! give {
        ($call:expr, $value:expr) => {{
            #[allow(unused_imports)]
            use crate::__rawr_island::{
                HasClone as _, HasDebug as _, HasDisplay as _, HasEqual as _, HasHash as _,
                HasOrder as _, LacksClone as _, LacksDebug as _, LacksDisplay as _,
                LacksEqual as _, LacksHash as _, LacksOrder as _,
            };
            let value = $value;
            let probe = &crate::__rawr_island::probe(&value);
            let traits = (
                probe.debug_of(),
                probe.display_of(),
                probe.clone_of(),
                probe.equal_of(),
                probe.hash_of(),
                probe.order_of(),
            );
            #[allow(unused_imports)]
            use crate::__rawr_island::{ByCopy as _, ByGift as _};
            (&crate::__rawr_island::hand(&value)).hand(&mut $call, value, traits);
        }};
    }
    pub(crate) use give;

    /// Put `$value`, a `&` of a native part of a marked type's value, as
    /// the `themedef::Theme` of `Theme::Selected(themedef::Theme)`, in
    /// `$out` (#1466): by the wire format when its type crosses by it, as
    /// an alias of `usize` does, and else as a clone the interpreter keeps,
    /// with what its type has of [`Traits`].
    macro_rules! part {
        ($out:expr, $value:expr) => {{
            #[allow(unused_imports)]
            use crate::__rawr_island::{
                HasClone as _, HasDebug as _, HasDisplay as _, HasEqual as _, HasHash as _,
                HasOrder as _, LacksClone as _, LacksDebug as _, LacksDisplay as _,
                LacksEqual as _, LacksHash as _, LacksOrder as _, PartByClone as _,
                PartByWire as _, PartUncloned as _,
            };
            let value = $value;
            let probe = &crate::__rawr_island::probe(value);
            let traits = (
                probe.debug_of(),
                probe.display_of(),
                probe.clone_of(),
                probe.equal_of(),
                probe.hash_of(),
                probe.order_of(),
            );
            (&&crate::__rawr_island::Part(value)).put($out, traits);
        }};
    }
    pub(crate) use part;

    /// A native part of a marked type's value on its way to the
    /// interpreter ([`part!`]): [`PartByWire`] is found first, then
    /// [`PartByClone`], then [`PartUncloned`].
    pub struct Part<'v, T>(pub &'v T);
    pub trait PartByWire {
        fn put(&self, out: &mut Out, traits: Traits);
    }
    impl<T: Put> PartByWire for &Part<'_, T> {
        fn put(&self, out: &mut Out, _: Traits) {
            self.0.put(out);
        }
    }
    pub trait PartByClone {
        fn put(&self, out: &mut Out, traits: Traits);
    }
    impl<T: ::std::clone::Clone + 'static> PartByClone for &&Part<'_, T> {
        fn put(&self, out: &mut Out, traits: Traits) {
            out.give(self.0.clone(), traits);
        }
    }
    pub trait PartUncloned {
        fn put(&self, out: &mut Out, traits: Traits);
    }
    impl<T> PartUncloned for Part<'_, T> {
        fn put(&self, _: &mut Out, _: Traits) {
            ::std::panic!(
                "rawr: native code can't give marked code a copy of its `{}`, which is not `Clone`",
                ::std::any::type_name::<T>()
            )
        }
    }

    /// A marked value that holds native parts (#1469), as `enum Theme {
    /// Selected(themedef::Theme) }`, crossing as Rust passes it: a borrow
    /// lends each part where it is ([`lend_part!`]), a value moves each
    /// part, and a `&mut` takes back what the call made of it. Its other
    /// fields cross by the wire format.
    pub trait Parts: Sized {
        fn lend<'c>(&'c self, call: &mut Call<'c>);
        fn lend_mut<'c>(&'c mut self, call: &mut Call<'c>);
        fn give(self, out: &mut Out);
        fn take_into(&mut self, input: &mut In<'_>);
    }

    /// Lend `$value`, a `&$ty` of a native part of a marked value, to the
    /// call `$call` where it is (#1469): by a key of its own, whose
    /// accessor reads its place as `$ty`, or copied by the wire format when
    /// `$ty` crosses by it, as an alias of `usize` does.
    macro_rules! lend_part {
        ($call:expr, $value:expr, $ty:ty) => {{
            struct Key;
            fn shared(
                nonce: u64,
                visit: &mut dyn FnMut(&$ty),
            ) -> ::std::result::Result<(), u8> {
                crate::__rawr_island::reach(nonce, ::std::any::TypeId::of::<Key>(), true, |place| {
                    // SAFETY: `Key`'s lend is this part, a `$ty`, claimed shared.
                    visit(unsafe { &*place.cast_const().cast::<$ty>() })
                })
            }
            static SHARED: fn(u64, &mut dyn FnMut(&$ty)) -> ::std::result::Result<(), u8> = shared;
            #[allow(unused_imports)]
            use crate::__rawr_island::{LendByLoan as _, LendByWire as _};
            let lend = crate::__rawr_island::Lend::new(::std::any::TypeId::of::<Key>(), None, &SHARED);
            let value: &$ty = $value;
            // SAFETY: the lend's accessor reads its key's place as `$ty`.
            #[allow(unsafe_code)]
            unsafe {
                (&crate::__rawr_island::hand(value)).lend_part($call, value, lend, ::std::any::type_name::<$ty>())
            };
        }};
    }
    pub(crate) use lend_part;

    /// [`lend_part!`] of a `&mut $ty`, lent mutably.
    macro_rules! lend_part_mut {
        ($call:expr, $value:expr, $ty:ty) => {{
            struct Key;
            fn mutable(
                nonce: u64,
                visit: &mut dyn FnMut(&mut $ty),
            ) -> ::std::result::Result<(), u8> {
                crate::__rawr_island::reach(nonce, ::std::any::TypeId::of::<Key>(), false, |place| {
                    // SAFETY: `Key`'s lend is this part, a `&mut $ty`, claimed exclusively.
                    visit(unsafe { &mut *place.cast::<$ty>() })
                })
            }
            fn shared(
                nonce: u64,
                visit: &mut dyn FnMut(&$ty),
            ) -> ::std::result::Result<(), u8> {
                crate::__rawr_island::reach(nonce, ::std::any::TypeId::of::<Key>(), true, |place| {
                    // SAFETY: `Key`'s lend is this part, a `$ty`, claimed shared.
                    visit(unsafe { &*place.cast_const().cast::<$ty>() })
                })
            }
            static MUTABLE: fn(u64, &mut dyn FnMut(&mut $ty)) -> ::std::result::Result<(), u8> = mutable;
            static SHARED: fn(u64, &mut dyn FnMut(&$ty)) -> ::std::result::Result<(), u8> = shared;
            #[allow(unused_imports)]
            use crate::__rawr_island::{LendByLoan as _, LendByWire as _};
            let lend = crate::__rawr_island::Lend::new(
                ::std::any::TypeId::of::<Key>(),
                ::std::option::Option::Some(&MUTABLE),
                &SHARED,
            );
            let value: &mut $ty = $value;
            // SAFETY: the lend's accessors read its key's place as `$ty`.
            #[allow(unsafe_code)]
            unsafe {
                (&crate::__rawr_island::hand(&*value)).lend_part_mut($call, value, lend, ::std::any::type_name::<$ty>())
            };
        }};
    }
    pub(crate) use lend_part_mut;

    /// Move `$value`, a native part of a marked value, to the interpreter
    /// by `$out` (#1469): by the wire format when its type crosses by it,
    /// and else as the native value the interpreter keeps, with what its
    /// type has of [`Traits`].
    macro_rules! give_part {
        ($out:expr, $value:expr) => {{
            #[allow(unused_imports)]
            use crate::__rawr_island::{
                GiveByMove as _, GiveByWire as _, HasClone as _, HasDebug as _, HasDisplay as _,
                HasEqual as _, HasHash as _, HasOrder as _, LacksClone as _, LacksDebug as _,
                LacksDisplay as _, LacksEqual as _, LacksHash as _, LacksOrder as _,
            };
            let value = $value;
            let probe = &crate::__rawr_island::probe(&value);
            let traits = (
                probe.debug_of(),
                probe.display_of(),
                probe.clone_of(),
                probe.equal_of(),
                probe.hash_of(),
                probe.order_of(),
            );
            (&crate::__rawr_island::hand(&value)).give_part($out, value, traits);
        }};
    }
    pub(crate) use give_part;

    /// How [`lend_part!`] lends a part: copied by the wire format when its
    /// type crosses by it ([`LendByWire`], found first), and else where it
    /// is ([`LendByLoan`]).
    pub trait LendByWire<T> {
        /// # Safety
        ///
        /// None needed: the part is copied.
        unsafe fn lend_part<'c>(&self, call: &mut Call<'c>, value: &'c T, lend: Lend, name: &'static str);
        /// # Safety
        ///
        /// None needed: the part is copied, and its copy comes back.
        unsafe fn lend_part_mut<'c>(&self, call: &mut Call<'c>, value: &'c mut T, lend: Lend, name: &'static str);
    }
    impl<T: Put> LendByWire<T> for Hand<T> {
        unsafe fn lend_part<'c>(&self, call: &mut Call<'c>, value: &'c T, _: Lend, _: &'static str) {
            call.part(value);
        }
        unsafe fn lend_part_mut<'c>(&self, call: &mut Call<'c>, value: &'c mut T, _: Lend, _: &'static str) {
            call.part(&*value);
        }
    }
    pub trait LendByLoan<T> {
        /// # Safety
        ///
        /// As for [`Call::lend_part`].
        unsafe fn lend_part<'c>(&self, call: &mut Call<'c>, value: &'c T, lend: Lend, name: &'static str);
        /// # Safety
        ///
        /// As for [`Call::lend_part`].
        unsafe fn lend_part_mut<'c>(&self, call: &mut Call<'c>, value: &'c mut T, lend: Lend, name: &'static str);
    }
    impl<T> LendByLoan<T> for &Hand<T> {
        unsafe fn lend_part<'c>(&self, call: &mut Call<'c>, value: &'c T, lend: Lend, name: &'static str) {
            debug_assert!(lend.mutable.is_none());
            // SAFETY: as the caller's.
            unsafe { call.lend_part(::std::ptr::from_ref(value).cast_mut().cast(), lend, name) }
        }
        unsafe fn lend_part_mut<'c>(&self, call: &mut Call<'c>, value: &'c mut T, lend: Lend, name: &'static str) {
            debug_assert!(lend.mutable.is_some());
            // SAFETY: as the caller's.
            unsafe { call.lend_part(::std::ptr::from_mut(value).cast(), lend, name) }
        }
    }

    /// How [`give_part!`] moves a part: by the wire format when its type
    /// crosses by it ([`GiveByWire`], found first), and else as the native
    /// value the interpreter keeps ([`GiveByMove`]).
    pub trait GiveByWire<T> {
        fn give_part(&self, out: &mut Out, value: T, traits: Traits);
    }
    impl<T: Put> GiveByWire<T> for Hand<T> {
        fn give_part(&self, out: &mut Out, value: T, _: Traits) {
            value.put(out);
        }
    }
    pub trait GiveByMove<T> {
        fn give_part(&self, out: &mut Out, value: T, traits: Traits);
    }
    impl<T: 'static> GiveByMove<T> for &Hand<T> {
        fn give_part(&self, out: &mut Out, value: T, traits: Traits) {
            out.give(value, traits);
        }
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
    const OK: u8 = 12;
    const ERR: u8 = 13;
    const STRUCT: u8 = 15;
    const ENUM: u8 = 16;
    const LENT: u8 = 20;
    const NATIVE: u8 = 21;
    const HANDLE: u8 = 22;
    const PINNED: u8 = 23;

    /// A value on its way to the interpreter, and the native values it
    /// gives the interpreter, each taken once.
    pub struct Out(::std::vec::Vec<u8>, ::std::vec::Vec<::std::option::Option<Given>>);

    impl Out {
        /// `value`, which the interpreter keeps, with what its type has of
        /// [`Traits`].
        pub fn give<T: 'static>(&mut self, value: T, traits: Traits) {
            self.byte(NATIVE);
            self.count(self.1.len());
            self.1.push(Some((
                ::std::boxed::Box::new(value),
                ::std::any::type_name::<T>(),
                traits,
            )));
        }
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
        /// An `Option`'s `None`.
        pub fn none(&mut self) {
            self.byte(NONE);
        }
        /// The start of an `Option`'s `Some`, whose value follows.
        pub fn some(&mut self) {
            self.byte(SOME);
        }
        /// The start of a `Vec` or a tuple of `members`, which follow.
        pub fn array(&mut self, members: usize) {
            self.byte(ARRAY);
            self.count(members);
        }
    }

    /// A value on its way back from the interpreter, and the native values
    /// the interpreter moved for the reply, each taken once.
    pub struct In<'a> {
        bytes: &'a [u8],
        path: &'a str,
        natives: &'a mut [::std::option::Option<::std::boxed::Box<dyn ::std::any::Any>>],
        /// Where each part the call lent is, by its index among the call's
        /// lends: a part still where it was lent comes back as its index
        /// (#1475).
        places: &'a [*mut ()],
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
        /// A `Vec` whose items `item` reads.
        pub fn vec<T>(&mut self, mut item: impl FnMut(&mut Self) -> T) -> ::std::vec::Vec<T> {
            self.expect(ARRAY, "a `Vec`");
            let count = self.count();
            (0..count).map(|_| item(self)).collect()
        }
        /// An `Option` whose value `some` reads.
        pub fn option<T>(&mut self, some: impl FnOnce(&mut Self) -> T) -> ::std::option::Option<T> {
            if self.some() {
                Some(some(self))
            } else {
                None
            }
        }
        /// Whether an `Option` is a `Some`, whose value follows.
        pub fn some(&mut self) -> bool {
            match self.byte() {
                NONE => false,
                SOME => true,
                _ => self.fail("expected an `Option`"),
            }
        }
        /// Where the first part the next value holds that is still where
        /// the call lent it is, without reading the value (#1475).
        fn first_lent(&self) -> ::std::option::Option<usize> {
            let mut found = None;
            let mut bytes = self.bytes;
            scan(&mut bytes, &mut found)?;
            Some(self.places.get(found?)?.addr())
        }
        /// What a call made of a native part it was lent where it is
        /// (#1469): `None` when the part is still there, and else the whole
        /// new value it gave the place, `*place = value`.
        pub fn changed<T: 'static>(&mut self, place: &T) -> ::std::option::Option<T> {
            match self.bytes.first() {
                Some(&UNIT) => {
                    self.byte();
                    None
                }
                Some(&LENT) => {
                    self.byte();
                    let index = self.count();
                    if self.places.get(index).map(|lent| lent.addr())
                        != Some(::std::ptr::from_ref(place).addr())
                    {
                        ::std::panic!(
                            "rawr: `{}` moved a `{}` native code lent it to another place, \
                             which can't come back yet",
                            self.path,
                            ::std::any::type_name::<T>()
                        );
                    }
                    None
                }
                _ => Some(self.native()),
            }
        }
        /// A `Result`, or an alias of one, whose parts `ok` and `err` read.
        pub fn result<R: Outcome>(
            &mut self,
            ok: impl FnOnce(&mut Self) -> R::Ok,
            err: impl FnOnce(&mut Self) -> R::Err,
        ) -> R {
            match self.byte() {
                OK => R::ok(ok(self)),
                ERR => R::err(err(self)),
                _ => self.fail("expected a `Result`"),
            }
        }
        /// The start of a tuple of `members`, which follow.
        pub fn tuple(&mut self, members: usize) {
            self.expect(ARRAY, "a tuple");
            let count = self.count();
            if count != members {
                self.fail(&::std::format!("expected a tuple of {members}, found {count}"));
            }
        }
        /// The value a call gave a lent `&mut T` as a whole, `*place =
        /// value`; `None` when the place is still the one lent.
        pub fn replaced<T: 'static>(&mut self) -> ::std::option::Option<T> {
            if self.bytes.first() == Some(&UNIT) {
                self.byte();
                return None;
            }
            Some(self.native())
        }
        /// That a lent `&mut` of `name`, whose type names a lifetime, was
        /// not given a whole new value, which can't come back yet.
        pub fn unreplaced(&mut self, name: &str) {
            if self.byte() != UNIT {
                ::std::panic!(
                    "rawr: `{}` gave the `&mut {name}` native code lent it a new value, \
                     which can't come back yet",
                    self.path
                );
            }
        }
        /// A static's own value, which the interpreter gave where it is for
        /// a call that asked for a `&'static` ([`Call::in_place`]).
        pub fn lasting<T: 'static>(&mut self) -> &'static T {
            let place = self.place::<T>();
            // SAFETY: for a call that asks for a `&'static`, the runner
            // gives only the place of a value that lives as long as the
            // program, and nothing writes it.
            unsafe { &*place }
        }
        /// A static slice of native values itself, for a call that asked
        /// for a `&'static [T]` ([`Call::in_place`]).
        pub fn lasting_slice<T: 'static>(&mut self) -> &'static [T] {
            self.native()
        }
        /// The program's own text, pinned for as long as the runner lives,
        /// for a call that asked for a `&'static str` ([`Call::in_place`]).
        pub fn lasting_text(&mut self) -> &'static str {
            self.native()
        }
        /// Where the native value the interpreter lent in place is, as a
        /// `T`.
        fn place<T: 'static>(&mut self) -> *const T {
            let place: *const dyn ::std::any::Any = self.native();
            // SAFETY: the runner gave the place of a value that lives at
            // least as long as the call's result borrows it.
            match unsafe { &*place }.downcast_ref::<T>() {
                Some(value) => value,
                None => self.fail(&::std::format!("expected a `{}`", ::std::any::type_name::<T>())),
            }
        }
        /// A native value the interpreter moved, as the `iced::Error` of an
        /// `iced::Result`.
        pub fn native<T: 'static>(&mut self) -> T {
            let name = ::std::any::type_name::<T>();
            if self.bytes.first() == Some(&LENT) {
                ::std::panic!(
                    "rawr: `{}` moved a `{name}` native code lent it to another place, \
                     which can't come back yet",
                    self.path
                );
            }
            self.expect(NATIVE, &::std::format!("a `{name}`"));
            let index = self.count();
            let moved = self.natives.get_mut(index).and_then(::std::option::Option::take);
            match moved.map(|value| value.downcast::<T>()) {
                Some(Ok(value)) => *value,
                _ => self.fail(&::std::format!("expected a `{name}`")),
            }
        }
    }

    /// A `Result`, or an alias of one such as `iced::Result`, by its parts.
    pub trait Outcome {
        type Ok;
        type Err;
        fn ok(value: Self::Ok) -> Self;
        fn err(error: Self::Err) -> Self;
    }
    impl<T, E> Outcome for ::std::result::Result<T, E> {
        type Ok = T;
        type Err = E;
        fn ok(value: T) -> Self {
            Ok(value)
        }
        fn err(error: E) -> Self {
            Err(error)
        }
    }

    /// How a value of `T` comes back: by the wire format when `T` crosses
    /// by it ([`ByWire`], found first), and else as the native value the
    /// interpreter moved ([`ByMove`]).
    pub struct Back<T>(::std::marker::PhantomData<T>);
    impl<T> Back<T> {
        pub fn new() -> Self {
            Self(::std::marker::PhantomData)
        }
    }
    /// A part lent `&mut` takes back its copy, or the whole new value the
    /// call gave its place, if it gave one (#1469).
    pub trait ByWire<T> {
        fn take(&self, input: &mut In<'_>) -> T;
        fn take_into(&self, input: &mut In<'_>, place: &mut T);
    }
    impl<T: Take> ByWire<T> for Back<T> {
        fn take(&self, input: &mut In<'_>) -> T {
            T::take(input)
        }
        fn take_into(&self, input: &mut In<'_>, place: &mut T) {
            *place = T::take(input);
        }
    }
    pub trait ByMove<T> {
        fn take(&self, input: &mut In<'_>) -> T;
        fn take_into(&self, input: &mut In<'_>, place: &mut T);
    }
    impl<T: 'static> ByMove<T> for &Back<T> {
        fn take(&self, input: &mut In<'_>) -> T {
            input.native()
        }
        fn take_into(&self, input: &mut In<'_>, place: &mut T) {
            if let Some(new) = input.changed(&*place) {
                *place = new;
            }
        }
    }

    /// Step `bytes` past one value, keeping in `found` the index of the
    /// first lent part it holds; `None` when the value is malformed.
    fn scan(bytes: &mut &[u8], found: &mut ::std::option::Option<usize>) -> ::std::option::Option<()> {
        fn skip(bytes: &mut &[u8], length: usize) -> ::std::option::Option<()> {
            *bytes = bytes.get(length..)?;
            Some(())
        }
        fn count(bytes: &mut &[u8]) -> ::std::option::Option<usize> {
            let head = bytes.get(..8)?;
            let mut eight = [0; 8];
            eight.copy_from_slice(head);
            *bytes = &bytes[8..];
            usize::try_from(u64::from_le_bytes(eight)).ok()
        }
        let (&tag, rest) = bytes.split_first()?;
        *bytes = rest;
        match tag {
            UNIT | FALSE | TRUE | NONE => {}
            INT | FLOAT | HANDLE => skip(bytes, 8)?,
            CHAR => skip(bytes, 4)?,
            PINNED => skip(bytes, 16)?,
            STR => {
                let length = count(bytes)?;
                skip(bytes, length)?;
            }
            LENT => {
                let index = count(bytes)?;
                found.get_or_insert(index);
            }
            NATIVE => {
                count(bytes)?;
            }
            SOME | OK | ERR => scan(bytes, found)?,
            ARRAY | STRUCT | ENUM => {
                // A struct's path, and an enum's path and variant, come
                // before its members.
                let names = match tag {
                    STRUCT => 1,
                    ENUM => 2,
                    _ => 0,
                };
                for _ in 0..names {
                    let length = count(bytes)?;
                    skip(bytes, length)?;
                }
                for _ in 0..count(bytes)? {
                    scan(bytes, found)?;
                }
            }
            _ => return None,
        }
        Some(())
    }

    /// Take back into `place`, a `Vec` whose elements' native parts the
    /// call lent where they are (#1475), the `Vec` the call made of it: an
    /// element that holds a part still where it was lent is that part's
    /// element, taken back into by `into` and moved where the call put it,
    /// and any other element is new, read by `take`. An element the call
    /// dropped drops.
    pub fn vec_into<T>(
        input: &mut In<'_>,
        place: &mut ::std::vec::Vec<T>,
        mut into: impl FnMut(&mut In<'_>, &mut T),
        mut take: impl FnMut(&mut In<'_>) -> T,
    ) {
        input.expect(ARRAY, "a `Vec`");
        let count = input.count();
        let size = ::std::mem::size_of::<T>();
        let start = place.as_ptr().addr();
        let mut moved = ::std::vec![false; place.len()];
        let mut order = ::std::vec::Vec::with_capacity(count);
        for index in 0..count {
            let element = input.first_lent().and_then(|lent| {
                // A part inside an element's own bytes names it; one
                // elsewhere, as in a `Box`, is the element at its index.
                let at = match lent.checked_sub(start) {
                    Some(offset) if size > 0 && offset < size * moved.len() => offset / size,
                    _ => index,
                };
                (at < moved.len() && !moved[at]).then_some(at)
            });
            match element {
                Some(at) => {
                    moved[at] = true;
                    into(input, &mut place[at]);
                    order.push(Ok(at));
                }
                None => order.push(Err(take(input))),
            }
        }
        let mut old = ::std::mem::take(place)
            .into_iter()
            .map(Some)
            .collect::<::std::vec::Vec<_>>();
        *place = order
            .into_iter()
            .map(|element| match element {
                Ok(at) => match old[at].take() {
                    Some(kept) => kept,
                    None => ::std::unreachable!("each element moves once, as `moved` marks it"),
                },
                Err(new) => new,
            })
            .collect();
    }

    /// How an argument of `T` whose type the marked crate doesn't declare
    /// goes to the interpreter: by the wire format when `T` crosses by it,
    /// as an alias of `usize` does ([`ByCopy`], found first), and else as
    /// the native value the interpreter keeps ([`ByGift`]), or lends for a
    /// borrow ([`ByLoan`]). A `&mut` comes back the same way: its copy, or
    /// the new value the lent place was given. Rustc reads the alias
    /// through to its target, as it does for the checker.
    pub struct Hand<T>(::std::marker::PhantomData<T>);
    pub fn hand<T>(_: &T) -> Hand<T> {
        Hand(::std::marker::PhantomData)
    }
    pub trait ByCopy<T> {
        fn hand(&self, call: &mut Call<'_>, value: T, traits: Traits);
        /// # Safety
        ///
        /// None needed: the value is copied.
        unsafe fn lend_ref<'c>(&self, call: &mut Call<'c>, value: &'c T, lend: Lend, name: &'static str);
        /// # Safety
        ///
        /// None needed: the value is copied, and its copy comes back.
        unsafe fn lend<'c>(&self, call: &mut Call<'c>, value: &'c mut T, lend: Lend, name: &'static str);
        /// The copy of a `&mut` argument [`ByCopy::lend`] copied, back.
        fn back(&self, reply: &mut Reply, place: &mut T)
        where
            T: Take;
    }
    impl<T: Put> ByCopy<T> for Hand<T> {
        fn hand(&self, call: &mut Call<'_>, value: T, _: Traits) {
            call.argument(&value);
        }
        unsafe fn lend_ref<'c>(&self, call: &mut Call<'c>, value: &'c T, _: Lend, _: &'static str) {
            call.argument(value);
        }
        unsafe fn lend<'c>(&self, call: &mut Call<'c>, value: &'c mut T, _: Lend, _: &'static str) {
            call.mutable(&*value);
        }
        fn back(&self, reply: &mut Reply, place: &mut T)
        where
            T: Take,
        {
            *place = reply.take();
        }
    }
    pub trait ByGift<T> {
        fn hand(&self, call: &mut Call<'_>, value: T, traits: Traits);
    }
    impl<T: 'static> ByGift<T> for &Hand<T> {
        fn hand(&self, call: &mut Call<'_>, value: T, traits: Traits) {
            call.give(value, traits);
        }
    }
    pub trait ByLoan<T> {
        /// # Safety
        ///
        /// As for [`Call::lend_ref`].
        unsafe fn lend_ref<'c>(&self, call: &mut Call<'c>, value: &'c T, lend: Lend, name: &'static str)
        where
            T: Sync;
        /// # Safety
        ///
        /// As for [`Call::lend`].
        unsafe fn lend<'c>(&self, call: &mut Call<'c>, value: &'c mut T, lend: Lend, name: &'static str)
        where
            T: Send;
        /// The whole new value the call gave the place [`ByLoan::lend`]
        /// lent, `*place = value`, if it gave one (#1139).
        fn back(&self, reply: &mut Reply, place: &mut T)
        where
            T: 'static;
    }
    impl<T> ByLoan<T> for &Hand<T> {
        unsafe fn lend_ref<'c>(&self, call: &mut Call<'c>, value: &'c T, lend: Lend, name: &'static str)
        where
            T: Sync,
        {
            // SAFETY: as the caller's.
            unsafe { call.lend_ref(value, lend, name) }
        }
        unsafe fn lend<'c>(&self, call: &mut Call<'c>, value: &'c mut T, lend: Lend, name: &'static str)
        where
            T: Send,
        {
            // SAFETY: as the caller's.
            unsafe { call.lend(value, lend, name) }
        }
        fn back(&self, reply: &mut Reply, place: &mut T)
        where
            T: 'static,
        {
            if let Some(new) = reply.read(|input| input.replaced::<T>()) {
                *place = new;
            }
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

    /// A marked type native code sees, which the runner holds a program
    /// value as where a crate's generic takes one, as `Task<Message>` does
    /// (#1249): a value goes to the interpreter as Rust moves it, its native
    /// parts moved, and comes back by [`Take`].
    pub trait Mirror: Take {
        fn give(self, out: &mut Out);
    }

    /// The mirror of the program value the runner wrote as `bytes`, its
    /// native values moved to `natives` (`rawr::island::mirror_out`), at
    /// `path` from the crate's root.
    pub fn mirrored<T: Mirror>(
        path: &'static str,
        bytes: &[u8],
        natives: ::std::vec::Vec<::std::boxed::Box<dyn ::std::any::Any>>,
    ) -> T {
        let mut natives = natives
            .into_iter()
            .map(::std::option::Option::Some)
            .collect::<::std::vec::Vec<_>>();
        let mut input = In {
            bytes,
            path,
            natives: &mut natives,
            places: &[],
        };
        T::take(&mut input)
    }

    /// A mirror's value on its way to the interpreter: its bytes and the
    /// native values it gives, for the runner to make the program value of
    /// (`rawr::island::mirror_in`).
    pub fn unmirrored<T: Mirror>(
        value: T,
    ) -> (::std::vec::Vec<u8>, ::std::vec::Vec<::std::option::Option<Given>>) {
        let mut out = Out(::std::vec::Vec::new(), ::std::vec::Vec::new());
        value.give(&mut out);
        (out.0, out.1)
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
    integers!(i8, i16, i32, i64, isize, u8, u16, u32);
    // The interpreter keeps a `u64` or a `usize` as the `i64` with the same
    // bits, so one past `i64::MAX` crosses too.
    macro_rules! wide {
        ($($ty:ty),*) => {$(
            impl Put for $ty {
                fn put(&self, out: &mut Out) {
                    out.byte(INT);
                    out.0.extend_from_slice(&(*self as u64 as i64).to_le_bytes());
                }
            }
            impl Take for $ty {
                fn take(input: &mut In<'_>) -> Self {
                    input.expect(INT, ::std::stringify!($ty));
                    i64::from_le_bytes(input.eight()) as u64 as $ty
                }
            }
        )*};
    }
    wide!(u64, usize);
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
    impl<T: Put, E: Put> Put for ::std::result::Result<T, E> {
        fn put(&self, out: &mut Out) {
            match self {
                Ok(value) => {
                    out.byte(OK);
                    value.put(out);
                }
                Err(error) => {
                    out.byte(ERR);
                    error.put(out);
                }
            }
        }
    }
    impl<T: Take, E: Take> Take for ::std::result::Result<T, E> {
        fn take(input: &mut In<'_>) -> Self {
            match input.byte() {
                OK => Ok(T::take(input)),
                ERR => Err(E::take(input)),
                _ => input.fail("expected a `Result`"),
            }
        }
    }
    macro_rules! tuples {
        ($(($($name:ident),+))*) => {$(
            impl<$($name: Put),+> Put for ($($name,)+) {
                #[allow(non_snake_case)]
                fn put(&self, out: &mut Out) {
                    let ($($name,)+) = self;
                    out.byte(ARRAY);
                    out.count([$(::std::stringify!($name)),+].len());
                    $($name.put(out);)+
                }
            }
            impl<$($name: Take),+> Take for ($($name,)+) {
                fn take(input: &mut In<'_>) -> Self {
                    input.tuple([$(::std::stringify!($name)),+].len());
                    ($($name::take(input),)+)
                }
            }
        )*};
    }
    tuples! {
        (A) (A, B) (A, B, C) (A, B, C, D) (A, B, C, D, E) (A, B, C, D, E, F)
    }
    impl<T: Put + ?Sized> Put for &T {
        fn put(&self, out: &mut Out) {
            (**self).put(out);
        }
    }

    type Accessor = &'static (dyn ::std::any::Any + Sync);

    /// What a trampoline lends one argument by (D558.3): the key its
    /// accessors claim, and the accessors, which the generated runner
    /// reaches the value through. A mutable one only for a `&mut`.
    pub struct Lend {
        key: ::std::any::TypeId,
        mutable: ::std::option::Option<Accessor>,
        shared: Accessor,
    }

    impl Lend {
        pub fn new(
            key: ::std::any::TypeId,
            mutable: ::std::option::Option<Accessor>,
            shared: Accessor,
        ) -> Self {
            Self { key, mutable, shared }
        }
    }

    /// Claim the lend `nonce` under `key` (shared, or exclusively) for
    /// `visit`, which receives the lent value's place, and end the claim
    /// however `visit` ends. `Err` holds `rawr::scope::Error`'s code.
    pub fn reach(
        nonce: u64,
        key: ::std::any::TypeId,
        shared: bool,
        visit: impl FnOnce(*mut ()),
    ) -> ::std::result::Result<(), u8> {
        struct Release(u64, bool);
        impl Drop for Release {
            fn drop(&mut self) {
                // SAFETY: the runner links `rawr`, which defines the
                // function with this exact signature.
                unsafe { __rawr_scope_release_v1(self.0, self.1) }
            }
        }
        // SAFETY: as above.
        let place = unsafe { __rawr_scope_claim_v1(nonce, key, shared) }?;
        let _release = Release(nonce, shared);
        visit(place);
        Ok(())
    }

    /// The anchor of `value` (#1473): its address and a hash of its type's
    /// name, which the runner keeps what native code borrows of it by.
    fn anchor_of<T: ?Sized>(value: &T) -> [u64; 2] {
        use ::std::hash::{Hash as _, Hasher as _};
        let mut hasher = ::std::collections::hash_map::DefaultHasher::new();
        ::std::any::type_name::<T>().hash(&mut hasher);
        [::std::ptr::from_ref(value).cast::<()>().addr() as u64, hasher.finish()]
    }

    /// One call of a marked function. `'c` is what its lent arguments
    /// borrow: they stay borrowed until the call is sent and answered.
    pub struct Call<'c> {
        path: &'static str,
        owned: bool,
        pinned: bool,
        in_place: u8,
        unit: bool,
        count: u32,
        mutable: u64,
        anchor: ::std::option::Option<[u64; 2]>,
        releases: ::std::vec::Vec<[u64; 2]>,
        /// The arguments given for the call alone ([`Call::for_call`]).
        for_call: u64,
        /// The lends kept with the call's anchor ([`Call::keep_lends`]).
        kept: u64,
        values: Out,
        lent: ::std::vec::Vec<(
            ::std::any::TypeId,
            &'static str,
            *mut (),
            ::std::option::Option<Accessor>,
            Accessor,
        )>,
        borrows: ::std::marker::PhantomData<&'c mut ()>,
    }

    impl<'c> Call<'c> {
        /// A call of the function at `path` from the crate root; `owned`
        /// when its last segment is a member of the type before it.
        pub fn new(path: &'static str, owned: bool) -> Self {
            Self {
                path,
                owned,
                pinned: false,
                in_place: 0,
                unit: false,
                count: 0,
                mutable: 0,
                anchor: ::std::option::Option::None,
                releases: ::std::vec::Vec::new(),
                for_call: 0,
                kept: 0,
                values: Out(::std::vec::Vec::new(), ::std::vec::Vec::new()),
                lent: ::std::vec::Vec::new(),
                borrows: ::std::marker::PhantomData,
            }
        }
        /// An argument lent to the interpreter for the call (D558.3), as
        /// ratatui's `&mut Frame<'_>` is. The interpreter's thread reaches
        /// it while this one waits for the answer, so it is `Send`.
        ///
        /// # Safety
        ///
        /// `lend`'s accessors read its key's place as a `T`, and no other
        /// lend uses its key. `name` is `T`'s name.
        pub unsafe fn lend<T: Send>(&mut self, value: &'c mut T, lend: Lend, name: &'static str) {
            debug_assert!(lend.mutable.is_some());
            self.lent_value(::std::ptr::from_mut(value).cast(), lend, name);
        }
        /// [`Call::lend`] of a shared borrow, which is `Sync`.
        ///
        /// # Safety
        ///
        /// As for [`Call::lend`], and `lend` has no mutable accessor.
        pub unsafe fn lend_ref<T: Sync>(&mut self, value: &'c T, lend: Lend, name: &'static str) {
            debug_assert!(lend.mutable.is_none());
            self.lent_value(::std::ptr::from_ref(value).cast_mut().cast(), lend, name);
        }
        fn lent_value(&mut self, place: *mut (), lend: Lend, name: &'static str) {
            // SAFETY: as the callers'.
            unsafe { self.lend_part(place, lend, name) };
            self.count += 1;
        }
        /// A part of an argument lent for the call where it is, as a
        /// native part of a marked value is (#1469). The call runs on this
        /// thread, so the part needn't be `Send` or `Sync`.
        ///
        /// # Safety
        ///
        /// `place` is borrowed for `'c`, mutably when `lend` has a mutable
        /// accessor. `lend`'s accessors read its key's place as the type it
        /// is, and no other lend uses its key. `name` is the type's name.
        pub unsafe fn lend_part(&mut self, place: *mut (), lend: Lend, name: &'static str) {
            self.values.byte(LENT);
            self.values.count(self.lent.len());
            self.lent.push((lend.key, name, place, lend.mutable, lend.shared));
        }
        /// A part of an argument copied by the wire format (#1469).
        pub fn part<T: Put + ?Sized>(&mut self, value: &T) {
            value.put(&mut self.values);
        }
        /// Where an argument's value is written, part by part.
        pub fn out(&mut self) -> &mut Out {
            &mut self.values
        }
        /// A borrow of a marked value that holds native parts, each lent
        /// where it is for the call (#1469).
        pub fn lent<T: Parts>(&mut self, value: &'c T) {
            value.lend(self);
            self.count += 1;
        }
        /// A `&mut` of a marked value that holds native parts, each lent
        /// mutably for the call, which comes back after it
        /// ([`Parts::take_into`]).
        pub fn lent_mut<T: Parts>(&mut self, value: &'c mut T) {
            self.releases.push(anchor_of(&*value));
            self.mutable |= 1 << self.count;
            value.lend_mut(self);
            self.count += 1;
        }
        /// A marked value that holds native parts, moved to the
        /// interpreter with them (#1469).
        pub fn given<T: Parts>(&mut self, value: T) {
            value.give(&mut self.values);
            self.count += 1;
        }
        /// An argument moved to the interpreter, which keeps it: a value of
        /// a type the marked crate doesn't declare, with what its type has
        /// of [`Traits`] ([`give!`]).
        pub fn give<T: 'static>(&mut self, value: T, traits: Traits) {
            self.values.give(value, traits);
            self.count += 1;
        }
        pub fn argument<T: Put + ?Sized>(&mut self, value: &T) {
            value.put(&mut self.values);
            self.count += 1;
        }
        /// A stand-in's receiver: the handle of the program value it holds,
        /// which the call takes as the impl's receiver.
        pub fn object(&mut self, object: &Object) {
            self.values.byte(HANDLE);
            self.values.0.extend_from_slice(&object.0.to_le_bytes());
            self.count += 1;
        }
        /// The call's result is a `&str` that borrows its stand-in's
        /// receiver, which comes back as the program's own text, pinned
        /// with the value ([`Object::pinned`]).
        pub fn pinned(&mut self) {
            self.pinned = true;
        }
        /// The call's result is a reference to a native value, which
        /// comes back where the value is: with `lasting`, a `&'static`
        /// ([`In::lasting`]), and else one its stand-in's receiver holds
        /// ([`Object::lent`]).
        pub fn in_place(&mut self, lasting: bool) {
            self.in_place = 4 | u8::from(lasting) << 3;
        }
        /// The call's result borrows `value`, a marked input it borrows
        /// (#1473): it is made in a region kept with what native code
        /// borrows of `value`, which the runner lets go of when native code
        /// next takes `value` `&mut`.
        pub fn anchor<T: ?Sized>(&mut self, value: &'c T) {
            self.anchor = ::std::option::Option::Some(anchor_of(value));
        }
        /// The next argument is given for the call alone, as a by-value
        /// `View<'a>` of borrows (#1225): what the call leaves of it drops
        /// once it returns.
        pub fn for_call(&mut self) {
            self.for_call |= 1 << self.count;
        }
        /// How many places the call has been lent so far.
        pub fn lends(&self) -> usize {
            self.lent.len()
        }
        /// Each place lent since there were `from` is kept borrowed as long
        /// as the call's anchored result (#1413), as `&'a inbox::State` of
        /// `view<'a>(&'a self, inbox: &'a inbox::State) -> Element<'a,
        /// Message>` is, so the result may borrow it.
        pub fn keep_lends(&mut self, from: usize) {
            for index in from..self.lent.len().min(64) {
                self.kept |= 1 << index;
            }
        }
        /// The function returns `()`, which comes back whatever value its
        /// body's last statement left.
        pub fn unit(&mut self) {
            self.unit = true;
        }
        /// An argument the call may change, which comes back after it.
        pub fn mutable<T: Put + ?Sized>(&mut self, value: &T) {
            self.releases.push(anchor_of(value));
            self.mutable |= 1 << self.count;
            self.argument(value);
        }
        pub fn send(mut self) -> Reply {
            register();
            let mut bytes = ::std::vec::Vec::with_capacity(13 + self.values.0.len());
            let anchored = u8::from(self.anchor.is_some()) << 5;
            let releases = u8::from(!self.releases.is_empty()) << 6;
            let scopes = u8::from(self.for_call != 0 || self.kept != 0) << 7;
            bytes.push(
                u8::from(self.owned)
                    | u8::from(self.pinned) << 1
                    | self.in_place
                    | u8::from(self.unit) << 4
                    | anchored
                    | releases
                    | scopes,
            );
            bytes.extend_from_slice(&self.count.to_le_bytes());
            bytes.extend_from_slice(&self.mutable.to_le_bytes());
            for [address, ty] in self.anchor.iter() {
                bytes.extend_from_slice(&address.to_le_bytes());
                bytes.extend_from_slice(&ty.to_le_bytes());
            }
            if !self.releases.is_empty() {
                bytes.extend_from_slice(&(self.releases.len() as u32).to_le_bytes());
                for [address, ty] in &self.releases {
                    bytes.extend_from_slice(&address.to_le_bytes());
                    bytes.extend_from_slice(&ty.to_le_bytes());
                }
            }
            if scopes != 0 {
                bytes.extend_from_slice(&self.for_call.to_le_bytes());
                bytes.extend_from_slice(&self.kept.to_le_bytes());
            }
            bytes.extend_from_slice(&self.values.0);
            // SAFETY: the runner links `rawr`, which defines the function
            // with this exact signature. Each lent place is borrowed for
            // `'c`, which outlives this call, and is reached only while the
            // call runs.
            let mut natives = ::std::vec::Vec::new();
            let reply = unsafe {
                __rawr_island_call_v4(self.path, &bytes, &mut self.values.1, &self.lent, &mut natives)
            };
            match reply.first() {
                Some(0) => Reply {
                    bytes: reply,
                    at: 1,
                    path: self.path,
                    natives: natives.into_iter().map(Some).collect(),
                    places: self.lent.iter().map(|lent| lent.2).collect(),
                },
                Some(_) => {
                    let message = ::std::string::String::from_utf8_lossy(&reply[1..]).into_owned();
                    ::std::panic!("{message}")
                }
                None => ::std::panic!("rawr: no answer to `{}`", self.path),
            }
        }
    }

    /// What a call returned, then each argument it could change, and the
    /// native values the interpreter moved for them.
    pub struct Reply {
        bytes: ::std::vec::Vec<u8>,
        at: usize,
        path: &'static str,
        natives: ::std::vec::Vec<::std::option::Option<::std::boxed::Box<dyn ::std::any::Any>>>,
        /// Where each part the call lent is ([`In::changed`]).
        places: ::std::vec::Vec<*mut ()>,
    }

    impl Reply {
        pub fn take<T: Take>(&mut self) -> T {
            self.read(T::take)
        }
        /// A `&T` into `receiver`, whose parts the call lent where they
        /// are, as `fn colors(&self) -> &Colors` gives `&self.0.colors`
        /// (#1468): the place the runner reached, and the accessor of the
        /// lend it reached it by, which proves the place a `T`. A place
        /// outside the receiver, as in a `Box` it owns, can't come back
        /// yet.
        ///
        /// # Safety
        ///
        /// The runner gave the place of a value the call reached through
        /// what it lent.
        pub unsafe fn lent<'r, T: 'static, R: ?Sized>(&mut self, receiver: &'r R) -> &'r T {
            type Shared<T> = fn(u64, &mut dyn FnMut(&T)) -> ::std::result::Result<(), u8>;
            let (place, accessor) = self.read(|input| input.native::<(*const (), Accessor)>());
            let accessor: &dyn ::std::any::Any = accessor;
            let start = ::std::ptr::from_ref(receiver).cast::<u8>().addr();
            let end = start + ::std::mem::size_of_val(receiver);
            let at = place.addr();
            if !accessor.is::<Shared<T>>() {
                ::std::panic!(
                    "rawr: `{}` gave back a reference that is no `&{}`",
                    self.path,
                    ::std::any::type_name::<T>()
                );
            }
            if at < start || at + ::std::mem::size_of::<T>() > end {
                ::std::panic!(
                    "rawr: `{}` gave back a reference to a `{}` the receiver holds outside \
                     itself, which can't come back yet",
                    self.path,
                    ::std::any::type_name::<T>()
                );
            }
            // SAFETY: the accessor reads its lend's place as a `T`, and the
            // place is in `receiver`, which is borrowed for `'r`.
            unsafe { &*place.cast::<T>() }
        }
        /// A `&str` the call's receiver holds (#1468): the text where it
        /// is in a part the call lent, or the program's own text, pinned
        /// for as long as the runner lives.
        ///
        /// # Safety
        ///
        /// The receiver stays borrowed for `'r`.
        pub unsafe fn pinned<'r>(&mut self) -> &'r str {
            let (address, length) = self.read(|input| {
                input.expect(PINNED, "a `&str`");
                (input.count(), input.count())
            });
            // SAFETY: the runner gave valid UTF-8 that lives at least as
            // long as the receiver is borrowed, which is `'r`.
            unsafe {
                ::std::str::from_utf8_unchecked(::std::slice::from_raw_parts(
                    ::std::ptr::with_exposed_provenance::<u8>(address),
                    length,
                ))
            }
        }
        /// The next value, as `read` reads it.
        pub fn read<T>(&mut self, read: impl FnOnce(&mut In<'_>) -> T) -> T {
            let mut input = In {
                bytes: &self.bytes[self.at..],
                path: self.path,
                natives: &mut self.natives,
                places: &self.places,
            };
            let value = read(&mut input);
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
        let rawr = root_text(
            text,
            false,
            &crate::project::cargo::features::Enabled::default(),
        );
        let lines = rawr.lines().collect::<Vec<_>>();
        assert_eq!(lines[0].trim(), "");
        assert_eq!(lines[1], "mod physics;");
        assert_eq!(lines[2], "pub mod util;");
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
                 let mut __call = crate::__rawr_island::Call::new(\"physics::Ball::advance\", true);\n        \
                 __call.unit();\n        __call.mutable(&*self);\n        __call.argument(&__a0);\n"
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
            text.contains(
                "pub fn scale(__a0: Scale) -> f64 {\n    \
                 let mut __call = crate::__rawr_island::Call::new(\"physics::scale\", false);\n    \
                 crate::__rawr_island::give!(__call, __a0);\n"
            ),
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

    /// A borrow of a type the marked crate doesn't declare, as ratatui's
    /// `&mut Frame<'_>`, is lent for the call with accessors of its own
    /// (D558.3); a `&` one is lent shared.
    #[test]
    fn a_borrow_of_another_crates_type_is_lent_with_its_accessors() {
        let shadow = generate(
            "// rawr\nuse ratatui::Frame;\npub struct App { pub n: i64 }\n\
             pub fn draw<'a>(frame: &mut Frame<'a>, app: &mut App) {}\n\
             pub fn look(frame: &Frame) -> i64 { 0 }\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        assert!(text.contains("use ratatui::Frame;\n"), "{text}");
        for expected in [
            "    let __a0_lend = {\n        struct Key;\n",
            "fn mutable(nonce: u64, visit: &mut dyn FnMut(&mut Frame<'_>)) -> ::std::result::Result<(), u8> {",
            "visit(unsafe { &mut *place.cast::<Frame<'_>>() })",
            "static SHARED: fn(u64, &mut dyn FnMut(&Frame<'_>)) -> ::std::result::Result<(), u8> = shared;",
            "crate::__rawr_island::Lend::new(::std::any::TypeId::of::<Key>(), Some(&MUTABLE), &SHARED)",
            "let () = unsafe { __call.lend(__a0, __a0_lend, ::std::any::type_name::<Frame<'_>>()) };",
            "    __call.mutable(&*__a1);\n",
            "crate::__rawr_island::Lend::new(::std::any::TypeId::of::<Key>(), None, &SHARED)",
            "let () = unsafe { { use crate::__rawr_island::{ByCopy as _, ByLoan as _}; \
             (&crate::__rawr_island::hand(__a0)).lend_ref(&mut __call, __a0, __a0_lend, \
             ::std::any::type_name::<Frame>()) } };",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        assert!(!text.contains("fn mutable(nonce: u64, visit: &mut dyn FnMut(&mut Frame))"));
        assert_eq!(
            unnamed_lifetimes("Cow<'static, Frame<'a>>"),
            "Cow<'static, Frame<'_>>"
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

    /// A marked type over a native one, as inboard's `enum Theme {
    /// Selected(themedef::Theme) }`, lends the interpreter its part where
    /// it is for a borrow, mutably for a `&mut`, which takes back what the
    /// call made of it, and moves it for a value (#1469). A `&self`
    /// method's reference into the part comes back where it is, and its
    /// `&str` as text (#1468).
    #[test]
    fn a_marked_type_over_a_native_one_lends_its_parts_and_moves_them() {
        let shadow = generate(
            "// rawr\nuse crate::palette;\n\
             pub enum Theme {\n    Selected(palette::Theme),\n}\n\
             pub struct Pair {\n    pub theme: Theme,\n    pub scale: u32,\n}\n\
             impl Theme {\n    \
             pub fn colors(&self) -> &palette::Colors { match self { Theme::Selected(t) => &t.colors } }\n    \
             pub fn name(&self) -> &str { \"\" }\n    \
             pub fn rename(&mut self, name: &str) {}\n    \
             pub fn into_native(self) -> palette::Theme { match self { Theme::Selected(t) => t } }\n}\n\
             pub fn scale(pair: &Pair) -> u32 { pair.scale }\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        for expected in [
            "impl crate::__rawr_island::Parts for Theme {",
            "crate::__rawr_island::lend_part!(call, __f0, palette::Theme);",
            "crate::__rawr_island::lend_part_mut!(call, __f0, palette::Theme);",
            "crate::__rawr_island::give_part!(out, __f0);",
            "(&crate::__rawr_island::Back::<palette::Theme>::new()).take_into(input, __f0);",
            "impl crate::__rawr_island::Parts for Pair {",
            "crate::__rawr_island::Parts::lend(&self.theme, call);",
            "call.part(&*&self.scale);",
            "    __call.lent(self);\n",
            "let __value: &palette::Colors = unsafe { __reply.lent(self) };",
            "let __value: &str = unsafe { __reply.pinned() };",
            "    __call.lent_mut(&mut *self);\n",
            "__reply.read(|input| crate::__rawr_island::Parts::take_into(&mut *self, input));",
            "    __call.given(self);\n",
            "    __call.lent(__a0);\n",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        assert!(!text.contains("panic!"), "{text}");
    }

    /// A result read part by part that borrows a marked input is anchored
    /// on that input, as rustc ties their lifetimes (#1473): `&'a self` of
    /// `view<'a>(&'a self, ..)`, the elided one of a free `view(&State)`,
    /// and not `&self` when the result names another input's lifetime. A
    /// `&mut` of a marked value lets go of what was borrowed of it. A
    /// native input lent at the anchor's lifetime is kept as long (#1413).
    #[test]
    fn a_result_that_borrows_a_marked_input_is_anchored_on_it() {
        let shadow = generate(
            "// rawr\nuse crate::{ui, Inbox};\n\
             pub struct State {\n    content: ui::Content,\n    title: String,\n}\n\
             impl State {\n    \
             pub fn view<'a>(&'a self, inbox: &'a Inbox) -> ui::Element<'a, u32> { todo!() }\n    \
             pub fn other<'a>(&self, inbox: &'a Inbox) -> ui::Element<'a, u32> { todo!() }\n    \
             pub fn edit(&mut self) {}\n}\n\
             pub fn show(state: &State) -> ui::Element<'_, u32> { todo!() }\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        let body = |name: &str| {
            let at = text.find(&format!("fn {name}")).unwrap();
            let end = text[at..].find("\n}\n").map_or(text.len(), |end| at + end);
            &text[at..end]
        };
        assert!(
            body("view").contains("    __call.anchor(self);\n"),
            "{text}"
        );
        assert!(!body("other").contains("__call.anchor("), "{text}");
        assert!(
            body("show").contains("    __call.anchor(__a0);\n"),
            "{text}"
        );
        assert!(!body("edit").contains("__call.anchor("), "{text}");
        assert_eq!(lifetimes("Element<'a, &'static str, '_>"), ["a", "_"]);
        // The native `&'a Inbox` the result may borrow is kept as long as
        // it (#1413); `other`'s isn't, as its call is anchored on nothing.
        assert!(
            body("view").contains("    let __a0_lends = __call.lends();\n")
                && body("view").contains("    __call.keep_lends(__a0_lends);\n"),
            "{text}"
        );
        assert!(!body("other").contains("keep_lends"), "{text}");
    }

    /// A marked type's native parts inside an `Option`, a `Vec`, a `Box`
    /// or a tuple, or a marked type that holds them inside one, are lent
    /// where they are, mutably too, moved for a value and taken back after
    /// a `&mut` call, as inboard's `home::State { wizard:
    /// Option<wizard::State> }` (#1475): nothing goes as a clone, and every
    /// method crosses.
    #[test]
    fn native_parts_in_an_option_a_vec_a_box_or_a_tuple_are_lent_where_they_are() {
        let shadow = generate(
            "// rawr\nuse crate::wizard;\n\
             pub struct Pane {\n    pub tab: wizard::Tab,\n}\n\
             pub struct State {\n    \
             wizard: Option<wizard::State>,\n    \
             tabs: Vec<wizard::Tab>,\n    \
             boxed: Box<wizard::Tab>,\n    \
             pair: (u32, wizard::Tab),\n    \
             panes: Vec<Option<Pane>>,\n    \
             count: Option<u32>,\n}\n\
             impl State {\n    \
             pub fn open(&mut self) { self.wizard = None; }\n    \
             pub fn count(&self) -> usize { self.tabs.len() }\n    \
             pub fn into_tabs(self) -> Vec<wizard::Tab> { self.tabs }\n}\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        for expected in [
            "impl crate::__rawr_island::Parts for State {",
            "match &self.wizard { ::std::option::Option::None => call.out().none(), \
             ::std::option::Option::Some(__p0) => { call.out().some(); \
             crate::__rawr_island::lend_part!(call, __p0, wizard::State); } }",
            "match &mut self.wizard { ::std::option::Option::None => call.out().none(), \
             ::std::option::Option::Some(__p0) => { call.out().some(); \
             crate::__rawr_island::lend_part_mut!(call, __p0, wizard::State); } }",
            "if input.some() { match &mut self.wizard { ::std::option::Option::Some(__p0) => \
             { (&crate::__rawr_island::Back::<wizard::State>::new()).take_into(input, __p0); } \
             __p0 => *__p0 = ::std::option::Option::Some(\
             (&crate::__rawr_island::Back::<wizard::State>::new()).take(input)), } } \
             else { *&mut self.wizard = ::std::option::Option::None; }",
            "{ let __p0 = &self.tabs; call.out().array(__p0.len()); for __e0 in __p0 \
             { crate::__rawr_island::lend_part!(call, __e0, wizard::Tab); } }",
            "crate::__rawr_island::vec_into(input, &mut self.tabs, |input, __p0| \
             { (&crate::__rawr_island::Back::<wizard::Tab>::new()).take_into(input, __p0); }, \
             |input| (&crate::__rawr_island::Back::<wizard::Tab>::new()).take(input));",
            "crate::__rawr_island::lend_part!(call, &**&self.boxed, wizard::Tab);",
            "crate::__rawr_island::give_part!(out, *__f2);",
            "{ let (__t0_0, __t0_1,) = &self.pair; call.out().array(2); \
             call.part(&*__t0_0); crate::__rawr_island::lend_part!(call, __t0_1, wizard::Tab); }",
            "crate::__rawr_island::Parts::lend(__p1, call);",
            "crate::__rawr_island::Parts::take_into(__p1, input);",
            "input.vec(|input| input.option(|input| crate::__rawr_island::Take::take(input)))",
            "call.part(&*&self.count);",
            "    __call.lent_mut(&mut *self);\n",
            "    __call.given(self);\n",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        assert!(!text.contains("panic!"), "{text}");
    }

    /// A `Result` of types that cross crosses as one (#1118). A result that
    /// is a `Result` with a native part, or an alias of one such as
    /// `iced::Result`, comes back as an outcome: each part by the wire
    /// format when its type crosses by it, and else as the native value the
    /// interpreter moves.
    #[test]
    fn a_result_crosses_and_an_alias_of_one_moves_its_native_parts() {
        let shadow = generate(
            "// rawr\npub struct Ball { pub x: f64 }\n\
             pub fn checked(n: i64) -> Result<i64, String> { Ok(n) }\n\
             pub fn run() -> iced::Result { iced::run(update, view) }\n\
             pub fn read(path: &str) -> std::io::Result<String> { std::fs::read_to_string(path) }\n\
             pub fn ball(x: f64) -> Result<Ball, iced::Error> { Ok(Ball { x }) }\n\
             pub fn give(result: Result<i64, String>) -> i64 { 0 }\n\
             pub fn pair() -> Result<(i64, i64), String> { Ok((1, 2)) }\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let text = &shadow.files[0].1;
        assert!(
            text.contains("    let __value: Result<i64, String> = __reply.take();\n"),
            "{text}"
        );
        assert!(text.contains("    __call.argument(&__a0);\n"), "{text}");
        assert!(!text.contains("can't call `physics::give`"), "{text}");
        let island = "crate::__rawr_island";
        let part = |output: &str, part: &str| {
            format!(
                "|input| (&{island}::Back::<<{output} as {island}::Outcome>::{part}>::new()).take(input)"
            )
        };
        for (output, function, read) in [
            (
                "iced::Result",
                "run",
                format!(
                    "input.result({}, {})",
                    part("iced::Result", "Ok"),
                    part("iced::Result", "Err")
                ),
            ),
            (
                "std::io::Result<String>",
                "read",
                format!(
                    "input.result({}, {})",
                    part("std::io::Result<String>", "Ok"),
                    part("std::io::Result<String>", "Err")
                ),
            ),
            (
                "Result<Ball, iced::Error>",
                "ball",
                format!(
                    "input.result(|input| {island}::Take::take(input), \
                     |input| (&{island}::Back::<iced::Error>::new()).take(input))"
                ),
            ),
        ] {
            let expected = format!(
                "    let __value: {output} = {{\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
                 __reply.read(|input| {read})\n    }};\n"
            );
            assert!(text.contains(&expected), "{function}: {expected}\n{text}");
        }
        assert!(
            text.contains("    let __value: Result<(i64, i64), String> = __reply.take();\n"),
            "{text}"
        );
    }

    /// A result of a type the marked crate doesn't declare, as an app's own
    /// `Message`, comes back as the native value the interpreter moves, bare
    /// or inside an `Option`, a `Vec`, a tuple or a `Result`, each lifetime
    /// it names read as `'static` (#1203). One taken by value is given to
    /// the interpreter with what its type has of `Traits` (#1139), for the
    /// call alone when its type names a lifetime (#1225).
    #[test]
    fn native_values_come_back_part_by_part_and_go_in_by_value() {
        let shadow = generate(
            "// rawr\nuse crate::{Item, Message};\npub struct Ball { pub x: f64 }\n\
             pub fn message() -> Message { Message::Closed }\n\
             pub fn maybe() -> Option<Item> { None }\n\
             pub fn items() -> Vec<Item> { Vec::new() }\n\
             pub fn pair() -> (Message, u32, Ball) { (Message::Closed, 1, Ball { x: 0.0 }) }\n\
             pub fn found() -> Result<Vec<Item>, String> { Ok(Vec::new()) }\n\
             pub fn view<'a>(count: u32) -> ui::Element<'a, Message> { ui::button(count) }\n\
             pub fn grow(item: Item, by: u32) -> Item { item }\n\
             pub fn keep(element: ui::Element<'static, Message>) {}\n\
             pub fn lose<'a>(element: ui::Element<'a, Message>) {}\n\
             pub fn raise(message: &mut Message) {}\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let text = &shadow.files[0].1;
        let island = "crate::__rawr_island";
        let back = |ty: &str| format!("(&{island}::Back::<{ty}>::new()).take(input)");
        let take = format!("{island}::Take::take(input)");
        for (output, read) in [
            ("Message", back("Message")),
            (
                "Option<Item>",
                format!("input.option(|input| {})", back("Item")),
            ),
            ("Vec<Item>", format!("input.vec(|input| {})", back("Item"))),
            (
                "(Message, u32, Ball)",
                format!(
                    "{{ input.tuple(3); ({}, {take}, {take},) }}",
                    back("Message")
                ),
            ),
            (
                "Result<Vec<Item>, String>",
                format!(
                    "input.result(|input| input.vec(|input| {}), |input| {take})",
                    back("Item")
                ),
            ),
            (
                "ui::Element<'a, Message>",
                back("ui::Element<'static, Message>"),
            ),
        ] {
            let expected = format!(
                "    let __value: {output} = {{\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
                 __reply.read(|input| {read})\n    }};\n"
            );
            assert!(text.contains(&expected), "{expected}\n{text}");
        }
        assert!(
            text.contains(&format!(
                "    {island}::give!(__call, __a0);\n    __call.argument(&__a1);\n"
            )),
            "{text}"
        );
        assert!(
            text.contains(&format!(
                "    let __a0 = unsafe {{ ::std::mem::transmute::<ui::Element<'_, Message>, \
                 ui::Element<'static, Message>>(__a0) }};\n    __call.for_call();\n    \
                 {island}::give!(__call, __a0);\n"
            )),
            "{text}"
        );
        assert!(
            text.contains(
                "    { use crate::__rawr_island::{ByCopy as _, ByLoan as _}; \
                 (&crate::__rawr_island::hand(&*__a0)).back(&mut __reply, __a0) }\n"
            ),
            "{text}"
        );
    }

    /// A marked type's impl of a crate trait gets a stand-in holding the
    /// program value's handle, whose functions take it as their receiver,
    /// and a maker the support registers by the type's path (#1264). A
    /// function that names `Self` can't be called on the stand-in.
    #[test]
    fn a_crate_traits_impl_gets_a_stand_in_and_a_maker() {
        let shadow = generate(
            "// rawr\nuse crate::page::{self, Message};\nuse std::fmt;\n\
             #[derive(Clone, PartialEq, Eq)]\npub struct Badge {\n    count: usize,\n}\n\
             impl page::Page for Badge {\n    fn title(&self) -> String { String::new() }\n    \
             fn update(&mut self, message: Message) -> Option<Message> { None }\n    \
             fn same(&self, other: &Self) -> bool { true }\n    \
             fn label(&self) -> &str { \"\" }\n    \
             fn items<'a>(&'a self) -> &'a [String] { &[] }\n    \
             fn named(&self) -> &'static str { \"\" }\n}\n\
             impl fmt::Display for Badge {\n    \
             fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { Ok(()) }\n}\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        assert!(
            text.contains("pub struct __RawrBadge(pub crate::__rawr_island::Object);\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "impl page::Page for crate::physics::__RawrBadge {\n    \
                 fn title(&self) -> String {\n        \
                 let mut __call = crate::__rawr_island::Call::new(\"physics::Badge::Page::title\", true);\n        \
                 __call.object(&self.0);\n"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "        __call.object(&self.0);\n        \
                 crate::__rawr_island::give!(__call, __a0);\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("can't call `physics::Badge::Page::same` yet"),
            "{text}"
        );
        // A `&str` that borrows the receiver is the program's own text,
        // pinned with the value (#1322); a `&[T]`, whose program value is
        // no native slice, is kept by the stand-in for as long as `&self`
        // is borrowed, and a `&mut self` call lets what it kept go
        // (#1305); a `&'static` one is no borrow of it.
        assert!(
            text.contains(
                "        __call.pinned();\n        __call.object(&self.0);\n        \
                 let mut __reply = __call.send();\n        \
                 let __value: &str = self.0.pinned(&mut __reply);\n"
            ),
            "{text}"
        );
        let expected = "    let __value: &'a [String] = \
                        self.0.keep::<::std::vec::Vec<String>>(__reply.take());\n";
        assert!(text.contains(expected), "{expected}\n{text}");
        assert!(
            text.contains(
                "fn update(&mut self, __a0: Message) -> Option<Message> {\n        \
                 self.0.release();\n"
            ),
            "{text}"
        );
        // A `&'static str` is the program's own text, pinned for as long as
        // the runner lives (#1442).
        assert!(
            text.contains(
                "        __call.in_place(true);\n        __call.object(&self.0);\n        \
                 let mut __reply = __call.send();\n        \
                 let __value: &'static str = __reply.read(|input| input.lasting_text());\n"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "pub fn __rawr_object_0(trait_name: &str, handle: u64) -> \
                 ::std::option::Option<crate::__rawr_island::Given> {\n    \
                 crate::__rawr_island::object!(trait_name, handle, dyn page::Page, \
                 crate::physics::__RawrBadge)\n}\n"
            ),
            "{text}"
        );
        // A std trait is not the crate's: no maker for it, but the
        // stand-in implements each the type does, derived or written, as a
        // supertrait may require (#1301).
        assert!(
            text.contains(
                "impl ::std::fmt::Display for crate::physics::__RawrBadge {\n    \
                 fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {\n        \
                 f.write_str(&self.0.text(\"physics::Badge\", crate::__rawr_island::DISPLAY))\n"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "impl ::std::clone::Clone for crate::physics::__RawrBadge {\n    \
                 fn clone(&self) -> Self {\n        \
                 Self(self.0.duplicate(\"physics::Badge\"))\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("impl ::std::cmp::Eq for crate::physics::__RawrBadge {\n}\n"),
            "{text}"
        );
        assert!(!text.contains("::std::hash::Hash for"), "{text}");
        assert_eq!(
            shadow.objects,
            [(
                "physics::Badge".to_owned(),
                "crate::physics::__rawr_object_0".to_owned()
            )]
        );
    }

    /// A stand-in's reference to a native value comes back where the value
    /// is (#1442): a `&'static T` or `&'static [T]` as a static's own place
    /// and a `&T` the receiver holds lent in place for as long as `&self`
    /// is. One that can't come back is an error before anything runs.
    #[test]
    fn a_stand_in_gives_native_references_in_place() {
        let shadow = generate(
            "// rawr\nuse crate::page;\npub struct Badge {\n    state: page::State,\n}\n\
             impl page::Page for Badge {\n    \
             fn meta(&self) -> &'static page::Meta { &META }\n    \
             fn blocks(&self) -> &'static [page::Block] { BLOCKS }\n    \
             fn state(&self) -> &page::State { &self.state }\n    \
             fn preview(&self, id: &str) -> ui::Element<'_, Message, Theme> { view(id) }\n}\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        for (output, read) in [
            (
                "&'static page::Meta",
                "__reply.read(|input| input.lasting())",
            ),
            (
                "&'static [page::Block]",
                "__reply.read(|input| input.lasting_slice())",
            ),
        ] {
            let expected = format!(
                "        __call.in_place(true);\n        __call.object(&self.0);\n        \
                 let mut __reply = __call.send();\n        let __value: {output} = {read};\n"
            );
            assert!(text.contains(&expected), "{expected}\n{text}");
        }
        assert!(
            text.contains(
                "        __call.in_place(false);\n        __call.object(&self.0);\n        \
                 let mut __reply = __call.send();\n        \
                 let __value: &page::State = self.0.lent(&mut __reply);\n"
            ),
            "{text}"
        );
        // An output that borrows `&self` is made in the region its call
        // runs in, kept with the value (#1321).
        assert!(
            text.contains(
                "__reply.read(|input| (&crate::__rawr_island::Back::<\
                 ui::Element<'static, Message, Theme>>::new()).take(input))"
            ),
            "{text}"
        );
        let shadow = generate(
            "// rawr\nuse crate::page;\npub struct Badge {\n    rows: Vec<page::Row>,\n}\n\
             impl page::Page for Badge {\n    \
             fn rows(&self) -> &[page::Row] { &self.rows }\n}\n",
        );
        assert_eq!(
            shadow.refused,
            [
                "error: native code can't borrow the result of `physics::Badge::Page::rows` \
              through rawr yet: a `Row` can't cross yet\n  --> /crate/src/physics.rs:7:23"
            ]
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

    /// Inboard's `theme.rs` (#1466): a marked `Theme` over a native
    /// module's `Theme` crosses with that part given and taken as a native
    /// value, a dependency's `Theme` is no marked one either, and every
    /// impl native code relies on is there, each function keyed by its
    /// trait's instance as the interpreter keys it.
    #[test]
    fn a_marked_type_over_a_native_one_crosses_and_keeps_its_impls() {
        let shadow = generate(
            "// rawr\nmod themedef;\nuse iced::widget::markdown::Settings;\n\
             #[derive(Debug, Clone, PartialEq)]\n\
             pub enum Theme {\n    Selected(themedef::Theme),\n}\n\
             impl Default for Theme {\n    fn default() -> Self { Theme::Selected(themedef::Theme::default()) }\n}\n\
             impl From<themedef::Theme> for Theme {\n    fn from(theme: themedef::Theme) -> Self { Theme::Selected(theme) }\n}\n\
             impl From<Theme> for iced::Theme {\n    fn from(theme: Theme) -> Self { iced::Theme::Light }\n}\n\
             impl From<&Theme> for Settings {\n    fn from(theme: &Theme) -> Self { Self::with_text_size(14.0) }\n}\n\
             impl Eq for Theme {}\n\
             pub fn named(theme: &Theme) -> String { String::new() }\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let text = &shadow.files[0].1;
        assert!(!text.contains("panic!"), "{text}");
        assert!(text.contains("pub mod themedef;"), "{text}");
        assert!(
            text.contains("crate::__rawr_island::part!(out, __f0);"),
            "{text}"
        );
        assert!(
            text.contains(
                "(\"Selected\", 1) => Self::Selected((&crate::__rawr_island::Back::<themedef::Theme>::new()).take(input)),"
            ),
            "{text}"
        );
        for (header, path) in [
            (
                "impl Default for Theme {",
                "physics::Theme::Default::default",
            ),
            (
                "impl From<themedef::Theme> for Theme {",
                "physics::Theme::From<themedef::Theme>::from",
            ),
            (
                "impl From<Theme> for iced::Theme {",
                "::Theme::From<Theme>::from",
            ),
            (
                "impl From<&Theme> for Settings {",
                "::Settings::From<&Theme>::from",
            ),
        ] {
            let at = text
                .find(header)
                .unwrap_or_else(|| panic!("{header}: {text}"));
            assert!(
                text[at..].contains(&format!("Call::new({path:?}, true)")),
                "{path}: {text}"
            );
        }
        // The native theme goes in by value; the marked one by the wire.
        assert!(
            text.contains("crate::__rawr_island::give!(__call, __a0);"),
            "{text}"
        );
        assert!(
            text.contains(
                "__reply.read(|input| (&crate::__rawr_island::Back::<Settings>::new()).take(input))"
            ),
            "{text}"
        );
        assert!(text.contains("impl Eq for Theme {\n}\n"), "{text}");
        assert!(
            text.contains("use iced::widget::markdown::Settings;"),
            "{text}"
        );
    }
}
