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
//! new native build, so the program restarts, and so does one that takes
//! an item away from native code. A save that leaves them unchanged (a
//! function body, a private item) swaps in the running program at the
//! next call through a trampoline, and so does a type's new derive or a
//! new item native code can see, which the native code built without it
//! never names (#1737, #1741). With the uses the running build's check
//! recorded, a change needs a new native build only where it reaches one
//! of them, so an item native code doesn't name goes away in place too
//! (`reach`, D935.3).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::item::{self, type_ref};
use crate::{ImmutableString, Span};

#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
mod reach;

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
    /// Each generic function native code calls through an instance entry
    /// (D869), by the path its call names, with its entry's path from the
    /// crate root, for [`support`]'s table.
    #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
    pub(crate) generic: BTreeMap<String, String>,
}

/// What native code sees of a crate's marked modules: each item's path
/// and generated text as rustc builds it (`built`), and each type's
/// derives apart from its text. Two shapes are equal exactly when the
/// generated files differ at most in their comments, their layout and
/// their inert attributes, which rustc builds the same, so an unchanged
/// shape needs no new native build (#1646). [`Shape::change`] says which
/// other changes need none.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Shape {
    entries: BTreeMap<String, Entry>,
    /// The derives each type writes, by its path, which its entry's text
    /// leaves out. Native code built without a derive's impl never calls
    /// it, so a new derive alone needs no new native build (#1737).
    derives: BTreeMap<String, BTreeSet<String>>,
}

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
    /// What built native code reaches without naming it, so a new one
    /// needs a new native build: a `Drop` impl's `drop`, which the drop
    /// glue of each value native code drops calls, and a stand-in, whose
    /// maker the runner looks up to give a marked value to native code as
    /// a `dyn Trait` (#1741).
    Implicit,
    /// A trait impl's function, or the impl when it has none: native code
    /// calls it through the trait, for a value of a type its header names,
    /// without naming it.
    Impl,
    Other,
}

impl Shape {
    /// Whether the crate has no marked item native code sees.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Why native code must be built again when the shape goes from `self`
    /// to `newer`, in one line; `None` when it needn't be.
    ///
    /// A type's new derive alone needs none: native code built without the
    /// derive's impl doesn't call it, native code edited to call it is a
    /// native change of its own, and a marked call that needs it at a
    /// native function is a native use the running build doesn't cover
    /// (#1737). A derive that goes away does, as native code may call it,
    /// and so does one that changes what else is generated for its type,
    /// as `Copy` ends the tracking of its drops and a native trait
    /// object's stand-in takes each std derive: the line names the derive.
    ///
    /// A new item native code can see needs none either, by the same
    /// reasoning (#1741): a function, type, constant, trait, module or
    /// re-export, an impl's function, or a trait's impl for a type, which
    /// native code built without it never names. A marked value at a native
    /// generic's new instance, as a new impl's type at a native bound, is a
    /// use the running build doesn't cover, which the runner's compile
    /// finds. A new [`Kind::Implicit`] entry does need one: native code
    /// reaches a `Drop` impl and a stand-in without naming them. An item
    /// that goes away needs one, as native code may name it and rustc must
    /// say so, and so does one whose text changes, as a signature does.
    #[must_use]
    pub fn change(&self, newer: &Self) -> Option<String> {
        if self == newer {
            return None;
        }
        let what = |kind: Kind| match kind {
            Kind::Type => "its fields",
            Kind::Function | Kind::Impl => "its signature",
            Kind::Constant => "its value, which native code reads",
            Kind::Implicit | Kind::Other => "what native code sees of it",
        };
        // An entry of a type whose derives changed, its own or one
        // generated for it (`Ball as dyn Trait`), changed with them.
        let derived = |path: &str| {
            newer.derives.keys().find_map(|owner| {
                let own = path
                    .strip_prefix(owner.as_str())
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '));
                own.then(|| self.derive_change(newer, owner)).flatten()
            })
        };
        for (path, entry) in &newer.entries {
            let changed = match self.entries.get(path) {
                None if entry.kind == Kind::Implicit => {
                    format!("native code can now see `{path}`")
                }
                Some(old) if old != entry => format!("`{path}` changed {}", what(entry.kind)),
                None | Some(_) => continue,
            };
            return Some(derived(path).unwrap_or(changed));
        }
        let removed = self
            .entries
            .keys()
            .filter(|path| !newer.entries.contains_key(*path))
            .collect::<Vec<_>>();
        if let Some(first) = removed.first() {
            // Each item gone, without the members of one that went with it.
            let items = removed
                .iter()
                .filter(|path| !removed.iter().any(|owner| member(path, owner)))
                .map(|path| format!("`{path}`"))
                .collect::<Vec<_>>();
            let items = items.iter().map(String::as_str).collect::<Vec<_>>();
            return Some(
                derived(first)
                    .unwrap_or_else(|| format!("native code can no longer see {}", listed(&items))),
            );
        }
        self.derives
            .iter()
            .filter(|(path, derives)| {
                newer
                    .derives
                    .get(*path)
                    .is_some_and(|kept| !derives.is_subset(kept))
            })
            .find_map(|(path, _)| self.derive_change(newer, path))
    }

    /// The first item native code can see in `newer` that it can't in
    /// `self`, in a line that names it, as "native code can now see
    /// `theme::Inset`"; `None` when there is none.
    #[must_use]
    pub fn added(&self, newer: &Self) -> Option<String> {
        newer
            .entries
            .keys()
            .find(|path| !self.entries.contains_key(*path))
            .map(|path| format!("native code can now see `{path}`"))
    }

    /// The first type whose derives differ from `self` to `newer`, in a
    /// line that names them, as "`physics::Ball` derives Clone now";
    /// `None` when no type's do.
    #[must_use]
    pub fn derived(&self, newer: &Self) -> Option<String> {
        newer
            .derives
            .keys()
            .find_map(|path| self.derive_change(newer, path))
    }

    /// How the derives of the type at `path` changed from `self` to
    /// `newer`, in one line; `None` when they didn't, or when one of the
    /// shapes has no such type.
    fn derive_change(&self, newer: &Self, path: &str) -> Option<String> {
        let (old, new) = (self.derives.get(path)?, newer.derives.get(path)?);
        let added = new.difference(old).map(String::as_str).collect::<Vec<_>>();
        let removed = old.difference(new).map(String::as_str).collect::<Vec<_>>();
        match (added.as_slice(), removed.as_slice()) {
            ([], []) => None,
            (added, []) => Some(format!("`{path}` derives {} now", listed(added))),
            ([], removed) => Some(format!("`{path}` no longer derives {}", listed(removed))),
            (added, removed) => Some(format!(
                "`{path}` derives {} now and no longer derives {}",
                listed(added),
                listed(removed)
            )),
        }
    }

    /// The shape of the generated `entries`, each as rustc builds it, a
    /// type's derives apart from its text.
    fn built(entries: BTreeMap<String, Entry>) -> Self {
        let mut shape = Self::default();
        for (path, Entry { kind, text }) in entries {
            let mut text = built(&text);
            if kind == Kind::Type {
                let (rest, derives) = without_derives(&text);
                text = rest;
                shape.derives.insert(path.clone(), derives);
            }
            shape.entries.insert(path, Entry { kind, text });
        }
        shape
    }
}

/// Whether the entry at `path` is a member of the item at `owner`, its
/// impl's function (`physics::Ball::fall`) or a stand-in for it
/// (`physics::Ball as dyn page::Page`).
fn member(path: &str, owner: &str) -> bool {
    path.strip_prefix(owner)
        .is_some_and(|rest| rest.starts_with("::") || rest.starts_with(' '))
}

/// `names` in a line: `A`, `A and B`, `A, B and C`.
fn listed(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The `built` text of a type without the derive attributes it starts
/// with, and the paths they derive as written (`Clone`, `serde::Serialize`).
fn without_derives(built: &str) -> (String, BTreeSet<String>) {
    let mut kept = String::with_capacity(built.len());
    let mut derives = BTreeSet::new();
    let mut rest = built;
    while let Some(attribute) = rest.strip_prefix("#[") {
        let Some(length) = bracketed(attribute) else {
            break;
        };
        match attribute[..length]
            .strip_prefix("derive(")
            .and_then(|inside| inside.strip_suffix(')'))
        {
            Some(inside) => derives.extend(
                inside
                    .split(',')
                    .filter(|derived| !derived.is_empty())
                    .map(str::to_owned),
            ),
            None => kept.push_str(&rest[..length + 3]),
        }
        rest = &attribute[length + 1..];
    }
    kept.push_str(rest);
    (kept, derives)
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
    /// `&str`: the text is copied, with where native code's own text is,
    /// which what the program makes of the copy borrows instead, as
    /// iced's `Text<'a>` of `subtitle<'a>(label: &'a str)` borrows `label`.
    Text,
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
    /// A value that holds native parts inside `Option`s, `Vec`s, `Box`es
    /// and tuples, as inboard's `home_grid: Option<Element<'a,
    /// grid::Message>>` (#1224): each part moved to the interpreter by
    /// `give`, which writes the value `__value` part by part, as the
    /// result's parts come back. When its type, written `ty`, names a
    /// lifetime, for the call alone, as [`Pass::GiveForCall`].
    GiveHeld { give: String, ty: Option<String> },
    /// `&[T]` of a type the marked crate doesn't declare, as inboard's
    /// `browser_updates: &'a [extension::registry::Update]` (#1292): the
    /// slice lent where it is for the call, as a `&T` is ([`Pass::Lend`]),
    /// its element written `element` with each named lifetime as `'_`.
    LendSlice { element: String },
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
    /// An argument-position `impl Trait` of a marked trait, at `path` as
    /// the signature writes it, as inboard's `impl IntoContent<'a,
    /// Message>`: the impl that applies to the argument's type gives it,
    /// by its [`GIVE`] function, as a value of a type it crosses as.
    /// `given` is the trait's [`GIVEN`] at the bound's arguments, by which
    /// a generic function's instance gives it boxed.
    Trait { path: String, given: String },
    /// An argument-position `impl` of a marked trait that a generic
    /// function's trampoline boxed as the trait's [`GIVEN`], at `path`,
    /// given by its [`GIVE_BOXED`].
    Boxed { path: String },
}

/// The function each marked trait native code sees has beside its own, by
/// which each of its impls gives a value of its type to a call: a value of
/// the impl's type, or of an owned type that implements the trait as the
/// impl's bound does. A trait native code implements itself keeps the
/// trait's own, which panics.
const GIVE: &str = "__rawr_give";

/// The prefix of the trait each marked trait native code sees has beside
/// it, by which a generic function's trampoline hands an `impl` of the
/// trait to its instance boxed, whose type it can name ([`given_boxed`]).
const GIVEN: &str = "__RawrGiven";

/// The function of a [`GIVEN`] trait: the boxed value's [`GIVE`].
const GIVE_BOXED: &str = "__rawr_give_boxed";

/// The function each marked trait native code sees has beside [`GIVE`],
/// by which each of its impls names itself to a generic function's
/// trampoline: `+` and its type's name as written, `+Content`, or `+_`
/// for an impl for every type of a bound, which the trampoline adds to its
/// instance's key, as the resolver compiles one instance for each impl
/// (D869).
const GIVEN_NAME: &str = "__rawr_given";

/// The [`GIVEN`] trait of the marked trait `declared`, generic as it is,
/// and its impl for every type that implements it. Its functions are
/// callable on a trait object, so a generic function's trampoline boxes an
/// argument-position `impl` of the trait as one, which its instance
/// entry, where each type is `'static`, gives by the impl's [`GIVE`]
/// (D869).
fn given_boxed(declared: &item::Trait) -> String {
    let island = format!("crate::{MODULE}");
    let name = &declared.name.text;
    let parameters = declared
        .lifetimes
        .params
        .iter()
        .map(|lifetime| format!("'{}", lifetime.text.trim_start_matches('\'')))
        .chain(
            declared
                .generics
                .iter()
                .map(|generic| generic.name.text.to_string()),
        )
        .collect::<Vec<_>>();
    let arguments = if parameters.is_empty() {
        String::new()
    } else {
        format!("<{}>", parameters.join(", "))
    };
    let generics = parameters
        .iter()
        .cloned()
        .chain(std::iter::once(format!("__T: {name}{arguments}")))
        .collect::<Vec<_>>()
        .join(", ");
    let statics = declared
        .generics
        .iter()
        .map(|generic| format!("{}: 'static", generic.name.text))
        .collect::<Vec<_>>();
    let clause = if statics.is_empty() {
        String::new()
    } else {
        format!("\n    where\n        {}", statics.join(",\n        "))
    };
    let signature = format!(
        "fn {GIVE_BOXED}(self: ::std::boxed::Box<Self>, __call: &mut {island}::Call<'_>){clause}"
    );
    let named = format!("fn {GIVEN_NAME}(&self) -> &'static str");
    format!(
        "#[doc(hidden)]\npub(crate) trait {GIVEN}{name}{arguments} {{\n    \
         {signature};\n    {named};\n}}\n\
         impl<{generics}> {GIVEN}{name}{arguments} for __T {{\n    \
         {signature} {{\n        {name}::{GIVE}(*self, __call)\n    }}\n    \
         {named} {{\n        {name}::{GIVEN_NAME}(self)\n    }}\n}}\n"
    )
}

/// What a trampoline makes of an argument-position `impl Trait`, as
/// inboard's `ids: impl IntoIterator<Item = Id>`, whose value may be of
/// any type and borrow anything: an owned value of `ty`, written
/// `written`, which implements the trait and which the body can use only
/// as the trait allows, made by `make` from the argument, named
/// `__value`. The argument's own value never reaches the interpreter, so
/// nothing of it outlives the call.
struct Stand {
    ty: type_ref::Type,
    written: String,
    make: String,
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
    /// A type parameter of the impl, as `T` of a marked trait's `impl<T>
    /// IntoContent for T where T: IntoIterator<..>`: no value of it
    /// crosses but by the trait's [`GIVE`].
    Generic,
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
    root: &'a [item::Parsed],
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
    let generator = Generator::new(&sources, root);
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
    root: &'a [item::Parsed],
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
    Generator::new(&sources, root)
        .crossing
        .iter()
        .map(|path| path.split("::").map(ImmutableString::from).collect())
        .collect()
}

/// Generate the shadow files of the marked sources of a crate whose root
/// is native. `module_root` is the directory of the crate root; `sources`
/// yields each marked source's module path, file, text and items. Only the
/// functions `called` names get their trampolines' bodies, when it names
/// any ([`Generator::called`]); the shape is the same either way.
pub(crate) fn shadow<'a>(
    module_root: &Path,
    sources: impl IntoIterator<Item = (&'a [ImmutableString], &'a Path, &'a str, &'a [item::Parsed])>,
    root: &'a [item::Parsed],
    called: Option<&'a BTreeSet<String>>,
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
    let mut generator = Generator::new(&sources, root);
    generator.called = called;
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
        shape: Shape::built(shape),
        #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
        refused,
        #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
        objects,
        #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
        generic: generator.generic.take(),
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
    /// Traits declared in marked code: their impls are not mirrored
    /// unless the trait is one of `shown`.
    traits: BTreeSet<&'a str>,
    /// Those of `traits` that are visible, which native code sees as
    /// declared, with their impls, so a marked function may take an `impl`
    /// of one ([`Pass::Trait`]).
    shown: BTreeSet<&'a str>,
    /// The path of each marked trait whose name another module's marked
    /// trait has too, which the interpreter knows it by, as
    /// `home::IntoContent` beside `sidebar::IntoContent`.
    shared: BTreeSet<String>,
    /// The std traits each marked type implements, derived or written, by
    /// the type's name: what its stand-in implements too (#1301).
    implemented: BTreeMap<&'a str, BTreeSet<Std>>,
    /// Those of `crossing` that hold a native part, directly or in a field
    /// of such a type, as `enum Theme { Selected(themedef::Theme) }`: a
    /// borrow of one lends the interpreter its parts where they are, and a
    /// value moves them (#1469).
    holding: BTreeSet<String>,
    /// Each generic function a trampoline stands for, which has an
    /// instance entry ([`generic_trampoline`], D869), by its call's path,
    /// with the entry's path.
    generic: std::cell::RefCell<BTreeMap<String, String>>,
    /// The instance entries of the methods of the impl being written,
    /// which go beside it.
    entries: std::cell::RefCell<Vec<String>>,
    /// The type parameters of the function being written and its impl,
    /// which shadow a marked type of their name, as `Message` of `fn
    /// tab<Message>` beside the module's `enum Message`.
    parameters: std::cell::RefCell<BTreeSet<String>>,
    /// What each name a `use` brings into a module stands for, by paths
    /// from the crate root, the native root's among them: inboard's root
    /// `pub use theme::Theme;` makes `crate::Theme` the marked
    /// `theme::Theme`.
    uses: BTreeMap<String, Vec<String>>,
    /// The marked functions native code names, as the last check recorded
    /// them (`package::called`, #1807): every other trampoline is its
    /// signature over a body that never runs. `None` gives each its body.
    called: Option<&'a BTreeSet<String>>,
}

/// A function's trampoline as its shape has it, and as native code that
/// doesn't call the function is built with: its signature over a body
/// that never runs, unless it is generic, whose instance entries the
/// support table may name, or says why it can't cross (#1807).
struct Stub {
    text: String,
    uncalled: Option<String>,
}

impl Stub {
    /// The text native code is built with, `called` or not.
    fn built(&self, called: bool) -> &str {
        match &self.uncalled {
            Some(uncalled) if !called => uncalled,
            _ => &self.text,
        }
    }
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
        let inline = if body.is_empty() {
            ""
        } else {
            "    #[inline]\n"
        };
        format!("impl {path} for {stand_in} {{\n{inline}{body}}}\n")
    }
}

