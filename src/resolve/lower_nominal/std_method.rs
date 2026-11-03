//! std's inherent methods on rawr's native types, from the std table (D711
//! step 4). Which methods a native type has, the probe step each is found
//! at, how it takes its receiver and what a call keeps of its inputs are
//! read from std's declarations: `Vec<T>`'s own, then `[T]`'s through
//! `Deref`, `String`'s, then `str`'s, `VecDeque<T>`'s own, and an
//! iterator's from `Iterator` and the traits and adapter it stands for.
//!
//! [`steps`] is the binding: which std type each native type's probe
//! steps stand for. It states no method, signature or receiver. How rawr
//! runs each method stays rawr's; a method std has and rawr doesn't run
//! yet resolves, and fails at its use with the not-yet error (D712.1).

use super::{sequence, std_impl, Float, Int, Ty};
use crate::receiver;
use crate::resolve::borrowck::{
    region::{Path as RegionPath, Step as RegionStep},
    signature::{self, Source, Summary, Tap},
};
use crate::resolve::item::Lifetimes;
use crate::resolve::type_ref::{Argument, Path as TypePath, Target, Type};
use crate::std_table::{self, Item, Stability, Written};
use crate::{Position, Span};

/// What rawr's iterator value stands for: any iterator std's adapters
/// make, so it has the methods of `Iterator`, `DoubleEndedIterator`,
/// `Peekable` and `IntoIterator`. Its `Self` is a type parameter, as a
/// trait method's is, and `Self::Item` is its items'.
const ITERATOR: [&str; 4] = [
    "core::iter::traits::iterator::Iterator",
    "core::iter::traits::double_ended::DoubleEndedIterator",
    "core::iter::adapters::peekable::Peekable",
    "core::iter::traits::collect::IntoIterator",
];

/// The traits std implements for an iterator, which a crate's iterator or
/// a range has the methods of where it implements them.
const ITERATOR_TRAITS: [&str; 4] = [
    "core::iter::traits::iterator::Iterator",
    "core::iter::traits::double_ended::DoubleEndedIterator",
    "core::iter::traits::exact_size::ExactSizeIterator",
    "core::iter::traits::collect::IntoIterator",
];

