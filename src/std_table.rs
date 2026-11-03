//! std's own paths, kinds, stability and generics, read from the sysroot of
//! the pinned toolchain (D711 step 1, D714.2).
//!
//! The table is three sorted, tab-separated files that rawr embeds, which
//! `scripts/std-table.sh` writes with the importer's driver:
//!
//! - `paths.tsv`: `path  namespace  kind  stability  definition`, one line
//!   for every path a program can write into `std`, `core` and `alloc`,
//!   through every re-export. The namespace is `t`ype, `v`alue or `m`acro.
//!   The definition is `=` when the path is the item's own.
//! - `items.tsv`: `definition  kind  stability  extra`, one line for each
//!   definition and each of its members (a trait's items, a type's inherent
//!   items, an enum's variants, a primitive's methods as `i64::pow`). For a
//!   type, trait or function, `extra` is its generics, as
//!   `K V S=std::hash::RandomState A=std::alloc::Global?allocator_api#32838`
//!   (a default after `=`, an unstable parameter's feature and issue after
//!   `?` and `#`); for a trait's member, `provided` or `required`. A trait
//!   and its members have one more field (D711 step 2): the trait's
//!   supertraits (`Eq + PartialOrd`), and each member's declaration as a
//!   program writes it (`fn from_str(s: &str) -> Result<Self, Self::Err>`,
//!   `type IntoIter: Iterator<Item = Self::Item>`), naming types by their
//!   public paths, bare for the prelude's. A type's inherent function
//!   (D711 step 4) has its impl's self type in `extra` where it isn't the
//!   type's own (`Option<&T>` of `copied`), and its declaration with its
//!   lifetimes and its impl's bounds: `fn iter(&self) -> std::slice::Iter<'_,
//!   T>`, `fn sort(&mut self) where T: Ord`. A free function has its
//!   declaration too, `fn read<P: AsRef<std::path::Path>>(path: P) ->
//!   Result<Vec<u8>, std::io::Error>`, and a type alias the type it names,
//!   `Result<T, std::io::Error>` (D711 step 6).
//!
//! - `impls.tsv` (D711 step 3): `trait  key  self  arguments  generics
//!   where  associated`, one line per impl of a stable trait that stable
//!   Rust can name: the trait's definition, the self type's simplified key
//!   (a primitive's name, an ADT's definition, `&`, `&mut`, `[]`, `[;]`,
//!   `()`, or `_` for a blanket impl), the self type and the trait's
//!   arguments as a program writes them, the impl's type and const
//!   parameters (`T U const:N`), its where-clauses (`T: PartialEq<U>`) and
//!   its associated types (`Target = str`). Lists are split by `, ` outside
//!   brackets.
//!
//! A stability is `s`, or `u feature issue reason` with `-` for no issue
//! or reason. What differs by target (`arch`, `simd`, `os`) is left out.
//! `TOOLCHAIN` names the pinned nightly and the target the table was taken
//! for; the importer's tests check it is `CHANNEL`.
//!
//! Nothing is decoded ahead of a lookup: each lookup is a binary search of
//! the embedded text, so a program pays only for the paths it names.

const PATHS: &str = include_str!("std_table/paths.tsv");
const ITEMS: &str = include_str!("std_table/items.tsv");
const IMPLS: &str = include_str!("std_table/impls.tsv");

/// The namespace a path is in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Namespace {
    Type,
    Value,
    Macro,
}

impl Namespace {
    const ALL: [Self; 3] = [Self::Type, Self::Value, Self::Macro];

    fn letter(self) -> &'static str {
        match self {
            Self::Type => "t",
            Self::Value => "v",
            Self::Macro => "m",
        }
    }
}

/// Whether a path or an item may be named on stable Rust.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stability {
    Stable,
    Unstable {
        feature: &'static str,
        issue: Option<&'static str>,
        reason: Option<&'static str>,
    },
}

impl Stability {
    fn parse(text: &'static str) -> Self {
        let Some(rest) = text.strip_prefix("u ") else {
            return Self::Stable;
        };
        let mut parts = rest.splitn(3, ' ');
        let feature = parts.next().unwrap_or_default();
        let some = |part: Option<&'static str>| part.filter(|part| *part != "-");
        let issue = some(parts.next());
        let reason = some(parts.next());
        Self::Unstable {
            feature,
            issue,
            reason,
        }
    }
}

