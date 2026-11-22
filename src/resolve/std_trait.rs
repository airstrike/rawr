//! std's traits from the std table (D711 step 2): a program implements and
//! bounds by any stable std trait, whose declaration is std's own, read as
//! a program's trait is.
//!
//! A trait's declaration lists its required items, with the signatures std
//! declares them with. A provided function is std's to run: an impl may
//! override it, and a call rawr can't run yet fails at its use (D712.1).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use super::{host, item, program};
use crate::std_table::{self, Namespace, Stability};
use crate::Span;

/// The edition whose prelude a program sees.
const EDITION: &str = "2024";

thread_local! {
    /// The std traits the program being resolved names by a path
    /// (`std::ops::Deref`, `fmt::Write` after `use std::fmt;`), by their
    /// last segment, which its bounds and impls carry.
    static WRITTEN: RefCell<BTreeMap<crate::ImmutableString, &'static str>> =
        const { RefCell::new(BTreeMap::new()) };

    /// What each resolution under way that is kept wrote of `WRITTEN`,
    /// innermost last (`writing`).
    static WRITING: RefCell<Vec<Vec<Written>>> = const { RefCell::new(Vec::new()) };
}

/// One path's naming of a std trait: its last segment and the trait's
/// definition.
pub(crate) type Written = (crate::ImmutableString, &'static str);

/// Start resolving a program: no std trait is named by a path yet.
pub(crate) fn begin() {
    WRITTEN.with_borrow_mut(BTreeMap::clear);
}

/// The program names the std trait `definition` by a path whose last
/// segment is `name`.
pub(crate) fn written(name: &str, definition: &'static str) {
    let name = crate::ImmutableString::from(name);
    WRITING.with_borrow_mut(|writing| {
        for frame in writing.iter_mut() {
            frame.push((name.clone(), definition));
        }
    });
    WRITTEN.with_borrow_mut(|written| {
        written.entry(name).or_insert(definition);
    });
}

/// What `write` gives, with each naming it wrote, in order: a resolution
/// that reuses it writes them again, as `write` would (#1876).
pub(crate) fn writing<T>(write: impl FnOnce() -> T) -> (T, Vec<Written>) {
    /// Takes the frame back off, as `write` ends or unwinds.
    struct Frame;

    impl Drop for Frame {
        fn drop(&mut self) {
            WRITING.with_borrow_mut(|writing| writing.pop());
        }
    }

    WRITING.with_borrow_mut(|writing| writing.push(Vec::new()));
    let frame = Frame;
    let value = write();
    let names = WRITING.with_borrow_mut(|writing| writing.last_mut().map(std::mem::take));
    drop(frame);
    (value, names.unwrap_or_default())
}

/// The stable std trait a program names `name` by, as rustc's scope has it:
/// through a `use` that binds it (`use std::str::FromStr;`), the prelude
/// (`Iterator`), or a path the program writes (`std::ops::Deref`). Its
/// definition: `core::str::traits::FromStr`. A bare name nothing brings
/// into scope is none (rustc's E0405).
pub(crate) fn named<'a>(
    name: &str,
    uses: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<&'static str> {
    for (binding, qualified) in uses {
        if binding == name
            && let Some(definition) = std_table::stable_trait(qualified)
        {
            return Some(definition);
        }
    }
    if let Some(found) = std_table::prelude(EDITION)
        .find(|path| path.kind == "trait" && path.path.rsplit("::").next() == Some(name))
    {
        return (found.stability == Stability::Stable).then_some(found.definition);
    }
    WRITTEN.with_borrow(|written| written.get(name).copied())
}

/// The stable std trait whose last segment is `name`, wherever std declares
/// it: `named`, else the first std module that has one (`std::error::Error`).
/// For what rawr asks of a trait it already knows the program names, never
/// for what the program's scope has.
pub(crate) fn anywhere(name: &str) -> Option<&'static str> {
    named(name, []).or_else(|| {
        std_table::children("std")
            .filter(|module| module.kind == "mod" && module.stability == Stability::Stable)
            .find_map(|module| std_table::stable_trait(&format!("{}::{name}", module.path)))
    })
}

