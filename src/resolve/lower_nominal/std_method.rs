//! std's inherent methods on rawr's native types, from the std table (D711
//! step 4). Which methods a native type has, the probe step each is found
//! at, how it takes its receiver and what a call keeps of its inputs are
//! read from std's declarations: `Vec<T>`'s own, then `[T]`'s through
//! `Deref`, `String`'s, then `str`'s.
//!
//! [`steps`] is the binding: which std type each native type's probe
//! steps stand for. It states no method, signature or receiver. How rawr
//! runs each method stays rawr's; a method std has and rawr doesn't run
//! yet resolves, and fails at its use with the not-yet error (D712.1).

use super::{std_impl, Float, Int, Ty};
use crate::receiver;
use crate::resolve::borrowck::{
    region::{Path as RegionPath, Step as RegionStep},
    signature::{self, Source, Summary, Tap},
};
use crate::resolve::item::Lifetimes;
use crate::resolve::type_ref::{Argument, Path as TypePath, Target, Type};
use crate::std_table::{self, Item, Stability, Written};
use crate::{Position, Span};

/// One step of a native type's method probe: the std type whose inherent
/// methods it sees, and whether the step before is a shared reference to
/// it (`&str` then `str`), whose by-value step finds its `&self` methods.
#[derive(Clone, Debug)]
pub(super) struct Step {
    pub owner: &'static str,
    /// The step's type, which an impl's self type is matched against.
    pub ty: Ty,
    pub behind_reference: bool,
}