/// A path's line in `paths.tsv`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Path {
    pub path: &'static str,
    pub namespace: Namespace,
    /// `mod`, `struct`, `enum`, `union`, `trait`, `type`, `fn`, `const`,
    /// `static`, `macro`, `variant`, `primitive`, ..
    pub kind: &'static str,
    /// The path's own: unstable when a module on the way or the item is.
    pub stability: Stability,
    /// The definition the path names: `alloc::vec::Vec` for `std::vec::Vec`.
    pub definition: &'static str,
}

impl Path {
    /// Whether the path names a type, as a type position may write it.
    pub(crate) fn is_type(&self) -> bool {
        matches!(
            self.kind,
            "struct" | "enum" | "union" | "type" | "primitive" | "foreign-type"
        )
    }
}

/// An item's line in `items.tsv`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Item {
    pub definition: &'static str,
    pub kind: &'static str,
    pub stability: Stability,
    extra: &'static str,
    detail: &'static str,
}

/// A generic parameter of a type, trait or function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Parameter {
    pub name: &'static str,
    pub kind: ParameterKind,
    /// The default type, as rustc prints it: `std::hash::RandomState`.
    pub default: Option<&'static str>,
    /// Whether stable Rust may name it: `allocator_api` gates `Vec`'s `A`.
    pub stability: Stability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParameterKind {
    Lifetime,
    Type,
    Const,
}

impl Item {
    /// The item's generic parameters, in order (empty for a member).
    pub(crate) fn generics(&self) -> impl Iterator<Item = Parameter> {
        let generics = match self.kind {
            "struct" | "enum" | "union" | "trait" | "type" | "fn" => self.extra,
            _ => "",
        };
        generics
            .split(' ')
            .filter(|text| !text.is_empty())
            .map(|text| {
                let (text, stability) = match text.split_once('?') {
                    Some((text, gate)) => {
                        let (feature, issue) = match gate.split_once('#') {
                            Some((feature, issue)) => (feature, Some(issue)),
                            None => (gate, None),
                        };
                        let stability = Stability::Unstable {
                            feature,
                            issue,
                            reason: None,
                        };
                        (text, stability)
                    }
                    None => (text, Stability::Stable),
                };
                let (text, default) = match text.split_once('=') {
                    Some((text, default)) => (text, Some(default)),
                    None => (text, None),
                };
                let (name, kind) = if let Some(name) = text.strip_prefix('\'') {
                    (name, ParameterKind::Lifetime)
                } else if let Some(name) = text.strip_prefix("const:") {
                    (name, ParameterKind::Const)
                } else {
                    (text, ParameterKind::Type)
                };
                Parameter {
                    name,
                    kind,
                    default,
                    stability,
                }
            })
    }

    /// Whether a trait's member has a default.
    pub(crate) fn provided(&self) -> bool {
        self.extra == "provided"
    }

    /// A trait's supertraits as its declaration writes them: `Eq +
    /// PartialOrd`, empty for none.
    pub(crate) fn supertraits(&self) -> &'static str {
        if self.kind == "trait" {
            self.detail
        } else {
            ""
        }
    }

    /// A trait member's declaration, as a program writes it: `fn
    /// from_str(s: &str) -> Result<Self, Self::Err>` or `type Err`.
    pub(crate) fn declaration(&self) -> &'static str {
        if self.kind == "trait" {
            ""
        } else {
            self.detail
        }
    }

    /// The last segment of the item's path: `from_str` of
    /// `core::str::traits::FromStr::from_str`.
    pub(crate) fn name(&self) -> &'static str {
        self.definition
            .rsplit("::")
            .next()
            .unwrap_or(self.definition)
    }

    /// How a trait's function takes `self`: `Some("&self")`, `Some("&mut
    /// self")`, `Some("self")` (also `self: Box<Self>`), or `None` for an
    /// associated function.
    pub(crate) fn receiver(&self) -> Option<&'static str> {
        let declaration = self.declaration();
        // The inputs start at the first `(` outside the generics, whose
        // closure bounds have parentheses of their own.
        let mut depth = 0usize;
        let mut open = None;
        for (index, c) in declaration.char_indices() {
            match c {
                '<' => depth += 1,
                // `->` closes nothing.
                '>' if index > 0 && declaration.as_bytes()[index - 1] == b'-' => {}
                '>' => depth = depth.saturating_sub(1),
                '(' if depth == 0 => {
                    open = Some(index);
                    break;
                }
                _ => {}
            }
        }
        let inputs = &declaration[open? + 1..];
        let first = inputs.split([',', ')']).next()?.trim();
        // `&'a self` takes `self` as `&self` does.
        let unnamed = match first.strip_prefix("&'") {
            Some(rest) => rest.split_once(' ').map_or(first, |(_, rest)| rest),
            None => first.strip_prefix('&').unwrap_or(first),
        };
        match (first.starts_with('&'), unnamed) {
            (true, "self") => Some("&self"),
            (true, "mut self") => Some("&mut self"),
            (false, "self") => Some("self"),
            _ if first.starts_with("self:") => Some("self"),
            _ => None,
        }
    }

    /// An inherent function's impl's self type, where it isn't its owner's
    /// own: `Option<&T>` of `Option::copied`, `[u8]` of `slice::is_ascii`.
    pub(crate) fn impl_self(&self) -> Option<&'static str> {
        (self.kind == "assoc-fn"
            && !self.extra.is_empty()
            && !matches!(self.extra, "provided" | "required"))
        .then_some(self.extra)
    }

    /// The type arity a path to the item may write: the parameters with no
    /// default, and every one stable Rust may name.
    pub(crate) fn type_arity(&self) -> (usize, usize) {
        let mut required = 0;
        let mut stable = 0;
        for parameter in self.generics() {
            if parameter.kind != ParameterKind::Type || parameter.stability != Stability::Stable {
                continue;
            }
            stable += 1;
            if parameter.default.is_none() {
                required += 1;
            }
        }
        (required, stable)
    }
}