/// The definition of the stable std trait whose last segment is `name`, as
/// a written path names it (`std::fmt::Display`, `fmt::Debug` after `use
/// std::fmt;`).
pub(crate) fn by_path(segments: &[&str]) -> Option<&'static str> {
    let mut path = segments.to_vec();
    if !path.first().is_some_and(|root| std_table::is_root(root)) {
        path.insert(0, "std");
    }
    match std_table::resolve(&path, Namespace::Type) {
        Ok(std_table::Resolved::Path(path))
            if path.kind == "trait" && path.stability == Stability::Stable =>
        {
            if let Some(name) = segments.last() {
                written(name, path.definition);
            }
            Some(path.definition)
        }
        _ => None,
    }
}

/// A std trait's definition and the provided functions its declaration
/// carries.
type Key = (&'static str, Vec<&'static str>);

thread_local! {
    /// Each std trait's declaration, by its definition and the provided
    /// functions it carries, resolved once per thread.
    static DECLARED: RefCell<HashMap<Key, Option<item::Trait>>> =
        RefCell::new(HashMap::new());
}

/// The declaration of the stable std trait `definition`, resolved as a
/// program's own trait is: its generics with their defaults, its
/// supertraits, its associated types with their bounds, its required
/// functions, and the provided functions named in `overridden`, which an
/// impl of the program overrides (`Iterator::size_hint`, `Error::source`).
///
/// A function whose signature names a type rawr has no representation for
/// yet (`io::Write::write`'s `io::Error`) is left out, so an impl declares
/// fine and its own signature says so.
pub(crate) fn declaration(
    definition: &str,
    overridden: &[&str],
    host: &host::Registry,
) -> Option<item::Trait> {
    let definition = std_table::item(definition)?.definition;
    let mut provided = std_table::members(definition)
        .filter(|member| {
            member.kind == "assoc-fn"
                && member.provided()
                && member.stability == Stability::Stable
                && (overridden.contains(&member.name())
                    || body(definition, member.name()).is_some())
        })
        .map(|member| member.name())
        .collect::<Vec<_>>();
    provided.sort_unstable();
    let key = (definition, provided);
    if let Some(found) = DECLARED.with_borrow(|declared| declared.get(&key).cloned()) {
        return found;
    }
    let declared = resolve(definition, &key.1, host);
    DECLARED.with_borrow_mut(|cache| cache.insert(key, declared.clone()));
    declared
}

/// The std traits rawr reads without their `Rhs = Self` parameter, which
/// its runtime compares by (`RUNS`): a binding until #708.
const SELF_ONLY: &[&str] = &["core::cmp::PartialEq", "core::cmp::PartialOrd"];

fn resolve(
    definition: &'static str,
    provided: &[&str],
    host: &host::Registry,
) -> Option<item::Trait> {
    let mut source = std_table::trait_source(definition, provided, |name| body(definition, name))?;
    // rawr compares a value with one of its own type (#708, D711 step 3):
    // `PartialEq` and `PartialOrd` are read with their `Rhs` as `Self`.
    if SELF_ONLY.contains(&definition) {
        source = source
            .replace("<Rhs = Self>", "")
            .replace("<Rhs>", "")
            .replace("Rhs", "Self");
    }
    let mut parsed = crate::parser::parse_trait(&source)?;
    let attempt = |parsed: crate::item::Trait| {
        super::resolve_item(
            crate::item::Id::from_index(0),
            program::source::Id::ROOT,
            None,
            None,
            crate::item::Parsed::Trait(parsed),
            &BTreeMap::new(),
            host,
            #[cfg(feature = "rust-import")]
            None,
        )
        .ok()
    };
    let shell = match attempt(parsed.clone()) {
        Some(shell) => shell,
        None => {
            // Each function that resolves on its own.
            let kept = parsed
                .functions
                .iter()
                .zip(parsed.provided.iter())
                .filter(|(function, provided)| {
                    let mut one = parsed.clone();
                    one.functions = Box::new([(*function).clone()]);
                    one.provided = Box::new([**provided]);
                    attempt(one).is_some()
                })
                .map(|(function, provided)| (function.clone(), *provided))
                .collect::<Vec<_>>();
            parsed.provided = kept.iter().map(|(_, provided)| *provided).collect();
            parsed.functions = kept.into_iter().map(|(function, _)| function).collect();
            attempt(parsed)?
        }
    };
    let item::Kind::Trait(mut declared) = shell.kind else {
        return None;
    };
    // `Self::Output` of `Add` is `<Self as Add>::Output`, as std means it:
    // a bound by several traits names each one's own.
    let own = declared
        .associated
        .iter()
        .map(|name| name.text.clone())
        .collect::<Vec<_>>();
    let trait_name = declared.name.text.clone();
    let mut functions = declared.functions.to_vec();
    for function in &mut functions {
        for parameter in function.parameters.iter_mut() {
            qualify(&mut parameter.ty, &trait_name, &own);
        }
        if let Some(output) = &mut function.output {
            qualify(output, &trait_name, &own);
        }
    }
    declared.functions = functions.into_boxed_slice();
    // std's: no span of the program's.
    declared.name.span = Span::NONE;
    declared.std = Some(definition);
    // The text std's bodies are read from (`body`).
    declared.declaration = Some(source.into());
    Some(declared)
}