/// The name `Self` of an iterator's method is read as: a type parameter
/// holding its items, which no declaration can write.
const ITERATOR_SELF: &str = "Self::Iterator";

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
/// `str`, `[T; N]` then `[T]`, `[T]` and `VecDeque<T>` alone. A `&str` is
/// one step before `str`'s.
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
        Ty::Ordering => one("core::cmp::Ordering", ty),
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
        Ty::Array { element, kind } => {
            let slice = Step {
                owner: "slice",
                ty: Ty::sequence(sequence::Kind::Slice, (**element).clone()),
                behind_reference: false,
            };
            let own = |owner| Step {
                owner,
                ty: ty.clone(),
                behind_reference: false,
            };
            match kind {
                sequence::Kind::Array(_) => vec![own("array"), slice],
                sequence::Kind::Vec => vec![own("alloc::vec::Vec"), slice],
                sequence::Kind::Slice => vec![slice],
                sequence::Kind::Deque => vec![own("alloc::collections::vec_deque::VecDeque")],
            }
        }
        Ty::Iter(_) => ITERATOR
            .iter()
            .map(|owner| Step {
                owner,
                ty: ty.clone(),
                behind_reference: false,
            })
            .collect(),
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
        let Some((item, mode)) = stable(step.owner, member) else {
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

/// The stable function `member` that `owner` declares, and how it takes its
/// receiver.
fn stable(owner: &str, member: &str) -> Option<(Item, receiver::Mode)> {
    let item = std_table::item(&format!("{owner}::{member}"))?;
    if item.kind != "assoc-fn" || item.stability != Stability::Stable {
        return None;
    }
    let mode = mode_of(&item)?;
    Some((item, mode))
}

/// The stable method `member` that std's iterator traits declare, of those
/// `implements` says an iterator has (by the trait's definition; `Iterator`
/// and `IntoIterator` always): `Iterator::map`,
/// `DoubleEndedIterator::rfind`, `ExactSizeIterator::len`. A crate's
/// iterator or a range has these.
pub(super) fn iterator(member: &str, implements: impl Fn(&str) -> bool) -> Option<Inherent> {
    ITERATOR_TRAITS.iter().find_map(|owner| {
        if !matches!(trait_name(owner), "Iterator" | "IntoIterator") && !implements(owner) {
            return None;
        }
        let (item, mode) = stable(owner, member)?;
        Some(Inherent {
            item,
            owner,
            mode,
            rank: 0,
        })
    })
}

/// The name a trait's definition ends in: `Iterator` of
/// `core::iter::traits::iterator::Iterator`.
pub(super) fn trait_name(definition: &str) -> &str {
    definition.rsplit("::").next().unwrap_or(definition)
}

/// How the method `member` of the native type `ty` takes its receiver, as
/// std declares it: its inherent method's, else the function of that name
/// of a std trait std implements for it (`clone`, `eq`, `hash`,
/// `to_owned`). `None` for a method std doesn't declare for it.
pub(super) fn receiver_mode(ty: &Ty, member: &str) -> Option<receiver::Mode> {
    if let Some(found) = inherent(ty, member) {
        return Some(found.mode);
    }
    let key = match ty {
        Ty::Reference { mutable: false, .. } => "&",
        Ty::Reference { mutable: true, .. } => "&mut",
        Ty::Array {
            kind: sequence::Kind::Slice,
            ..
        } => "[]",
        Ty::Array {
            kind: sequence::Kind::Array(_),
            ..
        } => "[;]",
        ty => std_impl::std_definition(ty)?.0,
    };
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

/// Whether `member` is a method of another of std's sequence types and
/// not of `receiver`'s (`[T]` or a reference to one, `Vec<T>`, `[T; N]`,
/// `VecDeque<T>`): `push` of a `&[T]`, `push_back` of a `Vec<T>`, `sort` of
/// a `VecDeque<T>`, which rustc refuses (E0599).
pub(super) fn foreign_sequence_method(receiver: &Ty, member: &str) -> bool {
    let native = match receiver {
        Ty::Reference { target, .. } => &**target,
        native => native,
    };
    let Ty::Array { element, kind } = native else {
        return false;
    };
    if receiver_mode(native, member).is_some()
        || (native != receiver && receiver_mode(receiver, member).is_some())
    {
        return false;
    }
    [
        sequence::Kind::Vec,
        sequence::Kind::Slice,
        sequence::Kind::Deque,
        sequence::Kind::Array(0),
    ]
    .into_iter()
    .filter(|other| std::mem::discriminant(other) != std::mem::discriminant(kind))
    .any(|other| receiver_mode(&Ty::sequence(other, (**element).clone()), member).is_some())
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

/// What std's declaration of a method gives each of a call's arguments.
#[derive(Debug, Default)]
pub(super) struct Arguments {
    /// The parameter types of each closure argument, as its `Fn` bound
    /// writes them: `[&T]` of `retain`'s `F: FnMut(&T) -> bool`. `None`
    /// for an argument that takes no closure, or one whose inputs name a
    /// type rawr has none of.
    pub closures: Vec<Option<Box<[Ty]>>>,
    /// The type of each argument up to the last that names one of the
    /// method's own type parameters (`init: B` of `fold`), whose value
    /// fixes it for the closures after it. Earlier ones are left to be
    /// inferred.
    pub generics: Vec<Ty>,
}

/// What std's declaration of the method `member` on the native `receiver`
/// (a reference around it is the call's) gives its arguments (#795): the
/// parameter types of `rposition`'s `P: FnMut(Self::Item) -> bool` are the
/// iterator's items, those of `map_or_else`'s `F: FnOnce(T) -> U` the
/// `Option`'s `T`. A type parameter of the method (`B` of `fold`) is a type
/// `fresh` gives, one per parameter, to be inferred. Empty for a method std
/// doesn't declare on the type.
pub(super) fn arguments(receiver: &Ty, member: &str, fresh: &mut impl FnMut() -> Ty) -> Arguments {
    let mut native = receiver;
    while let Ty::Reference { target, .. } = native {
        native = target;
    }
    let Some(found) = inherent(native, member) else {
        return Arguments::default();
    };
    let Some(declared) = std_table::Declared::parse(found.item.declaration()) else {
        return Arguments::default();
    };
    let Some(step) = steps(native)
        .into_iter()
        .find(|step| step.owner == found.owner)
    else {
        return Arguments::default();
    };
    let mut known = Known {
        types: Vec::new(),
        items: match native {
            Ty::Iter(item) => Some((**item).clone()),
            _ => None,
        },
    };
    match found.item.impl_self() {
        Some(own) => known.bind(&std_table::written(own), &step.ty),
        None => known.own(found.owner, &step.ty),
    }
    // The method's own type parameters, and the bounds each has.
    let mut bounds = Vec::new();
    for generic in std_table::split_list(declared.generics)
        .into_iter()
        .chain(std_table::split_list(declared.wheres))
    {
        let (name, bound) = match std_table::top_level(generic, ": ") {
            Some(colon) => (&generic[..colon], &generic[colon + 2..]),
            None => (generic, ""),
        };
        if name != "Self" && !name.starts_with('\'') && !name.contains(['<', ':']) {
            if !known.types.iter().any(|(known, _)| *known == name) {
                known.types.push((name, fresh()));
            }
            bounds.push((name, bound));
        }
    }
    let mut arguments = Arguments::default();
    let mut generic_until = 0;
    // The inputs after `self`.
    for (index, input) in std_table::split_list(declared.inputs)
        .into_iter()
        .skip(1)
        .enumerate()
    {
        let written = match std_table::top_level(input, ": ") {
            Some(colon) => input[colon + 2..].trim(),
            None => input,
        };
        let written_bounds: Vec<&'static str> = match written.strip_prefix("impl ") {
            Some(bound) => vec![bound],
            None => bounds
                .iter()
                .filter(|(name, _)| *name == written)
                .map(|(_, bound)| *bound)
                .collect(),
        };
        let closure = written_bounds
            .into_iter()
            .flat_map(std_table::split_bounds)
            .find_map(closure_inputs);
        match closure {
            Some(inputs) => {
                let inputs = std_table::split_list(inputs)
                    .into_iter()
                    .map(|input| known.ty(input))
                    .collect::<Option<Box<[Ty]>>>();
                arguments.closures.push(inputs);
                arguments.generics.push(fresh());
            }
            None => {
                arguments.closures.push(None);
                let generic = bounds.iter().any(|(name, _)| *name == written);
                match known.ty(written) {
                    Some(ty) if generic => {
                        arguments.generics.push(ty);
                        generic_until = index + 1;
                    }
                    _ => arguments.generics.push(fresh()),
                }
            }
        }
    }
    arguments.generics.truncate(generic_until);
    arguments
}

/// The inputs a closure bound writes: `&T, &T` of `FnMut(&T, &T) -> bool`.
fn closure_inputs(bound: &'static str) -> Option<&'static str> {
    let open = ["FnOnce(", "FnMut(", "Fn("]
        .iter()
        .find_map(|closure| bound.starts_with(closure).then_some(closure.len() - 1))?;
    let close = open + std_table::closing(&bound[open..], '(', ')')?;
    Some(&bound[open + 1..close])
}

/// The types a method's declaration names, as rawr's: its impl's and its
/// own type parameters, and an iterator's items.
struct Known {
    types: Vec<(&'static str, Ty)>,
    /// `Self::Item` of an iterator's method.
    items: Option<Ty>,
}

impl Known {
    /// Gives the impl parameters the impl's self type `written` has the
    /// parts of `ty` where they are: `T` of `Option<&T>` against
    /// `Option<&u8>`.
    fn bind(&mut self, written: &Written, ty: &Ty) {
        match (written, ty) {
            (
                Written::Path {
                    path, arguments, ..
                },
                _,
            ) if arguments.is_empty() && std_table::definition_of(path).is_none() => {
                self.types.push((path, ty.clone()));
            }
            (Written::Reference { target, .. }, Ty::Reference { target: found, .. }) => {
                self.bind(target, found)
            }
            (
                Written::Slice(element) | Written::Array(element, _),
                Ty::Array { element: found, .. },
            ) => self.bind(element, found),
            (Written::Tuple(members), Ty::Tuple(found)) => {
                for (member, found) in members.iter().zip(found.iter()) {
                    self.bind(member, found);
                }
            }
            (Written::Path { arguments, .. }, _) => {
                if let Some((_, found)) = std_impl::std_definition(ty) {
                    for (argument, found) in arguments.iter().zip(found.iter()) {
                        self.bind(argument, found);
                    }
                }
            }
            _ => {}
        }
    }

    /// Gives the parameters of the std type `owner`'s own inherent impl the
    /// arguments of `ty`: `T` of `Vec<T>`, `[T]`'s element.
    fn own(&mut self, owner: &str, ty: &Ty) {
        if let ("slice" | "array", Ty::Array { element, .. }) = (owner, ty) {
            self.types.push(("T", (**element).clone()));
            return;
        }
        let (Some(item), Some((_, arguments))) =
            (std_table::item(owner), std_impl::std_definition(ty))
        else {
            return;
        };
        for (parameter, argument) in item.generics().zip(arguments) {
            self.types.push((parameter.name, argument));
        }
    }

    /// The type std writes as `text`, as rawr's: `None` for one rawr has
    /// none of.
    fn ty(&self, text: &'static str) -> Option<Ty> {
        let text = text.trim();
        if text == "()" {
            return Some(Ty::Unit);
        }
        if let Some(rest) = text.strip_prefix('&') {
            let rest = match rest.strip_prefix('\'') {
                Some(named) => named.split_once(' ')?.1,
                None => rest,
            };
            let (mutable, rest) = match rest.strip_prefix("mut ") {
                Some(rest) => (true, rest),
                None => (false, rest),
            };
            if !mutable && rest == "str" {
                return Some(Ty::Str);
            }
            return Some(Ty::Reference {
                mutable,
                target: Box::new(self.ty(rest)?),
            });
        }
        if let Some(inner) = text
            .strip_prefix('[')
            .and_then(|text| text.strip_suffix(']'))
        {
            return Some(match std_table::top_level(inner, "; ") {
                Some(at) => Ty::sequence(
                    sequence::Kind::Array(inner[at + 2..].trim().parse().ok()?),
                    self.ty(&inner[..at])?,
                ),
                None => Ty::sequence(sequence::Kind::Slice, self.ty(inner)?),
            });
        }
        if let Some(inner) = text
            .strip_prefix('(')
            .and_then(|text| text.strip_suffix(')'))
        {
            let inner = inner.strip_suffix(',').unwrap_or(inner);
            let members = std_table::split_list(inner)
                .into_iter()
                .map(|member| self.ty(member))
                .collect::<Option<Box<[Ty]>>>()?;
            return Some(Ty::Tuple(members));
        }
        // `Self::Item`, `<I as Iterator>::Item`: the iterator's items.
        if let Some((_, "Item")) = projection(text) {
            return self.items.clone();
        }
        let (name, arguments) = match text.find('<') {
            Some(open) if text.ends_with('>') => (&text[..open], &text[open + 1..text.len() - 1]),
            Some(_) => return None,
            None => (text, ""),
        };
        if arguments.is_empty()
            && let Some((_, ty)) = self.types.iter().find(|(known, _)| *known == name)
        {
            return Some(ty.clone());
        }
        let definition = std_table::definition_of(name)?;
        let arguments = std_table::split_list(arguments)
            .into_iter()
            .filter(|argument| !argument.starts_with('\''))
            .map(|argument| self.ty(argument))
            .collect::<Option<Vec<_>>>()?;
        std_impl::std_type(definition, &arguments)
    }
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
        Written::Slice(element) => match ty {
            Ty::Array {
                element: found,
                kind: sequence::Kind::Slice,
            } => fits(element, found),
            _ => false,
        },
        Written::Array(element, _) => match ty {
            Ty::Array {
                element: found,
                kind: sequence::Kind::Array(_),
            } => fits(element, found),
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
/// An iterator's methods are read the same way, its adapters through
/// std's `Iterator` impls for them (`Map<Self, F>` gives `B`, `Cloned<Self>`
/// the `T` of `&T` items) and `collect`'s value through std's
/// `FromIterator` impl for it.
///
/// `receiver` is the native receiver (a reference around it is the
/// call's), `output` the call's value, which picks a `SliceIndex`
/// projection's type: `[T]` for `v.get(1..3)`, `T` for `v.get(1)`. `None`
/// for a method std doesn't declare on the type, or one whose value holds
/// what these facts can't place.
pub(super) fn summary(receiver: &Ty, output: &Ty, member: &str) -> Option<Summary> {
    let mut native = receiver;
    while let Ty::Reference { target, .. } = native {
        native = target;
    }
    let found = inherent(native, member)?;
    let declared = std_table::Declared::parse(found.item.declaration())?;
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
        generics: Vec::new(),
        iterator: None,
    };
    // `F: FnOnce(T) -> U`: what each closure parameter returns.
    for generic in std_table::split_list(declared.generics) {
        match std_table::top_level(generic, ": ") {
            Some(colon) => {
                reader.closure(&generic[..colon], &generic[colon + 2..]);
                reader
                    .generics
                    .push((&generic[..colon], &generic[colon + 2..]));
            }
            None => reader.generics.push((generic, "")),
        }
    }
    for clause in std_table::split_list(declared.wheres) {
        if let Some(colon) = std_table::top_level(clause, ": ") {
            reader.closure(&clause[..colon], &clause[colon + 2..]);
            reader
                .generics
                .push((&clause[..colon], &clause[colon + 2..]));
        }
    }
    let owner = match native {
        // `Self` of an iterator's method: what holds its items.
        Ty::Iter(item) if ITERATOR.contains(&found.owner) => {
            let items = reader.items(item, "Self::Item");
            let own = reader.iterator_self(items);
            reader.iterator = Some(own.clone());
            // `I` of `impl<I: Iterator> Peekable<I>`: the iterator below,
            // whose items are the same.
            if let Some(line) =
                std_table::impls("core::iter::traits::iterator::Iterator", found.owner).next()
            {
                for (subject, bounds) in line.wheres() {
                    if std_table::split_bounds(bounds).contains(&"Iterator") {
                        reader.substitution.push((subject, own.clone()));
                    }
                }
            }
            reader.collected(output);
            own
        }
        _ => match found.item.impl_self() {
            Some(own) => reader.ty(own)?,
            None => reader.owner(found.owner)?,
        },
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
    if reader.iterator.is_some() && reader.unplaced(&output_ty, &parameters) {
        return None;
    }
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
    if let Some(taps) = &mut summary.output {
        for (index, parameter) in parameters.iter().enumerate() {
            let Type::Path(path) = parameter else {
                continue;
            };
            // What a closure parameter returns is where its bound's output
            // is: `B` of `map`, and of `filter_map`'s `Option<B>`.
            if let Some((_, returns)) = reader
                .closures
                .iter()
                .find(|(name, _)| *name == path.name.as_str())
            {
                let mut places = Vec::new();
                if Some(*returns) == declared.output {
                    places.push(RegionPath::ROOT);
                } else {
                    let mut names = Vec::new();
                    named(&std_table::written(returns), &mut names);
                    for name in names {
                        parameter_paths(&output_ty, RegionPath::ROOT, name, &mut places);
                    }
                }
                for into in places {
                    taps.push(Tap {
                        into,
                        from: Source::Call(index),
                    });
                }
            }
            // `U: IntoIterator<Item = Self::Item>` of `chain`: its items
            // are where the receiver's are.
            if reader.iterator.is_some()
                && let Some(item) = reader.bound_item(path.name.as_str())
            {
                let mut places = Vec::new();
                type_paths(&output_ty, &item, RegionPath::ROOT, &mut places);
                for into in places {
                    taps.push(Tap {
                        into,
                        from: Source::Input(signature::Input::Argument(index), RegionPath::ROOT),
                    });
                }
            }
        }
    }
    Some(summary)
}

/// What a closure bound returns: `B` of `FnMut(T) -> B`, `()` of `FnMut(T)`.
fn returns(bound: &'static str) -> Option<&'static str> {
    ["FnOnce(", "FnMut(", "Fn("]
        .iter()
        .any(|closure| bound.starts_with(closure))
        .then(|| {
            std_table::top_level(bound, " -> ").map_or("()", |arrow| bound[arrow + 4..].trim())
        })
}

/// `W` of the binding `name = W` a bound writes: `Item` of
/// `IntoIterator<Item = Self::Item>`.
fn binding(bound: &'static str, name: &str) -> Option<&'static str> {
    let open = bound.find('<')?;
    let inner = bound[open + 1..].strip_suffix('>')?;
    std_table::split_list(inner)
        .into_iter()
        .find_map(|argument| argument.strip_prefix(name)?.strip_prefix(" = "))
}

/// The iterator and the name of an iterator projection std writes:
/// `Self::Item`, `<I as Iterator>::Item`, `<U as IntoIterator>::IntoIter`.
fn projection(text: &'static str) -> Option<(&'static str, &'static str)> {
    let (base, name) = if text.starts_with('<') {
        let close = std_table::closing(text, '<', '>')?;
        let inner = &text[1..close];
        let at = std_table::top_level(inner, " as ")?;
        let trait_ = inner[at + 4..].rsplit("::").next()?;
        if !matches!(trait_, "Iterator" | "IntoIterator" | "DoubleEndedIterator") {
            return None;
        }
        (&inner[..at], text[close + 1..].strip_prefix("::")?)
    } else {
        let (base, name) = text.split_once("::")?;
        if base.contains('<') {
            return None;
        }
        (base, name)
    };
    matches!(name, "Item" | "IntoIter").then_some((base, name))
}

/// Whether a type is an iterator: rawr's, or an iterator method's `Self`.
fn iterator_shaped(path: &TypePath) -> bool {
    match path.target {
        Target::Builtin => matches!(
            path.name.as_str(),
            crate::item::type_ref::ITER_TYPE | crate::item::type_ref::INTO_ITER_TYPE
        ),
        Target::Parameter => path.name.as_str() == ITERATOR_SELF,
        _ => false,
    }
}

/// The type parameters `ty` has.
fn parameter_names(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::Unit(_) => {}
        Type::Tuple { members, .. } => {
            for member in members.iter() {
                parameter_names(member, out);
            }
        }
        Type::Reference { target, .. } => parameter_names(target, out),
        Type::Path(path) => {
            if path.target == Target::Parameter {
                out.push(path.name.to_string());
            }
            for argument in path.arguments.iter() {
                if let Argument::Type(argument) = argument {
                    parameter_names(argument, out);
                }
            }
        }
    }
}

/// The names of the paths `written` has: `B` of `Option<B>`.
fn named(written: &Written, out: &mut Vec<&'static str>) {
    match written {
        Written::Path {
            path,
            arguments,
            bindings,
        } => {
            out.push(path);
            for argument in arguments.iter().chain(bindings.iter().map(|(_, ty)| ty)) {
                named(argument, out);
            }
        }
        Written::Reference { target, .. } => named(target, out),
        Written::Slice(element) | Written::Array(element, _) => named(element, out),
        Written::Tuple(members) => {
            for member in members {
                named(member, out);
            }
        }
        Written::Never | Written::Other(_) => {}
    }
}

/// The positions in `ty` where `target` is.
fn type_paths(ty: &Type, target: &Type, at: RegionPath, out: &mut Vec<RegionPath>) {
    if ty == target {
        out.push(at);
        return;
    }
    match ty {
        Type::Unit(_) => {}
        Type::Tuple { members, .. } => {
            for (index, member) in members.iter().enumerate() {
                type_paths(member, target, at.push(RegionStep::Arg(index as u8)), out);
            }
        }
        Type::Reference { target: inner, .. } => {
            type_paths(inner, target, at.push(RegionStep::Arg(0)), out)
        }
        Type::Path(path) => {
            let mut index = 0;
            for argument in path.arguments.iter() {
                if let Argument::Type(argument) = argument {
                    type_paths(argument, target, at.push(RegionStep::Arg(index)), out);
                    index += 1;
                }
            }
        }
    }
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
    /// An iterator's impl parameters, as its path's arguments give them,
    /// and the type parameters a call picks (`B` of `collect`).
    substitution: Vec<(&'static str, Type)>,
    /// The item of the call's value, an iterator, for one whose `Iterator`
    /// impl the table doesn't have (`str::Split`'s needs the unstable
    /// `Pattern`).
    output_item: Option<Ty>,
    /// Each closure parameter and what its bound returns: `F`, `U`.
    closures: Vec<(&'static str, &'static str)>,
    /// The function's type parameters and where-clauses, each with its
    /// bounds: (`U`, `IntoIterator<Item = Self::Item>`).
    generics: Vec<(&'static str, &'static str)>,
    /// `Self` of an iterator's method, which holds its items.
    iterator: Option<Type>,
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
            if let Some(returns) = returns(bound) {
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
            "alloc::collections::vec_deque::VecDeque" => "std::collections::VecDeque<T>",
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
        // `Self::Item`, `<I as Iterator>::Item`, `<U as IntoIterator>::IntoIter`:
        // what an iterator's method names of the iterators it takes.
        if self.iterator.is_some()
            && let Some((base, name)) = projection(text)
        {
            let base = self.ty(base)?;
            return Some(match name {
                "Item" => self.item_of(base),
                _ => self.iterated(base),
            });
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
            return Some(match &self.iterator {
                Some(own) => own.clone(),
                None => self.parameter("Self"),
            });
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
            "alloc::collections::vec_deque::VecDeque" => Some(("VecDeque", 1)),
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
        // The impl's other parameters, as its where-clauses give them; one
        // they don't is a type these facts can't place.
        let generics: Vec<&'static str> = line.generics().collect();
        self.bind(&line, &generics, &saved);
        for generic in &generics {
            if !self.bound(generic) {
                let unknown = self.unknown(generic);
                self.substitution.push((generic, unknown));
            }
        }
        let item = self.ty(item);
        self.substitution = saved;
        let item = item?;
        Some(self.builtin(crate::item::type_ref::ITER_TYPE, vec![Argument::Type(item)]))
    }

    /// Whether the parameter `name` is given.
    fn bound(&self, name: &str) -> bool {
        self.substitution.iter().any(|(param, _)| *param == name)
    }

    /// Gives the impl `line`'s parameters `generics` that its where-clauses
    /// determine: `T` of `I: Iterator<Item = &T>`, `U` of `I::Item:
    /// IntoIterator<IntoIter = U>`, and `B` of `F: FnMut(I::Item) -> B` where
    /// `F` is the call's closure, whose bound in the call's own `outer`
    /// parameters says what it returns.
    fn bind(
        &mut self,
        line: &std_table::Impl,
        generics: &[&'static str],
        outer: &[(&'static str, Type)],
    ) {
        // A clause can name a parameter a later one gives.
        for _ in 0..2 {
            for (subject, bounds) in line.wheres() {
                if generics.contains(&subject) && !self.bound(subject) {
                    continue;
                }
                let Some(ty) = self.ty(subject) else {
                    continue;
                };
                for bound in std_table::split_bounds(bounds) {
                    if let Some(written) = returns(bound) {
                        let Type::Path(path) = &ty else {
                            continue;
                        };
                        let Some((_, returned)) = self
                            .closures
                            .iter()
                            .find(|(name, _)| *name == path.name.as_str())
                            .copied()
                        else {
                            continue;
                        };
                        let inner = std::mem::replace(&mut self.substitution, outer.to_vec());
                        let found = self.ty(returned);
                        self.substitution = inner;
                        if let Some(found) = found {
                            self.unify(&std_table::written(written), &found, generics);
                        }
                        continue;
                    }
                    if let Some(item) = binding(bound, "Item") {
                        let found = self.item_of(ty.clone());
                        self.unify(&std_table::written(item), &found, generics);
                    }
                    if let Some(iterator) = binding(bound, "IntoIter") {
                        let found = self.iterated(ty.clone());
                        self.unify(&std_table::written(iterator), &found, generics);
                    }
                }
            }
        }
    }

    /// Gives each of `generics` that `written` has the part of `ty` where it
    /// is: `T` of `&T` against `&'a u8`. A type parameter in `ty` holds all
    /// of what its parts hold.
    fn unify(&mut self, written: &Written, ty: &Type, generics: &[&'static str]) {
        match (written, ty) {
            (
                Written::Path {
                    path, arguments, ..
                },
                _,
            ) if arguments.is_empty() && generics.contains(path) => {
                if !self.bound(path) {
                    self.substitution.push((path, ty.clone()));
                }
            }
            (Written::Reference { target, .. }, Type::Reference { target: found, .. }) => {
                self.unify(target, found, generics)
            }
            (Written::Tuple(members), Type::Tuple { members: found, .. })
                if members.len() == found.len() =>
            {
                for (member, found) in members.iter().zip(found.iter()) {
                    self.unify(member, found, generics);
                }
            }
            (Written::Path { arguments, .. }, Type::Path(found))
                if found.target == Target::Builtin =>
            {
                let found: Vec<&Type> = found
                    .arguments
                    .iter()
                    .filter_map(|argument| match argument {
                        Argument::Type(ty) => Some(ty),
                        Argument::Lifetime(_) => None,
                    })
                    .collect();
                for (argument, found) in arguments.iter().zip(found) {
                    self.unify(argument, found, generics);
                }
            }
            (_, Type::Path(found)) if found.target == Target::Parameter => {
                let mut names = Vec::new();
                named(written, &mut names);
                for name in names {
                    if generics.contains(&name) && !self.bound(name) {
                        self.substitution.push((name, ty.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    /// The items of rawr's iterator of `item`, each region of them named
    /// by where it is (`at`): `Self::Item` of an iterator's method.
    fn items(&mut self, item: &Ty, at: &str) -> Type {
        match item {
            Ty::Reference { mutable, target } => {
                let lifetime = self.lifetime(Some(at));
                let target = self.items(target, &format!("{at}.0"));
                Type::Reference {
                    mutable: *mutable,
                    lifetime,
                    target: Box::new(target),
                    span: self.span(),
                }
            }
            Ty::Str => {
                let lifetime = self.lifetime(Some(at));
                self.text(lifetime)
            }
            Ty::Tuple(members) => {
                let members = members
                    .iter()
                    .enumerate()
                    .map(|(index, member)| self.items(member, &format!("{at}.{index}")))
                    .collect();
                Type::Tuple {
                    members,
                    span: self.span(),
                }
            }
            Ty::Unit => Type::Unit(self.span()),
            _ => self.parameter(at),
        }
    }

    /// `Self` of an iterator's method: a type parameter holding `items`.
    fn iterator_self(&mut self, items: Type) -> Type {
        Type::Path(TypePath {
            name: ITERATOR_SELF.into(),
            target: Target::Parameter,
            arguments: vec![Argument::Type(items)].into_boxed_slice(),
            span: self.span(),
        })
    }

    /// The items of `ty`: an iterator's, those its bound gives a type
    /// parameter (`U: IntoIterator<Item = Self::Item>`), else what a
    /// collection holds, which its items hold at most.
    fn item_of(&mut self, ty: Type) -> Type {
        if let Type::Path(path) = &ty {
            if iterator_shaped(path)
                && let Some(item) = path.arguments.iter().find_map(|argument| match argument {
                    Argument::Type(item) => Some(item),
                    Argument::Lifetime(_) => None,
                })
            {
                return item.clone();
            }
            if path.target == Target::Parameter
                && let Some(item) = self.bound_item(path.name.as_str())
            {
                return item;
            }
        }
        ty
    }

    /// The iterator `ty` gives as `IntoIterator`: itself, or one of its
    /// items.
    fn iterated(&mut self, ty: Type) -> Type {
        if let Type::Path(path) = &ty
            && iterator_shaped(path)
        {
            return ty;
        }
        let item = self.item_of(ty);
        self.builtin(crate::item::type_ref::ITER_TYPE, vec![Argument::Type(item)])
    }

    /// `W` of the function's type parameter `name` bound by
    /// `IntoIterator<Item = W>`.
    fn bound_item(&mut self, name: &str) -> Option<Type> {
        let written = self
            .generics
            .iter()
            .filter(|(generic, _)| *generic == name)
            .flat_map(|(_, bounds)| std_table::split_bounds(bounds))
            .find_map(|bound| binding(bound, "Item"))?;
        self.ty(written)
    }

    /// Gives the function's type parameter bound by `FromIterator` (`B` of
    /// `collect`) the call's value `output`, as std's `FromIterator` impl
    /// for it says it holds the items: `Vec<T>` of `T`, `HashMap<K, V>` of
    /// `(K, V)`.
    fn collected(&mut self, output: &Ty) {
        let Some((definition, _)) = std_impl::std_definition(output) else {
            return;
        };
        let Some(own) = self.iterator.clone() else {
            return;
        };
        let items = self.item_of(own);
        for (name, bounds) in self.generics.clone() {
            if !std_table::split_bounds(bounds)
                .iter()
                .any(|bound| bound.starts_with("FromIterator<"))
            {
                continue;
            }
            for line in std_table::impls("core::iter::traits::collect::FromIterator", definition) {
                let arguments = line.arguments();
                let [argument] = arguments[..] else {
                    continue;
                };
                if !fits(&std_table::written(line.self_ty), output) {
                    continue;
                }
                let generics: Vec<&'static str> = line.generics().collect();
                let saved = std::mem::take(&mut self.substitution);
                self.unify(&std_table::written(argument), &items, &generics);
                for generic in &generics {
                    if !self.bound(generic) {
                        let unknown = self.unknown(generic);
                        self.substitution.push((generic, unknown));
                    }
                }
                let collection = self.ty(line.self_ty);
                self.substitution = saved;
                if let Some(collection) = collection {
                    self.substitution.push((name, collection));
                    break;
                }
            }
        }
    }

    /// Whether the call's value `output` has a type parameter the call picks
    /// that no input gives and whose bound may make it of the items
    /// (`S: Sum<Self::Item>`, `B: Extend<Self::Item>`), which these facts
    /// can't place.
    fn unplaced(&self, output: &Type, parameters: &[Type]) -> bool {
        let mut given = Vec::new();
        for parameter in parameters {
            parameter_names(parameter, &mut given);
        }
        for (_, returns) in &self.closures {
            let mut names = Vec::new();
            named(&std_table::written(returns), &mut names);
            given.extend(names.into_iter().map(str::to_owned));
        }
        let mut found = Vec::new();
        parameter_names(output, &mut found);
        found.iter().any(|name| {
            !given.contains(name)
                && self
                    .generics
                    .iter()
                    .any(|(generic, bounds)| *generic == name.as_str() && bounds.contains('<'))
        })
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
        Ty::sequence(sequence::Kind::Vec, element)
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

    /// An iterator's methods and what each keeps come from `Iterator`'s
    /// declaration and std's impls for its adapters (#768): an adapter
    /// holds the iterator and closure it takes, its items are where the
    /// declaration says, and `collect` holds them where `FromIterator` for
    /// its value puts them.
    #[test]
    fn iterator_summaries_come_from_declarations() {
        use signature::{Input, Receiver};
        let at = |steps: &[RegionStep]| RegionPath::of(steps);
        let receiver = |steps: &[RegionStep]| Source::Input(Input::Receiver, at(steps));
        let argument =
            |index, steps: &[RegionStep]| Source::Input(Input::Argument(index), at(steps));
        let tap = |into: &[RegionStep], from| Tap {
            into: at(into),
            from,
        };
        let (own, a0, a1) = (RegionStep::Own, RegionStep::Arg(0), RegionStep::Arg(1));
        let reference = Ty::Reference {
            mutable: false,
            target: Box::new(Ty::Int(Int::I64)),
        };
        let iter = |item: Ty| Ty::Iter(Box::new(item));
        let refs = iter(reference.clone());
        let taps = |member: &str, output: &Ty| {
            let found = summary(&refs, output, member).unwrap_or_else(|| panic!("{member}"));
            (
                found.mode,
                found.output.unwrap_or_else(|| panic!("{member}")),
            )
        };
        let has = |member: &str, output: &Ty, mode, expected: &[Tap]| {
            let (found, taps) = taps(member, output);
            assert_eq!(found, Some(mode), "{member}");
            for expected in expected {
                assert!(
                    taps.contains(expected),
                    "{member}: {expected:?} in {taps:?}"
                );
            }
        };
        let adapted = [
            tap(&[own], receiver(&[own])),
            tap(&[own], argument(0, &[own])),
        ];
        // `map`'s items are what its closure returns, `filter`'s the
        // receiver's.
        let mut map = adapted.to_vec();
        map.push(tap(&[a0], Source::Call(0)));
        has("map", &iter(Ty::Unknown), Receiver::Owned, &map);
        has("filter_map", &iter(Ty::Unknown), Receiver::Owned, &map);
        let mut filter = adapted.to_vec();
        filter.extend([
            tap(&[a0, own], receiver(&[a0, own])),
            tap(&[a0, a0], receiver(&[a0, a0])),
        ]);
        has("filter", &refs, Receiver::Owned, &filter);
        // `Copied<I>`'s items are the `T` of `I: Iterator<Item = &T>`.
        has(
            "copied",
            &iter(Ty::Int(Int::I64)),
            Receiver::Owned,
            &[
                tap(&[own], receiver(&[own])),
                tap(&[a0], receiver(&[a0, a0])),
            ],
        );
        has(
            "enumerate",
            &Ty::Unknown,
            Receiver::Owned,
            &[tap(&[a0, a1, own], receiver(&[a0, own]))],
        );
        // `zip`'s and `chain`'s items hold the argument's.
        has(
            "zip",
            &Ty::Unknown,
            Receiver::Owned,
            &[
                tap(&[a0, a0, own], receiver(&[a0, own])),
                tap(&[a0, a1], argument(0, &[])),
            ],
        );
        has(
            "chain",
            &refs,
            Receiver::Owned,
            &[
                tap(&[a0, own], receiver(&[a0, own])),
                tap(&[a0], argument(0, &[])),
            ],
        );
        has(
            "next",
            &Ty::Option(Box::new(reference.clone())),
            Receiver::Mutable,
            &[tap(&[a0, own], receiver(&[a0, a0, own]))],
        );
        // `Peekable::peek` borrows the iterator.
        has(
            "peek",
            &Ty::Unknown,
            Receiver::Mutable,
            &[
                tap(&[a0, own], receiver(&[own])),
                tap(&[a0, a0, own], receiver(&[a0, a0, own])),
            ],
        );
        has(
            "by_ref",
            &Ty::Unknown,
            Receiver::Mutable,
            &[tap(&[own], receiver(&[own])), tap(&[a0], receiver(&[a0]))],
        );
        has(
            "into_iter",
            &refs,
            Receiver::Owned,
            &[tap(&[], receiver(&[]))],
        );
        // `FromIterator<T> for Vec<T>`: the items are the elements.
        let collected = Ty::sequence(sequence::Kind::Vec, reference.clone());
        has(
            "collect",
            &collected,
            Receiver::Owned,
            &[tap(&[a0, own], receiver(&[a0, own]))],
        );
        // A value the declaration can't place is left to rustc.
        assert!(summary(&refs, &Ty::Unknown, "collect").is_none());
        assert!(summary(&refs, &Ty::Int(Int::I64), "sum").is_none());
        // `count` keeps nothing.
        assert_eq!(taps("count", &Ty::Int(Int::Usize)).1, Vec::new());
    }
}