/// The line of `text` whose key (what comes before its `fields`th tab) is
/// `key`, by binary search over the sorted text.
fn find(text: &'static str, key: &str, fields: usize) -> Option<&'static str> {
    let key_of = |line: &'static str| -> &'static str {
        let mut end = 0;
        for _ in 0..fields {
            match line[end..].find('\t') {
                Some(tab) => end += tab + 1,
                None => return line,
            }
        }
        &line[..end.saturating_sub(1)]
    };
    let bytes = text.as_bytes();
    let line_at = |position: usize| -> (usize, &'static str) {
        let start = bytes[..position]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |newline| newline + 1);
        let end = bytes[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |newline| start + newline);
        (start, &text[start..end])
    };
    let (mut low, mut high) = (0, bytes.len());
    while low < high {
        let middle = low + (high - low) / 2;
        let (start, line) = line_at(middle);
        match key_of(line).cmp(key) {
            std::cmp::Ordering::Equal => return Some(line),
            std::cmp::Ordering::Less => low = start + line.len() + 1,
            std::cmp::Ordering::Greater => high = start,
        }
    }
    None
}

fn path_line(line: &'static str) -> Path {
    let mut fields = line.split('\t');
    let path = fields.next().unwrap_or_default();
    let namespace = match fields.next() {
        Some("v") => Namespace::Value,
        Some("m") => Namespace::Macro,
        _ => Namespace::Type,
    };
    let kind = fields.next().unwrap_or_default();
    let stability = Stability::parse(fields.next().unwrap_or("s"));
    let definition = match fields.next() {
        Some("=") | None => path,
        Some(definition) => definition,
    };
    Path {
        path,
        namespace,
        kind,
        stability,
        definition,
    }
}

/// The path `path` in `namespace`, as a program writes it:
/// `std::collections::HashMap`.
pub(crate) fn path(path: &str, namespace: Namespace) -> Option<Path> {
    find(PATHS, &format!("{path}\t{}", namespace.letter()), 2).map(path_line)
}

/// The path in any namespace: a type's first, then a value's, then a
/// macro's.
pub(crate) fn any(path: &str) -> Option<Path> {
    Namespace::ALL
        .into_iter()
        .find_map(|namespace| self::path(path, namespace))
}

/// Whether `path` is a module.
pub(crate) fn is_module(path: &str) -> bool {
    self::path(path, Namespace::Type).is_some_and(|found| found.kind == "mod")
}

/// The definition `definition`, or a member of one: `alloc::vec::Vec`,
/// `alloc::vec::Vec::push`, `i64::pow`.
pub(crate) fn item(definition: &str) -> Option<Item> {
    find(ITEMS, definition, 1).map(item_line)
}

fn item_line(line: &'static str) -> Item {
    let mut fields = line.split('\t');
    let definition = fields.next().unwrap_or_default();
    let kind = fields.next().unwrap_or_default();
    let stability = Stability::parse(fields.next().unwrap_or("s"));
    let extra = fields.next().unwrap_or_default();
    let detail = fields.next().unwrap_or_default();
    Item {
        definition,
        kind,
        stability,
        extra,
        detail,
    }
}