/// std's body of the provided function `name` of the std trait
/// `definition`, which an impl of the program's that doesn't override it
/// receives as a program trait's default (#2461): its declaration with the
/// body, as std writes it where rawr runs what it calls.
pub(crate) fn body(definition: &str, name: &str) -> Option<&'static str> {
    BODIES
        .iter()
        .find(|(trait_, function, _)| *trait_ == definition && *function == name)
        .map(|(_, _, body)| *body)
}

/// Each std trait's provided functions rawr runs from std's body (`body`),
/// by the trait's definition and the function's name.
const BODIES: &[(&str, &str, &str)] = &[
    (
        "core::io::write::Write",
        "write_all",
        r#"fn write_all(&mut self, mut buf: &[u8]) -> std::io::Result<()> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "failed to write whole buffer",
                    ));
                }
                Ok(n) => buf = &buf[n..],
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }"#,
    ),
    (
        "core::io::write::Write",
        "write_fmt",
        "fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> std::io::Result<()> {
        self.write_all(args.to_string().as_bytes())
    }",
    ),
    (
        "std::io::Read",
        "read_to_end",
        r#"fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize> {
        let start = buf.len();
        let mut chunk = [0u8; 32];
        loop {
            match self.read(&mut chunk) {
                Ok(0) => return Ok(buf.len() - start),
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }"#,
    ),
    (
        "std::io::Read",
        "read_to_string",
        r#"fn read_to_string(&mut self, buf: &mut String) -> std::io::Result<usize> {
        let mut bytes = Vec::new();
        let read = self.read_to_end(&mut bytes)?;
        match String::from_utf8(bytes) {
            Ok(text) => {
                buf.push_str(&text);
                Ok(read)
            }
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )),
        }
    }"#,
    ),
    (
        "std::io::Read",
        "read_exact",
        r#"fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        if filled == buf.len() {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "failed to fill whole buffer",
            ))
        }
    }"#,
    ),
    (
        "core::hash::Hasher",
        "write_u8",
        "fn write_u8(&mut self, i: u8) {
        self.write(&[i])
    }",
    ),
    (
        "core::hash::Hasher",
        "write_u16",
        "fn write_u16(&mut self, i: u16) {
        self.write(&i.to_ne_bytes())
    }",
    ),
    (
        "core::hash::Hasher",
        "write_u32",
        "fn write_u32(&mut self, i: u32) {
        self.write(&i.to_ne_bytes())
    }",
    ),
    (
        "core::hash::Hasher",
        "write_u64",
        "fn write_u64(&mut self, i: u64) {
        self.write(&i.to_ne_bytes())
    }",
    ),
    (
        "core::hash::Hasher",
        "write_u128",
        "fn write_u128(&mut self, i: u128) {
        self.write(&i.to_ne_bytes())
    }",
    ),
    (
        "core::hash::Hasher",
        "write_usize",
        "fn write_usize(&mut self, i: usize) {
        self.write(&i.to_ne_bytes())
    }",
    ),
    (
        "core::hash::Hasher",
        "write_i8",
        "fn write_i8(&mut self, i: i8) {
        self.write_u8(i as u8)
    }",
    ),
    (
        "core::hash::Hasher",
        "write_i16",
        "fn write_i16(&mut self, i: i16) {
        self.write_u16(i as u16)
    }",
    ),
    (
        "core::hash::Hasher",
        "write_i32",
        "fn write_i32(&mut self, i: i32) {
        self.write_u32(i as u32)
    }",
    ),
    (
        "core::hash::Hasher",
        "write_i64",
        "fn write_i64(&mut self, i: i64) {
        self.write_u64(i as u64)
    }",
    ),
    (
        "core::hash::Hasher",
        "write_i128",
        "fn write_i128(&mut self, i: i128) {
        self.write_u128(i as u128)
    }",
    ),
    (
        "core::hash::Hasher",
        "write_isize",
        "fn write_isize(&mut self, i: isize) {
        self.write_usize(i as usize)
    }",
    ),
    (
        "core::error::Error",
        "source",
        "fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }",
    ),
    (
        "core::iter::traits::iterator::Iterator",
        "by_ref",
        "fn by_ref(&mut self) -> &mut Self {
        self
    }",
    ),
    (
        "core::fmt::Write",
        "write_fmt",
        "fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> std::fmt::Result {
        self.write_str(&args.to_string())
    }",
    ),
    (
        "core::fmt::Write",
        "write_char",
        "fn write_char(&mut self, c: char) -> std::fmt::Result {
        self.write_str(&c.to_string())
    }",
    ),
];