impl<'a> Generator<'a> {
    fn new(sources: &'a [Source<'a>], root: &'a [item::Parsed]) -> Self {
        let root = Source {
            modules: &[],
            path: Path::new(""),
            text: "",
            items: root,
        };
        let uses = uses(sources.iter().chain([&root]));
        let mut types = BTreeMap::new();
        let mut named = BTreeSet::new();
        let mut traits = BTreeSet::new();
        let mut shown = BTreeSet::new();
        let mut trait_paths = BTreeMap::<&str, Vec<String>>::new();
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
                        trait_paths
                            .entry(declared.name.text.as_str())
                            .or_default()
                            .push(path_of(source.modules, &declared.name.text));
                        if declared.header.visibility.is_some() {
                            shown.insert(declared.name.text.as_str());
                        }
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
            shown,
            shared: trait_paths
                .into_values()
                .filter(|paths| paths.len() > 1)
                .flatten()
                .collect(),
            implemented,
            holding: BTreeSet::new(),
            generic: std::cell::RefCell::default(),
            entries: std::cell::RefCell::default(),
            parameters: std::cell::RefCell::default(),
            uses,
            called: None,
        };
        // A generic one crosses too: each part of a type its parameters
        // name is a native one, which only an instance entry moves (D869).
        generator.crossing = generator.types.keys().cloned().collect();
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
        // A generic marked type's arguments are its parameters' types.
        let typed = last
            .arguments
            .iter()
            .any(|argument| matches!(argument, type_ref::Argument::Type(_)));
        match (last.name.text.as_str(), owner) {
            ("Self", Owner::Marked(owner)) if path.segments.len() == 1 && !typed => {
                self.holding.contains(owner)
            }
            _ => self.marked(file, path).is_some_and(|marked| {
                self.holding.contains(marked) && (!typed || self.types[marked].generic)
            }),
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
        if let [segment] = &path.segments[..]
            && self
                .parameters
                .borrow()
                .contains(segment.name.text.as_str())
        {
            return None;
        }
        let names = path
            .segments
            .iter()
            .map(|segment| segment.name.text.as_str())
            .collect::<Vec<_>>();
        let found = |absolute: Vec<String>| {
            let key = |path: &[String]| {
                self.types
                    .get_key_value(path.join("::").as_str())
                    .map(|(path, _)| path.as_str())
            };
            key(&absolute).or_else(|| key(&self.used(absolute)))
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

    /// The path from the crate root `path` stands for once each `use` on
    /// the way is followed, as `Theme` of the native root's `pub use
    /// theme::Theme;` stands for `theme::Theme`.
    fn used(&self, mut path: Vec<String>) -> Vec<String> {
        // As `absolute` does, a few `use`s deep, never round a cycle.
        for _ in 0..8 {
            let Some((at, target)) = (1..=path.len()).rev().find_map(|at| {
                self.uses
                    .get(path[..at].join("::").as_str())
                    .map(|target| (at, target))
            }) else {
                break;
            };
            let mut used = target.clone();
            used.extend(path.drain(at..));
            path = used;
        }
        path
    }

    /// The trait `trait_` an impl in `file` names, as the interpreter keys
    /// its functions (the free `instance`), by its path when another module's
    /// marked trait has its name (`Generator::shared`).
    fn instance(&self, file: &Source<'_>, trait_: &type_ref::Path) -> String {
        let names = trait_
            .segments
            .iter()
            .map(|segment| segment.name.text.as_str())
            .collect::<Vec<_>>();
        let known = absolute(file, &names, 0)
            .map(|path| path.join("::"))
            .filter(|path| !trait_.global && self.shared.contains(path));
        let written = instance(trait_);
        match (known, trait_.segments.last()) {
            (Some(known), Some(last)) => {
                format!("{known}{}", &written[last.name.text.len()..])
            }
            _ => written,
        }
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
            // A boxed trait object is one native part: the box. One of
            // std's `Iterator`, as inboard's `Box<dyn Iterator<Item =
            // Element<'a, Message>> + 'a>`, may hold the program's own
            // iterator, which native code pulls from (#1600).
            type_ref::Type::Path(path)
                if let Some((Container::Box, inner)) = container(path)
                    && slice(file.text, inner.span())
                        .trim_start()
                        .starts_with("dyn ") =>
            {
                let written = slice(file.text, ty.span());
                Ok(match pulled_item(file.text, inner) {
                    Some(item) => Held::Pulled { written, item },
                    None => Held::Native(written),
                })
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
            // A `&'a str` field, as `title` of inboard's `Header<'a,
            // Message>`: native code's own text (#1557).
            type_ref::Type::Reference {
                mutable: false,
                target,
                ..
            } if slice(file.text, target.span()).trim() == "str" => Ok(Held::Text),
            _ => {
                let Err(reason) = crossed else {
                    return Ok(Held::Wire);
                };
                if self.lendable(file, ty, owner) {
                    Ok(Held::Native(slice(file.text, ty.span())))
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
                // A marked type's lifetimes are its fields' own (#1557).
                let marked = self.marked(file, path).is_some();
                let arguments = last
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        type_ref::Argument::Type(ty) => Some(Ok(ty)),
                        type_ref::Argument::Lifetime(_) if marked => None,
                        type_ref::Argument::Lifetime(_) => {
                            Some(Err(format!("`{name}` with a lifetime can't cross yet")))
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
                    // A keyed collection crosses as the interpreter keeps
                    // one, its entries sorted by key, as inboard's
                    // `read_ids: HashSet<Id>`.
                    ("HashSet" | "BTreeSet", [inner]) if single => self.crosses(file, inner, owner),
                    ("HashMap" | "BTreeMap", [key, value]) if single => {
                        self.crosses(file, key, owner)?;
                        self.crosses(file, value, owner)
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
                        Some(marked)
                            if (arguments.is_empty() || self.types[marked].generic)
                                && self.crossing.contains(marked) =>
                        {
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
                // Native parts inside an `Option`, a `Vec`, a `Box` or a
                // tuple move part by part (#1224).
                Err(reason) => {
                    let held = self.given(file, ty, owner).ok_or(reason)?;
                    let text = text.trim();
                    Ok(Pass::GiveHeld {
                        give: held.code("__value", Use::Give, 0),
                        ty: (static_lifetimes(text) != text).then(|| text.to_owned()),
                    })
                }
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
            return Ok(Pass::Text);
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
            // So is a slice of one, where it is (#1292).
            Err(reason) => match slice_element(target) {
                Some(element) if !*mutable && self.lendable(file, element, owner) => {
                    Ok(Pass::LendSlice {
                        element: unnamed_lifetimes(slice(file.text, element.span())),
                    })
                }
                _ => Err(reason),
            },
        }
    }

    /// How `parameter` of `function` crosses when its type is an
    /// argument-position `impl Trait`: `None` for any other type. Rustc
    /// lets the body use the value only as the trait allows, and keep
    /// nothing of it past the call unless the trait says so, so an owned
    /// value of a type that implements the trait, made in the trampoline,
    /// stands in for it: an `impl IntoIterator<Item = T>`'s items drained
    /// into a `Vec<T>`, an `impl Into<T>`'s `T`.
    fn owned(
        &self,
        file: &Source<'_>,
        function: &item::Function,
        parameter: &item::Parameter,
        owner: Owner<'_>,
    ) -> Option<Result<(Option<Stand>, Pass), String>> {
        let written = slice(file.text, parameter.ty.span()).trim();
        if !written.starts_with("impl") {
            return None;
        }
        let type_ref::Type::Path(path) = &parameter.ty else {
            return None;
        };
        let [segment] = &path.segments[..] else {
            return None;
        };
        let name = segment.name.text.as_str();
        let stand = if name == type_ref::INTO_ITER_TYPE {
            let [type_ref::Argument::Type(item)] = &segment.arguments[..] else {
                return None;
            };
            Ok(collected(file, item, path.span))
        } else if name == type_ref::ITER_TYPE || type_ref::is_fn_type(name) {
            Err(format!("`{written}` can't cross yet"))
        } else {
            let generic = function
                .generics
                .iter()
                .find(|generic| generic.name.text == segment.name.text)?;
            // The argument's own impl of a marked trait says how it crosses.
            if let Some(bound) = self.shown_bound(generic) {
                return Some(Ok((
                    None,
                    Pass::Trait {
                        path: trait_path(bound),
                        given: given_path(file, bound),
                    },
                )));
            }
            stand_for(file, generic, written)
        };
        Some(stand.and_then(|stand| {
            let pass = self.pass(file, &stand.ty, &stand.written, owner)?;
            Ok((Some(stand), pass))
        }))
    }

    /// The one bound of `generic`, besides auto traits, when it is a
    /// marked trait native code sees.
    fn shown_bound<'g>(&self, generic: &'g item::GenericParam) -> Option<&'g type_ref::Path> {
        let [bound] = bounds(generic)[..] else {
            return None;
        };
        let last = bound.segments.last()?;
        self.shown
            .contains(last.name.text.as_str())
            .then_some(bound)
    }

    /// How a by-value parameter of `ty` moves the native parts it holds
    /// inside `Option`s, `Vec`s, `Box`es and tuples (#1224), as [`Held`]
    /// with `Use::Give`: each part by its type as written, lifetimes and
    /// all, as the trampoline gives the whole value at `'static` first.
    /// `None` for a type that holds none so, or one that can't cross.
    fn given<'f>(
        &self,
        file: &Source<'f>,
        ty: &type_ref::Type,
        owner: Owner<'_>,
    ) -> Option<Held<'f>> {
        let part = |ty: &type_ref::Type| {
            if self.holds(file, ty, owner) {
                Some(Held::Marked)
            } else if self.crosses(file, ty, owner).is_ok() {
                Some(Held::Wire)
            } else if self.lendable(file, ty, owner) {
                Some(Held::Native(slice(file.text, ty.span())))
            } else {
                self.given(file, ty, owner)
            }
        };
        match ty {
            type_ref::Type::Tuple { members, .. } => Some(Held::Tuple(
                members.iter().map(part).collect::<Option<_>>()?,
            )),
            type_ref::Type::Path(path) => {
                let (name, inner) = container(path)?;
                let inner = Box::new(part(inner)?);
                Some(match name {
                    Container::Option => Held::Option(inner),
                    Container::Vec => Held::Vec(inner),
                    Container::Box => Held::Boxed(inner),
                })
            }
            _ => None,
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
            // A generic impl's native part comes back by its move alone, as
            // only an instance entry, whose types are `'static`, reads it
            // (D869).
            _ if name != "Result" && !self.lendable(file, ty, owner) => Err(reason),
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
        // So is a text at the lifetime of the impl's own type, as `title(
        // &self) -> &'a str` of a `Panel<'a>`: native code's own text, which
        // the receiver holds (#1557).
        let text = matches!(target.as_ref(), type_ref::Type::Path(path)
            if path.segments.len() == 1 && path.segments[0].name.text == "str");
        if let Some(lifetime) = lifetime
            && lifetime.name.text != "_"
            && !slice(file.text, *receiver).contains(&format!("'{}", lifetime.name.text))
            && (!text
                || function
                    .lifetimes
                    .params
                    .iter()
                    .any(|declared| declared.text == lifetime.name.text))
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
    /// elides it, `&self` or the one input that borrows. A marked value
    /// native code holds, lent by its parts or copied as `receiver` and
    /// `passes` say, is the anchor, and else a native value lent `&`.
    /// `None` when there is none.
    fn anchor(
        &self,
        file: &Source<'_>,
        function: &item::Function,
        receiver: Option<&Pass>,
        passes: &[Pass],
    ) -> Option<Anchor> {
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
            return Some(Anchor::Receiver);
        }
        // With no marked input to anchor on, as `view<'a>(threads: &'a
        // Store) -> Element<'a, Message>`, the native value lent at the
        // result's lifetime is the anchor: rustc ends every borrow of the
        // result before native code moves, drops or takes that value
        // `&mut` (#1491).
        let lent = |index: &usize| {
            matches!(
                passes[*index],
                Pass::Lend { mutable: false, .. } | Pass::LendSlice { .. }
            )
        };
        tied.iter()
            .find(|index| held(**index, &function.parameters[**index].ty))
            .or_else(|| tied.iter().find(|index| lent(index)))
            .map(|index| Anchor::Argument(*index))
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
                    if self.tracks_drop(file, declared) {
                        let text = format!(
                            "impl ::std::ops::Drop for {} {{\n    #[inline]\n    fn drop(&mut self) {{\n        \
                             crate::{MODULE}::ended(self);\n    }}\n}}\n",
                            declared.name.text
                        );
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
                    match self.stub(file, declared, None, Owner::None, false, &path) {
                        Ok(stub) => {
                            items.push_str(stub.built(self.calls(&path)));
                            shape.insert(
                                path,
                                Entry {
                                    kind: Kind::Function,
                                    text: stub.text,
                                },
                            );
                        }
                        Err(reason) => refused.push(reason),
                    }
                }
                item::Parsed::Trait(declared) if declared.header.visibility.is_some() => {
                    let text = self.declaration(file, declared);
                    items.push_str(&text);
                    shape.insert(
                        path_of(file.modules, &declared.name.text),
                        Entry {
                            kind: Kind::Other,
                            text,
                        },
                    );
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

    /// Whether native code may call the marked function at `path`: it
    /// names it, or no check recorded what native code names.
    fn calls(&self, path: &str) -> bool {
        self.called.is_none_or(|called| called.contains(path))
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

    /// The marked trait `declared` as native code sees it: as declared,
    /// each provided function's body a panic, as native code can't call
    /// one yet, and [`GIVE`] beside its own functions, so a marked
    /// function can take an `impl` of it.
    fn declaration(&self, file: &Source<'_>, declared: &item::Trait) -> String {
        let name = &declared.name.text;
        let path = path_of(file.modules, name);
        let mut text = String::new();
        let mut cursor = declared.span.start();
        for (function, provided) in declared.functions.iter().zip(&declared.provided) {
            if !provided {
                continue;
            }
            text.push_str(slice(
                file.text,
                Span::new(cursor, function.body_span.start()),
            ));
            let _ = write!(
                text,
                "{{\n        ::core::panic!({:?})\n    }}",
                format!(
                    "rawr: native code can't call `{path}::{}` yet: a trait's provided \
                     function can't cross yet",
                    function.name.text
                )
            );
            cursor = function.body_span.end();
        }
        text.push_str(slice(file.text, Span::new(cursor, declared.span.end())));
        let Some(end) = text.rfind('}') else {
            return format!("{}\n", without_rawr_attributes(&text));
        };
        let island = format!("crate::{MODULE}");
        let message = format!(
            "rawr: native code can't give a value to a marked function as an `impl {name}` \
             when native code implements `{name}`"
        );
        // A value moves to the interpreter only where each type it is
        // generic over is `'static`, as in a generic function's instance
        // entry (D869).
        let statics = declared
            .generics
            .iter()
            .map(|generic| format!(",\n        {}: 'static", generic.name.text))
            .collect::<String>();
        text.insert_str(
            end,
            &format!(
                "    #[doc(hidden)]\n    \
                 fn {GIVE}(self, __call: &mut {island}::Call<'_>)\n    \
                 where\n        Self: ::std::marker::Sized{statics},\n    \
                 {{\n        ::core::panic!({message:?})\n    }}\n    \
                 #[doc(hidden)]\n    \
                 fn {GIVEN_NAME}(&self) -> &'static str\n    \
                 where\n        Self: ::std::marker::Sized,\n    \
                 {{\n        \"\"\n    }}\n"
            ),
        );
        let mut text = format!("{}\n", without_rawr_attributes(&text));
        text.push_str(&given_boxed(declared));
        text
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
        let marked_trait = trait_
            .and_then(|path| path.segments.last())
            .map(|last| last.name.text.as_str())
            .filter(|name| self.traits.contains(name));
        if marked_trait.is_some_and(|name| !self.shown.contains(name)) {
            return;
        }
        let generic = match &written.segments[..] {
            [segment] if segment.arguments.is_empty() => declared
                .generics
                .iter()
                .find(|generic| generic.name.text == segment.name.text),
            _ => None,
        };
        let (owner, path) = match self.marked(file, written) {
            Some(path) => (Owner::Marked(path), path.to_owned()),
            None if generic.is_some() && marked_trait.is_some() => {
                (Owner::Generic, native_owner(written))
            }
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
        let instance = trait_.map(|trait_| self.instance(file, trait_));
        let prefix = match &instance {
            Some(instance) => format!("{path}::{instance}"),
            None => path.clone(),
        };
        if functions.is_empty() {
            shape.insert(
                prefix.clone(),
                Entry {
                    kind: Kind::Impl,
                    text: format!("{header} {{}}"),
                },
            );
        }
        let formats = trait_.is_some_and(formatting);
        // Drop glue calls a `Drop` impl's `drop` unnamed.
        let kind = match trait_.and_then(|path| path.segments.last()) {
            Some(last) if last.name.text == "Drop" => Kind::Implicit,
            Some(_) => Kind::Impl,
            None => Kind::Function,
        };
        let mut beside = Vec::new();
        for function in functions {
            let member = format!("{prefix}::{}", function.name.text);
            let stub = match owner {
                Owner::Marked(_) if formats && function.name.text == "fmt" => Ok(Stub {
                    text: rendered_fmt(&member),
                    uncalled: None,
                }),
                _ => self.stub(file, function, Some(declared), owner, false, &member),
            };
            match stub {
                Ok(stub) => {
                    // A trait's function is called through the trait, unnamed.
                    let called = trait_.is_some() || self.calls(&member);
                    for line in stub.built(called).lines() {
                        let _ = writeln!(text, "    {line}");
                    }
                    // A generic method's instance entry goes beside the impl.
                    let entries = self.entries.borrow().concat();
                    shape.insert(
                        member,
                        Entry {
                            kind,
                            text: format!("{header}\n{}{entries}", stub.text),
                        },
                    );
                }
                Err(reason) => refused.push(reason),
            }
            beside.extend(self.entries.borrow_mut().drain(..));
        }
        if marked_trait.is_some()
            && let Some(given) = self.given_self(file, declared, generic, owner)
        {
            for line in given.lines() {
                let _ = writeln!(text, "    {line}");
            }
            shape.insert(
                format!("{prefix}::{GIVE}"),
                Entry {
                    kind: Kind::Impl,
                    text: format!("{header}\n{given}"),
                },
            );
        }
        text.push_str("}\n");
        text.extend(beside);
        items.push_str(&text);
    }

    /// The [`GIVE`] of `declared`, an impl of a marked trait, for its own
    /// type, which is `generic` when it is one of the impl's type
    /// parameters: the value given as itself, or as the owned value that
    /// stands in for it by its bound, made first, as an `impl IntoIterator<Item =
    /// T>` drains into a `Vec<T>`. `None` when no value of the type can
    /// cross, so the trait's own panics.
    fn given_self(
        &self,
        file: &Source<'_>,
        declared: &item::Impl,
        generic: Option<&item::GenericParam>,
        owner: Owner<'_>,
    ) -> Option<String> {
        // The trait's own type arguments, as `Message` of `impl<'a,
        // Message> IntoContent<'a, Message> for Content<'a, Message>`: each
        // `'static` where the value moves, as the trait's own `GIVE` says.
        let arguments = declared
            .trait_
            .as_ref()
            .and_then(|trait_| trait_.segments.last())
            .map(|last| {
                last.arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        type_ref::Argument::Type(ty) => Some(ty),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let argument = |name: &str| {
            arguments.iter().any(|ty| {
                matches!(ty, type_ref::Type::Path(path)
                    if matches!(&path.segments[..], [segment]
                        if segment.name.text == name && segment.arguments.is_empty()))
            })
        };
        // A value whose type names another of the impl's type parameters
        // crosses no more than a generic function's.
        let others = declared.generics.iter().any(|other| {
            generic.is_none_or(|generic| other.name.text != generic.name.text)
                && !argument(&other.name.text)
        });
        if others {
            return None;
        }
        let statics = arguments
            .iter()
            .map(|ty| format!("{}: 'static", slice(file.text, ty.span()).trim()))
            .collect::<Vec<_>>();
        let clause = if statics.is_empty() {
            " ".to_owned()
        } else {
            format!("\nwhere\n    {},\n", statics.join(",\n    "))
        };
        let owner_written = slice(file.text, declared.owner.span()).trim();
        let (stand, pass) = match generic {
            Some(generic) => {
                let stand = stand_for(file, generic, owner_written).ok()?;
                let pass = self
                    .pass(file, &stand.ty, &stand.written, Owner::None)
                    .ok()?;
                (Some(stand), pass)
            }
            None => (
                None,
                self.pass(file, &declared.owner, owner_written, owner)
                    .ok()?,
            ),
        };
        let island = format!("crate::{MODULE}");
        let mut text =
            format!("#[inline]\nfn {GIVE}(self, __call: &mut {island}::Call<'_>){clause}{{\n");
        match stand {
            Some(Stand { written, make, .. }) => {
                let _ = writeln!(
                    text,
                    "    let __value: {written} = {{ let __value = self; {make} }};"
                );
            }
            None => text.push_str("    let __value = self;\n"),
        }
        // A marked value moved in ends every borrow of it (#1492).
        if matches!(pass, Pass::Value) && matches!(owner, Owner::Marked(_)) {
            text.push_str("    __call.release(&__value);\n");
        }
        give(&pass, "__value", "*__call", &island, &mut text);
        text.push_str("}\n");
        // The impl's name, by its type's as written, or `_` for an impl for
        // every type of a bound, as the resolver names its instance
        // (`resolve::lower_nominal::imported::given_types`).
        let named = match (&declared.owner, generic) {
            (_, Some(_)) => "_".into(),
            (type_ref::Type::Path(path), None) => path.segments.last()?.name.text.clone(),
            _ => return None,
        };
        let _ = writeln!(
            text,
            "#[inline]\nfn {GIVEN_NAME}(&self) -> &'static str {{\n    \"+{named}\"\n}}"
        );
        Some(text)
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
            match self.stub(file, function, None, Owner::None, true, &member) {
                Ok(stub) => {
                    for line in stub.text.lines() {
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
                kind: Kind::Implicit,
                text: text.clone(),
            },
        );
        place.items.push_str(&text);
        place
            .objects
            .push((path, format!("crate::{}", path_of(file.modules, &maker))));
    }

    /// One function's trampoline. `outer` is the impl of an associated
    /// function, and `owner` its type; `path` is what the interpreter finds
    /// it by. A stand-in's function (`object`) has no `owner`: its `Self` is
    /// the stand-in, which doesn't cross, and its receiver is the program
    /// value's handle.
    fn stub(
        &self,
        file: &Source<'_>,
        function: &item::Function,
        outer: Option<&item::Impl>,
        owner: Owner<'_>,
        object: bool,
        path: &str,
    ) -> Result<Stub, String> {
        // The function's and its impl's type parameters are in scope
        // while it is written ([`Generator::parameters`]).
        let parameters = outer
            .into_iter()
            .flat_map(|outer| outer.generics.iter())
            .chain(function.generics.iter())
            .map(|generic| generic.name.text.to_string())
            .collect();
        let outside = self.parameters.replace(parameters);
        let stub = self.stub_in_scope(file, function, outer, owner, object, path);
        self.parameters.replace(outside);
        stub
    }

    /// [`Generator::stub`], its type parameters in scope.
    fn stub_in_scope(
        &self,
        file: &Source<'_>,
        function: &item::Function,
        outer: Option<&item::Impl>,
        owner: Owner<'_>,
        object: bool,
        path: &str,
    ) -> Result<Stub, String> {
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
        let mut stands = Vec::new();
        let mut reasons = Vec::new();
        // An argument-position `impl Trait` is a generic the parser lifted,
        // which the trampoline keeps as written.
        let lifted = |generic: &item::GenericParam| {
            function.parameters.iter().any(|parameter| {
                matches!(&parameter.ty, type_ref::Type::Path(path)
                    if matches!(&path.segments[..], [segment]
                        if segment.name.text == generic.name.text)
                    && slice(file.text, path.span).trim_start().starts_with("impl"))
            })
        };
        // A function generic over named types, its impl's first, has one
        // trampoline for them all, which hands its call to the instance
        // native code's types select (D869). An `impl Trait` beside them
        // has no name an instance could be keyed by: the trampoline makes
        // its owned stand-in first, which the instance takes.
        let named = outer
            .into_iter()
            .flat_map(|outer| outer.generics.iter())
            .chain(function.generics.iter().filter(|generic| !lifted(generic)))
            .map(|generic| generic.name.text.as_str())
            .collect::<Vec<_>>();
        let generic = !named.is_empty();
        if generic && object {
            reasons.push("a generic method can't cross yet".to_owned());
        }
        let tuple = format!(
            "({})",
            named
                .iter()
                .map(|name| format!("{name},"))
                .collect::<String>()
        );
        if matches!(owner, Owner::Generic) {
            reasons
                .push("a function of an impl for every type of a bound can't cross yet".to_owned());
        }
        for (index, parameter) in function.parameters.iter().enumerate() {
            signature.push_str(slice(file.text, Span::new(cursor, parameter.span.start())));
            let written = slice(file.text, parameter.ty.span());
            let _ = write!(signature, "__a{index}: {written}");
            cursor = parameter.span.end();
            let pass = match self.owned(file, function, parameter, owner) {
                Some(owned) => owned,
                None => self
                    .pass(file, &parameter.ty, written, owner)
                    .map(|pass| (None, pass)),
            };
            match pass {
                Ok((stand, pass)) => {
                    stands.push(stand);
                    passes.push(pass);
                }
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
        // So does one of a marked type copied whole, as inboard's
        // `Id::as_str` gives `&self.0`: its text is in native code's value.
        let crossing = matches!(owner, Owner::Marked(owner) if self.crossing.contains(owner));
        let lent = crossing && matches!(function.receiver, Some(item::Receiver::Shared(_)));
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
        // A marked trait's impl gives its value by the impl's own type,
        // which no instance is keyed by and which only an instance entry,
        // where each type is `'static`, can give: the trampoline boxes it as
        // the trait's `GIVEN`, whose type the entry names.
        if generic {
            for (pass, stand) in passes.iter_mut().zip(&mut stands) {
                let Pass::Trait { given, .. } = pass else {
                    continue;
                };
                let path = given
                    .split_once('<')
                    .map_or(given.as_str(), |(path, _)| path)
                    .to_owned();
                // Only its written type is read: no pass is found for it.
                *stand = Some(Stand {
                    ty: type_ref::Type::Unit(function.span),
                    written: format!("::std::boxed::Box<dyn {given} + '_>"),
                    make: "::std::boxed::Box::new(__value)".to_owned(),
                });
                *pass = Pass::Boxed { path };
            }
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
            return Ok(Stub {
                text: inlined(function, text),
                uncalled: None,
            });
        }
        let uncalled = inlined(
            function,
            format!("{signature} {{\n    {island}::uncalled({path:?})\n}}\n"),
        );
        // Each `impl Trait`'s owned stand-in, made before the call starts,
        // as making it may run native code that calls marked code too; a
        // generic function's trampoline makes it before it hands the call
        // to its instance.
        let mut prelude = String::new();
        for (index, stand) in stands.iter().enumerate() {
            if let Some(Stand { written, make, .. }) = stand {
                let _ = writeln!(
                    if generic { &mut prelude } else { &mut text },
                    "    let __a{index}: {written} = {{ let __value = __a{index}; {make} }};"
                );
            }
        }
        // A `&mut self` call ends every borrow of the receiver, as rustc
        // allows it only once none is left: what the stand-in kept for
        // them goes (#1305).
        if object && matches!(function.receiver, Some(item::Receiver::Mutable(_))) {
            text.push_str("    self.0.release();\n");
        }
        let owned = !matches!(owner, Owner::None) || object;
        if generic {
            // The instance's own function, keyed by its types' names and the
            // impl each boxed argument names itself by.
            let given = passes
                .iter()
                .enumerate()
                .filter_map(|(index, pass)| match pass {
                    Pass::Boxed { path } => Some(format!("{path}::{GIVEN_NAME}(&*__a{index})")),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let _ = if given.is_empty() {
                writeln!(
                    text,
                    "    let mut __call = {island}::call({island}::keyed({path:?}, \
                     ::std::any::type_name::<{tuple}>()), {owned});"
                )
            } else {
                writeln!(
                    text,
                    "    let mut __call = {island}::call({island}::keyed_given({path:?}, \
                     ::std::any::type_name::<{tuple}>(), &[{}]), {owned});",
                    given.join(", ")
                )
            };
        } else {
            let _ = writeln!(
                text,
                "    let mut __call = {island}::call({path:?}, {owned});"
            );
        }
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
            Pass::Value
            | Pass::GiveParts
            | Pass::Give
            | Pass::GiveForCall { .. }
            | Pass::GiveHeld { .. } => give(&pass, name, "__call", &island, text),
            Pass::Trait { path, .. } => {
                let _ = writeln!(text, "    {path}::{GIVE}({name}, &mut __call);");
            }
            Pass::Boxed { path } => {
                let _ = writeln!(text, "    {path}::{GIVE_BOXED}({name}, &mut __call);");
            }
            Pass::Borrow => {
                let _ = writeln!(text, "    __call.argument({name});");
            }
            Pass::Text => {
                let _ = writeln!(text, "    __call.kept_text({name});");
            }
            Pass::Mutable => {
                let _ = writeln!(text, "    __call.mutable(&*{name});");
                changed.push((name.to_owned(), Changed::Copied));
            }
            Pass::Lend { mutable, referent } => {
                // A `&mut` ends every borrow of the value, so what native
                // code borrowed of it as an anchor goes (#1491).
                if mutable {
                    let _ = writeln!(text, "    __call.release(&*{name});");
                }
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
            Pass::LendSlice { element } => lend_slice(name, &element, &island, text),
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
        // A call anchored on nothing whose result borrows nothing, as
        // `label(inbox: &inbox::State, updates: &[Update]) -> String`, and
        // that is lent a native value, keeps each shared lend for the call
        // alone: the runner runs it in
        // a region of its own that ends with it, where a crate fn's output
        // that keeps a lend lives as long as rustc lets the program hold it
        // (#1491, #1503).
        let for_call = anchor.is_none()
            && borrows_nothing(file, function)
            && passes.iter().any(|pass| {
                matches!(
                    pass,
                    Pass::Lend { mutable: false, .. } | Pass::LendSlice { .. }
                )
            });
        // So are the native parts of a marked `&'a self` the result is
        // anchored on, as inboard's open `wizard` is (#1413).
        match receiver {
            Some(pass @ Pass::LendParts { mutable: false })
                if anchor == Some(Anchor::Receiver) || for_call =>
            {
                text.push_str("    let self_lends = __call.lends();\n");
                argument("self", pass, &mut text);
                text.push_str("    __call.keep_lends(self_lends);\n");
            }
            Some(pass) => {
                // A marked value moved in ends every borrow of it (#1492).
                if matches!(pass, Pass::Value) && matches!(owner, Owner::Marked(_)) {
                    text.push_str("    __call.release(&self);\n");
                }
                argument("self", pass, &mut text);
            }
            None => {}
        }
        // A native value lent `&'a`, as `inbox: &'a inbox::State` of
        // `view<'a>(&'a self, ..)`, is borrowed as long as the anchored
        // result, which may borrow it (#1413).
        let mut lends_kept = if anchor.is_some() {
            kept_lends(file, function, &passes)
        } else {
            Vec::new()
        };
        // So is an anchor the result's elided lifetime names, as
        // `view(threads: &Store) -> Element<'_, Message>` names `threads`,
        // native or marked: a marked `sidebar(theme: &Theme) -> Element<'_,
        // Message>` gives its lent theme on to native code through the
        // marked `inbox<'a>(theme: &'a Theme)`, whose native call reaches
        // it in the anchored region.
        if let Some(Anchor::Argument(index)) = anchor
            && matches!(
                passes[index],
                Pass::Lend { mutable: false, .. }
                    | Pass::LendSlice { .. }
                    | Pass::LendParts { mutable: false }
            )
            && !lends_kept.contains(&index)
        {
            lends_kept.push(index);
        }
        if for_call {
            lends_kept.extend(passes.iter().enumerate().filter_map(|(index, pass)| {
                matches!(
                    pass,
                    Pass::Lend { mutable: false, .. }
                        | Pass::LendSlice { .. }
                        | Pass::LendParts { mutable: false }
                )
                .then_some(index)
            }));
        }
        if let Some(anchor) = anchor {
            let _ = writeln!(text, "    __call.anchor({});", anchor.name());
        }
        for (index, pass) in passes.into_iter().enumerate() {
            let name = format!("__a{index}");
            if matches!(pass, Pass::Value)
                && let type_ref::Type::Path(path) = &function.parameters[index].ty
                && self.marked(file, path).is_some()
            {
                let _ = writeln!(text, "    __call.release(&{name});");
            }
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
            // The text the receiver holds, copied first or in its lent parts
            // (#1468).
            let _ = writeln!(
                text,
                "    // SAFETY: the text is in `self`, written first, which stays borrowed for\n    \
                 // as long as the result is, or pinned for as long as the runner lives.\n    \
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
        } else if let Some(read) = &back
            && static_lifetimes(output) != output
        {
            // A result whose type names a lifetime comes back at `'static`,
            // as the interpreter moves it, and is the signature's type: an
            // invariant one, as iced's `Text<'a>`, doesn't coerce.
            let lasting = static_lifetimes(output);
            let _ = writeln!(
                text,
                "    let __value: {output} = {{\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
                 let __value: {lasting} = __reply.read(|input| {read});\n        \
                 // SAFETY: the type is the signature's but for its lifetimes, and what the\n        \
                 // value borrows lives as long as they say: native code's own text and\n        \
                 // lends, or what the anchor keeps.\n        \
                 #[allow(unsafe_code)]\n        \
                 unsafe {{ {island}::at_lifetimes(__value) }}\n    }};"
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
        if generic {
            let stood = stands
                .iter()
                .map(|stand| stand.as_ref().map(|stand| stand.written.clone()))
                .collect::<Vec<_>>();
            let generic = Generic {
                path,
                signature: &signature,
                concrete: &text,
                prelude: &prelude,
                tuple: &tuple,
                named: &named,
                stood: &stood,
                outer,
            };
            let (public, entry, name) = generic_trampoline(file, function, &generic);
            let mut modules = file
                .modules
                .iter()
                .map(ImmutableString::as_str)
                .collect::<Vec<_>>();
            modules.push(&name);
            self.generic
                .borrow_mut()
                .insert(path.to_owned(), format!("crate::{}", modules.join("::")));
            // A method's entry is beside its impl, which can't hold it.
            let text = if outer.is_some() {
                self.entries.borrow_mut().push(entry);
                public
            } else {
                format!("{public}{entry}")
            };
            return Ok(Stub {
                text,
                uncalled: None,
            });
        }
        Ok(Stub {
            text: inlined(function, text),
            uncalled: (!object).then_some(uncalled),
        })
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
        let generic = self
            .types
            .get(path)
            .is_some_and(|mirrored| mirrored.generic);
        self.field_held(file, path, ty)
            .code_in(binding, purpose, 0, generic)
    }

    /// The `Parts` impl of the marked type `name` at `path`, which holds
    /// native parts (#1469), from each `Use`'s body: how a borrow of it
    /// lends its parts, a `&mut` one too, a value moves them, and a `&mut`
    /// one takes back what the call made of it.
    fn parts(crossed: &Crossed, bodies: [String; 4]) -> String {
        let island = format!("crate::{MODULE}");
        let [lend, lend_mut, give, take_into] = bodies;
        let Crossed {
            generics,
            ty: name,
            lasting,
        } = crossed;
        // A generic impl names no item of its own: its lend is keyed by
        // its type (`Accessors`).
        let whole = if crossed.generic() {
            format!("{island}::lend_whole_of!(call, self, {lasting});")
        } else {
            format!("{island}::lend_whole!(call, self, {name});")
        };
        format!(
            "impl{generics} {island}::Parts for {name} {{\n    \
             #[inline]\n    \
             fn lend<'c>(&'c self, call: &mut {island}::Call<'c>) {{\n        \
             {whole}\n{lend}    }}\n    \
             #[inline]\n    \
             fn lend_mut<'c>(&'c mut self, call: &mut {island}::Call<'c>) {{\n{lend_mut}    }}\n    \
             #[inline]\n    \
             fn give(self, out: &mut {island}::Out) {{\n{give}    }}\n    \
             #[inline]\n    \
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
        if self.tracks_drop(file, declared) {
            // Its `Drop` forbids moving its fields out by a pattern: each
            // is read out once, and the value, moved for good, never drops.
            let _ = writeln!(
                bodies[2],
                "        crate::{MODULE}::ended(&self);\n        \
                 let __value = ::std::mem::ManuallyDrop::new(self);"
            );
            for (field, binding) in declared.fields.iter().zip(&bindings) {
                let _ = writeln!(
                    bodies[2],
                    "        // SAFETY: each field is read once, from a value that never drops.\n        \
                     #[allow(unsafe_code)]\n        \
                     let {binding} = unsafe {{ ::std::ptr::read(&__value.{}) }};",
                    field.name.text
                );
            }
        } else {
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
        }
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
        Self::parts(
            &Crossed::new(
                file,
                &declared.name.text,
                &declared.lifetimes,
                &declared.generics,
            ),
            bodies,
        )
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
        Self::parts(
            &Crossed::new(
                file,
                &declared.name.text,
                &declared.lifetimes,
                &declared.generics,
            ),
            bodies,
        )
    }

    /// Whether the shadow tells the runner when a value of the marked
    /// struct `declared` drops (#1492), so what results anchored on it
    /// borrowed goes with it: an `impl Drop` native code can't tell from
    /// the declared struct. Only for a crossing struct, which a call can
    /// anchor on, with no field native code sees, no parameters, no `Copy`
    /// and no `Drop` of the program's own.
    fn tracks_drop(&self, file: &Source<'_>, declared: &item::Struct) -> bool {
        let name = declared.name.text.as_str();
        if !self.crossing.contains(&path_of(file.modules, name))
            || declared.fields.is_empty()
            || !declared.generics.is_empty()
            || !declared.lifetimes.params.is_empty()
        {
            return false;
        }
        let copied = declared.header.attributes.iter().any(|attribute| {
            matches!(attribute.path.segments.as_ref(), [only] if only.name.text == "derive")
                && attribute.arguments.iter().any(|derived| {
                    derived
                        .segments
                        .last()
                        .is_some_and(|last| last.name.text == "Copy")
                })
        });
        // A field's span starts at its name, after any `pub` or `pub(..)`.
        let visible = declared.fields.iter().any(|field| {
            let before = slice(
                file.text,
                Span::new(declared.span.start(), field.span.start()),
            )
            .trim_end();
            before.ends_with("pub") || before.ends_with(')')
        });
        let dropped = file.items.iter().any(|item| {
            matches!(item, item::Parsed::Impl(declared)
                if declared.trait_.as_ref().and_then(|path| path.segments.last())
                    .is_some_and(|last| last.name.text == "Drop")
                    && matches!(&declared.owner, type_ref::Type::Path(owner)
                        if owner.segments.last().is_some_and(|last| last.name.text == name)))
        });
        !copied && !visible && !dropped
    }

    fn cross_struct(&self, file: &Source<'_>, declared: &item::Struct) -> String {
        let name = declared.name.text.as_str();
        let crossed = Crossed::new(file, name, &declared.lifetimes, &declared.generics);
        let Crossed { generics, ty, .. } = &crossed;
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
            "impl{generics} {island}::Put for {ty} {{\n    #[inline]\n    fn put(&self, out: &mut {island}::Out) {{\n        \
             out.structure({path:?}, {count});\n{put}    }}\n}}\n\
             impl{generics} {island}::Take for {ty} {{\n    #[inline]\n    fn take(input: &mut {island}::In<'_>) -> Self {{\n        \
             #[allow(unused_imports)]\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
             input.structure({path:?}, {count});\n        {take}\n    }}\n}}\n"
        );
        if self.holding.contains(&path) {
            text.push_str(&self.struct_parts(file, declared, &path));
        }
        text.push_str(&self.mirror_impl(&crossed, &path));
        text
    }

    fn cross_enum(&self, file: &Source<'_>, declared: &item::Enum) -> String {
        let name = declared.name.text.as_str();
        let crossed = Crossed::new(file, name, &declared.lifetimes, &declared.generics);
        let Crossed { generics, ty, .. } = &crossed;
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
            "impl{generics} {island}::Put for {ty} {{\n    #[inline]\n    fn put(&self, out: &mut {island}::Out) {{\n        \
             match self {{\n{put}        }}\n    }}\n}}\n\
             impl{generics} {island}::Take for {ty} {{\n    #[inline]\n    fn take(input: &mut {island}::In<'_>) -> Self {{\n        \
             #[allow(unused_imports)]\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
             match input.variant({path:?}) {{\n{take}            (variant, count) => input.unknown({path:?}, variant, count),\n        }}\n    }}\n}}\n"
        );
        if self.holding.contains(&path) {
            text.push_str(&self.enum_parts(file, declared, &path, &take));
        }
        text.push_str(&self.mirror_impl(&crossed, &path));
        text
    }

    /// The `Mirror` impl of the crossing type `crossed` at `path`, which
    /// the runner holds a program value as where a crate's generic takes
    /// one (#1249): a value moves its native parts, as Rust moves it.
    fn mirror_impl(&self, crossed: &Crossed, path: &str) -> String {
        let island = format!("crate::{MODULE}");
        let Crossed { generics, ty, .. } = crossed;
        let (give, lend) = if self.holding.contains(path) {
            (
                format!("{island}::Parts::give(self, out);"),
                "call.lent(self);",
            )
        } else {
            (
                format!("{island}::Put::put(&self, out);"),
                "call.argument(self);",
            )
        };
        format!(
            "impl{generics} {island}::Mirror for {ty} {{\n    #[inline]\n    fn give(self, out: &mut {island}::Out) {{\n        \
             {give}\n    }}\n    \
             #[inline]\n    \
             fn lend<'c>(&'c self, call: &mut {island}::Call<'c>) {{\n        {lend}\n    }}\n}}\n"
        )
    }
}

/// `trampoline`, the text of `function`'s, `#[inline]` unless `function`
/// says how it inlines. rustc then builds it only where it is called, as
/// cargo's build of a binary builds only what `main` reaches, and not as
/// a public item of rawr's copy of the crate, a library (#1801).
fn inlined(function: &item::Function, trampoline: String) -> String {
    let attributed = function.header.attributes.iter().any(|attribute| {
        matches!(attribute.path.segments.as_ref(), [only] if only.name.text == "inline")
    });
    if attributed {
        trampoline
    } else {
        format!("#[inline]\n{trampoline}")
    }
}

/// The marked type `name` as an impl names it, each of its `lifetimes`
/// `'_`, as `Panel<'_>` (#1557).
fn with_lifetimes(name: &str, lifetimes: &item::Lifetimes) -> String {
    if lifetimes.params.is_empty() {
        return name.to_owned();
    }
    let unnamed = vec!["'_"; lifetimes.params.len()];
    format!("{name}<{}>", unnamed.join(", "))
}

/// How the crossing impls of a marked type are written: the impl's
/// generics, the type as it names it, each lifetime `'_`, and the type each
/// lifetime `'static`, which a lend's accessors are keyed by. A generic
/// type's parameters are each `'static` and keep their declared bounds,
/// as only an instance entry moves its native parts (D869): `impl<Message:
/// 'static>`, `Sidebar<'_, Message>` and `Sidebar<'static, Message>`.
struct Crossed {
    generics: String,
    ty: String,
    lasting: String,
}

impl Crossed {
    fn new(
        file: &Source<'_>,
        name: &str,
        lifetimes: &item::Lifetimes,
        generics: &[item::GenericParam],
    ) -> Self {
        if generics.is_empty() {
            let ty = with_lifetimes(name, lifetimes);
            return Self {
                generics: String::new(),
                lasting: static_lifetimes(&ty),
                ty,
            };
        }
        let declared = generics
            .iter()
            .map(|generic| {
                let bounds = generic
                    .bounds
                    .iter()
                    .map(|bound| slice(file.text, bound.span).trim())
                    .collect::<Vec<_>>();
                if bounds.is_empty() {
                    format!("{}: 'static", generic.name.text)
                } else {
                    format!("{}: 'static + {}", generic.name.text, bounds.join(" + "))
                }
            })
            .collect::<Vec<_>>();
        let names = generics
            .iter()
            .map(|generic| generic.name.text.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let arguments = |lifetime: &str| {
            let mut arguments = vec![lifetime; lifetimes.params.len()].join(", ");
            if !arguments.is_empty() {
                arguments.push_str(", ");
            }
            format!("{name}<{arguments}{names}>")
        };
        Self {
            generics: format!("<{}>", declared.join(", ")),
            ty: arguments("'_"),
            lasting: arguments("'static"),
        }
    }

    /// Whether the type is generic over types, so its impls are too.
    fn generic(&self) -> bool {
        !self.generics.is_empty()
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
/// Whether `trait_` is one of std's formatting traits, whose `fmt` writes
/// a value into a `fmt::Formatter`: `Display`, `Debug`, `LowerHex` and
/// their kin, by the name an impl writes last.
fn formatting(trait_: &type_ref::Path) -> bool {
    let own = matches!(
        trait_.segments[0].name.text.as_str(),
        "crate" | "self" | "super"
    );
    trait_.segments.last().is_some_and(|last| {
        !own && last.arguments.is_empty()
            && matches!(
                last.name.text.as_str(),
                "Display"
                    | "Debug"
                    | "LowerHex"
                    | "UpperHex"
                    | "Octal"
                    | "Binary"
                    | "LowerExp"
                    | "UpperExp"
                    | "Pointer"
            )
    })
}

/// The `fmt` of a marked impl of a std formatting trait (`formatting`)
/// for a marked type native code holds a copy of, which the interpreter's
/// `member` keys: the text the marked `fmt` writes, which the interpreter
/// renders from the copy when native code formats the value, written into
/// native code's formatter.
fn rendered_fmt(member: &str) -> String {
    let island = format!("crate::{MODULE}");
    format!(
        "#[inline]\nfn fmt(&self, __f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n    \
         let mut __call = {island}::call({member:?}, true);\n    \
         __call.argument(self);\n    \
         __call.argument(&::std::string::String::new());\n    \
         let mut __reply = __call.send();\n    \
         let __text: ::std::string::String = __reply.take();\n    \
         __f.write_str(&__text)\n}}\n"
    )
}

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

/// The input a call's result borrows, which the runner keeps what the
/// result borrows with (`Generator::anchor`, #1473).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Anchor {
    Receiver,
    /// The parameter at this index.
    Argument(usize),
}

impl Anchor {
    /// Its name in the trampoline.
    fn name(self) -> String {
        match self {
            Self::Receiver => "self".to_owned(),
            Self::Argument(index) => format!("__a{index}"),
        }
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
    /// moved as itself. One whose type names a lifetime moves at
    /// `'static`, as the interpreter keeps it, and comes back at the
    /// marked type's own.
    Native(&'t str),
    /// A `&str`: native code's own text, copied with where it is, which
    /// comes back where the program's is its copy (`In::kept_text`).
    Text,
    /// An `Option` of what the inner one holds.
    Option(Box<Held<'t>>),
    /// A `Vec` of what the inner one holds.
    Vec(Box<Held<'t>>),
    /// A `Box` of what the inner one holds, which the interpreter holds as
    /// its value itself.
    Boxed(Box<Held<'t>>),
    /// A `Box<dyn Iterator<Item = item>>`, written as `written`: a native
    /// part, or the program's own iterator, which native code holds by
    /// its handle (the support's `Pulled`) and pulls from one item at a
    /// time (#1600).
    Pulled { written: &'t str, item: &'t str },
    /// A tuple of what each member holds.
    Tuple(Vec<Held<'t>>),
}

impl Held<'_> {
    /// One statement that does `purpose` with the field `binding` reaches,
    /// as [`Use`] says, its own bindings suffixed by `depth`.
    fn code(&self, binding: &str, purpose: Use, depth: usize) -> String {
        self.code_in(binding, purpose, depth, false)
    }

    /// [`Held::code`] in a `generic` impl, which declares no item of its
    /// own, so a part is lent by accessors keyed by its type, each lifetime
    /// `'static` (`lend_part_of!`).
    fn code_in(&self, binding: &str, purpose: Use, depth: usize, generic: bool) -> String {
        let island = format!("crate::{MODULE}");
        let out = match purpose {
            Use::Lend | Use::LendMut => "call.out()",
            Use::Give | Use::TakeInto | Use::Put => "out",
        };
        let inner = format!("__p{depth}");
        match self {
            Self::Pulled { written, item } => {
                let lasting = static_lifetimes(item);
                let object = pulled_object(&lasting);
                let native = Self::Native(written).code_in(binding, purpose, depth, generic);
                match purpose {
                    // The program's iterator is lent by its handle, so the
                    // call pulls from the one native code holds.
                    Use::Lend | Use::LendMut | Use::Put => format!(
                        "match {island}::pulled_handle(&**{binding}) {{ \
                         ::std::option::Option::Some(__h{depth}) => {out}.kept_lent(__h{depth}), \
                         ::std::option::Option::None => {{ {native} }} }}"
                    ),
                    Use::Give => format!(
                        "{{ // SAFETY: only its lifetimes change, which have no layout; the \
                         interpreter keeps it no longer than rustc lets the marked value.\n\
                         #[allow(unsafe_code)] let {inner} = unsafe {{ \
                         {island}::at_lifetimes::<_, {object}>({binding}) }}; \
                         match {island}::take_pulled({inner}) {{ \
                         ::std::result::Result::Ok(__h{depth}) => out.kept(__h{depth}), \
                         ::std::result::Result::Err({inner}) => {island}::give_part!(out, {inner}), \
                         }} }}"
                    ),
                    Use::TakeInto => format!(
                        "{{ // SAFETY: only its lifetimes change, which have no layout, and \
                         what the call gives the place borrows what the marked value does.\n\
                         #[allow(unsafe_code)] let {inner} = unsafe {{ \
                         &mut *::std::ptr::from_mut({binding}).cast::<{object}>() }}; \
                         input.pulled_into({inner}, |input| \
                         (&{island}::Back::<{lasting}>::new()).take(input)); }}"
                    ),
                }
            }
            Self::Native(written) if generic && matches!(purpose, Use::Lend | Use::LendMut) => {
                let lasting = static_lifetimes(written);
                let lend = if matches!(purpose, Use::Lend) {
                    "lend_part_of"
                } else {
                    "lend_part_mut_of"
                };
                format!("{island}::{lend}!(call, {binding}, {lasting});")
            }
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
            Self::Native(written) if static_lifetimes(written) != *written => {
                let unnamed = unnamed_lifetimes(written);
                let lasting = static_lifetimes(written);
                match purpose {
                    Use::Lend => format!("{island}::lend_part!(call, {binding}, {unnamed});"),
                    Use::LendMut => {
                        format!("{island}::lend_part_mut!(call, {binding}, {unnamed});")
                    }
                    Use::Give => format!(
                        "{{ // SAFETY: only its lifetimes change, which have no layout; the \
                         interpreter keeps it no longer than rustc lets the marked value.\n\
                         #[allow(unsafe_code)] let {inner} = unsafe {{ \
                         {island}::at_lifetimes::<_, {lasting}>({binding}) }}; \
                         {island}::give_part!(out, {inner}); }}"
                    ),
                    Use::TakeInto => format!(
                        "{{ // SAFETY: only its lifetimes change, which have no layout, and \
                         what the call gives the place borrows what the marked value does.\n\
                         #[allow(unsafe_code)] let {inner} = unsafe {{ \
                         &mut *::std::ptr::from_mut({binding}).cast::<{lasting}>() }}; \
                         (&{island}::Back::<{lasting}>::new()).take_into(input, {inner}); }}"
                    ),
                    Use::Put => format!("{island}::part!(out, {binding});"),
                }
            }
            Self::Native(written) => match purpose {
                Use::Lend => format!("{island}::lend_part!(call, {binding}, {written});"),
                Use::LendMut => format!("{island}::lend_part_mut!(call, {binding}, {written});"),
                Use::Give => format!("{island}::give_part!(out, {binding});"),
                Use::TakeInto => {
                    format!("(&{island}::Back::<{written}>::new()).take_into(input, {binding});")
                }
                Use::Put => format!("{island}::part!(out, {binding});"),
            },
            Self::Text => match purpose {
                Use::Lend | Use::LendMut => format!("call.out().kept_text({binding});"),
                Use::Give | Use::Put => format!("out.kept_text({binding});"),
                Use::TakeInto => format!("*{binding} = input.kept_text();"),
            },
            // A `None` the call left or made drops what was there, and a
            // `Some` where there was none is new.
            Self::Option(held) if matches!(purpose, Use::TakeInto) => format!(
                "if input.some() {{ match {binding} {{ \
                 ::std::option::Option::Some({inner}) => {{ {} }} \
                 {inner} => *{inner} = ::std::option::Option::Some({}), }} }} \
                 else {{ *{binding} = ::std::option::Option::None; }}",
                held.code_in(&inner, purpose, depth + 1, generic),
                held.take(),
            ),
            Self::Option(held) => format!(
                "match {binding} {{ ::std::option::Option::None => {out}.none(), \
                 ::std::option::Option::Some({inner}) => {{ {out}.some(); {} }} }}",
                held.code_in(&inner, purpose, depth + 1, generic)
            ),
            // Each element still holding a part it was lent is taken back
            // into, and moves where the call put it; any other is new.
            Self::Vec(held) if matches!(purpose, Use::TakeInto) => format!(
                "{island}::vec_into(input, {binding}, |input, {inner}| {{ {} }}, \
                 |input| {});",
                held.code_in(&inner, purpose, depth + 1, generic),
                held.take(),
            ),
            // A `Vec` lent `&mut` moves its elements to the call, which may
            // move them on, as `children.append(&mut header.actions)` does
            // (#1587), and takes back each the call leaves in it.
            Self::Vec(held) if matches!(purpose, Use::LendMut) => format!(
                "{{ let out = call.out(); let {inner} = ::std::mem::take({binding}); \
                 out.array({inner}.len()); for __e{depth} in {inner} {{ {} }} }}",
                held.code_in(&format!("__e{depth}"), Use::Give, depth + 1, generic)
            ),
            Self::Vec(held) => format!(
                "{{ let {inner} = {binding}; {out}.array({inner}.len()); \
                 for __e{depth} in {inner} {{ {} }} }}",
                held.code_in(&format!("__e{depth}"), purpose, depth + 1, generic)
            ),
            Self::Boxed(held) => {
                let unboxed = match purpose {
                    Use::Lend | Use::Put => format!("&**{binding}"),
                    Use::LendMut | Use::TakeInto => format!("&mut **{binding}"),
                    Use::Give => format!("*{binding}"),
                };
                held.code_in(&unboxed, purpose, depth + 1, generic)
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
                    .map(|(member, name)| member.code_in(name, purpose, depth + 1, generic))
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
            Self::Native(written) if static_lifetimes(written) != *written => format!(
                "{{ // SAFETY: only its lifetimes change, which have no layout, and it \
                 borrows what the marked value does.\n\
                 #[allow(unsafe_code)] let __taken = unsafe {{ {island}::at_lifetimes(\
                 (&{island}::Back::<{}>::new()).take(input)) }}; __taken }}",
                static_lifetimes(written)
            ),
            Self::Native(written) => format!("(&{island}::Back::<{written}>::new()).take(input)"),
            Self::Pulled { item, .. } => {
                let lasting = static_lifetimes(item);
                format!(
                    "{{ // SAFETY: only its lifetimes change, which have no layout, and it \
                     borrows what the marked value does.\n\
                     #[allow(unsafe_code)] let __taken = unsafe {{ {island}::at_lifetimes::<{}, _>(\
                     input.pulled(|input| (&{island}::Back::<{lasting}>::new()).take(input))) }}; \
                     __taken }}",
                    pulled_object(&lasting)
                )
            }
            Self::Text => "input.kept_text()".to_owned(),
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

/// The item type, as written, of `inner`, the trait object of a boxed
/// field written in `text`, when it is std's `dyn Iterator<Item = T>` with
/// at most a lifetime beside it, as inboard's `dyn Iterator<Item =
/// Element<'a, Message>> + 'a`: what [`Held::Pulled`] stands in for.
fn pulled_item<'t>(text: &'t str, inner: &type_ref::Type) -> Option<&'t str> {
    let type_ref::Type::Path(path) = inner else {
        return None;
    };
    let [segment] = &path.segments[..] else {
        return None;
    };
    let [type_ref::Argument::Type(item)] = &segment.arguments[..] else {
        return None;
    };
    if segment.name.text != type_ref::ITER_TYPE {
        return None;
    }
    let written = slice(text, inner.span())
        .trim_start()
        .strip_prefix("dyn ")?;
    let (trait_, _) = written.split_once('<')?;
    let trait_ = trait_.trim();
    if !(trait_ == "Iterator" || trait_.ends_with("::iter::Iterator")) {
        return None;
    }
    // Each bound after the trait's, at the top level, is a lifetime.
    let mut depth = 0_i32;
    let mut bounds = Vec::new();
    let mut start = 0;
    for (at, character) in written.char_indices() {
        match character {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            '+' if depth == 0 => {
                bounds.push(&written[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    bounds.push(&written[start..]);
    bounds[1..]
        .iter()
        .all(|bound| bound.trim().starts_with('\''))
        .then(|| slice(text, item.span()))
}

/// The box of std's iterator of `item` a [`Held::Pulled`] crosses as.
fn pulled_object(item: &str) -> String {
    format!("::std::boxed::Box<dyn ::std::iter::Iterator<Item = {item}>>")
}

/// The std containers a marked type's field may hold native parts in
/// (`Generator::held`).
#[derive(Clone, Copy)]
enum Container {
    Option,
    Vec,
    Box,
}

/// `T` of a slice type `[T]`.
fn slice_element(ty: &type_ref::Type) -> Option<&type_ref::Type> {
    let type_ref::Type::Path(path) = ty else {
        return None;
    };
    let last = path.segments.last()?;
    match &last.arguments[..] {
        [type_ref::Argument::Type(element)]
            if path.segments.len() == 1 && last.name.text == crate::item::type_ref::SLICE_TYPE =>
        {
            Some(element)
        }
        _ => None,
    }
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

/// What rustc builds of the generated `text`: it without comments and inert
/// attributes (`crate::item::inert`), a space only between two words, and
/// its literals as written. Two texts that differ only in what this leaves
/// out build the same native code: a doc comment, a lint, or a signature
/// written over more lines (#1646).
fn built(text: &str) -> String {
    let is_word = |character: char| character.is_alphanumeric() || character == '_';
    let mut built = String::with_capacity(text.len());
    let mut spaced = false;
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        let length = if rest.starts_with("//") {
            spaced = true;
            rest.find('\n').unwrap_or(rest.len())
        } else if rest.starts_with("/*") {
            spaced = true;
            block_comment(rest)
        } else if first.is_whitespace() {
            spaced = true;
            first.len_utf8()
        } else if let Some(length) = rest.strip_prefix('#').and_then(inert_attribute) {
            spaced = true;
            length + 1
        } else {
            let length = literal(rest).unwrap_or_else(|| {
                if is_word(first) {
                    rest.find(|character: char| !is_word(character))
                        .unwrap_or(rest.len())
                } else {
                    first.len_utf8()
                }
            });
            if spaced && built.ends_with(is_word) && rest.starts_with(is_word) {
                built.push(' ');
            }
            spaced = false;
            built.push_str(&rest[..length]);
            length
        };
        rest = &rest[length..];
    }
    built
}

/// How long the block comment `text` starts with is, nested ones in it.
fn block_comment(text: &str) -> usize {
    let mut depth = 0_usize;
    let mut at = 0;
    while at < text.len() {
        if text[at..].starts_with("/*") {
            depth += 1;
            at += 2;
        } else if text[at..].starts_with("*/") {
            depth -= 1;
            at += 2;
            if depth == 0 {
                return at;
            }
        } else {
            at += text[at..].chars().next().map_or(1, char::len_utf8);
        }
    }
    text.len()
}

/// How long the string or character literal `text` starts with is, its
/// prefix (`b`, `r#`) with it; `None` when it starts none.
fn literal(text: &str) -> Option<usize> {
    let prefix = text
        .find(|character: char| !matches!(character, 'b' | 'c' | 'r'))
        .filter(|prefix| *prefix <= 2)?;
    let after = &text[prefix..];
    if text[..prefix].contains('r') {
        let hashes = after.bytes().take_while(|byte| *byte == b'#').count();
        let open = &after[hashes..];
        open.starts_with('"').then_some(())?;
        let close = format!("\"{}", "#".repeat(hashes));
        return Some(
            open[1..]
                .find(&close)
                .map_or(text.len(), |end| prefix + hashes + 1 + end + close.len()),
        );
    }
    let quote = after.chars().next()?;
    if quote == '"' {
        let mut escaped = false;
        for (at, character) in after.char_indices().skip(1) {
            match character {
                '"' if !escaped => return Some(prefix + at + 1),
                '\\' => escaped = !escaped,
                _ => escaped = false,
            }
        }
        return Some(text.len());
    }
    // A character, not a lifetime: `'x'` or `'\n'`.
    if quote == '\'' {
        let mut characters = after.char_indices().skip(1);
        let (_, first) = characters.next()?;
        if first == '\\' {
            // After the quote, the backslash and the character it escapes.
            return after.get(3..)?.find('\'').map(|end| prefix + 3 + end + 1);
        }
        let (at, close) = characters.next()?;
        return (close == '\'').then_some(prefix + at + 1);
    }
    None
}

/// How long the inert attribute `text`, after its `#`, is through its `]`
/// (`crate::item::inert`); `None` when it is no inert attribute.
fn inert_attribute(text: &str) -> Option<usize> {
    let inside = text.strip_prefix('!').unwrap_or(text);
    let opened = text.len() - inside.len();
    let inside = inside.strip_prefix('[')?;
    let written = inside.trim_start();
    let path = written
        .find(|character: char| {
            !(character.is_alphanumeric() || character == '_' || character == ':')
        })
        .map_or(written, |end| &written[..end]);
    if !crate::item::inert(path.split("::")) {
        return None;
    }
    Some(opened + 1 + bracketed(inside)? + 1)
}

/// How long `inside`, the text after an attribute's `[`, is up to the `]`
/// that closes it, past nested brackets and literals; `None` when none
/// does.
fn bracketed(inside: &str) -> Option<usize> {
    let mut depth = 1_usize;
    let mut at = 0;
    while at < inside.len() {
        let rest = &inside[at..];
        if let Some(length) = literal(rest) {
            at += length;
            continue;
        }
        let character = rest.chars().next()?;
        match character {
            '[' => depth += 1,
            ']' if depth == 1 => return Some(at),
            ']' => depth -= 1,
            _ => {}
        }
        at += character.len_utf8();
    }
    None
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

/// What each name a `use` of `sources` brings into its module stands for,
/// both by paths from the crate root ([`Generator::uses`]).
fn uses<'s>(sources: impl IntoIterator<Item = &'s Source<'s>>) -> BTreeMap<String, Vec<String>> {
    let mut uses = BTreeMap::new();
    for source in sources {
        for item in source.items {
            let item::Parsed::Use(used) = item else {
                continue;
            };
            if used.scope.is_some() || used.global() {
                continue;
            }
            for (name, path) in used.bindings() {
                let path = path.iter().map(ImmutableString::as_str).collect::<Vec<_>>();
                if let Some(target) = absolute(source, &path, 0) {
                    uses.insert(path_of(source.modules, &name.text), target);
                }
            }
        }
    }
    uses
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

/// What a generic function's trampoline is written from (D869): `named`
/// are the types it is generic over, its impl's first, and `concrete` the
/// trampoline its body would be at concrete types; `prelude` makes each
/// `impl Trait`'s owned stand-in, whose type `stood` writes.
struct Generic<'g> {
    path: &'g str,
    signature: &'g str,
    concrete: &'g str,
    prelude: &'g str,
    tuple: &'g str,
    named: &'g [&'g str],
    stood: &'g [Option<String>],
    /// The impl of an associated function.
    outer: Option<&'g item::Impl>,
}

/// A generic function's trampoline: a public one with the signature as
/// written, which adds no bound and hands its arguments and result place
/// to the instance its types' names select, and that instance's entry,
/// the concrete trampoline's body with each type `'static`, which only the
/// support table instantiates, at the types native code calls the function
/// with (D869). A method's entry is a function beside its impl, its
/// receiver `__self` and its `Self` the impl's type as written; its name
/// is the entry's.
fn generic_trampoline(
    file: &Source<'_>,
    function: &item::Function,
    generic: &Generic<'_>,
) -> (String, String, String) {
    let island = format!("crate::{MODULE}");
    let Generic {
        path,
        signature,
        concrete,
        prelude,
        tuple,
        ..
    } = generic;
    let owner = generic
        .outer
        .map(|outer| slice(file.text, outer.owner.span()).trim());
    // `Self` and `self` of a method, outside its impl.
    let outside = |text: &str| match owner {
        Some(owner) => replace_word(&replace_word(text, "self", "__self"), "Self", owner),
        None => text.to_owned(),
    };
    let written_output = function
        .output
        .as_ref()
        .map_or("()", |ty| slice(file.text, ty.span()));
    let output = outside(written_output);
    let receiver = function.receiver.as_ref().map(|receiver| {
        let owner = owner.unwrap_or("Self");
        let lifetime = function
            .receiver_lifetime
            .as_ref()
            .map_or(String::new(), |lifetime| format!("'{} ", lifetime.text));
        match receiver {
            item::Receiver::Shared(_) => ("self", "__self", format!("&{lifetime}{owner}")),
            item::Receiver::Mutable(_) => ("self", "__self", format!("&{lifetime}mut {owner}")),
            item::Receiver::Owned(_) => ("self", "__self", owner.to_owned()),
            item::Receiver::OwnedMutable(_) => ("self", "mut __self", owner.to_owned()),
        }
    });
    let mut given = Vec::new();
    let mut bound = Vec::new();
    let mut types = Vec::new();
    if let Some((name, binding, ty)) = &receiver {
        given.push((*name).to_owned());
        bound.push((*binding).to_owned());
        types.push(ty.clone());
    }
    for (index, parameter) in function.parameters.iter().enumerate() {
        given.push(format!("__a{index}"));
        bound.push(format!("__a{index}"));
        types.push(match generic.stood.get(index) {
            Some(Some(stood)) => outside(stood),
            _ => outside(slice(file.text, parameter.ty.span())),
        });
    }
    let tuple_of = |items: &[String]| {
        items
            .iter()
            .map(|item| format!("{item},"))
            .collect::<String>()
    };
    let (given, bound, types) = (tuple_of(&given), tuple_of(&bound), tuple_of(&types));
    let public = format!(
        "{signature} {{\n{prelude}    \
         let mut __arguments = ::std::mem::ManuallyDrop::new(({given}));\n    \
         let mut __value = ::std::mem::MaybeUninit::<{written_output}>::uninit();\n    \
         // SAFETY: the instance reads the arguments once, as the values they\n    \
         // are, and writes the result in its place.\n    \
         #[allow(unsafe_code)]\n    \
         unsafe {{\n        \
         {island}::instance(\n            {path:?},\n            \
         ::std::any::type_name::<{tuple}>(),\n            \
         ::std::ptr::from_mut(&mut *__arguments).cast(),\n            \
         __value.as_mut_ptr().cast(),\n        );\n        \
         __value.assume_init()\n    }}\n}}\n"
    );
    let (name, header) = instance_header(function, file, generic, owner);
    let body = outside(concrete[signature.len()..].trim());
    let entry = format!(
        "#[doc(hidden)]\n#[allow(unsafe_code, non_snake_case, unused_mut)]\n{} {{\n    \
         // SAFETY: the trampoline moved its arguments into the tuple this reads\n    \
         // once, and keeps them from dropping.\n    \
         let ({bound}): ({types}) = unsafe {{ ::std::ptr::read(__arguments.cast::<({types})>()) }};\n    \
         let __result: {output} = {body};\n    \
         // SAFETY: `__out` is the trampoline's place for a value of this type.\n    \
         unsafe {{ ::std::ptr::write(__out.cast::<{output}>(), __result) }};\n}}\n",
        outside(&header)
    );
    (public, entry, name)
}

/// The name and header of a generic function's instance entry: its
/// generics as written, its impl's first, each lifetime before each type,
/// each named type `'static`, taking the trampoline's argument tuple and
/// result place. A method's is named by its impl's type and its own name,
/// as `__rawr_instance_Sidebar_content`, a trait's by its trait too.
fn instance_header(
    function: &item::Function,
    file: &Source<'_>,
    generic: &Generic<'_>,
    owner: Option<&str>,
) -> (String, String) {
    let after = slice(
        file.text,
        Span::new(function.name.span.end(), function.body_span.start()),
    );
    let mut written = Vec::new();
    let mut predicates = generic
        .named
        .iter()
        .map(|name| format!("{name}: 'static"))
        .collect::<Vec<_>>();
    let mut name = String::new();
    if let (Some(outer), Some(owner)) = (generic.outer, owner) {
        // The impl's header as written: `impl<..> Trait for Type where ..`.
        let header = slice(
            file.text,
            Span::new(outer.span.start(), outer.header.span.end()),
        );
        let header = header.trim_end().trim_end_matches('{').trim_end();
        let after_impl = header.trim_start().trim_start_matches("impl");
        written.extend(parameters(generics_written(after_impl)));
        predicates
            .extend(where_clause(header).map(|clause| clause.trim_end_matches(',').to_owned()));
        let trait_ = outer.trait_.as_ref().map_or(String::new(), |trait_| {
            slice(file.text, trait_.span).to_owned()
        });
        name = identifier(&format!("{trait_} {owner} "));
    }
    written.extend(parameters(generics_written(after)));
    predicates.extend(
        where_clause(generic.signature).map(|clause| clause.trim_end_matches(',').to_owned()),
    );
    name.push_str(&function.name.text);
    let name = format!("{INSTANCE_PREFIX}{name}");
    // A lifetime parameter comes before every type parameter.
    written.sort_by_key(|parameter| !parameter.starts_with('\''));
    let generics = if written.is_empty() {
        String::new()
    } else {
        format!("<{}>", written.join(", "))
    };
    let header = format!(
        "pub(crate) unsafe fn {name}{generics}(__arguments: *mut (), __out: *mut ())\nwhere\n    {},",
        predicates.join(",\n    ")
    );
    (name, header)
}

/// Each parameter of the generic parameter list `list`, as written.
fn parameters(list: &str) -> Vec<String> {
    let Some(inner) = list
        .strip_prefix('<')
        .and_then(|inner| inner.strip_suffix('>'))
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;
    let mut previous = ' ';
    for (index, character) in inner.char_indices() {
        match character {
            '<' | '(' | '[' => depth += 1,
            '>' if previous != '-' => depth -= 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                found.push(inner[start..index].trim().to_owned());
                start = index + 1;
            }
            _ => {}
        }
        previous = character;
    }
    found.push(inner[start..].trim().to_owned());
    found.retain(|parameter| !parameter.is_empty());
    found
}

/// `text` as a Rust identifier: each run of other characters one `_`,
/// none at the start or end.
fn identifier(text: &str) -> String {
    let mut name = String::new();
    for character in text.chars() {
        if character.is_alphanumeric() || character == '_' {
            name.push(character);
        } else if !name.is_empty() && !name.ends_with('_') {
            name.push('_');
        }
    }
    name
}

/// `text` with each whole word `word`, outside string literals, `with`.
fn replace_word(text: &str, word: &str, with: &str) -> String {
    let mut replaced = String::with_capacity(text.len());
    let mut rest = text;
    let mut quoted = false;
    let mut previous = ' ';
    while let Some(character) = rest.chars().next() {
        let word_start = !(previous.is_alphanumeric() || previous == '_');
        if !quoted
            && word_start
            && rest.starts_with(word)
            && !rest[word.len()..].starts_with(|next: char| next.is_alphanumeric() || next == '_')
        {
            replaced.push_str(with);
            rest = &rest[word.len()..];
            previous = 'w';
            continue;
        }
        if character == '"' && previous != '\\' {
            quoted = !quoted;
        }
        replaced.push(character);
        rest = &rest[character.len_utf8()..];
        previous = if previous == '\\' && character == '\\' {
            ' '
        } else {
            character
        };
    }
    replaced
}

/// The generic parameter list `after`, the text after a function's name,
/// starts with, as written: empty when it has none.
fn generics_written(after: &str) -> &str {
    let trimmed = after.trim_start();
    if !trimmed.starts_with('<') {
        return "";
    }
    let mut depth = 0_usize;
    let mut previous = ' ';
    for (index, character) in trimmed.char_indices() {
        match character {
            '<' => depth += 1,
            // `->` of a bound's `Fn(..) -> T` closes nothing.
            '>' if previous != '-' => {
                depth -= 1;
                if depth == 0 {
                    return &trimmed[..=index];
                }
            }
            _ => {}
        }
        previous = character;
    }
    ""
}

/// The predicates of the `where` clause `signature` ends with, outside any
/// brackets.
fn where_clause(signature: &str) -> Option<&str> {
    let mut depth = 0_i32;
    let bytes = signature.as_bytes();
    for (index, character) in signature.char_indices() {
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            'w' if depth == 0
                && signature[index..].starts_with("where")
                && index > 0
                && bytes[index - 1].is_ascii_whitespace()
                && signature[index + 5..].starts_with(char::is_whitespace) =>
            {
                return Some(signature[index + 5..].trim());
            }
            _ => {}
        }
    }
    None
}

/// The prefix of a generic function's instance entry, beside it (D869).
pub(crate) const INSTANCE_PREFIX: &str = "__rawr_instance_";

/// The statements that give the parameter `name` to the call by value,
/// as `pass`, a by-value pass, says: `call` is the call as `give!` takes
/// it, `__call` or, through a `&mut`, `*__call`.
fn give(pass: &Pass, name: &str, call: &str, island: &str, text: &mut String) {
    match pass {
        Pass::Value => {
            let _ = writeln!(text, "    __call.argument(&{name});");
        }
        Pass::GiveParts => {
            // A move ends every borrow of the value (#1492).
            let _ = writeln!(text, "    __call.release(&{name});");
            let _ = writeln!(text, "    __call.given({name});");
        }
        Pass::Give => {
            let _ = writeln!(text, "    {island}::give!({call}, {name});");
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
                 {island}::give!({call}, {name});",
                unnamed_lifetimes(ty),
                static_lifetimes(ty)
            );
        }
        Pass::GiveHeld { give, ty } => {
            if let Some(ty) = ty {
                let _ = writeln!(
                    text,
                    "    // SAFETY: as for a value given for the call alone.\n    \
                     #[allow(unsafe_code)]\n    \
                     let {name} = unsafe {{ ::std::mem::transmute::<{}, {}>({name}) }};\n    \
                     __call.for_call();",
                    unnamed_lifetimes(ty),
                    static_lifetimes(ty)
                );
            }
            let _ = writeln!(
                text,
                "    __call.give_parts(|out| {{ let __value = {name}; {give} }});"
            );
        }
        Pass::Borrow
        | Pass::Text
        | Pass::Mutable
        | Pass::Lend { .. }
        | Pass::LendSlice { .. }
        | Pass::LendParts { .. }
        | Pass::Object
        | Pass::Trait { .. }
        | Pass::Boxed { .. } => {
            unreachable!("only a by-value pass gives its value")
        }
    }
}

/// The `Vec` of `item`, written in `file`, that stands in for an `impl
/// IntoIterator<Item = item>` at `span`: its items drained into it.
fn collected(file: &Source<'_>, item: &type_ref::Type, span: Span) -> Stand {
    let item_written = slice(file.text, item.span());
    Stand {
        ty: std_path("Vec", vec![type_ref::Argument::Type(item.clone())], span),
        written: format!("::std::vec::Vec<{item_written}>"),
        make: format!(
            "::std::iter::Iterator::collect::<::std::vec::Vec<{item_written}>>(\
             ::std::iter::IntoIterator::into_iter(__value))"
        ),
    }
}

/// `generic`'s bounds but auto traits.
fn bounds(generic: &item::GenericParam) -> Vec<&type_ref::Path> {
    generic
        .bounds
        .iter()
        .filter(|bound| {
            bound.segments.last().is_none_or(|last| {
                !matches!(last.name.text.as_str(), "Send" | "Sync" | "Unpin" | "Sized")
            })
        })
        .collect()
}

/// The trait `bound` names, as a path without its arguments, which rustc
/// infers: `IntoContent` of `IntoContent<'a, Message>`.
fn trait_path(bound: &type_ref::Path) -> String {
    let names = bound
        .segments
        .iter()
        .map(|segment| segment.name.text.as_str())
        .collect::<Vec<_>>()
        .join("::");
    if bound.global {
        format!("::{names}")
    } else {
        names
    }
}

/// The [`GIVEN`] trait of the marked trait `bound` names in `file`, at
/// the bound's arguments: `crate::widget::sidebar::__RawrGivenIntoContent<'a,
/// Message>` of `IntoContent<'a, Message>`, by its path from the crate's
/// root, as no `use` brings it.
fn given_path(file: &Source<'_>, bound: &type_ref::Path) -> String {
    let names = bound
        .segments
        .iter()
        .map(|segment| segment.name.text.as_str())
        .collect::<Vec<_>>();
    let written = slice(file.text, bound.span).trim();
    let arguments = written.find('<').map_or("", |at| &written[at..]);
    let mut path = match absolute(file, &names, 0).filter(|_| !bound.global) {
        Some(path) => std::iter::once("crate".to_owned()).chain(path).collect(),
        None => names.iter().map(ToString::to_string).collect::<Vec<_>>(),
    };
    if let Some(last) = path.last_mut() {
        *last = format!("{GIVEN}{last}");
    }
    format!("{}{arguments}", path.join("::"))
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
             #[inline]\n        \
             fn mutable(nonce: u64, visit: &mut dyn FnMut(&mut {referent})) -> {result} {{\n            \
             {island}::reach(nonce, {key}, false, |place| {{\n                \
             // SAFETY: `Key`'s lend is this call's `&mut {referent}`, claimed exclusively.\n                \
             visit(unsafe {{ &mut *place.cast::<{referent}>() }})\n            }})\n        }}\n        \
             const MUTABLE: fn(u64, &mut dyn FnMut(&mut {referent})) -> {result} = mutable;"
        );
    }
    let _ = writeln!(
        text,
        "        #[allow(unsafe_code)]\n        \
         #[inline]\n        \
         fn shared(nonce: u64, visit: &mut dyn FnMut(&{referent})) -> {result} {{\n            \
         {island}::reach(nonce, {key}, true, |place| {{\n                \
         // SAFETY: `Key`'s lend is this call's `{referent}`, claimed shared.\n                \
         visit(unsafe {{ &*place.cast_const().cast::<{referent}>() }})\n            }})\n        }}\n        \
         const SHARED: fn(u64, &mut dyn FnMut(&{referent})) -> {result} = shared;\n        \
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

/// Lend `name`, a `&[element]`, to the call where it is (#1292): its place
/// is the trampoline's own reference, which the accessor reads the slice
/// through, and which lives while the call runs.
fn lend_slice(name: &str, element: &str, island: &str, text: &mut String) {
    let result = "::std::result::Result<(), u8>";
    let key = "::std::any::TypeId::of::<Key>()";
    let _ = writeln!(
        text,
        "    let {name}_lend = {{
        struct Key;
        #[allow(unsafe_code)]
        #[inline]
        fn shared(nonce: u64, visit: &mut dyn FnMut(&[{element}])) -> {result} {{
            {island}::reach(nonce, {key}, true, |place| {{
                // SAFETY: `Key`'s lend is this call's `&[{element}]`, claimed shared.
                visit(unsafe {{ *place.cast_const().cast::<&[{element}]>() }})
            }})
        }}
        const SHARED: fn(u64, &mut dyn FnMut(&[{element}])) -> {result} = shared;
        {island}::Lend::new({key}, None, &SHARED)
    }};
    // SAFETY: the lend's accessor reads its key's place as a `&[{element}]`.
    #[allow(unsafe_code)]
    let () = unsafe {{ __call.lend_slice(&{name}, {name}_lend, ::std::any::type_name::<[{element}]>()) }};"
    );
}

/// The owned value that stands in for an argument-position `impl Trait`
/// written `written`, which the parser lifted to `generic`: of the one
/// trait it is bounded by besides auto traits, a type that implements it.
fn stand_for(
    file: &Source<'_>,
    generic: &item::GenericParam,
    written: &str,
) -> Result<Stand, String> {
    let refused = || format!("`{written}` can't cross yet");
    let [bound] = bounds(generic)[..] else {
        return Err(refused());
    };
    let Some(last) = bound.segments.last() else {
        return Err(refused());
    };
    let string = || std_path("String", Vec::new(), bound.span);
    match (last.name.text.as_str(), &last.arguments[..]) {
        // `T: IntoIterator<Item = X>`, as a blanket impl's bound writes it.
        ("IntoIterator", []) => generic
            .projections
            .iter()
            .find(|(trait_, name, _)| trait_.text == last.name.text && name.text.as_str() == "Item")
            .map(|(_, _, item)| collected(file, item, bound.span))
            .ok_or_else(refused),
        ("Into", [type_ref::Argument::Type(target)]) => {
            let target_written = slice(file.text, target.span()).to_owned();
            Ok(Stand {
                ty: target.clone(),
                make: format!("::std::convert::Into::<{target_written}>::into(__value)"),
                written: target_written,
            })
        }
        ("AsRef", [type_ref::Argument::Type(target)])
            if slice(file.text, target.span()).trim() == "str" =>
        {
            Ok(Stand {
                ty: string(),
                written: "::std::string::String".to_owned(),
                make: "::std::string::String::from(::std::convert::AsRef::<str>::as_ref(&__value))"
                    .to_owned(),
            })
        }
        ("ToString" | "Display", []) => Ok(Stand {
            ty: string(),
            written: "::std::string::String".to_owned(),
            make: "::std::string::ToString::to_string(&__value)".to_owned(),
        }),
        _ => Err(refused()),
    }
}

/// The std type `name` of `arguments`, by its own name, at `span`.
fn std_path(name: &str, arguments: Vec<type_ref::Argument>, span: Span) -> type_ref::Type {
    type_ref::Type::Path(type_ref::Path {
        segments: Box::new([type_ref::Segment {
            name: item::Name {
                text: name.into(),
                span,
            },
            arguments: arguments.into_boxed_slice(),
            span,
        }]),
        span,
        global: false,
    })
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
/// &'a self, inbox: &'a inbox::State) -> Element<'a, Message>` (#1413), a
/// slice of native values (#1292) or a marked value's native parts: native
/// code keeps each borrowed as long as the result.
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
            matches!(
                pass,
                Pass::Lend { mutable: false, .. }
                    | Pass::LendSlice { .. }
                    | Pass::LendParts { mutable: false }
            ) && lifetimes(slice(file.text, parameter.ty.span()))
                .iter()
                .any(|name| named.contains(name))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Whether `function`'s result can borrow nothing, so nothing of its call
/// outlives it: no result, or one that names no lifetime and to which
/// rustc can't elide one, as it would to `Element<Message>` from the one
/// input that borrows.
fn borrows_nothing(file: &Source<'_>, function: &item::Function) -> bool {
    let Some(output) = &function.output else {
        return true;
    };
    if matches!(output, type_ref::Type::Reference { .. }) {
        return false;
    }
    let written = slice(file.text, output.span());
    if written.contains('\'') {
        return false;
    }
    if !written.contains('<') {
        return true;
    }
    // Elision ties a borrowing receiver's lifetime to the result, or the
    // lifetime of the one input that borrows.
    if matches!(
        function.receiver,
        Some(item::Receiver::Shared(_) | item::Receiver::Mutable(_))
    ) {
        return false;
    }
    let borrowing = function.parameters.iter().filter(|parameter| {
        matches!(parameter.ty, type_ref::Type::Reference { .. })
            || slice(file.text, parameter.ty.span()).contains('\'')
    });
    borrowing.count() != 1
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
pub(crate) fn support(
    main: bool,
    objects: &[(String, String)],
    instances: &[crate::package::instances::Entry],
) -> String {
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
    // Each instance native code calls a generic function at, by the path
    // its call names and the name of the tuple of its types (D869).
    let instances = instances
        .iter()
        .map(|instance| {
            let types = instance.types.join(", ");
            format!(
                "({:?}, ::std::any::type_name::<({types},)> as fn() -> &'static str, \
                 {}::<{types}> as Instance)",
                instance.path, instance.function
            )
        })
        .collect::<Vec<_>>()
        .join(",\n        ");
    SUPPORT
        .replace("ENTRY", entry)
        .replace("__MAKER_LIST__", &format!("[{makers}]"))
        .replace(
            "__INSTANCE_LIST__",
            &format!("[\n        {instances}\n    ]"),
        )
}

/// The island's support package (#1808): what a copy's [`MODULE`]
/// re-exports, the same for every crate and every save, which rawr writes
/// beside the copy as [`PACKAGE`] at [`PACKAGE_DIRECTORY`], so Cargo
/// builds it once.
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
pub(crate) const PACKAGE_DIRECTORY: &str = "island";

/// The package's name, and its library's, by which a copy depends on it.
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
pub(crate) const PACKAGE_NAME: &str = "rawr-island-support";
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
pub(crate) const PACKAGE_CRATE: &str = "__rawr_island_support";

/// The package's files, by their paths in its directory.
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
pub(crate) fn package() -> [(&'static str, String); 2] {
    let manifest = format!(
        "# Generated by Rawr: the island's support, which each copy of a crate\n\
         # with marked modules re-exports.\n\
         [package]\nname = {PACKAGE_NAME:?}\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         build = false\nautobins = false\npublish = false\n\n\
         [lib]\nname = {PACKAGE_CRATE:?}\npath = \"src/lib.rs\"\n\n[workspace]\n"
    );
    [("Cargo.toml", manifest), ("src/lib.rs", PACKAGE.to_owned())]
}

/// The support package's library. The wire format matches
/// [`crate::island`]'s.
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
const PACKAGE: &str = r#"//! Generated by Rawr: how the native code of a crate reaches its marked
//! modules, which Rawr interprets. Each copy of a crate with marked modules
//! re-exports this package from its `__rawr_island` module, which adds what
//! depends on the crate: its stand-ins' makers, its instance table and its
//! `main`. Nothing here depends on the crate, so Cargo builds it once.
#![doc(hidden)]
#![allow(unsafe_code, dead_code, unused_imports, clippy::all)]

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
    fn __rawr_island_unanchor_v1(address: u64, ty: u64);
    fn __rawr_island_trait_v1(owner: &str, op: u8, handle: u64, other: u64) -> ::std::vec::Vec<u8>;
    fn __rawr_island_next_v1(
        handle: u64,
        natives: &mut ::std::vec::Vec<::std::boxed::Box<dyn ::std::any::Any>>,
    ) -> ::std::vec::Vec<u8>;
}

/// What makes a native trait's object of a program value: the
/// trait's path below the crate's root and the value's handle give a
/// box of the stand-in, or `None` for another trait
/// (`rawr::island::Maker`).
pub type Maker = fn(&str, u64) -> ::std::option::Option<Given>;

/// Register `makers`, a crate's stand-ins' makers by their types' paths
/// from the crate's root, before its first call into marked code, which
/// is where a program value becomes a trait object.
pub fn register(makers: &[(&'static str, Maker)]) {
    // SAFETY: the runner links `rawr`, which defines the function with
    // this exact signature.
    unsafe { __rawr_island_objects_v1(makers) }
}

/// Start the program at `entry`, a crate root's `main`: the runner calls
/// it once this returns, on a thread of its own.
pub fn start(entry: fn() -> i32) {
    // SAFETY: as for `register`.
    unsafe { __rawr_island_start_v1(entry) }
}

/// The body of the trampoline of the marked function at `path` when the
/// crate's last check found no native code naming it, so its call can't
/// come: native code built since names it, and was checked again first.
#[cold]
pub fn uncalled(path: &str) -> ! {
    ::std::panic!(
        "rawr: native code called `{path}`, which the last check didn't see it name; \
         this is a rawr bug"
    )
}

/// A generic function's instance entry: it reads the call's arguments
/// as the tuple of the types they are and writes the result in place.
pub type Instance = unsafe fn(*mut (), *mut ());

/// The path a generic function's instance is found by: its own, with
/// the hash of its types' tuple's name, as rawr names the instance
/// (`package::instances::key`), interned for the program's life.
pub fn keyed(path: &'static str, name: &'static str) -> &'static str {
    static KEYS: ::std::sync::Mutex<::std::vec::Vec<(&'static str, &'static str, &'static str)>> =
        ::std::sync::Mutex::new(::std::vec::Vec::new());
    let mut keys = KEYS.lock().unwrap_or_else(::std::sync::PoisonError::into_inner);
    if let Some((_, _, key)) = keys.iter().find(|(known, named, _)| *known == path && *named == name) {
        return key;
    }
    // FNV-1a, as `package::instances::key` hashes it.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let key: &'static str = ::std::boxed::Box::leak(::std::format!("{path}#{hash:016x}").into_boxed_str());
    keys.push((path, name, key));
    key
}

/// [`keyed`], and the name each impl of a marked trait a boxed
/// argument is of gives itself (`+Content`), as the resolver compiles
/// one instance for each impl.
pub fn keyed_given(path: &'static str, name: &'static str, given: &[&'static str]) -> &'static str {
    static KEYS: ::std::sync::Mutex<::std::vec::Vec<(&'static str, ::std::vec::Vec<&'static str>, &'static str)>> =
        ::std::sync::Mutex::new(::std::vec::Vec::new());
    let base = keyed(path, name);
    let mut keys = KEYS.lock().unwrap_or_else(::std::sync::PoisonError::into_inner);
    if let Some((_, _, key)) = keys.iter().find(|(known, names, _)| *known == base && names.as_slice() == given) {
        return key;
    }
    let key: &'static str = ::std::boxed::Box::leak(::std::format!("{base}{}", given.concat()).into_boxed_str());
    keys.push((base, given.to_vec(), key));
    key
}

/// The panic of a failed `reply`: a marked function's panic, which the
/// runner printed as Rust prints it at its place in the marked file,
/// unwinds with its message (`2`); rawr's own failure panics with its
/// message.
fn failed(reply: &[u8]) -> ! {
    let message = ::std::string::String::from_utf8_lossy(reply.get(1..).unwrap_or_default()).into_owned();
    match reply.first() {
        ::std::option::Option::Some(2) => ::std::panic::resume_unwind(::std::boxed::Box::new(message)),
        _ => ::std::panic!("{message}"),
    }
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
    // SAFETY: as for `register`.
    let mut reply = unsafe { __rawr_island_trait_v1(owner, op, handle, other) };
    match reply.first() {
        ::std::option::Option::Some(0) => {
            reply.remove(0);
            reply
        }
        _ => failed(&reply),
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
#[macro_export]
macro_rules! object {
    ($trait_name:expr, $handle:expr, $object:ty, $stand:path) => {{
        #[allow(unused_imports)]
        use $crate::{
            HasClone as _, HasDebug as _, HasDisplay as _, HasEqual as _, HasHash as _,
            HasOrder as _, LacksClone as _, LacksDebug as _, LacksDisplay as _,
            LacksEqual as _, LacksHash as _, LacksOrder as _,
        };
        if !$crate::names(::std::any::type_name::<$object>(), $trait_name) {
            return ::std::option::Option::None;
        }
        let value: ::std::boxed::Box<$object> =
            ::std::boxed::Box::new($stand($crate::Object::new($handle)));
        let probe = &$crate::probe(&value);
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
#[macro_export]
macro_rules! give {
    ($call:expr, $value:expr) => {{
        #[allow(unused_imports)]
        use $crate::{
            HasClone as _, HasDebug as _, HasDisplay as _, HasEqual as _, HasHash as _,
            HasOrder as _, LacksClone as _, LacksDebug as _, LacksDisplay as _,
            LacksEqual as _, LacksHash as _, LacksOrder as _,
        };
        let value = $value;
        let probe = &$crate::probe(&value);
        let traits = (
            probe.debug_of(),
            probe.display_of(),
            probe.clone_of(),
            probe.equal_of(),
            probe.hash_of(),
            probe.order_of(),
        );
        #[allow(unused_imports)]
        use $crate::{ByCopy as _, ByGift as _};
        (&$crate::hand(&value)).hand(&mut $call, value, traits);
    }};
}

/// Put `$value`, a `&` of a native part of a marked type's value, as
/// the `themedef::Theme` of `Theme::Selected(themedef::Theme)`, in
/// `$out` (#1466): by the wire format when its type crosses by it, as
/// an alias of `usize` does, and else as a clone the interpreter keeps,
/// with what its type has of [`Traits`].
#[macro_export]
macro_rules! part {
    ($out:expr, $value:expr) => {{
        #[allow(unused_imports)]
        use $crate::{
            HasClone as _, HasDebug as _, HasDisplay as _, HasEqual as _, HasHash as _,
            HasOrder as _, LacksClone as _, LacksDebug as _, LacksDisplay as _,
            LacksEqual as _, LacksHash as _, LacksOrder as _, PartByClone as _,
            PartByWire as _, PartUncloned as _,
        };
        let value = $value;
        let probe = &$crate::probe(value);
        let traits = (
            probe.debug_of(),
            probe.display_of(),
            probe.clone_of(),
            probe.equal_of(),
            probe.hash_of(),
            probe.order_of(),
        );
        (&&$crate::Part(value)).put($out, traits);
    }};
}

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
#[macro_export]
macro_rules! lend_part {
    ($call:expr, $value:expr, $ty:ty) => {{
        struct Key;
        #[inline]
        fn shared(
            nonce: u64,
            visit: &mut dyn FnMut(&$ty),
        ) -> ::std::result::Result<(), u8> {
            $crate::reach(nonce, ::std::any::TypeId::of::<Key>(), true, |place| {
                // SAFETY: `Key`'s lend is this part, a `$ty`, claimed shared.
                visit(unsafe { &*place.cast_const().cast::<$ty>() })
            })
        }
        const SHARED: fn(u64, &mut dyn FnMut(&$ty)) -> ::std::result::Result<(), u8> = shared;
        #[allow(unused_imports)]
        use $crate::{LendByLoan as _, LendByWire as _};
        let lend = $crate::Lend::new(::std::any::TypeId::of::<Key>(), None, &SHARED);
        let value: &$ty = $value;
        // SAFETY: the lend's accessor reads its key's place as `$ty`.
        #[allow(unsafe_code)]
        unsafe {
            (&$crate::hand(value)).lend_part($call, value, lend, ::std::any::type_name::<$ty>())
        };
    }};
}

/// Lend `$value`, a `&$ty` of a marked value whose `Parts` impl lends
/// its parts, to the call `$call` whole, where it is (#1484): the value
/// written next is the one this lend lends, so native code the call
/// gives it to by reference is given this reference back.
#[macro_export]
macro_rules! lend_whole {
    ($call:expr, $value:expr, $ty:ty) => {{
        // Not `Key`, which a marked type `$ty` may be named.
        struct WholeKey;
        #[inline]
        fn shared(
            nonce: u64,
            visit: &mut dyn FnMut(&$ty),
        ) -> ::std::result::Result<(), u8> {
            $crate::reach(nonce, ::std::any::TypeId::of::<WholeKey>(), true, |place| {
                // SAFETY: `WholeKey`'s lend is this value, a `$ty`, claimed shared.
                visit(unsafe { &*place.cast_const().cast::<$ty>() })
            })
        }
        const SHARED: fn(u64, &mut dyn FnMut(&$ty)) -> ::std::result::Result<(), u8> = shared;
        let lend = $crate::Lend::new(
            ::std::any::TypeId::of::<WholeKey>(),
            ::std::option::Option::None,
            &SHARED,
        );
        let value: &$ty = $value;
        // SAFETY: the lend's accessor reads its key's place as `$ty`.
        #[allow(unsafe_code)]
        unsafe {
            $call.lend_whole(::std::ptr::from_ref(value).cast_mut().cast(), lend, ::std::any::type_name::<$ty>())
        };
    }};
}

/// [`lend_part!`] of a `&mut $ty`, lent mutably.
#[macro_export]
macro_rules! lend_part_mut {
    ($call:expr, $value:expr, $ty:ty) => {{
        struct Key;
        #[inline]
        fn mutable(
            nonce: u64,
            visit: &mut dyn FnMut(&mut $ty),
        ) -> ::std::result::Result<(), u8> {
            $crate::reach(nonce, ::std::any::TypeId::of::<Key>(), false, |place| {
                // SAFETY: `Key`'s lend is this part, a `&mut $ty`, claimed exclusively.
                visit(unsafe { &mut *place.cast::<$ty>() })
            })
        }
        #[inline]
        fn shared(
            nonce: u64,
            visit: &mut dyn FnMut(&$ty),
        ) -> ::std::result::Result<(), u8> {
            $crate::reach(nonce, ::std::any::TypeId::of::<Key>(), true, |place| {
                // SAFETY: `Key`'s lend is this part, a `$ty`, claimed shared.
                visit(unsafe { &*place.cast_const().cast::<$ty>() })
            })
        }
        const MUTABLE: fn(u64, &mut dyn FnMut(&mut $ty)) -> ::std::result::Result<(), u8> = mutable;
        const SHARED: fn(u64, &mut dyn FnMut(&$ty)) -> ::std::result::Result<(), u8> = shared;
        #[allow(unused_imports)]
        use $crate::{LendByLoan as _, LendByWire as _};
        let lend = $crate::Lend::new(
            ::std::any::TypeId::of::<Key>(),
            ::std::option::Option::Some(&MUTABLE),
            &SHARED,
        );
        let value: &mut $ty = $value;
        // SAFETY: the lend's accessors read its key's place as `$ty`.
        #[allow(unsafe_code)]
        unsafe {
            (&$crate::hand(&*value)).lend_part_mut($call, value, lend, ::std::any::type_name::<$ty>())
        };
    }};
}

/// The accessors of a lent `T`, keyed by `T`, for a generic marked
/// type's `Parts` impl, in which no item can name the impl's parameters
/// as [`lend_part!`]'s do (D869).
pub struct Accessors<T>(::std::marker::PhantomData<T>);

impl<T: 'static> Accessors<T> {
    const SHARED: fn(u64, &mut dyn FnMut(&T)) -> ::std::result::Result<(), u8> = Self::shared_of;
    const MUTABLE: fn(u64, &mut dyn FnMut(&mut T)) -> ::std::result::Result<(), u8> = Self::mutable_of;

    fn key() -> ::std::any::TypeId {
        ::std::any::TypeId::of::<Self>()
    }

    #[allow(unsafe_code)]
    fn shared_of(nonce: u64, visit: &mut dyn FnMut(&T)) -> ::std::result::Result<(), u8> {
        reach(nonce, Self::key(), true, |place| {
            // SAFETY: this key's lend is a `T`, claimed shared.
            visit(unsafe { &*place.cast_const().cast::<T>() })
        })
    }

    #[allow(unsafe_code)]
    fn mutable_of(nonce: u64, visit: &mut dyn FnMut(&mut T)) -> ::std::result::Result<(), u8> {
        reach(nonce, Self::key(), false, |place| {
            // SAFETY: this key's lend is a `T`, claimed exclusively.
            visit(unsafe { &mut *place.cast::<T>() })
        })
    }

    /// A shared lend of a `T`.
    pub fn shared() -> Lend {
        Lend::new(Self::key(), ::std::option::Option::None, &Self::SHARED)
    }

    /// A mutable lend of a `T`.
    pub fn mutable() -> Lend {
        Lend::new(Self::key(), ::std::option::Option::Some(&Self::MUTABLE), &Self::SHARED)
    }
}

/// [`lend_part!`] in a generic marked type's `Parts` impl: `$value`, a
/// `&` of the part, is lent as a `&$ty`, its type with each lifetime
/// `'static`, by [`Accessors`].
#[macro_export]
macro_rules! lend_part_of {
    ($call:expr, $value:expr, $ty:ty) => {{
        #[allow(unused_imports)]
        use $crate::{LendByLoan as _, LendByWire as _};
        // SAFETY: only its lifetimes change, which have no layout; the
        // lend ends before the marked value's borrow does.
        #[allow(unsafe_code)]
        let value: &$ty = unsafe { &*::std::ptr::from_ref($value).cast::<$ty>() };
        let lend = $crate::Accessors::<$ty>::shared();
        // SAFETY: the lend's accessor reads its key's place as `$ty`.
        #[allow(unsafe_code)]
        unsafe {
            (&$crate::hand(value)).lend_part($call, value, lend, ::std::any::type_name::<$ty>())
        };
    }};
}

/// [`lend_part_of!`] of a `&mut`, lent mutably.
#[macro_export]
macro_rules! lend_part_mut_of {
    ($call:expr, $value:expr, $ty:ty) => {{
        #[allow(unused_imports)]
        use $crate::{LendByLoan as _, LendByWire as _};
        // SAFETY: as `lend_part_of!`'s.
        #[allow(unsafe_code)]
        let value: &mut $ty = unsafe { &mut *::std::ptr::from_mut($value).cast::<$ty>() };
        let lend = $crate::Accessors::<$ty>::mutable();
        // SAFETY: the lend's accessors read its key's place as `$ty`.
        #[allow(unsafe_code)]
        unsafe {
            (&$crate::hand(&*value)).lend_part_mut($call, value, lend, ::std::any::type_name::<$ty>())
        };
    }};
}

/// [`lend_whole!`] in a generic marked type's `Parts` impl, by
/// [`Accessors`] of `$ty`, its type with each lifetime `'static`.
#[macro_export]
macro_rules! lend_whole_of {
    ($call:expr, $value:expr, $ty:ty) => {{
        let lend = $crate::Accessors::<$ty>::shared();
        // SAFETY: the lend's accessor reads its key's place as `$ty`,
        // which differs from the value's type only in its lifetimes.
        #[allow(unsafe_code)]
        unsafe {
            $call.lend_whole(::std::ptr::from_ref($value).cast_mut().cast(), lend, ::std::any::type_name::<$ty>())
        };
    }};
}

/// Move `$value`, a native part of a marked value, to the interpreter
/// by `$out` (#1469): by the wire format when its type crosses by it,
/// and else as the native value the interpreter keeps, with what its
/// type has of [`Traits`].
#[macro_export]
macro_rules! give_part {
    ($out:expr, $value:expr) => {{
        #[allow(unused_imports)]
        use $crate::{
            GiveByMove as _, GiveByWire as _, HasClone as _, HasDebug as _, HasDisplay as _,
            HasEqual as _, HasHash as _, HasOrder as _, LacksClone as _, LacksDebug as _,
            LacksDisplay as _, LacksEqual as _, LacksHash as _, LacksOrder as _,
        };
        let value = $value;
        let probe = &$crate::probe(&value);
        let traits = (
            probe.debug_of(),
            probe.display_of(),
            probe.clone_of(),
            probe.equal_of(),
            probe.hash_of(),
            probe.order_of(),
        );
        (&$crate::hand(&value)).give_part($out, value, traits);
    }};
}

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

/// What a crate root's `main` returns, as the exit code it ends with.
pub trait Exit {
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
const WHOLE: u8 = 24;
const TEXT_AT: u8 = 25;
const HASHED_SET: u8 = 26;
const HASHED_MAP: u8 = 27;
const KEPT_TEXT: u8 = 28;
const KEPT: u8 = 29;
const KEPT_LENT: u8 = 30;

/// A value on its way to the interpreter, the native values it gives
/// the interpreter, each taken once, and, when a call asks
/// ([`Call::pinned`]), where each text it copies is, in the order it
/// writes them.
pub struct Out(
    ::std::vec::Vec<u8>,
    ::std::vec::Vec<::std::option::Option<Given>>,
    ::std::option::Option<::std::vec::Vec<*const str>>,
);

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
    /// A `&str` native code gives: copied, with where native code's own
    /// text is ([`Call::kept_text`]).
    pub fn kept_text(&mut self, text: &str) {
        self.byte(KEPT_TEXT);
        self.count(text.as_ptr().expose_provenance());
        self.count(text.len());
        text.put(self);
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
    /// The program value native code held under `handle` ([`Pulled`]),
    /// given back to the interpreter, which lets go of the handle.
    pub fn kept(&mut self, handle: u64) {
        self.byte(KEPT);
        self.0.extend_from_slice(&handle.to_le_bytes());
    }
    /// The program value native code holds under `handle`
    /// ([`Pulled`]), lent where it is: the call shares it with native
    /// code, which pulls on from where the call left it.
    pub fn kept_lent(&mut self, handle: u64) {
        self.byte(KEPT_LENT);
        self.0.extend_from_slice(&handle.to_le_bytes());
    }
}

/// The program's own iterator, which native code holds as std's `Box<dyn
/// Iterator<Item = T>>` by the handle the runner keeps it under, as
/// inboard's `Header` holds the `Box::new(actions.into_iter())` its
/// marked `new` made (#1600). Each `next` asks the interpreter for one
/// item, which `item` reads as `T` crosses, so the program's iterator
/// runs only as far as native code pulls.
pub struct Pulled<T> {
    /// `0` once the value went back to the interpreter ([`take_pulled`]).
    handle: u64,
    item: fn(&mut In<'_>) -> T,
}

/// The address of each [`Pulled`], with its handle, by which a `dyn
/// Iterator` native code lends or gives back is found to be one.
static PULLED: ::std::sync::Mutex<::std::vec::Vec<(usize, u64)>> =
    ::std::sync::Mutex::new(::std::vec::Vec::new());

fn pulled_at<R>(change: impl FnOnce(&mut ::std::vec::Vec<(usize, u64)>) -> R) -> R {
    change(&mut PULLED.lock().unwrap_or_else(::std::sync::PoisonError::into_inner))
}

impl<T: 'static> Pulled<T> {
    fn boxed(handle: u64, item: fn(&mut In<'_>) -> T) -> ::std::boxed::Box<dyn ::std::iter::Iterator<Item = T>> {
        let pulled = ::std::boxed::Box::new(Self { handle, item });
        let at = ::std::ptr::from_ref(&*pulled).addr();
        pulled_at(|pulled| pulled.push((at, handle)));
        pulled
    }
}

impl<T> ::std::iter::Iterator for Pulled<T> {
    type Item = T;

    fn next(&mut self) -> ::std::option::Option<T> {
        let mut natives = ::std::vec::Vec::new();
        // SAFETY: as for `register`.
        let reply = unsafe { __rawr_island_next_v1(self.handle, &mut natives) };
        match reply.first() {
            Some(0) => {
                let mut reply = Reply {
                    bytes: reply,
                    at: 1,
                    path: "Iterator::next",
                    natives: natives.into_iter().map(Some).collect(),
                    places: ::std::vec::Vec::new(),
                    texts: ::std::vec::Vec::new(),
                };
                let item = self.item;
                reply.read(|input| input.option(item))
            }
            Some(_) => failed(&reply),
            None => ::std::panic!("rawr: no answer to `Iterator::next`"),
        }
    }
}

impl<T> Drop for Pulled<T> {
    fn drop(&mut self) {
        let at = ::std::ptr::from_ref(self).addr();
        pulled_at(|pulled| pulled.retain(|(known, _)| *known != at));
        if self.handle != 0 {
            // SAFETY: as for `register`.
            unsafe { __rawr_island_release_v1(self.handle) }
        }
    }
}

/// The handle of the program value `object` holds, when it is a
/// [`Pulled`].
pub fn pulled_handle<T>(object: &(dyn ::std::iter::Iterator<Item = T> + '_)) -> ::std::option::Option<u64> {
    let at = ::std::ptr::from_ref(object).cast::<()>().addr();
    pulled_at(|pulled| pulled.iter().find(|(known, _)| *known == at).map(|(_, handle)| *handle))
}

/// The handle of the program value `object` holds, when it is a
/// [`Pulled`], which native code gives back to the interpreter; else
/// `object` itself, a native value.
pub fn take_pulled<T: 'static>(
    object: ::std::boxed::Box<dyn ::std::iter::Iterator<Item = T>>,
) -> ::std::result::Result<u64, ::std::boxed::Box<dyn ::std::iter::Iterator<Item = T>>> {
    if pulled_handle(&*object).is_none() {
        return Err(object);
    }
    // SAFETY: the box was made by `Pulled::boxed` at this address and
    // is of std's iterator of `T`, which only a `Pulled<T>` is.
    let mut pulled = unsafe { ::std::boxed::Box::from_raw(::std::boxed::Box::into_raw(object).cast::<Pulled<T>>()) };
    Ok(::std::mem::replace(&mut pulled.handle, 0))
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
    /// The `&str` a marked value's field holds: native code's own text
    /// when the program's is its copy, and else the program's text,
    /// which rustc lets be only a `'static` one, as a literal is, kept
    /// once per text for as long as the program runs.
    pub fn kept_text(&mut self) -> &'static str {
        if self.bytes.first() == Some(&KEPT_TEXT) {
            self.byte();
            let address = self.count();
            let length = self.count();
            self.expect(STR, "a string");
            self.text();
            let native = ::std::ptr::slice_from_raw_parts(
                ::std::ptr::with_exposed_provenance::<u8>(address),
                length,
            ) as *const str;
            // SAFETY: native code's own `&str`, which it gave the call
            // and which outlives what native code takes back holding
            // it, as the marked type's lifetime says.
            return unsafe { &*native };
        }
        self.expect(STR, "a string");
        lasting(self.text())
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
    /// A `Box<dyn ::std::iter::Iterator<Item = T>>`: the program's own iterator, which
    /// native code pulls from by its handle ([`Pulled`]), each item read
    /// by `item`, or a native one the interpreter moved (#1600).
    pub fn pulled<T: 'static>(
        &mut self,
        item: fn(&mut In<'_>) -> T,
    ) -> ::std::boxed::Box<dyn ::std::iter::Iterator<Item = T>> {
        if self.bytes.first() != Some(&KEPT) {
            return self.native();
        }
        self.byte();
        let handle = u64::from_le_bytes(self.eight());
        Pulled::boxed(handle, item)
    }
    /// What a call made of the iterator `place` native code lent it:
    /// the program's iterator it pulled from where it is, a new one,
    /// or what a native one became ([`In::changed`]).
    pub fn pulled_into<T: 'static>(
        &mut self,
        place: &mut ::std::boxed::Box<dyn ::std::iter::Iterator<Item = T>>,
        item: fn(&mut In<'_>) -> T,
    ) {
        if self.bytes.first() != Some(&KEPT) {
            if let Some(new) = self.changed(&*place) {
                *place = new;
            }
            return;
        }
        let mut bytes = [0; 8];
        match self.bytes.get(1..9) {
            Some(handle) => bytes.copy_from_slice(handle),
            None => self.fail("it ended early"),
        }
        if pulled_handle(&**place) == Some(u64::from_le_bytes(bytes)) {
            self.take(9);
            return;
        }
        *place = self.pulled(item);
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

/// `text`, kept once per text for as long as the program runs, as a
/// literal is: a `&'static str` the program gives native code.
fn lasting(text: &str) -> &'static str {
    static TEXTS: ::std::sync::Mutex<::std::collections::BTreeSet<&'static str>> =
        ::std::sync::Mutex::new(::std::collections::BTreeSet::new());
    let mut texts = TEXTS.lock().unwrap_or_else(::std::sync::PoisonError::into_inner);
    match texts.get(text) {
        Some(kept) => kept,
        None => {
            let kept: &'static str = ::std::boxed::Box::leak(text.into());
            texts.insert(kept);
            kept
        }
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
/// `value`, which came back at `'static` (`S`), at the lifetimes the
/// signature names (`T`), as an invariant `Text<'a>` can't be coerced.
///
/// # Safety
///
/// `T` is `S` but for its lifetimes, and what `value` borrows lives as
/// long as `T`'s lifetimes say.
pub unsafe fn at_lifetimes<S, T>(value: S) -> T {
    ::std::assert_eq!(::std::mem::size_of::<S>(), ::std::mem::size_of::<T>());
    let value = ::std::mem::ManuallyDrop::new(value);
    // SAFETY: as the caller's; `value` is never dropped, so its bytes
    // are moved into the result once.
    unsafe { ::std::mem::transmute_copy(&*value) }
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
        INT | FLOAT | HANDLE | KEPT | KEPT_LENT => skip(bytes, 8)?,
        CHAR => skip(bytes, 4)?,
        PINNED => skip(bytes, 16)?,
        KEPT_TEXT => {
            skip(bytes, 16)?;
            scan(bytes, found)?;
        }
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
    unsafe fn lend_ref<'c>(&self, call: &mut Call<'c>, value: &'c T, lend: Lend, name: &'static str);
    /// # Safety
    ///
    /// As for [`Call::lend`].
    unsafe fn lend<'c>(&self, call: &mut Call<'c>, value: &'c mut T, lend: Lend, name: &'static str);
    /// The whole new value the call gave the place [`ByLoan::lend`]
    /// lent, `*place = value`, if it gave one (#1139).
    fn back(&self, reply: &mut Reply, place: &mut T)
    where
        T: 'static;
}
impl<T> ByLoan<T> for &Hand<T> {
    unsafe fn lend_ref<'c>(&self, call: &mut Call<'c>, value: &'c T, lend: Lend, name: &'static str) {
        // SAFETY: as the caller's.
        unsafe { call.lend_ref(value, lend, name) }
    }
    unsafe fn lend<'c>(&self, call: &mut Call<'c>, value: &'c mut T, lend: Lend, name: &'static str) {
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
    /// A borrow of the value lent to the interpreter as a marked
    /// function's `&` argument is (#1583): its native parts and the
    /// value whole where they are, its other fields by the wire format.
    fn lend<'c>(&'c self, call: &mut Call<'c>);
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
    let mut out = Out(::std::vec::Vec::new(), ::std::vec::Vec::new(), None);
    value.give(&mut out);
    (out.0, out.1)
}

/// `visit` of a mirror native code lends a program function by `&`,
/// as a crate calls a style closure with its `&Theme` (#1583): the
/// value written as a marked function's `&` argument is
/// ([`Mirror::lend`]), the native values it gives, and what it lends,
/// for the runner to make the program value of while they are lent
/// (`rawr::island::mirror_lent`).
pub fn lent_mirror<T: Mirror, R>(
    value: &T,
    visit: impl FnOnce(
        &[u8],
        &mut [::std::option::Option<Given>],
        &[(::std::any::TypeId, &'static str, *mut (), ::std::option::Option<Accessor>, Accessor)],
    ) -> R,
) -> R {
    let mut call = Call::new("", false);
    value.lend(&mut call);
    visit(&call.values.0, &mut call.values.1, &call.lent)
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
        if let Some(texts) = &mut out.2 {
            texts.push(::std::ptr::from_ref(self));
        }
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
        // A copy of native code's own text is a copy all the same.
        if input.bytes.first() == Some(&KEPT_TEXT) {
            input.byte();
            input.count();
            input.count();
        }
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
/// A set's keys or a map's `(key, value)` entries, as an array: in a
/// B-tree's own order, which is its keys' `Ord`, and for a hashed one
/// after `hashed`, [`HASHED_SET`] or [`HASHED_MAP`], which the runner
/// sorts by key, as the interpreter keeps every keyed collection.
fn put_entries<E: Put>(
    out: &mut Out,
    hashed: ::std::option::Option<u8>,
    entries: impl ExactSizeIterator<Item = E>,
) {
    if let Some(hashed) = hashed {
        out.byte(hashed);
    }
    out.array(entries.len());
    for entry in entries {
        entry.put(out);
    }
}
/// Each entry a set's or a map's array holds ([`put_entries`]).
fn take_entries<E: Take, C: ::std::iter::FromIterator<E>>(input: &mut In<'_>) -> C {
    input.expect(ARRAY, "a set or a map");
    let count = input.count();
    (0..count).map(|_| E::take(input)).collect()
}
impl<T: Put, S> Put for ::std::collections::HashSet<T, S> {
    fn put(&self, out: &mut Out) {
        put_entries(out, Some(HASHED_SET), self.iter());
    }
}
impl<T, S> Take for ::std::collections::HashSet<T, S>
where
    T: Take + ::std::cmp::Eq + ::std::hash::Hash,
    S: ::std::hash::BuildHasher + ::std::default::Default,
{
    fn take(input: &mut In<'_>) -> Self {
        take_entries::<T, Self>(input)
    }
}
impl<T: Put> Put for ::std::collections::BTreeSet<T> {
    fn put(&self, out: &mut Out) {
        put_entries(out, None, self.iter());
    }
}
impl<T: Take + ::std::cmp::Ord> Take for ::std::collections::BTreeSet<T> {
    fn take(input: &mut In<'_>) -> Self {
        take_entries::<T, Self>(input)
    }
}
impl<K: Put, V: Put, S> Put for ::std::collections::HashMap<K, V, S> {
    fn put(&self, out: &mut Out) {
        put_entries(out, Some(HASHED_MAP), self.iter());
    }
}
impl<K, V, S> Take for ::std::collections::HashMap<K, V, S>
where
    K: Take + ::std::cmp::Eq + ::std::hash::Hash,
    V: Take,
    S: ::std::hash::BuildHasher + ::std::default::Default,
{
    fn take(input: &mut In<'_>) -> Self {
        take_entries::<(K, V), Self>(input)
    }
}
impl<K: Put, V: Put> Put for ::std::collections::BTreeMap<K, V> {
    fn put(&self, out: &mut Out) {
        put_entries(out, None, self.iter());
    }
}
impl<K: Take + ::std::cmp::Ord, V: Take> Take for ::std::collections::BTreeMap<K, V> {
    fn take(input: &mut In<'_>) -> Self {
        take_entries::<(K, V), Self>(input)
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

/// `value` drops, or moves into the interpreter, where it is (#1492):
/// rustc has ended every borrow of it, so the runner lets go of what
/// results anchored on it borrowed.
pub fn ended<T: ?Sized>(value: &T) {
    let [address, ty] = anchor_of(value);
    // SAFETY: the runner links `rawr`, which defines the function with
    // this exact signature.
    unsafe { __rawr_island_unanchor_v1(address, ty) }
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
            values: Out(::std::vec::Vec::new(), ::std::vec::Vec::new(), None),
            lent: ::std::vec::Vec::new(),
            borrows: ::std::marker::PhantomData,
        }
    }
    /// An argument lent to the interpreter for the call (D558.3), as
    /// ratatui's `&mut Frame<'_>` is. The call runs on this thread
    /// while it is lent, so it needn't be `Send`.
    ///
    /// # Safety
    ///
    /// `lend`'s accessors read its key's place as a `T`, and no other
    /// lend uses its key. `name` is `T`'s name.
    pub unsafe fn lend<T>(&mut self, value: &'c mut T, lend: Lend, name: &'static str) {
        debug_assert!(lend.mutable.is_some());
        self.lent_value(::std::ptr::from_mut(value).cast(), lend, name);
    }
    /// [`Call::lend`] of a shared borrow, which needn't be `Sync`.
    ///
    /// # Safety
    ///
    /// As for [`Call::lend`], and `lend` has no mutable accessor.
    pub unsafe fn lend_ref<T>(&mut self, value: &'c T, lend: Lend, name: &'static str) {
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
    /// A borrow of a marked value lent whole for the call where it is,
    /// beside its parts (#1484): the value written next is the one it
    /// lends.
    ///
    /// # Safety
    ///
    /// As for [`Call::lend_part`], `lend` having no mutable accessor.
    pub unsafe fn lend_whole(&mut self, place: *mut (), lend: Lend, name: &'static str) {
        debug_assert!(lend.mutable.is_none());
        self.values.byte(WHOLE);
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
        self.release(&*value);
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
    /// An argument that holds native parts inside `Option`s, `Vec`s,
    /// `Box`es and tuples, which `give` writes part by part, each part
    /// moved to the interpreter (#1224).
    pub fn give_parts(&mut self, give: impl FnOnce(&mut Out)) {
        give(&mut self.values);
        self.count += 1;
    }
    /// A `&[T]` argument lent for the call where it is (#1292): the
    /// lend's place is `value`, the trampoline's own reference.
    ///
    /// # Safety
    ///
    /// As for [`Call::lend_ref`], `lend`'s accessor reading its key's
    /// place as a `&[T]`.
    pub unsafe fn lend_slice<T>(&mut self, value: &'c &'c [T], lend: Lend, name: &'static str) {
        debug_assert!(lend.mutable.is_none());
        self.lent_value(::std::ptr::from_ref(value).cast_mut().cast(), lend, name);
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
    /// A `&str` argument, as `label` of `subtitle<'a>(label: &'a str)
    /// -> Text<'a>`: copied, and where native code's own text is, so
    /// what the call makes that borrows it borrows that text, which
    /// native code keeps borrowed as long as rustc says.
    pub fn kept_text(&mut self, value: &'c str) {
        self.values.kept_text(value);
        self.count += 1;
    }
    /// A stand-in's receiver: the handle of the program value it holds,
    /// which the call takes as the impl's receiver.
    pub fn object(&mut self, object: &Object) {
        self.values.byte(HANDLE);
        self.values.0.extend_from_slice(&object.0.to_le_bytes());
        self.count += 1;
    }
    /// The call's result is a `&str` that borrows its receiver: a
    /// stand-in's comes back as the program's own text, pinned with the
    /// value ([`Object::pinned`]), and a marked value's as text it
    /// holds ([`Reply::pinned`]), so where each text the call copies
    /// is gets noted.
    pub fn pinned(&mut self) {
        self.pinned = true;
        self.values.2 = Some(::std::vec::Vec::new());
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
    /// Native code ended every borrow of `value`, which the call takes
    /// `&mut` or by value: the runner lets go of what results anchored
    /// on it borrowed (#1473).
    pub fn release<T: ?Sized>(&mut self, value: &T) {
        self.releases.push(anchor_of(value));
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
        self.release(value);
        self.mutable |= 1 << self.count;
        self.argument(value);
    }
    pub fn send(mut self) -> Reply {
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
                texts: self.values.2.take().unwrap_or_default(),
            },
            Some(_) => failed(&reply),
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
    /// Where each text the call copied is, in the order it wrote them
    /// ([`Call::pinned`]).
    texts: ::std::vec::Vec<*const str>,
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
    /// A `&str` the call's receiver holds (#1468): text in native
    /// code's own value, in a text the call copied from it, as `&self.0`
    /// of a marked `Id(String)` is, or the program's own text, pinned
    /// for as long as the runner lives.
    ///
    /// # Safety
    ///
    /// The receiver stays borrowed for `'r`, and the call copied its
    /// texts from it before any other.
    pub unsafe fn pinned<'r>(&mut self) -> &'r str {
        let texts = ::std::mem::take(&mut self.texts);
        let found = self.read(|input| match input.byte() {
            PINNED => Err((input.count(), input.count())),
            TEXT_AT => {
                let (index, start, length) = (input.count(), input.count(), input.count());
                texts
                    .get(index)
                    // SAFETY: the receiver, which holds the text, is
                    // borrowed for `'r`.
                    .map(|text| unsafe { &**text })
                    .and_then(|text| text.get(start..start.checked_add(length)?))
                    .map(Ok)
                    .unwrap_or_else(|| input.fail("a text the call didn't copy"))
            }
            _ => input.fail("expected a `&str`"),
        });
        match found {
            Ok(text) => text,
            // SAFETY: the runner gave valid UTF-8 that lives at least as
            // long as the receiver is borrowed, which is `'r`.
            Err((address, length)) => unsafe {
                ::std::str::from_utf8_unchecked(::std::slice::from_raw_parts(
                    ::std::ptr::with_exposed_provenance::<u8>(address),
                    length,
                ))
            },
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
"#;

/// The part of the island's support that depends on the crate, appended to
/// its root as [`MODULE`]: the package's items, its stand-ins' makers, its
/// instance table and its `main`.
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
const SUPPORT: &str = r#"
/// Generated by Rawr: how the native code of this crate reaches the marked
/// modules, which Rawr interprets. `cargo build` never sees this module.
#[doc(hidden)]
#[allow(unsafe_code, dead_code, unused_imports, clippy::all)]
pub mod __rawr_island {
    pub use ::__rawr_island_support::*;

    /// Each stand-in's maker, by its type's path from the crate's root.
    static MAKERS: &[(&str, Maker)] = &__MAKER_LIST__;

    /// Each instance native code calls a generic function at, as the
    /// record of the crate's last check found them (D869): the function's
    /// path, the name of the tuple of its types and its entry there.
    static INSTANCES: &[(&str, fn() -> &'static str, Instance)] = &__INSTANCE_LIST__;

    /// The call of the marked function at `path`, the crate's [`MAKERS`]
    /// registered first, once.
    #[inline]
    pub fn call<'c>(path: &'static str, owned: bool) -> Call<'c> {
        static ONCE: ::std::sync::Once = ::std::sync::Once::new();
        ONCE.call_once(|| register(MAKERS));
        Call::new(path, owned)
    }

    /// Run the instance of the generic function at `path` whose types'
    /// tuple is named `name`, with the arguments at `arguments` and the
    /// result's place at `out`.
    ///
    /// # Safety
    ///
    /// `arguments` holds the function's arguments, as a tuple in order,
    /// which the instance moves out; `out` is a place for its result.
    pub unsafe fn instance(path: &'static str, name: &'static str, arguments: *mut (), out: *mut ()) {
        for (instance, named, entry) in INSTANCES {
            if *instance == path && named() == name {
                // SAFETY: as the caller's.
                return unsafe { entry(arguments, out) };
            }
        }
        ::std::panic!(
            "rawr: native code called `{path}` at `{name}`, which the last check didn't see;              this is a rawr bug"
        )
    }

    /// Start the program: the runner calls the native `main` once this
    /// returns, on a thread of its own.
    pub fn main() {
        start(entry)
    }

    fn entry() -> i32 {
        ENTRY
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn generate(text: &str) -> Shadow {
        generate_called(text, None)
    }

    /// [`generate`], with the bodies of the functions `called` names.
    fn generate_called(text: &str, called: Option<&BTreeSet<String>>) -> Shadow {
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
            &[],
            called,
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

    /// Each trampoline and each function of a crossing type's glue is
    /// `#[inline]`, so rustc builds only those native code or the runner
    /// calls, as a binary's build does (#1801). A function that says how it
    /// inlines keeps its own word alone.
    #[test]
    fn trampolines_and_glue_are_inline() {
        let shadow = generate(
            "// rawr\npub struct Ball {\n    pub x: f64,\n}\n\
             pub fn fall(ball: &mut Ball) {}\n\
             #[inline(never)]\npub fn rise(ball: &mut Ball) {}\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        for expected in [
            "#[inline]\npub fn fall(",
            "impl crate::__rawr_island::Put for Ball {\n    #[inline]\n    fn put(",
            "impl crate::__rawr_island::Take for Ball {\n    #[inline]\n    fn take(",
            "impl crate::__rawr_island::Mirror for Ball {\n    #[inline]\n    fn give(",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        let rise = text.find("pub fn rise(").expect("`rise` has a trampoline");
        assert!(!text[..rise].ends_with("#[inline]\n"), "{text}");
    }

    /// A copy's island module holds only what depends on its crate, its
    /// makers, its instance table and its `main`, over the support package
    /// it re-exports, which names no crate's items, so one build of the
    /// package serves every crate and every save (#1808). The package's
    /// macros are exported, as the copy names them through the module.
    #[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
    #[test]
    fn the_island_module_re_exports_a_package_that_names_no_crate() {
        let objects = [(
            "page::Page".to_owned(),
            "crate::page::__rawr_object_0".to_owned(),
        )];
        let module = support(true, &objects, &[]);
        assert!(
            module.contains("pub use ::__rawr_island_support::*;"),
            "{module}"
        );
        assert!(
            module.contains("[(\"page::Page\", crate::page::__rawr_object_0 as Maker)]"),
            "{module}"
        );
        assert!(module.contains("Exit::code(super::main())"), "{module}");
        let [(_, manifest), (_, library)] = package();
        assert!(
            manifest.contains("name = \"__rawr_island_support\"\npath = \"src/lib.rs\""),
            "{manifest}"
        );
        for named in [
            " crate::",
            "super::",
            "__MAKER_LIST__",
            "__INSTANCE_LIST__",
            "ENTRY",
        ] {
            assert!(!library.contains(named), "{named}");
        }
        for exported in ["object", "give", "part", "lend_part", "give_part"] {
            assert!(
                library.contains(&format!("#[macro_export]\nmacro_rules! {exported} {{")),
                "{exported}"
            );
        }
    }

    /// With a record of what native code names, only a named function's
    /// trampoline has its body: every other one is its signature over a
    /// body that never runs, so native code type-checks as it did, and a
    /// trait's function, called unnamed, and a generic one, whose instance
    /// entries the table may name, keep theirs. The shape is the same with
    /// any record (#1807).
    #[test]
    fn only_a_function_native_code_names_has_its_trampoline_s_body() {
        let text = "// rawr\n#[derive(Clone, Debug)]\npub struct Ball {\n    pub x: f64,\n}\n\
                    impl Ball {\n    pub fn new(x: f64) -> Ball { Ball { x } }\n    \
                    pub fn height(&self) -> f64 { self.x }\n}\n\
                    impl Default for Ball {\n    fn default() -> Ball { Ball { x: 0.0 } }\n}\n\
                    pub fn fall(ball: &mut Ball) {}\n\
                    pub fn rise(ball: &mut Ball, by: f64) -> f64 { by }\n\
                    pub fn generic<T: Into<String>>(value: T) {}\n";
        let called = ["physics::fall", "physics::Ball::new"]
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let all = generate(text);
        let some = generate_called(text, Some(&called));
        assert!(some.refused.is_empty(), "{:?}", some.refused);
        assert_eq!(all.shape, some.shape);
        let [(_, text)] = some.files.as_slice() else {
            panic!("{:?}", some.files);
        };
        for expected in [
            "pub fn fall(__a0: &mut Ball) {\n    let mut __call = crate::__rawr_island::call(",
            "pub fn new(__a0: f64) -> Ball {\n        let mut __call = crate::__rawr_island::call(",
            "#[inline]\npub fn rise(__a0: &mut Ball, __a1: f64) -> f64 {\n    \
             crate::__rawr_island::uncalled(\"physics::rise\")\n}\n",
            "    pub fn height(&self) -> f64 {\n        \
             crate::__rawr_island::uncalled(\"physics::Ball::height\")\n    }\n",
            "fn default() -> Ball {\n        let mut __call = crate::__rawr_island::call(",
            "crate::__rawr_island::instance(\n            \"physics::generic\"",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        let [(_, every)] = all.files.as_slice() else {
            panic!("{:?}", all.files);
        };
        assert!(!every.contains("uncalled"), "{every}");
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
                 let mut __call = crate::__rawr_island::call(\"physics::Ball::advance\", true);\n        \
                 __call.unit();\n        __call.mutable(&*self);\n        __call.argument(&__a0);\n"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "pub fn render(__a0: &Ball, __a1: usize, __a2: &str) -> String {\n    \
                 let mut __call = crate::__rawr_island::call(\"physics::render\", false);\n    \
                 __call.argument(__a0);\n    __call.argument(&__a1);\n    __call.kept_text(__a2);\n    \
                 let mut __reply = __call.send();\n    let __value: String = __reply.take();\n    __value\n}\n"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "pub fn scale(__a0: Scale) -> f64 {\n    \
                 let mut __call = crate::__rawr_island::call(\"physics::scale\", false);\n    \
                 crate::__rawr_island::give!(__call, __a0);\n"
            ),
            "{text}"
        );
        let paths = shadow.shape.entries.keys().cloned().collect::<Vec<_>>();
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

    /// A `&self` method of a marked type copied whole, as inboard's
    /// `Id::as_str`, gives a `&str` in native code's own value: the call
    /// notes where each text it copies is, and the reply names one.
    #[test]
    fn a_copied_marked_value_gives_text_it_holds_in_place() {
        let shadow = generate(
            "// rawr\npub struct Id(String);\n\
             impl Id {\n    pub fn as_str(&self) -> &str { &self.0 }\n}\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        for expected in [
            "    __call.pinned();\n",
            "    __call.argument(self);\n",
            "let __value: &str = unsafe { __reply.pinned() };",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        assert!(!text.contains("panic!"), "{text}");
    }

    /// A keyed collection of values that cross, as inboard's `read_ids:
    /// HashSet<Id>`, crosses by copy as a `Vec` does, so the interpreter
    /// keeps its own collection, never a native part it can't read.
    #[test]
    fn a_keyed_collection_of_crossing_values_crosses_by_copy() {
        let shadow = generate(
            "// rawr\nuse std::collections::{BTreeMap, HashSet};\n\
             #[derive(PartialEq, Eq, Hash)]\npub struct Id(String);\n\
             pub struct Read {\n    read_ids: HashSet<Id>,\n    counts: BTreeMap<String, u32>,\n}\n\
             impl Read {\n    pub fn is_read(&self, id: &Id) -> bool { self.read_ids.contains(id) }\n    \
             pub fn mark_read(&mut self, id: &Id) {}\n}\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        for expected in [
            "    __call.argument(self);\n",
            "    __call.mutable(&*self);\n",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        assert!(
            !text.contains("panic!") && !text.contains("Parts for Read"),
            "{text}"
        );
    }

    /// A borrow of a type the marked crate doesn't declare, as ratatui's
    /// `&mut Frame<'_>`, is lent for the call with accessors of its own
    /// (D558.3); a `&` one is lent shared.
    #[test]
    fn an_impl_trait_argument_crosses_as_an_owned_value_made_before_the_call() {
        let shadow = generate(
            "// rawr\n#[derive(Debug, Clone)]\npub struct Id(String);\n\
             impl Id {\n    pub fn new(value: impl Into<String>) -> Self { Self(value.into()) }\n}\n\
             pub fn mark(ids: impl IntoIterator<Item = Id>, label: impl AsRef<str>) {}\n\
             pub fn each(visit: impl Fn(i64)) {}\n\
             pub fn generic<T: Into<String>>(value: T) {}\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        for expected in [
            "    pub fn new(__a0: impl Into<String>) -> Self {\n        \
             let __a0: String = { let __value = __a0; ::std::convert::Into::<String>::into(__value) };\n        \
             let mut __call = crate::__rawr_island::call(\"physics::Id::new\", true);\n        \
             __call.argument(&__a0);\n",
            "pub fn mark(__a0: impl IntoIterator<Item = Id>, __a1: impl AsRef<str>) {\n    \
             let __a0: ::std::vec::Vec<Id> = { let __value = __a0; \
             ::std::iter::Iterator::collect::<::std::vec::Vec<Id>>(\
             ::std::iter::IntoIterator::into_iter(__value)) };\n    \
             let __a1: ::std::string::String = { let __value = __a1; \
             ::std::string::String::from(::std::convert::AsRef::<str>::as_ref(&__value)) };\n    \
             let mut __call = crate::__rawr_island::call(\"physics::mark\", false);\n",
            "can't call `physics::each` yet: parameter `visit`: `impl Fn(i64)` can't cross yet",
            // A named generic hands its call to the instance its types'
            // names select (D869).
            "        crate::__rawr_island::instance(\n            \"physics::generic\",\n            \
             ::std::any::type_name::<(T,)>(),\n",
            "pub(crate) unsafe fn __rawr_instance_generic<T: Into<String>>\
             (__arguments: *mut (), __out: *mut ())\nwhere\n    T: 'static, {\n",
            "call(crate::__rawr_island::keyed(\"physics::generic\", \
             ::std::any::type_name::<(T,)>()), false);",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        let concrete = &text[..text.find("pub fn generic").unwrap_or(text.len())];
        assert!(!concrete.contains("give!(__call, __a0)"), "{text}");
    }

    /// A marked trait is declared for native code with its impls, as
    /// inboard's `sidebar::IntoContent`, and a marked function taking an
    /// `impl` of it is given the argument by the impl that applies: the
    /// marked type's as itself, the blanket one's over `IntoIterator` as a
    /// `Vec` of its items.
    #[test]
    fn an_impl_of_a_marked_trait_crosses_by_the_impl_that_applies() {
        let shadow = generate(
            "// rawr\n#[derive(Debug, Default)]\npub struct Lines { lines: Vec<String> }\n\
             pub trait IntoLines {\n    fn into_lines(self) -> Lines;\n    \
             fn count(&self) -> usize { 0 }\n}\n\
             impl IntoLines for Lines {\n    fn into_lines(self) -> Lines { self }\n}\n\
             impl<T> IntoLines for T\nwhere\n    T: IntoIterator<Item = String>,\n{\n    \
             fn into_lines(self) -> Lines { Lines { lines: self.into_iter().collect() } }\n}\n\
             pub fn lines(lines: impl IntoLines) -> Lines { lines.into_lines() }\n",
        );
        assert!(shadow.refused.is_empty(), "{:?}", shadow.refused);
        let [(_, text)] = shadow.files.as_slice() else {
            panic!("{:?}", shadow.files);
        };
        for expected in [
            "pub trait IntoLines {\n    fn into_lines(self) -> Lines;\n    \
             fn count(&self) -> usize {\n        ::core::panic!(\"rawr: native code can't call \
             `physics::IntoLines::count` yet: a trait's provided function can't cross yet\")\n    }\n    \
             #[doc(hidden)]\n    fn __rawr_give(self, __call: &mut crate::__rawr_island::Call<'_>)\n",
            "impl IntoLines for Lines {\n",
            "    fn __rawr_give(self, __call: &mut crate::__rawr_island::Call<'_>) {\n        \
             let __value = self;\n        __call.release(&__value);\n        \
             __call.argument(&__value);\n    }\n",
            "impl<T> IntoLines for T\nwhere\n    T: IntoIterator<Item = String>, {\n",
            "can't call `::T::IntoLines::into_lines` yet: a function of an impl for every \
             type of a bound can't cross yet",
            "        let __value: ::std::vec::Vec<String> = { let __value = self; \
             ::std::iter::Iterator::collect::<::std::vec::Vec<String>>(\
             ::std::iter::IntoIterator::into_iter(__value)) };\n        \
             __call.argument(&__value);\n",
            "pub fn lines(__a0: impl IntoLines) -> Lines {\n    \
             let mut __call = crate::__rawr_island::call(\"physics::lines\", false);\n    \
             IntoLines::__rawr_give(__a0, &mut __call);\n",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
    }

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
            "const SHARED: fn(u64, &mut dyn FnMut(&Frame<'_>)) -> ::std::result::Result<(), u8> = shared;",
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
        // A doc comment, a lint and a signature over more lines build the
        // same native code (#1646).
        let inert = generate(
            "// rawr\n/// A ball.\n#[allow(dead_code)]\npub struct Ball {\n    \
             #[doc = \"Across.\"] pub x: f64\n}\nconst G: f64 = 9.8;\n\n\
             #[inline]\n#[must_use = \"a fall\"]\npub fn fall(\n    ball: &mut Ball // the ball\n) {\n    \
             ball.x -= G;\n}\n",
        );
        assert_eq!(before.shape.change(&inert.shape), None);
    }

    /// Native code built without a derive's impl doesn't call it: a new
    /// derive keeps the shape, and one that goes away, or that changes
    /// what else is generated for its type, names the derive (#1737).
    #[test]
    fn a_new_derive_keeps_the_shape_and_a_lost_one_names_the_derive() {
        let ball = |derives: &str| {
            generate(&format!(
                "// rawr\n{derives}pub struct Ball {{ pub x: f64 }}\n\
                 pub fn fall(ball: &mut Ball) {{ ball.x -= 1.0; }}\n"
            ))
        };
        let before = ball("");
        let derived = ball("#[derive(Clone)]\n");
        assert_ne!(before.files, derived.files);
        assert_eq!(before.shape.change(&derived.shape), None);
        assert_eq!(
            before.shape.derived(&derived.shape).as_deref(),
            Some("`physics::Ball` derives Clone now")
        );
        let more = ball("#[derive(Debug, Clone)]\n#[derive(PartialEq)]\n");
        assert_eq!(derived.shape.change(&more.shape), None);
        assert_eq!(
            more.shape.change(&before.shape).as_deref(),
            Some("`physics::Ball` no longer derives Clone, Debug and PartialEq")
        );
        assert_eq!(
            more.shape
                .change(&ball("#[derive(Hash)]\n").shape)
                .as_deref(),
            Some(
                "`physics::Ball` derives Hash now and no longer derives Clone, Debug and PartialEq"
            )
        );
        // A crossing struct with no field native code sees tells the runner
        // when it drops (#1492), unless it is `Copy`.
        let id = |derives: &str| {
            generate(&format!(
                "// rawr\n{derives}pub struct Id {{ value: i64 }}\n\
                 pub fn id() -> Id {{ Id {{ value: 1 }} }}\n"
            ))
        };
        assert_eq!(
            id("#[derive(Clone)]\n")
                .shape
                .change(&id("#[derive(Clone, Copy)]\n").shape)
                .as_deref(),
            Some("`physics::Id` derives Copy now")
        );
        assert_eq!(id("").shape.change(&id("#[derive(Debug)]\n").shape), None);
    }

    /// Native code built without an item never names it: a new item keeps
    /// the shape, unless native code reaches it unnamed, as a `Drop` impl
    /// or a stand-in, and the items that go away are named (#1741).
    #[test]
    fn a_new_item_keeps_the_shape_and_the_lost_ones_are_named() {
        let ball = |more: &str| {
            generate(&format!(
                "// rawr\nuse crate::page;\npub struct Ball {{ pub x: f64 }}\n\
                 pub fn fall(ball: &mut Ball) {{ ball.x -= 1.0; }}\n{more}"
            ))
        };
        let before = ball("");
        let added = ball(
            "#[derive(Debug)]\npub struct Inset { pub radius: f32 }\n\
             pub fn inset() -> Inset { Inset { radius: 1.0 } }\n\
             pub const RADIUS: f32 = 1.0;\npub trait Rounded { fn round(&self) -> f32; }\n\
             impl Ball { pub fn rise(&mut self) { self.x += 1.0; } }\n\
             impl Default for Ball { fn default() -> Ball { Ball { x: 0.0 } } }\n",
        );
        assert_ne!(before.files, added.files);
        assert_eq!(before.shape.change(&added.shape), None);
        assert_eq!(
            before.shape.added(&added.shape).as_deref(),
            Some("native code can now see `physics::Ball::Default::default`")
        );
        assert_eq!(
            added.shape.change(&before.shape).as_deref(),
            Some(
                "native code can no longer see `physics::Ball::Default::default`, \
                 `physics::Ball::rise`, `physics::Inset`, `physics::RADIUS`, \
                 `physics::Rounded` and `physics::inset`"
            )
        );
        // A type's members go with it.
        let point = ball("pub struct Point { pub y: f64 }\nimpl Point { pub fn up(&self) {} }\n");
        assert_eq!(
            point.shape.change(&before.shape).as_deref(),
            Some("native code can no longer see `physics::Point`")
        );
        let dropped = ball("impl Drop for Ball { fn drop(&mut self) {} }\n");
        assert_eq!(
            before.shape.change(&dropped.shape).as_deref(),
            Some("native code can now see `physics::Ball::Drop::drop`")
        );
        let held =
            ball("impl page::Page for Ball { fn title(&self) -> String { String::new() } }\n");
        assert_eq!(
            before.shape.change(&held.shape).as_deref(),
            Some("native code can now see `physics::Ball as dyn page::Page`")
        );
    }

    /// With the uses the running build's check recorded, a change needs a
    /// new native build only where it reaches one (D935.3): an item native
    /// code doesn't name goes away in place, one it names or a type it
    /// holds builds again and is named, and a new item builds again only
    /// where native code looked its name up (#1744).
    #[cfg(feature = "rust-import")]
    #[test]
    fn a_change_builds_again_only_where_it_reaches_a_recorded_use() {
        let ball = |more: &str| {
            generate(&format!(
                "// rawr\nuse crate::page;\npub struct Ball {{ pub x: f64 }}\n\
                 impl Ball {{ pub fn rise(&mut self) {{ self.x += 1.0; }} }}\n\
                 pub fn fall(ball: &mut Ball) {{ ball.x -= 1.0; }}\n{more}"
            ))
        };
        let mut recorder = rawr_import::uses::Recorder::default();
        recorder.name("physics::Ball".to_owned());
        recorder.hold("physics::Ball".to_owned());
        recorder.name("physics::fall".to_owned());
        recorder.probe("physics::Ball", "rise");
        recorder.probe("physics::Ball", "describe");
        recorder.scope("physics", "Result");
        recorder.trait_("Clone".to_owned());
        let uses = recorder.finish();
        let before = ball("pub fn inset() -> f32 { 1.0 }\n");
        // Gone, unnamed: in place. Gone, named: again, naming it.
        let unnamed = ball("");
        assert_eq!(before.shape.used_change(&unnamed.shape, &uses), None);
        assert!(before.shape.change(&unnamed.shape).is_some());
        let named = generate(
            "// rawr\nuse crate::page;\npub struct Ball { pub x: f64 }\n\
             impl Ball { pub fn rise(&mut self) { self.x += 1.0; } }\n\
             pub fn inset() -> f32 { 1.0 }\n",
        );
        assert_eq!(
            before.shape.used_change(&named.shape, &uses).as_deref(),
            Some("native code can no longer see `physics::fall`")
        );
        // A held type's fields, and a named function's signature.
        let fields = generate(
            "// rawr\nuse crate::page;\npub struct Ball { pub x: f64, pub y: f64 }\n\
             impl Ball { pub fn rise(&mut self) { self.x += 1.0; } }\n\
             pub fn fall(ball: &mut Ball) { ball.x -= 1.0; }\npub fn inset() -> f32 { 1.0 }\n",
        );
        assert_eq!(
            before.shape.used_change(&fields.shape, &uses).as_deref(),
            Some("`physics::Ball` changed its fields")
        );
        let signature = ball("pub fn inset() -> f64 { 1.0 }\n");
        assert_eq!(before.shape.used_change(&signature.shape, &uses), None);
        // A new name where native code looked: a member it probed, a name a
        // glob of the module resolved, a trait it uses for a held type.
        let probed = ball("impl Ball { pub fn describe(&self) {} }\n");
        assert_eq!(
            unnamed.shape.used_change(&probed.shape, &uses).as_deref(),
            Some("native code now finds `physics::Ball::describe` where it looked up its name")
        );
        let shadowing = ball("pub type Result = std::result::Result<(), ()>;\n");
        assert_eq!(
            unnamed
                .shape
                .used_change(&shadowing.shape, &uses)
                .as_deref(),
            Some("native code now finds `physics::Result` through a glob import")
        );
        let elsewhere = ball("pub type Outcome = std::result::Result<(), ()>;\n");
        assert_eq!(unnamed.shape.used_change(&elsewhere.shape, &uses), None);
        let defaulted =
            ball("impl Default for Ball { fn default() -> Ball { Ball { x: 0.0 } } }\n");
        assert_eq!(unnamed.shape.used_change(&defaulted.shape, &uses), None);
        let cloned =
            ball("impl Clone for Ball { fn clone(&self) -> Ball { Ball { x: self.x } } }\n");
        assert_eq!(
            unnamed.shape.used_change(&cloned.shape, &uses).as_deref(),
            Some("native code may now select `physics::Ball::Clone::clone`")
        );
        // A derive: a lost one on a held type builds again, a new one only
        // for a trait native code uses.
        let derived = |derives: &str| {
            generate(&format!(
                "// rawr\n{derives}pub struct Ball {{ pub x: f64 }}\n\
                 pub fn fall(ball: &mut Ball) {{ ball.x -= 1.0; }}\n"
            ))
        };
        assert_eq!(
            derived("")
                .shape
                .used_change(&derived("#[derive(Debug)]\n").shape, &uses),
            None
        );
        assert_eq!(
            derived("")
                .shape
                .used_change(&derived("#[derive(Clone)]\n").shape, &uses)
                .as_deref(),
            Some("`physics::Ball` derives Clone now")
        );
        assert_eq!(
            derived("#[derive(Debug)]\n")
                .shape
                .used_change(&derived("").shape, &uses)
                .as_deref(),
            Some("`physics::Ball` no longer derives Debug")
        );
        // A stand-in, reached through a trait object native code holds.
        let held =
            ball("impl page::Page for Ball { fn title(&self) -> String { String::new() } }\n");
        assert_eq!(
            unnamed.shape.used_change(&held.shape, &uses).as_deref(),
            Some("native code can now see `physics::Ball as dyn page::Page`")
        );
    }

    #[test]
    fn what_rustc_builds_of_a_text_keeps_its_words_and_literals() {
        assert_eq!(
            built("pub fn f<'a>(\n    x: &'a str, // x\n) -> &'a str /* y */ {"),
            "pub fn f<'a>(x:&'a str,)->&'a str{"
        );
        assert_eq!(
            built("#[doc = \"a ] b\"]\n#[rustfmt::skip] #[derive(Clone)] struct S;"),
            "#[derive(Clone)]struct S;"
        );
        assert_eq!(
            built("f(\"a  //b\", '\\'', r#\"c \"d\"#)"),
            "f(\"a  //b\",'\\'',r#\"c \"d\"#)"
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
    /// and not `&self` when the result names another input's lifetime,
    /// whose native input is the anchor then (#1491). A `&mut` of a marked
    /// value lets go of what was borrowed of it. A native input lent at the
    /// anchor's lifetime is kept as long (#1413).
    #[test]
    fn a_result_that_borrows_a_marked_input_is_anchored_on_it() {
        let shadow = generate(
            "// rawr\nuse crate::{ui, Inbox, Update};\n\
             pub struct State {\n    content: ui::Content,\n    title: String,\n}\n\
             impl State {\n    \
             pub fn view<'a>(&'a self, inbox: &'a Inbox, updates: &'a [Update], \
             grid: Option<ui::Element<'a, u32>>) -> ui::Element<'a, u32> { todo!() }\n    \
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
        assert!(
            body("show").contains("    __call.anchor(__a0);\n"),
            "{text}"
        );
        assert!(!body("edit").contains("__call.anchor("), "{text}");
        assert_eq!(lifetimes("Element<'a, &'static str, '_>"), ["a", "_"]);
        // The native `&'a Inbox` the result may borrow is kept as long as
        // it (#1413). With no marked input at the result's lifetime, as
        // `other`'s, the native one is the anchor, kept too (#1491).
        assert!(
            body("view").contains("    let __a0_lends = __call.lends();\n")
                && body("view").contains("    __call.keep_lends(__a0_lends);\n"),
            "{text}"
        );
        assert!(
            body("other").contains("    __call.anchor(__a0);\n")
                && body("other").contains("    __call.keep_lends(__a0_lends);\n"),
            "{text}"
        );
        // So are the native parts of `self` it is anchored on, and the
        // lent slice of native values (#1292). The `Option` of a native
        // value of borrows moves part by part, for the call alone (#1224).
        assert!(
            body("view").contains(
                "let self_lends = __call.lends();\n        __call.lent(self);\n        \
                 __call.keep_lends(self_lends);\n"
            ),
            "{text}"
        );
        assert!(
            body("view").contains("__call.lend_slice(&__a1, __a1_lend, ")
                && body("view").contains("visit: &mut dyn FnMut(&[Update])")
                && body("view").contains("    __call.keep_lends(__a1_lends);\n"),
            "{text}"
        );
        assert!(
            body("view").contains(
                "::std::mem::transmute::<Option<ui::Element<'_, u32>>, \
                 Option<ui::Element<'static, u32>>>(__a2) };\n        __call.for_call();\n        \
                 __call.give_parts(|out| { let __value = __a2; match __value { \
                 ::std::option::Option::None => out.none(), \
                 ::std::option::Option::Some(__p0) => { out.some(); { // SAFETY: only its \
                 lifetimes change, which have no layout; the interpreter keeps it no longer \
                 than rustc lets the marked value.\n    #[allow(unsafe_code)] let __p1 = unsafe { \
                 crate::__rawr_island::at_lifetimes::<_, ui::Element<'static, u32>>(__p0) }; \
                 crate::__rawr_island::give_part!(out, __p1); } } } });\n"
            ),
            "{text}"
        );
    }

    /// A call anchored on nothing whose result borrows nothing keeps its
    /// shared lends for the call alone when it is lent a native value
    /// (#1491, #1503), and one whose result may borrow by elision doesn't. A `&mut` of a native value and a
    /// marked value moved in let go of what was borrowed of them, and a
    /// marked struct native code can't take apart says when it drops
    /// (#1492): moved in, its fields are read out of a value that never
    /// drops.
    #[test]
    fn lends_are_kept_for_the_call_and_ended_borrows_let_go() {
        let shadow = generate(
            "// rawr\nuse crate::{ui, Inbox, Update};\n\
             pub struct State {\n    content: ui::Content,\n    title: String,\n}\n\
             pub struct Open {\n    pub content: ui::Content,\n}\n\
             pub fn label(inbox: &Inbox, updates: &[Update]) -> String { todo!() }\n\
             pub fn elided(inbox: &Inbox) -> ui::Element<u32> { todo!() }\n\
             pub fn draw(state: &State, inbox: &Inbox, out: &mut Inbox) -> usize { todo!() }\n\
             pub fn close(state: State) {}\n\
             pub fn count(state: &State) -> usize { todo!() }\n",
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
            !body("label").contains("__call.anchor(")
                && body("label").contains("    __call.keep_lends(__a0_lends);\n")
                && body("label").contains("    __call.keep_lends(__a1_lends);\n"),
            "{text}"
        );
        assert!(
            body("elided").contains("    __call.anchor(__a0);\n"),
            "{text}"
        );
        // Lent no native value, a call keeps nothing for itself.
        assert!(!body("count").contains("keep_lends"), "{text}");
        assert!(
            body("draw").contains("    __call.keep_lends(__a0_lends);\n")
                && body("draw").contains("    __call.keep_lends(__a1_lends);\n")
                && body("draw").contains("    __call.release(&*__a2);\n")
                && !body("draw").contains("__call.keep_lends(__a2_lends)"),
            "{text}"
        );
        assert!(
            body("close").contains("    __call.release(&__a0);\n    __call.given(__a0);\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "impl ::std::ops::Drop for State {\n    #[inline]\n    fn drop(&mut self) {\n        \
                 crate::__rawr_island::ended(self);\n    }\n}\n"
            ) && text.contains("let __value = ::std::mem::ManuallyDrop::new(self);")
                && text.contains("let __f1 = unsafe { ::std::ptr::read(&__value.title) };"),
            "{text}"
        );
        assert!(!text.contains("Drop for Open"), "{text}");
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
        ] {
            let expected = format!(
                "    let __value: {output} = {{\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
                 __reply.read(|input| {read})\n    }};\n"
            );
            assert!(text.contains(&expected), "{expected}\n{text}");
        }
        // A result whose type names a lifetime comes back at `'static` and
        // is re-lifetimed to the signature's (D867).
        let expected = format!(
            "    let __value: ui::Element<'a, Message> = {{\n        use {island}::{{ByMove as _, ByWire as _}};\n        \
             let __value: ui::Element<'static, Message> = __reply.read(|input| {});\n",
            back("ui::Element<'static, Message>")
        );
        assert!(text.contains(&expected), "{expected}\n{text}");
        assert!(
            text.contains(&format!(
                "        unsafe {{ {island}::at_lifetimes(__value) }}\n    }};\n"
            )),
            "{text}"
        );
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
                 #[inline]\n    \
                 fn title(&self) -> String {\n        \
                 let mut __call = crate::__rawr_island::call(\"physics::Badge::Page::title\", true);\n        \
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
                 #[inline]\n    \
                 fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {\n        \
                 f.write_str(&self.0.text(\"physics::Badge\", crate::__rawr_island::DISPLAY))\n"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "impl ::std::clone::Clone for crate::physics::__RawrBadge {\n    \
                 #[inline]\n    \
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
                text[at..].contains(&format!("call({path:?}, true)")),
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

    /// A marked type generic over the message crosses by impls whose
    /// parameters are `'static`, and each method of its generic impl, and of
    /// a native type's impl over it, hands its call to an instance entry
    /// beside the impl, as a generic free function does (D869).
    #[test]
    fn a_generic_marked_types_methods_cross_through_instance_entries() {
        let text = generate(
            "// rawr\nuse ui::Element;\n\
             pub struct Sidebar<'a, Message: 'a> { pub title: &'a str, pub items: Vec<Element<'a, Message>>, \
             pub actions: Box<dyn Iterator<Item = Element<'a, Message>> + 'a> }\n\
             impl<'a, Message: 'a> Sidebar<'a, Message> {\n    \
             pub fn spacing(mut self, spacing: impl Into<ui::Pixels>) -> Self { self }\n    \
             pub fn to_element(&mut self) -> Element<'a, Message> { ui::void() }\n}\n\
             pub struct Group<'a, Message> { pub rows: Vec<Element<'a, Message>> }\n\
             impl<'a, Message: 'a> From<Group<'a, Message>> for Element<'a, Message> {\n    \
             fn from(group: Group<'a, Message>) -> Self { ui::void() }\n}\n",
        );
        assert!(text.refused.is_empty(), "{:?}", text.refused);
        let text = &text.files[0].1;
        for expected in [
            "impl<Message: 'static> crate::__rawr_island::Parts for Sidebar<'_, Message> {",
            "crate::__rawr_island::lend_whole_of!(call, self, Sidebar<'static, Message>);",
            // A `Vec` lent `&mut` moves its elements to the call (#1587).
            "{ let out = call.out(); let __p0 = ::std::mem::take(&mut self.items); \
             out.array(__p0.len()); for __e0 in __p0 {",
            // A boxed trait object is one native part.
            "crate::__rawr_island::lend_part_of!(call, &self.actions, \
             Box<dyn Iterator<Item = Element<'static, Message>> + 'static>);",
            "impl<Message: 'static> crate::__rawr_island::Mirror for Group<'_, Message> {",
            // The stand-in for `impl Into<Pixels>` is made before the call
            // goes to its instance, which takes it.
            "let __a0: ui::Pixels = { let __value = __a0; \
             ::std::convert::Into::<ui::Pixels>::into(__value) };\n        \
             let mut __arguments = ::std::mem::ManuallyDrop::new((self,__a0,));",
            "\"physics::Sidebar::spacing\",\n                \
             ::std::any::type_name::<(Message,)>(),",
            "pub(crate) unsafe fn __rawr_instance_Sidebar_a_Message_spacing<'a, Message: 'a>\
             (__arguments: *mut (), __out: *mut ())\nwhere\n    Message: 'static, {",
            "let (mut __self,__a0,): (Sidebar<'a, Message>,ui::Pixels,) = unsafe",
            "let (__self,): (&mut Sidebar<'a, Message>,) = unsafe",
            "fn __rawr_instance_From_Group_a_Message_Element_a_Message_from<'a, Message: 'a>",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        // Each entry is beside its impl, outside it.
        let entry = text
            .find("unsafe fn __rawr_instance_Sidebar_a_Message_spacing")
            .unwrap();
        let close = text
            .find("impl<'a, Message: 'a> Sidebar<'a, Message> {")
            .unwrap();
        assert!(entry > close, "{text}");
        assert!(!text.contains("can't call"), "{text}");
    }

    /// A generic method's `impl` of a marked trait, as inboard's `impl
    /// IntoContent<'a, Message>`, is boxed as the trait's `GIVEN` for its
    /// instance, which gives it by the impl that applies and keys its call
    /// by that impl's name; each impl's `GIVE` is `'static` in the trait's
    /// arguments (D869, #1586).
    #[test]
    fn a_generic_functions_impl_of_a_marked_trait_is_boxed_for_its_instance() {
        let text = generate(
            "// rawr\nuse ui::Element;\n\
             pub struct Content<'a, Message: 'a> { pub items: Vec<Element<'a, Message>> }\n\
             pub struct Sidebar<'a, Message: 'a> { pub content: Content<'a, Message> }\n\
             pub trait IntoContent<'a, Message> { fn into_content(self) -> Content<'a, Message>; }\n\
             impl<'a, Message: 'a> IntoContent<'a, Message> for Content<'a, Message> {\n    \
             fn into_content(self) -> Content<'a, Message> { self }\n}\n\
             impl<'a, T, Message: 'a> IntoContent<'a, Message> for T\n\
             where\n    T: IntoIterator<Item = Element<'a, Message>> + 'a,\n{\n    \
             fn into_content(self) -> Content<'a, Message> { Content { items: self.into_iter().collect() } }\n}\n\
             impl<'a, Message: 'a> Sidebar<'a, Message> {\n    \
             pub fn content(mut self, content: impl IntoContent<'a, Message>) -> Self { self }\n}\n",
        );
        assert!(text.refused.is_empty(), "{:?}", text.refused);
        let text = &text.files[0].1;
        for expected in [
            "pub(crate) trait __RawrGivenIntoContent<'a, Message> {",
            "impl<'a, Message, __T: IntoContent<'a, Message>> __RawrGivenIntoContent<'a, Message> for __T {",
            "where\n        Self: ::std::marker::Sized,\n        Message: 'static,",
            "let __a0: ::std::boxed::Box<dyn crate::physics::__RawrGivenIntoContent<'a, Message> + '_> = \
             { let __value = __a0; ::std::boxed::Box::new(__value) };",
            "crate::__rawr_island::keyed_given(\"physics::Sidebar::content\", \
             ::std::any::type_name::<(Message,)>(), \
             &[crate::physics::__RawrGivenIntoContent::__rawr_given(&*__a0)])",
            "crate::physics::__RawrGivenIntoContent::__rawr_give_boxed(__a0, &mut __call);",
            "fn __rawr_give(self, __call: &mut crate::__rawr_island::Call<'_>)\n    where\n        Message: 'static,\n    {",
            "\"+Content\"",
            "\"+_\"",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        assert!(
            !text.contains("can't call `physics::Sidebar::content`"),
            "{text}"
        );
    }
}