/// `text`, a sorted table, from its first line at or after `prefix`.
fn lines_from(text: &'static str, prefix: &str) -> &'static str {
    let bytes = text.as_bytes();
    let (mut low, mut high) = (0, bytes.len());
    while low < high {
        let middle = low + (high - low) / 2;
        let start = bytes[..middle]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |newline| newline + 1);
        let end = bytes[start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(bytes.len(), |newline| start + newline);
        if text[start..end] < *prefix {
            low = end + 1;
        } else {
            high = start;
        }
    }
    &text[low.min(text.len())..]
}

/// The members the table lists under `definition`: a trait's items, a
/// type's inherent items, an enum's variants.
pub(crate) fn members(definition: &str) -> impl Iterator<Item = Item> {
    let prefix = format!("{definition}::");
    let length = prefix.len();
    lines_from(ITEMS, &prefix)
        .lines()
        .take_while(move |line| line.starts_with(&prefix))
        .filter(move |line| {
            let rest = &line[length..];
            !rest.split('\t').next().unwrap_or_default().contains("::")
        })
        .map(item_line)
}

/// The stable functions named `name` of std's stable traits: `clone` of
/// `core::clone::Clone`, `eq` of `core::cmp::PartialEq`. One pass over the
/// table, for the method probe's traits.
pub(crate) fn trait_functions(name: &str) -> Vec<Item> {
    let suffix = format!("::{name}\tassoc-fn\ts\t");
    ITEMS
        .lines()
        .filter(|line| {
            line.find(&suffix).is_some_and(|at| {
                let rest = &line[at + suffix.len()..];
                !line[..at].contains('\t')
                    && (rest.starts_with("provided\t") || rest.starts_with("required\t"))
            })
        })
        .map(item_line)
        .filter(|member| {
            let owner = &member.definition[..member.definition.len() - name.len() - 2];
            item(owner)
                .is_some_and(|owner| owner.kind == "trait" && owner.stability == Stability::Stable)
        })
        .collect()
}

/// The stable std trait a program names by `path` (`std::str::FromStr`),
/// as its definition: `core::str::traits::FromStr`.
pub(crate) fn stable_trait(path: &str) -> Option<&'static str> {
    let found = self::path(path, Namespace::Type)?;
    (found.kind == "trait" && found.stability == Stability::Stable).then_some(found.definition)
}

/// A stable std trait's declaration as a program would write it, from its
/// line and its members': its stable items, each provided function with an
/// empty body when `provided` names it, else none. `pub trait FromStr:
/// Sized { type Err; fn from_str(s: &str) -> Result<Self, Self::Err>; }`.
///
/// Every type a rawr value has is sized, so `?Sized` is left out.
pub(crate) fn trait_source(definition: &str, provided: &[&str]) -> Option<String> {
    let declared = item(definition)?;
    if declared.kind != "trait" || declared.stability != Stability::Stable {
        return None;
    }
    let mut text = format!("pub trait {}", declared.name());
    let generics = declared
        .generics()
        .filter(|parameter| {
            parameter.kind == ParameterKind::Type && parameter.stability == Stability::Stable
        })
        .map(|parameter| match parameter.default {
            Some(default) => format!("{} = {default}", parameter.name),
            None => parameter.name.to_owned(),
        })
        .collect::<Vec<_>>();
    if !generics.is_empty() {
        text.push('<');
        text.push_str(&generics.join(", "));
        text.push('>');
    }
    if !declared.supertraits().is_empty() {
        text.push_str(": ");
        text.push_str(declared.supertraits());
    }
    text.push_str(" {\n");
    for member in members(definition) {
        if member.stability != Stability::Stable
            || member.declaration().is_empty()
            || member.provided() && member.kind == "assoc-fn" && !provided.contains(&member.name())
        {
            continue;
        }
        text.push_str("    ");
        let declaration = member.declaration();
        let declaration = declaration.replace(": ?Sized", "").replace(" + ?Sized", "");
        text.push_str(&declaration);
        text.push_str(if member.provided() && member.kind == "assoc-fn" {
            " {}\n"
        } else {
            ";\n"
        });
    }
    text.push('}');
    Some(text)
}

/// Whether the path is under a module whose contents differ by target,
/// which the table leaves out: `std::os::unix`, `core::arch`.
pub(crate) fn target_specific(path: &str) -> bool {
    path.split("::").any(|segment| {
        matches!(
            segment,
            "arch" | "core_arch" | "std_detect" | "simd" | "std_float" | "os"
        )
    })
}

/// Whether `name` is a crate of the table: `std`, `core` or `alloc`.
pub(crate) fn is_root(name: &str) -> bool {
    matches!(name, "std" | "core" | "alloc")
}

/// The paths one level under `module`, in every namespace: what
/// `use module::*;` binds.
pub(crate) fn children(module: &str) -> impl Iterator<Item = Path> {
    let prefix = format!("{module}::");
    lines_from(PATHS, &prefix)
        .lines()
        .take_while(move |line| line.starts_with(&prefix))
        .map(path_line)
        .filter(move |found| !found.path[module.len() + 2..].contains("::"))
}