/// std's impls of its traits for its own types, by the trait's definition,
/// with std's bodies where rawr runs what they call: each one a program
/// names the trait of is the program's impl, so a generic bounded by the
/// trait runs it as it runs the program's own (#2483). `{trait}` is the
/// name the program knows the trait by.
const IMPLS: &[(&str, &str)] = &[
    (
        "core::fmt::Write",
        "impl {trait} for String {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.push_str(s);
        Ok(())
    }
    fn write_char(&mut self, c: char) -> std::fmt::Result {
        self.push(c);
        Ok(())
    }
}",
    ),
    (
        "core::io::write::Write",
        "impl {trait} for Vec<u8> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.extend_from_slice(buf);
        Ok(())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}",
    ),
];

/// std's impls (`IMPLS`) of the std traits the program names, `traits`,
/// by the names it knows them by, each resolved as a program's impl is,
/// its bodies read from std's text. A trait of one that only a `use` of
/// `items` brings into scope, whose methods a call on std's value names,
/// joins `traits`. `next` numbers their items.
pub(crate) fn impls(
    traits: &mut BTreeMap<crate::ImmutableString, item::Trait>,
    items: &[item::Shell],
    next: usize,
    host: &host::Registry,
) -> Vec<item::Shell> {
    let declared = items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Trait(own) => Some(own.name.text.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    for (name, qualified) in uses(items) {
        if traits.contains_key(&name) || declared.contains(&name) {
            continue;
        }
        if let Some(definition) = std_table::stable_trait(&qualified)
            && IMPLS
                .iter()
                .any(|(implemented, _)| *implemented == definition)
            && let Some(trait_) = declaration(definition, &[], host)
        {
            traits.insert(name, trait_);
        }
    }
    let mut shells = Vec::new();
    for (name, declared) in traits {
        for (definition, source) in IMPLS
            .iter()
            .filter(|(definition, _)| declared.std == Some(*definition))
        {
            let source = source.replace("{trait}", name);
            let Some(parsed) = crate::parser::parse_impl(&source) else {
                continue;
            };
            let Ok(mut shell) = super::resolve_item(
                crate::item::Id::from_index(next + shells.len()),
                program::source::Id::ROOT,
                None,
                None,
                crate::item::Parsed::Impl(parsed),
                &BTreeMap::new(),
                host,
                #[cfg(feature = "rust-import")]
                None,
            ) else {
                continue;
            };
            let item::Kind::Impl(implemented) = &mut shell.kind else {
                continue;
            };
            // std's, which rustc's orphan rule (E0117) doesn't refuse.
            implemented.std = Some(definition);
            let text = crate::ImmutableString::from(source);
            for function in implemented.functions.iter_mut() {
                function.body.text = Some(text.clone());
            }
            shells.push(shell);
        }
    }
    shells
}

/// `Self::Name` in `ty` as `<Self as trait_name>::Name` for each of the
/// trait's own associated types `own`.
fn qualify(
    ty: &mut super::type_ref::Type,
    trait_name: &crate::ImmutableString,
    own: &[crate::ImmutableString],
) {
    use super::type_ref::{Argument, Target, Type};
    match ty {
        Type::Unit(_) => {}
        Type::Tuple { members, .. } => {
            for member in members.iter_mut() {
                qualify(member, trait_name, own);
            }
        }
        Type::Reference { target, .. } => qualify(target, trait_name, own),
        Type::Path(path) => {
            if let Target::Projection(base, written @ None) = &mut path.target
                && matches!(&**base, Type::Path(base)
                    if base.target == Target::Parameter && base.name == "Self")
                && own.contains(&path.name)
            {
                *written = Some(trait_name.clone());
            }
            for argument in path.arguments.iter_mut() {
                if let Argument::Type(ty) = argument {
                    qualify(ty, trait_name, own);
                }
            }
        }
    }
}

/// The program's `use` bindings, by name: what a trait's name resolves
/// through.
pub(crate) fn uses(items: &[item::Shell]) -> Vec<(crate::ImmutableString, crate::ImmutableString)> {
    items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Use(used) => Some(used.bindings.iter()),
            _ => None,
        })
        .flatten()
        .map(|binding| (binding.name.text.clone(), binding.qualified.clone()))
        .collect()
}