/// The autoderef steps of a native receiver (with no reference left
/// around it), each one's std type: `Vec<T>` then `[T]`, `String` then
/// `str`, `[T; N]` then `[T]`. A `&str` is one step before `str`'s.
/// Empty for a type std's inherent impls don't cover.
pub(super) fn steps(ty: &Ty) -> Vec<Step> {
    let one = |owner, ty: &Ty| {
        vec![Step {
            owner,
            ty: ty.clone(),
            behind_reference: false,
        }]
    };
    match ty {
        Ty::Bool => one("bool", ty),
        Ty::Char => one("char", ty),
        // A literal still to be inferred has every integer's methods.
        Ty::Int(Int::Var(_)) => one("i32", ty),
        Ty::Int(kind) => one(kind.name(), ty),
        Ty::Float(Float::F32) => one("f32", ty),
        Ty::Float(_) => one("f64", ty),
        Ty::Option(_) => one("core::option::Option", ty),
        Ty::Result(..) => one("core::result::Result", ty),
        Ty::String => vec![
            Step {
                owner: "alloc::string::String",
                ty: Ty::String,
                behind_reference: false,
            },
            Step {
                owner: "str",
                ty: Ty::Str,
                behind_reference: false,
            },
        ],
        // `&str`: no inherent impl of its own, then `str`'s through the
        // reference.
        Ty::Str => vec![Step {
            owner: "str",
            ty: Ty::Str,
            behind_reference: true,
        }],
        Ty::Array { element, length } => {
            let slice = Step {
                owner: "slice",
                ty: Ty::Array {
                    element: element.clone(),
                    length: None,
                },
                behind_reference: false,
            };
            let own = Step {
                owner: if length.is_some() {
                    "array"
                } else {
                    "alloc::vec::Vec"
                },
                ty: ty.clone(),
                behind_reference: false,
            };
            vec![own, slice]
        }
        Ty::Keyed(..) => match std_impl::std_definition(ty) {
            Some((definition, _)) => one(definition, ty),
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// An inherent method the probe finds on a native type.
#[derive(Clone, Copy, Debug)]
pub(super) struct Inherent {
    pub item: Item,
    /// The std type that declares it: `slice` of `sort` on a `Vec`.
    pub owner: &'static str,
    /// How it takes its receiver, from its declaration.
    pub mode: receiver::Mode,
    /// Where Rust's probe finds it: three per autoderef step, by value,
    /// then `&`, then `&mut`.
    pub rank: u8,
}

/// The receiver mode a declaration's `self` takes.
pub(super) fn mode_of(item: &Item) -> Option<receiver::Mode> {
    Some(match item.receiver()? {
        "&self" => receiver::Mode::Shared,
        "&mut self" => receiver::Mode::Mutable,
        _ => receiver::Mode::Owned,
    })
}

/// The stable inherent method `member` that Rust's probe finds on the
/// native type `ty` (no reference around it), at the first step that has
/// one whose impl's self type fits.
pub(super) fn inherent(ty: &Ty, member: &str) -> Option<Inherent> {
    for (index, step) in steps(ty).into_iter().enumerate() {
        let Some(item) = std_table::item(&format!("{}::{member}", step.owner)) else {
            continue;
        };
        if item.kind != "assoc-fn" || item.stability != Stability::Stable {
            continue;
        }
        let Some(mode) = mode_of(&item) else {
            continue;
        };
        if let Some(own) = item.impl_self()
            && !fits(&std_table::written(own), &step.ty)
        {
            continue;
        }
        let step_rank = index as u8 * 3;
        let rank = match mode {
            receiver::Mode::Owned => step_rank,
            // `&self` of `str` is `&str` itself, the step before.
            receiver::Mode::Shared if step.behind_reference => step_rank.saturating_sub(3),
            receiver::Mode::Shared => step_rank + 1,
            receiver::Mode::Mutable => step_rank + 2,
        };
        return Some(Inherent {
            item,
            owner: step.owner,
            mode,
            rank,
        });
    }
    None
}

/// How the method `member` of the native type `ty` takes its receiver, as
/// std declares it: its inherent method's, else the function of that name
/// of a std trait std implements for it (`clone`, `eq`, `hash`,
/// `to_owned`). `None` for a method std doesn't declare for it.
pub(super) fn receiver_mode(ty: &Ty, member: &str) -> Option<receiver::Mode> {
    if let Some(found) = inherent(ty, member) {
        return Some(found.mode);
    }
    let (key, _) = std_impl::std_definition(ty)?;
    thread_local! {
        static TRAIT_MODES: std::cell::RefCell<
            std::collections::HashMap<(&'static str, String), Option<receiver::Mode>>,
        > = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let cache_key = (key, member.to_owned());
    if let Some(mode) = TRAIT_MODES.with_borrow(|modes| modes.get(&cache_key).copied()) {
        return mode;
    }
    let mode = std_table::trait_functions(member)
        .into_iter()
        .find_map(|function| {
            let trait_ = &function.definition[..function.definition.len() - member.len() - 2];
            let implemented = std_table::impls(trait_, key).next().is_some()
                || std_table::impls(trait_, "_").next().is_some();
            implemented.then(|| mode_of(&function)).flatten()
        });
    TRAIT_MODES.with_borrow_mut(|modes| modes.insert(cache_key, mode));
    mode
}

/// The not-yet path of an inherent method: `Vec::dedup_by_key`,
/// `str::char_indices`, `[T]::rsplit`.
pub(super) fn path(found: &Inherent) -> String {
    let owner = match found.owner {
        "slice" => "[T]",
        "array" => "[T; N]",
        owner => owner.rsplit("::").next().unwrap_or(owner),
    };
    format!("{owner}::{}", found.item.name())
}

/// Whether an inherent impl's self type as std writes it (`Option<&T>`,
/// `[u8]`, `Vec<u8, A>`) fits the native type `ty`: an impl parameter fits
/// anything, and a type still to be inferred fits what it may become.
fn fits(written: &Written, ty: &Ty) -> bool {
    if matches!(ty, Ty::Infer(_) | Ty::Unknown) {
        return true;
    }
    match written {
        Written::Reference { mutable, target } => match ty {
            Ty::Reference {
                mutable: found,
                target: inner,
            } => found == mutable && fits(target, inner),
            Ty::Str => !mutable && fits(target, &Ty::Str),
            _ => false,
        },
        Written::Slice(element) | Written::Array(element, _) => match ty {
            Ty::Array { element: found, .. } => fits(element, found),
            _ => false,
        },
        Written::Tuple(members) => match ty {
            Ty::Tuple(found) => {
                found.len() == members.len()
                    && members
                        .iter()
                        .zip(found.iter())
                        .all(|(member, found)| fits(member, found))
            }
            Ty::Unit => members.is_empty(),
            _ => false,
        },
        Written::Path {
            path, arguments, ..
        } => {
            // `str` of `&str`.
            if *path == "str" {
                return matches!(ty, Ty::Str | Ty::String);
            }
            let Some(definition) = std_table::definition_of(path) else {
                // An impl's parameter.
                return true;
            };
            let Some((found, found_arguments)) = std_impl::std_definition(ty) else {
                return false;
            };
            found == definition
                && arguments
                    .iter()
                    .zip(found_arguments.iter())
                    .all(|(argument, found)| fits(argument, found))
        }
        Written::Never | Written::Other(_) => true,
    }
}

/// What a call of std's inherent method keeps of its inputs, read from its
/// declaration by `borrowck::signature::summary`, as a crate function's is
/// (D711 step 4): its receiver mode, its lifetimes (elided ones as rustc
/// elides them), and its type parameters, with a closure parameter's
/// value where its bound's output is (`map`'s `U` is what `f` returns).
///
/// `receiver` is the native receiver (a reference around it is the
/// call's), `output` the call's value, which picks a `SliceIndex`
/// projection's type: `[T]` for `v.get(1..3)`, `T` for `v.get(1)`. `None`
/// for a method std doesn't declare on the type.
pub(super) fn summary(receiver: &Ty, output: &Ty, member: &str) -> Option<Summary> {
    let mut native = receiver;
    while let Ty::Reference { target, .. } = native {
        native = target;
    }
    let found = inherent(native, member)?;
    let declared = Declared::parse(found.item.declaration())?;
    let mut reader = Reader {
        lifetimes: Lifetimes::default(),
        next: 0,
        slice_output: holds_array(output),
        output_item: match output {
            Ty::Iter(item) => Some((**item).clone()),
            _ => None,
        },
        substitution: Vec::new(),
        closures: Vec::new(),
    };
    // `F: FnOnce(T) -> U`: what each closure parameter returns.
    for generic in std_table::split_list(declared.generics) {
        if let Some(colon) = std_table::top_level(generic, ": ") {
            reader.closure(&generic[..colon], &generic[colon + 2..]);
        }
    }
    let owner = match found.item.impl_self() {
        Some(own) => reader.ty(own)?,
        None => reader.owner(found.owner)?,
    };
    reader.lifetimes.owner = Some(Box::new(owner));
    let mut inputs = std_table::split_list(declared.inputs);
    if !inputs.is_empty() {
        let first = inputs.remove(0);
        if let Some(rest) = first.strip_prefix("&'")
            && let Some((name, _)) = rest.split_once(' ')
        {
            reader.lifetimes.receiver = Some(name.into());
        }
    }
    let mut parameters = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let colon = std_table::top_level(input, ": ")?;
        let written = input[colon + 2..].trim();
        // `f: impl FnOnce(T) -> bool`: a closure parameter of its own.
        if let Some(bound) = written.strip_prefix("impl ") {
            let name: &'static str = Box::leak(format!("impl#{index}").into_boxed_str());
            reader.closure(name, bound);
            parameters.push(reader.parameter(name));
            continue;
        }
        parameters.push(reader.ty(written)?);
    }
    let output_ty = match declared.output {
        Some(output) => reader.ty(output)?,
        None => Type::Unit(Span::NONE),
    };
    let mode = match found.mode {
        receiver::Mode::Owned => signature::Receiver::Owned,
        receiver::Mode::Shared => signature::Receiver::Shared,
        receiver::Mode::Mutable => signature::Receiver::Mutable,
    };
    let lifetimes = reader.lifetimes.clone();
    let signature = signature::Signature {
        receiver: Some(mode),
        parameters: parameters.iter().collect(),
        output: &output_ty,
        lifetimes: &lifetimes,
    };
    let mut summary = signature::summary(&signature, &|_| Some(0));
    // A value whose regions std's declaration names in a type these facts
    // can't place (`Entry<'_, K, V>`, `<[T] as Concat<Item>>::Output`):
    // rawr's binding says, or the call is left to rustc (D572.5).
    summary.output.as_ref()?;
    // What a closure parameter returns is where its bound's output is.
    if let Some(taps) = &mut summary.output {
        for (index, parameter) in parameters.iter().enumerate() {
            let Type::Path(path) = parameter else {
                continue;
            };
            let Some((_, returns)) = reader
                .closures
                .iter()
                .find(|(name, _)| *name == path.name.as_str())
            else {
                continue;
            };
            let mut places = Vec::new();
            if Some(*returns) == declared.output {
                places.push(RegionPath::ROOT);
            } else {
                parameter_paths(&output_ty, RegionPath::ROOT, returns, &mut places);
            }
            for into in places {
                taps.push(Tap {
                    into,
                    from: Source::Call(index),
                });
            }
        }
    }
    Some(summary)
}

/// A declaration's parts: `fn name<generics>(inputs) -> output where ..`.
struct Declared {
    generics: &'static str,
    inputs: &'static str,
    output: Option<&'static str>,
}

impl Declared {
    fn parse(text: &'static str) -> Option<Self> {
        let open = text.find(['<', '('])?;
        let (generics, rest) = if text[open..].starts_with('<') {
            let close = open + closing(&text[open..], '<', '>')?;
            (&text[open + 1..close], &text[close + 1..])
        } else {
            ("", &text[open..])
        };
        let close = closing(rest, '(', ')')?;
        let inputs = &rest[1..close];
        let rest = rest[close + 1..].trim();
        let rest = match std_table::top_level(rest, " where ") {
            Some(at) => &rest[..at],
            None => rest.split(" where ").next().unwrap_or(rest),
        };
        let output = rest.strip_prefix("-> ").map(str::trim);
        Some(Self {
            generics,
            inputs,
            output,
        })
    }
}

/// The index in `text` (which starts with `open`) of its matching `close`.
fn closing(text: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (index, c) in text.char_indices() {
        if c == '>' && index > 0 && text.as_bytes()[index - 1] == b'-' {
            continue;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// Whether a call's value holds an array where a reference is: `v.get(1..3)`
/// gives `Option<&[T]>`.
fn holds_array(ty: &Ty) -> bool {
    match ty {
        Ty::Option(inner) => holds_array(inner),
        Ty::Reference { target, .. } => matches!(&**target, Ty::Array { .. }),
        _ => false,
    }
}

/// The positions of the type parameter `name` in `ty`.
fn parameter_paths(ty: &Type, at: RegionPath, name: &str, out: &mut Vec<RegionPath>) {
    match ty {
        Type::Unit(_) => {}
        Type::Tuple { members, .. } => {
            for (index, member) in members.iter().enumerate() {
                parameter_paths(member, at.push(RegionStep::Arg(index as u8)), name, out);
            }
        }
        Type::Reference { target, .. } => {
            parameter_paths(target, at.push(RegionStep::Arg(0)), name, out)
        }
        Type::Path(path) => {
            if path.target == Target::Parameter && path.name.as_str() == name {
                out.push(at);
                return;
            }
            let mut index = 0;
            for argument in path.arguments.iter() {
                if let Argument::Type(argument) = argument {
                    parameter_paths(argument, at.push(RegionStep::Arg(index)), name, out);
                    index += 1;
                }
            }
        }
    }
}

/// Reads std's written types as the borrow checker's.
struct Reader {
    lifetimes: Lifetimes,
    next: u16,
    /// A `SliceIndex<[T]>` projection is `[T]` (else `T`).
    slice_output: bool,
    /// An iterator's impl parameters, as its path's arguments give them.
    substitution: Vec<(&'static str, Type)>,
    /// The item of the call's value, an iterator, for one whose `Iterator`
    /// impl the table doesn't have (`str::Split`'s needs the unstable
    /// `Pattern`).
    output_item: Option<Ty>,
    /// Each closure parameter and what its bound returns: `F`, `U`.
    closures: Vec<(&'static str, &'static str)>,
}

impl Reader {
    fn span(&mut self) -> Span {
        self.next += 1;
        let at = Position::new(1, self.next);
        Span::new(at, at)
    }

    /// A lifetime written as `'a`, or elided where `None` or `'_`.
    fn lifetime(&mut self, name: Option<&str>) -> Option<Span> {
        let name = name?.trim_start_matches('\'');
        if name == "_" {
            return None;
        }
        let span = self.span();
        self.lifetimes.names.insert(span, name.into());
        Some(span)
    }

    fn closure(&mut self, name: &'static str, bounds: &'static str) {
        for bound in std_table::split_bounds(bounds) {
            if ["FnOnce(", "FnMut(", "Fn("]
                .iter()
                .any(|closure| bound.starts_with(closure))
            {
                let returns = std_table::top_level(bound, " -> ")
                    .map_or("()", |arrow| bound[arrow + 4..].trim());
                self.closures.push((name, returns));
            }
        }
    }

    fn parameter(&mut self, name: &str) -> Type {
        Type::Path(TypePath {
            name: name.into(),
            target: Target::Parameter,
            arguments: Box::default(),
            span: self.span(),
        })
    }

    fn builtin(&mut self, name: &str, arguments: Vec<Argument>) -> Type {
        Type::Path(TypePath {
            name: name.into(),
            target: Target::Builtin,
            arguments: arguments.into_boxed_slice(),
            span: self.span(),
        })
    }

    /// `&'a str`: rawr's `String` with the reference's lifetime.
    fn text(&mut self, lifetime: Option<Span>) -> Type {
        let span = lifetime.unwrap_or_else(|| self.span());
        self.builtin("String", vec![Argument::Lifetime(span)])
    }

    /// The impl's self type of a std type's own inherent impl: `Option<T>`
    /// of `core::option::Option`, `[T]` of `slice`.
    fn owner(&mut self, owner: &str) -> Option<Type> {
        let written: &'static str = match owner {
            "slice" | "array" => "[T]",
            "str" => "String",
            "core::option::Option" => "Option<T>",
            "core::result::Result" => "Result<T, E>",
            "alloc::vec::Vec" => "Vec<T>",
            "alloc::string::String" => "String",
            "std::collections::hash::map::HashMap" => "std::collections::HashMap<K, V>",
            "alloc::collections::btree::map::BTreeMap" => "std::collections::BTreeMap<K, V>",
            "std::collections::hash::set::HashSet" => "std::collections::HashSet<T>",
            "alloc::collections::btree::set::BTreeSet" => "std::collections::BTreeSet<T>",
            // A primitive holds no borrow.
            _ => return Some(self.builtin("bool", Vec::new())),
        };
        self.ty(written)
    }

    /// The type std writes as `text`, as the borrow checker reads it.
    fn ty(&mut self, text: &'static str) -> Option<Type> {
        let text = text.trim();
        if text == "()" || text == "!" {
            return Some(Type::Unit(self.span()));
        }
        if let Some(rest) = text.strip_prefix('&') {
            let (lifetime, rest) = match rest.strip_prefix('\'') {
                Some(named) => {
                    let (name, rest) = named.split_once(' ')?;
                    (self.lifetime(Some(name)), rest)
                }
                None => (None, rest),
            };
            let (mutable, rest) = match rest.strip_prefix("mut ") {
                Some(rest) => (true, rest),
                None => (false, rest),
            };
            if !mutable && (rest == "str" || rest.starts_with("<I as std::slice::SliceIndex<str>>"))
            {
                return Some(self.text(lifetime));
            }
            let target = self.ty(rest)?;
            return Some(Type::Reference {
                mutable,
                lifetime,
                target: Box::new(target),
                span: self.span(),
            });
        }
        if let Some(inner) = text
            .strip_prefix('[')
            .and_then(|text| text.strip_suffix(']'))
        {
            let element = match std_table::top_level(inner, "; ") {
                Some(at) => &inner[..at],
                None => inner,
            };
            let element = self.ty(element)?;
            return Some(self.builtin("Vec", vec![Argument::Type(element)]));
        }
        if let Some(inner) = text
            .strip_prefix('(')
            .and_then(|text| text.strip_suffix(')'))
        {
            let inner = inner.strip_suffix(',').unwrap_or(inner);
            let members = std_table::split_list(inner)
                .into_iter()
                .map(|member| self.ty(member))
                .collect::<Option<Vec<_>>>()?;
            return Some(Type::Tuple {
                members: members.into_boxed_slice(),
                span: self.span(),
            });
        }
        // `<I as SliceIndex<[T]>>::Output`: what an index gives.
        if text.starts_with("<I as std::slice::SliceIndex<[T]>>") {
            return self.ty(if self.slice_output { "[T]" } else { "T" });
        }
        // `<T as Deref>::Target` holds what `T` holds.
        if let Some(rest) = text.strip_prefix('<')
            && let Some(base) = rest.strip_suffix(" as std::ops::Deref>::Target")
        {
            return self.ty(base);
        }
        if text.starts_with('<') || text.starts_with("dyn ") || text.starts_with("impl ") {
            return Some(self.unknown(text));
        }
        let (name, arguments) = match text.find('<') {
            Some(open) if text.ends_with('>') => (&text[..open], &text[open + 1..text.len() - 1]),
            Some(_) => return Some(self.unknown(text)),
            None => (text, ""),
        };
        let mut types = Vec::new();
        let mut lifetimes = Vec::new();
        for argument in std_table::split_list(arguments) {
            if argument.starts_with('\'') {
                lifetimes.push(argument);
            } else {
                types.push(argument);
            }
        }
        if let Some((_, ty)) = self.substitution.iter().find(|(param, _)| *param == name) {
            return Some(ty.clone());
        }
        if name == "Self" {
            return Some(self.parameter("Self"));
        }
        let Some(definition) = std_table::definition_of(name) else {
            // An impl's or the function's type parameter.
            return Some(if name.contains("::") {
                self.unknown(text)
            } else {
                self.parameter(name)
            });
        };
        let convert = |this: &mut Self, count: usize| -> Option<Vec<Argument>> {
            types
                .iter()
                .take(count)
                .map(|argument| this.ty(argument).map(Argument::Type))
                .collect()
        };
        let builtin = match definition {
            "core::option::Option" => Some(("Option", 1)),
            "core::result::Result" => Some(("Result", 2)),
            "alloc::vec::Vec" => Some(("Vec", 1)),
            "alloc::boxed::Box" => Some(("Box", 1)),
            "alloc::rc::Rc" => Some(("Rc", 1)),
            "alloc::sync::Arc" => Some(("Arc", 1)),
            "std::collections::hash::map::HashMap" => Some(("HashMap", 2)),
            "alloc::collections::btree::map::BTreeMap" => Some(("BTreeMap", 2)),
            "std::collections::hash::set::HashSet" => Some(("HashSet", 1)),
            "alloc::collections::btree::set::BTreeSet" => Some(("BTreeSet", 1)),
            "alloc::string::String" => Some(("String", 0)),
            "core::cmp::Ordering" => Some(("Ordering", 0)),
            primitive if !primitive.contains("::") => Some((primitive, 0)),
            _ => None,
        };
        if let Some((builtin, count)) = builtin {
            let arguments = convert(self, count)?;
            return Some(self.builtin(builtin, arguments));
        }
        // An iterator std declares (`slice::Iter<'_, T>`, `str::Split<'_,
        // P>`): rawr's iterator of its `Item`, which captures what made it.
        let iterator = std_table::impls("core::iter::traits::iterator::Iterator", definition)
            .find_map(|line| Some((line, line.associated("Item")?)));
        let Some((line, item)) = iterator else {
            // The call's own item, its references elided as std's are.
            if !lifetimes.is_empty()
                && let Some(item) = self.output_item.take()
            {
                let item = self.native(&item)?;
                return Some(
                    self.builtin(crate::item::type_ref::ITER_TYPE, vec![Argument::Type(item)]),
                );
            }
            return Some(self.unknown(text));
        };
        let own = match std_table::written(line.self_ty) {
            std_table::Written::Path { arguments, .. } => arguments,
            _ => Vec::new(),
        };
        let mut substitution = Vec::new();
        for (parameter, argument) in own.iter().zip(types.iter()) {
            if let std_table::Written::Path { path, .. } = parameter {
                let argument = self.ty(argument)?;
                substitution.push((*path, argument));
            }
        }
        let saved = std::mem::replace(&mut self.substitution, substitution);
        let item = self.ty(item);
        self.substitution = saved;
        let item = item?;
        Some(self.builtin(crate::item::type_ref::ITER_TYPE, vec![Argument::Type(item)]))
    }

    /// A native type as a signature with its lifetimes elided writes it.
    fn native(&mut self, ty: &Ty) -> Option<Type> {
        Some(match ty {
            Ty::Str => self.text(None),
            Ty::Reference { mutable, target } => {
                let target = self.native(target)?;
                Type::Reference {
                    mutable: *mutable,
                    lifetime: None,
                    target: Box::new(target),
                    span: self.span(),
                }
            }
            Ty::Tuple(members) => {
                let members = members
                    .iter()
                    .map(|member| self.native(member))
                    .collect::<Option<Vec<_>>>()?;
                Type::Tuple {
                    members: members.into_boxed_slice(),
                    span: self.span(),
                }
            }
            Ty::Unit => Type::Unit(self.span()),
            Ty::String => self.builtin("String", Vec::new()),
            Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char => self.builtin("i64", Vec::new()),
            _ => return None,
        })
    }

    /// A type whose regions these facts can't place.
    fn unknown(&mut self, text: &str) -> Type {
        Type::Path(TypePath {
            name: text.into(),
            target: Target::Std(text.into()),
            arguments: Box::default(),
            span: self.span(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(ty: &Ty, member: &str) -> Option<(receiver::Mode, &'static str, u8)> {
        inherent(ty, member).map(|found| (found.mode, found.owner, found.rank))
    }

    fn vec_of(element: Ty) -> Ty {
        Ty::Array {
            element: Box::new(element),
            length: None,
        }
    }

    /// Step 0 of D711 found these in the hand list std's declarations
    /// replace: `clone` and `to_string` are trait methods, `into_bytes`
    /// takes `self`, and `sort` takes `&mut self`, on `[T]`.
    #[test]
    fn std_inherent_methods_are_stds() {
        let v = vec_of(Ty::Int(Int::I64));
        assert!(inherent(&Ty::String, "clone").is_none());
        assert!(inherent(&Ty::Int(Int::I64), "to_string").is_none());
        assert!(inherent(&v, "clone").is_none());
        assert!(inherent(&Ty::Option(Box::new(Ty::Bool)), "clone").is_none());
        assert_eq!(
            mode(&Ty::String, "into_bytes"),
            Some((receiver::Mode::Owned, "alloc::string::String", 0))
        );
        assert_eq!(
            mode(&v, "sort"),
            Some((receiver::Mode::Mutable, "slice", 5))
        );
        assert_eq!(
            mode(&v, "push"),
            Some((receiver::Mode::Mutable, "alloc::vec::Vec", 2))
        );
        assert_eq!(
            mode(&v, "len"),
            Some((receiver::Mode::Shared, "alloc::vec::Vec", 1))
        );
        assert_eq!(mode(&v, "iter"), Some((receiver::Mode::Shared, "slice", 4)));
        // `str`'s `&self` methods on a `&str` are its by-value step's.
        assert_eq!(
            mode(&Ty::Str, "trim"),
            Some((receiver::Mode::Shared, "str", 0))
        );
        assert_eq!(
            mode(&Ty::String, "trim"),
            Some((receiver::Mode::Shared, "str", 4))
        );
        assert_eq!(
            mode(&Ty::Int(Int::I64), "pow"),
            Some((receiver::Mode::Owned, "i64", 0))
        );
        // `copied` is `Option<&T>`'s.
        assert!(inherent(&Ty::Option(Box::new(Ty::Bool)), "copied").is_none());
        let reference = Ty::Option(Box::new(Ty::Reference {
            mutable: false,
            target: Box::new(Ty::Bool),
        }));
        assert_eq!(
            mode(&reference, "copied"),
            Some((receiver::Mode::Owned, "core::option::Option", 0))
        );
        // `is_ascii` of `[u8]`, not of `[i64]`.
        assert!(inherent(&v, "is_ascii").is_none());
        assert!(inherent(&vec_of(Ty::Int(Int::U8)), "is_ascii").is_some());
    }

    /// What a call keeps comes from std's declaration, as a crate
    /// function's does: `push` stores its argument in the elements,
    /// `map`'s value is what its closure returns, `iter`'s items borrow
    /// the receiver.
    #[test]
    fn std_summaries_come_from_declarations() {
        let at = |steps: &[RegionStep]| RegionPath::of(steps);
        let reference = Ty::Reference {
            mutable: false,
            target: Box::new(Ty::Int(Int::I64)),
        };
        let v = vec_of(reference.clone());
        let push = summary(&v, &Ty::Unit, "push").expect("push");
        assert_eq!(push.mode, Some(signature::Receiver::Mutable));
        assert_eq!(
            push.stores,
            Some(vec![Tap {
                into: at(&[RegionStep::Arg(0)]),
                from: Source::Input(signature::Input::Argument(0), RegionPath::ROOT),
            }])
        );
        let option = Ty::Option(Box::new(reference.clone()));
        let map = summary(&option, &Ty::Unknown, "map").expect("map");
        assert_eq!(map.mode, Some(signature::Receiver::Owned));
        assert!(map.output.expect("known").contains(&Tap {
            into: at(&[RegionStep::Arg(0)]),
            from: Source::Call(0),
        }));
        let iter = summary(&v, &Ty::Iter(Box::new(reference)), "iter").expect("iter");
        let taps = iter.output.expect("known");
        assert!(taps.contains(&Tap {
            into: at(&[RegionStep::Arg(0), RegionStep::Own]),
            from: Source::Input(signature::Input::Receiver, RegionPath::OWN),
        }));
        let bytes = summary(&Ty::String, &Ty::Unknown, "into_bytes").expect("into_bytes");
        assert_eq!(bytes.mode, Some(signature::Receiver::Owned));
    }
}