/// What a path names, through its owner for an associated item:
/// `std::string::String::from_utf8` is `alloc::string::String::from_utf8`,
/// and `std::primitive::i64::pow` is `i64::pow`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Resolved {
    /// A path of `paths.tsv`.
    Path(Path),
    /// A member of the type or trait `owner` names: one the table has (an
    /// inherent item, a trait's item, a variant), or `None` for one it
    /// doesn't list, as a trait impl's method.
    Member { owner: Path, member: Option<Item> },
}

impl Resolved {
    pub(crate) fn stability(&self) -> Stability {
        match self {
            Self::Path(path) => path.stability,
            Self::Member { owner, member } => match owner.stability {
                Stability::Stable => member.map_or(Stability::Stable, |member| member.stability),
                unstable => unstable,
            },
        }
    }
}

/// Why a path into std does not resolve, as rustc's resolver says it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Missing {
    /// The segments that resolve, as written: `core`.
    pub found: Vec<String>,
    /// The first segment that doesn't: `thread`.
    pub segment: String,
    /// Whether it is the path's last segment.
    pub last: bool,
}

/// The path `segments` names, from a root of the table, preferring the
/// namespace `last` for its last segment. A path through a type or trait
/// to a member resolves to that member.
pub(crate) fn resolve(segments: &[&str], last: Namespace) -> Result<Resolved, Missing> {
    let missing = |index: usize| Missing {
        found: segments[..index]
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect(),
        segment: segments[index].to_owned(),
        last: index + 1 == segments.len(),
    };
    if segments.first().is_none_or(|root| !is_root(root)) {
        return Err(missing(0));
    }
    let mut written = segments[0].to_owned();
    for (index, segment) in segments.iter().enumerate().skip(1) {
        written.push_str("::");
        written.push_str(segment);
        let is_last = index + 1 == segments.len();
        let found = if is_last {
            path(&written, last).or_else(|| any(&written))
        } else {
            path(&written, Namespace::Type)
        };
        if let Some(found) = found {
            if is_last {
                return Ok(Resolved::Path(found));
            }
            if found.kind == "mod" {
                continue;
            }
            // A type, trait or primitive: the rest is one of its members,
            // which the table may not list (a trait impl's).
            let member = (index + 2 == segments.len())
                .then(|| item(&format!("{}::{}", found.definition, segments[index + 1])))
                .flatten();
            return Ok(Resolved::Member {
                owner: found,
                member,
            });
        }
        return Err(missing(index));
    }
    any(&written).map(Resolved::Path).ok_or_else(|| missing(0))
}

/// The prelude of `edition` (`2015`, `2018`, `2021`, `2024`): the names
/// every module of a crate of that edition sees.
pub(crate) fn prelude(edition: &str) -> impl Iterator<Item = Path> {
    children(match edition {
        "2015" => "std::prelude::rust_2015",
        "2018" => "std::prelude::rust_2018",
        "2021" => "std::prelude::rust_2021",
        _ => "std::prelude::rust_2024",
    })
}

/// An impl's line in `impls.tsv`: std's impl header, as a program would
/// write it (D711 step 3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Impl {
    /// The trait's definition: `core::cmp::PartialEq`.
    pub trait_: &'static str,
    /// The self type: `Vec<T, A>`, `&A`, `str`.
    pub self_ty: &'static str,
    arguments: &'static str,
    generics: &'static str,
    wheres: &'static str,
    associated: &'static str,
}

impl Impl {
    /// The trait's arguments after `Self`: `Vec<U, A>` of
    /// `PartialEq<Vec<U, A>>`.
    pub(crate) fn arguments(&self) -> Vec<&'static str> {
        split_list(self.arguments)
    }

    /// The impl's type and const parameters: `T`, `U`, `const:N`.
    pub(crate) fn generics(&self) -> impl Iterator<Item = &'static str> {
        self.generics.split(' ').filter(|name| !name.is_empty())
    }

    /// Its where-clauses, each as its subject and its bounds: (`T`,
    /// `PartialEq<U> + Clone`).
    pub(crate) fn wheres(&self) -> Vec<(&'static str, &'static str)> {
        split_list(self.wheres)
            .into_iter()
            .filter_map(|clause| {
                let colon = top_level(clause, ": ")?;
                Some((&clause[..colon], &clause[colon + 2..]))
            })
            .collect()
    }

    /// The associated types it declares, each with its type: (`Target`,
    /// `str`) of `impl Deref for String`.
    pub(crate) fn associated_types(&self) -> impl Iterator<Item = (&'static str, &'static str)> {
        split_list(self.associated)
            .into_iter()
            .filter_map(|binding| binding.split_once(" = "))
    }

    /// The associated type `name` it declares: `str` for `Target` of
    /// `impl Deref for String`.
    pub(crate) fn associated(&self, name: &str) -> Option<&'static str> {
        split_list(self.associated).into_iter().find_map(|binding| {
            let (bound, ty) = binding.split_once(" = ")?;
            (bound == name).then_some(ty)
        })
    }
}