/// The std traits the program's items name, which it doesn't declare
/// itself, with their supertraits: each by its name, with its declaration.
/// An impl's functions that override a provided function come with it.
pub(crate) fn declarations(
    items: &[item::Shell],
    host: &host::Registry,
) -> BTreeMap<crate::ImmutableString, item::Trait> {
    let declared = items
        .iter()
        .filter_map(|shell| match &shell.kind {
            item::Kind::Trait(own) => Some(own.name.text.clone()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut named = Vec::<crate::ImmutableString>::new();
    let mut overridden = BTreeMap::<crate::ImmutableString, Vec<crate::ImmutableString>>::new();
    let generics = |generics: &[item::Generic], named: &mut Vec<crate::ImmutableString>| {
        for generic in generics {
            named.extend(generic.traits.iter().map(|name| name.text.clone()));
        }
    };
    let signature = |function: &item::Function, named: &mut Vec<crate::ImmutableString>| {
        for parameter in function.parameters.iter() {
            objects(&parameter.ty, named);
        }
        if let Some(output) = &function.output {
            objects(output, named);
        }
        // `-> impl Display`: its opaque type's bounds (D1109).
        for (_, output) in function.opaque_outputs.iter() {
            named.extend(output.bounds.traits.iter().map(|name| name.text.clone()));
        }
    };
    for shell in items {
        match &shell.kind {
            item::Kind::Struct(own) => {
                generics(&own.generics, &mut named);
                for field in own.fields.iter() {
                    objects(&field.ty, &mut named);
                }
            }
            item::Kind::Enum(own) => {
                generics(&own.generics, &mut named);
                for ty in own
                    .variants
                    .iter()
                    .flat_map(|variant| variant.payload.iter())
                {
                    objects(ty, &mut named);
                }
            }
            item::Kind::Function(own) => {
                generics(&own.generics, &mut named);
                signature(own, &mut named);
            }
            item::Kind::Impl(own) => {
                generics(&own.generics, &mut named);
                for function in own.functions.iter() {
                    generics(&function.generics, &mut named);
                    signature(function, &mut named);
                }
                if let Some(item::TraitImpl::User { name, .. }) = &own.trait_ {
                    named.push(name.text.clone());
                    overridden.entry(name.text.clone()).or_default().extend(
                        own.functions
                            .iter()
                            .map(|function| function.name.text.clone()),
                    );
                }
            }
            item::Kind::Trait(own) => {
                for function in own.functions.iter() {
                    generics(&function.generics, &mut named);
                    signature(function, &mut named);
                }
                named.extend(own.supertraits.1.iter().map(|name| name.text.clone()));
                for (_, traits) in own.associated_bounds.iter() {
                    named.extend(traits.iter().map(|name| name.text.clone()));
                }
            }
            item::Kind::Use(_) => {}
        }
    }
    let uses = uses(items);
    let mut found = BTreeMap::new();
    while let Some(name) = named.pop() {
        if declared.contains(&name) || found.contains_key(&name) {
            continue;
        }
        let Some(definition) = self::named(
            &name,
            uses.iter()
                .map(|(binding, qualified)| (binding.as_str(), qualified.as_str())),
        ) else {
            continue;
        };
        let names = overridden
            .get(&name)
            .map(|names| names.iter().map(|name| name.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();
        let Some(trait_) = declaration(definition, &names, host) else {
            continue;
        };
        named.extend(trait_.supertraits.1.iter().map(|name| name.text.clone()));
        found.insert(name, trait_);
    }
    found
}

/// The trait of each trait object `ty` holds (`Display` of `Vec<Box<dyn
/// Display>>`), which a std trait is when the program names no own.
fn objects(ty: &super::type_ref::Type, named: &mut Vec<crate::ImmutableString>) {
    use super::type_ref::{Argument, Type};
    if let Some(name) = ty.dyn_trait()
        && !super::type_ref::is_native_trait(name)
    {
        named.push(name.into());
    }
    match ty {
        Type::Unit(_) => {}
        Type::Tuple { members, .. } => {
            for member in members.iter() {
                objects(member, named);
            }
        }
        Type::Reference { target, .. } => objects(target, named),
        Type::Path(path) => {
            for argument in path.arguments.iter() {
                if let Argument::Type(ty) = argument {
                    objects(ty, named);
                }
            }
        }
    }
}

/// Whether the std trait `definition` has a stable function `name`, as
/// `Iterator::size_hint`.
pub(crate) fn member(definition: &str, name: &str) -> bool {
    std_table::item(&format!("{definition}::{name}"))
        .is_some_and(|member| member.kind == "assoc-fn" && member.stability == Stability::Stable)
}

/// The std traits whose methods rawr's runtime runs for its own values,
/// which a trait's dispatcher falls back to for a value with no impl of the
/// program's (D711's execution mapping): the derivable traits, formatting,
/// iteration and collection, indexing, `Error`, `Drop`, and the operator
/// traits of `core::ops::arith` and `core::ops::bit`.
const RUNS: &[&str] = &[
    "core::clone::Clone",
    "core::marker::Copy",
    "core::cmp::PartialEq",
    "core::cmp::Eq",
    "core::cmp::PartialOrd",
    "core::cmp::Ord",
    "core::hash::Hash",
    "core::default::Default",
    "core::fmt::Debug",
    "core::fmt::Display",
    "core::iter::traits::iterator::Iterator",
    "core::iter::traits::collect::FromIterator",
    "core::ops::index::Index",
    "core::ops::index::IndexMut",
    "core::error::Error",
    "core::ops::drop::Drop",
];

/// Whether `name` is one of std's operator traits (`Add`, `BitOrAssign`),
/// the traits of `core::ops::arith` and `core::ops::bit`.
pub(crate) fn is_operator(name: &str) -> bool {
    anywhere(name).is_some_and(|definition| {
        definition.starts_with("core::ops::arith::") || definition.starts_with("core::ops::bit::")
    })
}

/// Whether rawr's runtime runs the std trait `definition`'s methods for
/// its own values (`RUNS`).
pub(crate) fn runs_builtin(definition: &str) -> bool {
    RUNS.contains(&definition)
        || definition.starts_with("core::ops::arith::")
        || definition.starts_with("core::ops::bit::")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What an impl is checked against: each function's receiver, inputs
    /// and output, the associated types, the generics with their defaults
    /// and the supertraits.
    fn summary(declared: &item::Trait) -> String {
        let functions = declared
            .functions
            .iter()
            .zip(declared.provided.iter())
            .map(|(function, provided)| {
                let receiver = match &function.receiver {
                    None => "",
                    Some(crate::item::Receiver::Shared(_)) => "&self ",
                    Some(crate::item::Receiver::Mutable(_)) => "&mut self ",
                    Some(_) => "self ",
                };
                format!(
                    "{}{}({receiver}{}) -> {}{}",
                    function.name.text,
                    if function.generics.is_empty() {
                        ""
                    } else {
                        "<..>"
                    },
                    function
                        .parameters
                        .iter()
                        .map(|parameter| super::super::canonical_type_name(&parameter.ty))
                        .collect::<Vec<_>>()
                        .join(", "),
                    function
                        .output
                        .as_ref()
                        .map_or_else(|| "()".to_owned(), super::super::canonical_type_name),
                    if *provided { " provided" } else { "" }
                )
            })
            .collect::<Vec<_>>();
        let generics = declared
            .generics
            .iter()
            .map(|generic| match &generic.default {
                Some(default) => format!(
                    "{}={}",
                    generic.name.text,
                    super::super::canonical_type_name(default)
                ),
                None => generic.name.text.to_string(),
            })
            .collect::<Vec<_>>();
        let associated = declared
            .associated
            .iter()
            .map(|name| name.text.to_string())
            .collect::<Vec<_>>();
        let supertraits = declared
            .supertraits
            .1
            .iter()
            .map(|name| name.text.to_string())
            .collect::<Vec<_>>();
        format!(
            "<{}> : {:?} {} | {} | {}",
            generics.join(", "),
            declared.supertraits.0,
            supertraits.join(" + "),
            associated.join(", "),
            functions.join("; ")
        )
    }

    fn declared(name: &str) -> String {
        let host = host::Registry::new();
        let definition = anywhere(name).unwrap_or_else(|| panic!("std has `{name}`"));
        summary(&declaration(definition, &[], &host).unwrap_or_else(|| panic!("`{name}` declares")))
    }

    /// std's declarations, as an impl is checked against them. Shadow mode
    /// (D711.3) compared these with the hand list they replace; they differ
    /// where std's is what rustc checks: `Hash` requires `hash`, `Copy`
    /// needs `Clone`, `Eq` and `PartialOrd` need `PartialEq`, `Error` needs
    /// `Debug` and `Display`, `IndexMut`'s `Output` is its `Index`'s, and
    /// the pinned nightly gives `Drop::drop` a default.
    #[test]
    fn std_trait_declarations_are_stds() {
        assert_eq!(
            declared("FromStr"),
            "<> : Set(0)  | Err | from_str(&str) -> Result<Self, Self::Err>"
        );
        // `by_ref` is run from std's body (`BODIES`).
        assert_eq!(
            declared("Iterator"),
            "<> : Set(0)  | Item | by_ref(&mut self ) -> &mut Self provided; next(&mut self ) \
             -> Option<Self::Item>"
        );
        assert_eq!(
            declared("Add"),
            "<Rhs=Self> : Set(0)  | Output | add(self Rhs) -> Self::Output"
        );
        assert_eq!(
            declared("Display"),
            "<> : Set(0)  |  | fmt(&self &mut String) -> Result<(), ()>"
        );
        assert_eq!(
            declared("PartialOrd"),
            "<> : Set(16)  |  | partial_cmp(&self &Self) -> Option<Ordering>"
        );
        assert_eq!(
            declared("IndexMut"),
            "<Idx> : Set(0) Index |  | index_mut(&mut self Idx) -> &mut Self::Output"
        );
        assert_eq!(declared("Error"), "<> : Set(2) Display |  | ");
        // `io::Write::write` names `io::Error`, which rawr has no
        // representation for yet: the impl's own signature says so.
        let host = host::Registry::new();
        let write = declaration("core::io::write::Write", &[], &host).expect("io::Write declares");
        assert!(write.functions.is_empty(), "{}", summary(&write));
        assert_eq!(anywhere("Write"), Some("core::fmt::Write"));
        // A bare name nothing brings into scope is none (E0405).
        begin();
        assert_eq!(named("Deref", []), None);
        assert_eq!(
            by_path(&["std", "ops", "Deref"]),
            Some("core::ops::deref::Deref")
        );
        assert_eq!(named("Deref", []), Some("core::ops::deref::Deref"));
        begin();
    }
}