/// The first `pattern` in `text` outside brackets.
pub(crate) fn top_level(text: &str, pattern: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, c) in text.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            // `->` closes nothing.
            '>' if index > 0 && text.as_bytes()[index - 1] == b'-' => {}
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 && text[index..].starts_with(pattern) => return Some(index),
            _ => {}
        }
    }
    None
}

/// `text` split at each top-level `separator`.
fn split_top(text: &'static str, separator: &str) -> Vec<&'static str> {
    let mut parts = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match top_level(rest, separator) {
            Some(at) => {
                parts.push(&rest[..at]);
                rest = &rest[at + separator.len()..];
            }
            None => {
                parts.push(rest);
                break;
            }
        }
    }
    parts
}

/// `text` split at each top-level `, `.
pub(crate) fn split_list(text: &'static str) -> Vec<&'static str> {
    split_top(text, ", ")
}

/// `text` split at each top-level ` + `: a bound's traits.
pub(crate) fn split_bounds(text: &'static str) -> Vec<&'static str> {
    split_top(text, " + ")
}

/// The impls of the trait `trait_` (its definition) whose self type has the
/// simplified key `key`: `alloc::string::String`, `&`, `[]`, `_`.
pub(crate) fn impls(trait_: &str, key: &str) -> impl Iterator<Item = Impl> {
    impl_lines(format!("{trait_}\t{key}\t"))
}

/// Every impl of the trait `trait_` (its definition), of any self type.
pub(crate) fn trait_impls(trait_: &str) -> impl Iterator<Item = Impl> {
    impl_lines(format!("{trait_}\t"))
}

/// The impls whose lines start with `prefix`.
fn impl_lines(prefix: String) -> impl Iterator<Item = Impl> {
    lines_from(IMPLS, &prefix)
        .lines()
        .take_while(move |line| line.starts_with(&prefix))
        .map(|line| {
            let mut fields = line.split('\t');
            let mut next = || fields.next().unwrap_or_default();
            let trait_ = next();
            let _key = next();
            Impl {
                trait_,
                self_ty: next(),
                arguments: next(),
                generics: next(),
                wheres: next(),
                associated: next(),
            }
        })
}

/// A type as std's declarations write it, parsed: what an impl header's
/// types are matched by.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Written {
    /// A path with its type arguments and its associated type bindings
    /// (`Iterator<Item = T>`): a type, a trait, or an impl's parameter.
    Path {
        path: &'static str,
        arguments: Vec<Written>,
        bindings: Vec<(&'static str, Written)>,
    },
    Reference {
        mutable: bool,
        target: Box<Written>,
    },
    Slice(Box<Written>),
    /// `[T; N]`, with its length as written.
    Array(Box<Written>, &'static str),
    Tuple(Vec<Written>),
    Never,
    /// What rawr matches no value by: a pointer, a `fn` or `dyn` type, a
    /// projection, a closure bound.
    Other(&'static str),
}

/// The type `text` writes.
pub(crate) fn written(text: &'static str) -> Written {
    let text = text.trim();
    if text == "!" {
        return Written::Never;
    }
    if let Some(rest) = text.strip_prefix("&mut ") {
        return Written::Reference {
            mutable: true,
            target: Box::new(written(rest)),
        };
    }
    if let Some(rest) = text.strip_prefix('&') {
        return Written::Reference {
            mutable: false,
            target: Box::new(written(rest)),
        };
    }
    if let Some(inner) = text
        .strip_prefix('[')
        .and_then(|text| text.strip_suffix(']'))
    {
        return match top_level(inner, "; ") {
            Some(semicolon) => Written::Array(
                Box::new(written(&inner[..semicolon])),
                inner[semicolon + 2..].trim(),
            ),
            None => Written::Slice(Box::new(written(inner))),
        };
    }
    if let Some(inner) = text
        .strip_prefix('(')
        .and_then(|text| text.strip_suffix(')'))
    {
        // `(A) -> B` is no tuple: its first `(` closes before its end.
        let mut depth = 0usize;
        for (index, c) in text.char_indices() {
            match c {
                '<' | '(' | '[' => depth += 1,
                '>' if index > 0 && text.as_bytes()[index - 1] == b'-' => {}
                '>' | ')' | ']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && index + 1 < text.len() {
                        return Written::Other(text);
                    }
                }
                _ => {}
            }
        }
        let inner = inner.strip_suffix(',').unwrap_or(inner);
        return Written::Tuple(split_list(inner).into_iter().map(written).collect());
    }
    if text.starts_with(['*', '<'])
        || text.starts_with("dyn ")
        || text.starts_with("fn(")
        || text.starts_with("impl ")
        || top_level(text, " ").is_some()
    {
        return Written::Other(text);
    }
    let (path, rest) = match text.find('<') {
        Some(open) if text.ends_with('>') => (&text[..open], &text[open + 1..text.len() - 1]),
        Some(_) => return Written::Other(text),
        None => (text, ""),
    };
    if path.contains(['(', ')']) {
        return Written::Other(text);
    }
    let mut arguments = Vec::new();
    let mut bindings = Vec::new();
    for argument in split_list(rest) {
        match top_level(argument, " = ") {
            Some(equals) => bindings.push((&argument[..equals], written(&argument[equals + 3..]))),
            None => arguments.push(written(argument)),
        }
    }
    Written::Path {
        path,
        arguments,
        bindings,
    }
}

/// A declaration's parts: `fn name<generics>(inputs) -> output where ..`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Declared {
    pub generics: &'static str,
    pub inputs: &'static str,
    pub output: Option<&'static str>,
    /// Its where-clauses: `B: Default + Extend<Self::Item>`.
    pub wheres: &'static str,
}

impl Declared {
    /// The parts of the declaration `text`, as `items.tsv` writes it.
    pub(crate) fn parse(text: &'static str) -> Option<Self> {
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
        let (rest, wheres) = match top_level(rest, " where ") {
            Some(at) => (&rest[..at], &rest[at + 7..]),
            None => match rest.split_once(" where ") {
                Some((rest, wheres)) => (rest, wheres),
                None => match rest.strip_prefix("where ") {
                    Some(wheres) => ("", wheres),
                    None => (rest, ""),
                },
            },
        };
        let output = rest.strip_prefix("-> ").map(str::trim);
        Some(Self {
            generics,
            inputs,
            output,
            wheres,
        })
    }
}

/// The index in `text` (which starts with `open`) of its matching `close`.
pub(crate) fn closing(text: &str, open: char, close: char) -> Option<usize> {
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

/// The definition a path std's declarations write names: a primitive's
/// name, or the definition of a prelude name or a path
/// (`std::rc::Rc` is `alloc::rc::Rc`, `String` is `alloc::string::String`).
pub(crate) fn definition_of(path: &str) -> Option<&'static str> {
    const PRIMITIVES: &[&str] = &[
        "bool", "char", "str", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32",
        "u64", "u128", "usize", "f16", "f32", "f64", "f128",
    ];
    if let Some(primitive) = PRIMITIVES.iter().find(|primitive| **primitive == path) {
        return Some(primitive);
    }
    if path.contains("::") {
        return self::path(path, Namespace::Type).map(|found| found.definition);
    }
    self::path(&format!("std::prelude::rust_2024::{path}"), Namespace::Type)
        .map(|found| found.definition)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn std_table_paths_resolve_through_reexports() {
        let map = path("std::collections::HashMap", Namespace::Type).expect("HashMap");
        assert_eq!(map.kind, "struct");
        assert_eq!(map.stability, Stability::Stable);
        assert_eq!(map.definition, "std::collections::hash::map::HashMap");
        let vec = path("std::vec::Vec", Namespace::Type).expect("Vec");
        assert_eq!(vec.definition, "alloc::vec::Vec");
        // `std::vec` is a module and a macro.
        assert_eq!(
            path("std::vec", Namespace::Type).map(|p| p.kind),
            Some("mod")
        );
        assert_eq!(
            path("std::vec", Namespace::Macro).map(|p| p.kind),
            Some("macro")
        );
        assert!(any("core::collections").is_none());
        assert!(any("core::time::Instant").is_none());
        assert!(any("std::time::Instant").is_some());
    }

    #[test]
    fn std_table_items_carry_generics_with_defaults() {
        let map = item("std::collections::hash::map::HashMap").expect("HashMap");
        let names = map.generics().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(names, ["K", "V", "S", "A"]);
        assert_eq!(map.type_arity(), (2, 3));
        let s = map.generics().nth(2).expect("S");
        assert_eq!(s.default, Some("std::hash::RandomState"));
        let a = map.generics().nth(3).expect("A");
        assert_eq!(
            a.stability,
            Stability::Unstable {
                feature: "allocator_api",
                issue: Some("32838"),
                reason: None
            }
        );
        assert_eq!(
            item("alloc::vec::Vec").map(|v| v.type_arity()),
            Some((1, 1))
        );
        assert_eq!(
            item("core::result::Result").map(|v| v.type_arity()),
            Some((2, 2))
        );
    }

    #[test]
    fn std_table_functions_and_aliases_carry_declarations() {
        assert_eq!(
            item("std::fs::read").map(|read| read.declaration()),
            Some("fn read<P: AsRef<std::path::Path>>(path: P) -> Result<Vec<u8>, std::io::Error>")
        );
        assert_eq!(
            item("core::io::error::Result").map(|result| result.declaration()),
            Some("Result<T, std::io::Error>")
        );
        // std's `impl io::Error` for core's `io::Error`.
        assert!(item("core::io::error::Error::new").is_some());
    }

    #[test]
    fn std_table_stability_and_members() {
        let transmute = path("std::intrinsics::transmute", Namespace::Value).expect("transmute");
        let Stability::Unstable { feature, .. } = transmute.stability else {
            panic!("transmute is unstable");
        };
        assert_eq!(feature, "core_intrinsics");
        match resolve(&["std", "string", "String", "from_utf8"], Namespace::Value) {
            Ok(Resolved::Member {
                member: Some(member),
                ..
            }) => {
                assert_eq!(member.definition, "alloc::string::String::from_utf8");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            resolve(&["core", "thread", "sleep"], Namespace::Value),
            Err(Missing {
                found: vec!["core".into()],
                segment: "thread".into(),
                last: false
            })
        );
        assert!(item("i64::pow").is_some());
        assert!(item("core::fmt::Display::fmt").is_some_and(|fmt| fmt.kind == "assoc-fn"));
    }

    #[test]
    fn std_table_children_and_preludes() {
        let names = children("std::cmp").map(|p| p.path).collect::<Vec<_>>();
        assert!(names.contains(&"std::cmp::Ordering"));
        assert!(names.contains(&"std::cmp::max"));
        assert!(!names
            .iter()
            .any(|name| name.starts_with("std::cmp::Ordering::")));
        let names = prelude("2021").map(|p| p.path).collect::<Vec<_>>();
        assert!(names.contains(&"std::prelude::rust_2021::TryFrom"));
        assert!(!prelude("2018").any(|p| p.path.ends_with("::TryFrom")));
        assert!(prelude("2024").any(|p| p.path.ends_with("::Future")));
    }

    #[test]
    fn std_table_impls_carry_their_headers() {
        let deref = impls("core::ops::deref::Deref", "alloc::string::String")
            .next()
            .expect("impl Deref for String");
        assert_eq!(deref.self_ty, "String");
        assert_eq!(deref.associated("Target"), Some("str"));
        let between = impls("core::cmp::PartialEq", "alloc::vec::Vec")
            .find(|line| line.arguments() == ["[U; N]"])
            .expect("impl PartialEq<[U; N]> for Vec<T, A>");
        assert_eq!(
            between.generics().collect::<Vec<_>>(),
            ["T", "U", "A", "const:N"]
        );
        assert_eq!(between.wheres(), [("T", "PartialEq<U>")]);
        let parameter = |path| Written::Path {
            path,
            arguments: vec![],
            bindings: vec![],
        };
        assert_eq!(
            written("Vec<T, A>"),
            Written::Path {
                path: "Vec",
                arguments: vec![parameter("T"), parameter("A")],
                bindings: vec![],
            }
        );
        assert!(matches!(written("[U; N]"), Written::Array(_, "N")));
        assert!(matches!(
            written("&mut [T]"),
            Written::Reference { mutable: true, .. }
        ));
        assert!(matches!(written("(A, B)"), Written::Tuple(members) if members.len() == 2));
        assert!(matches!(written("(T,)"), Written::Tuple(members) if members.len() == 1));
        assert!(matches!(written("FnOnce() -> T"), Written::Other(_)));
        assert_eq!(definition_of("std::rc::Rc"), Some("alloc::rc::Rc"));
        assert_eq!(definition_of("String"), Some("alloc::string::String"));
        assert_eq!(
            split_bounds("PartialEq<U> + Clone"),
            ["PartialEq<U>", "Clone"]
        );
        assert_eq!(definition_of("PartialEq"), Some("core::cmp::PartialEq"));
    }
}
