//! The concrete items of a crate-source module, each named by a function
//! of the program module's own that nothing calls: checking it binds the
//! item, so the runner holds every concrete item of the module whatever
//! the program's generic instances reach of it (`crate_source`). Its
//! statics are bound by a borrow, its named-field structs by a literal,
//! its functions, inherent methods and trait impls' methods by a call.

use std::fmt::Write as _;
use std::ops::Range;

use crate::package::polyglot::{self, ModuleItem, Token};

/// The prefix of each function that binds a concrete item.
pub(super) const BINDING: &str = "__rawr_bind_";

/// The traits whose derives the runner binds, each with how its method is
/// called: what it takes and gives, `{}` standing for the type.
const DERIVES: [(&str, &str, &str); 3] = [
    ("Default", "() -> {}", "<{} as Default>::default()"),
    ("Clone", "(__a0: &{}) -> {}", "<{} as Clone>::clone(__a0)"),
    (
        "PartialEq",
        "(__a0: &{}, __a1: &{}) -> bool",
        "<{} as PartialEq>::eq(__a0, __a1)",
    ),
];

/// The names of the prelude, which a binding written in another module
/// spells as the defining module does: rustc resolves them alike in both.
#[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
const PRELUDE: [&str; 37] = [
    "From",
    "Into",
    "TryFrom",
    "TryInto",
    "Default",
    "Clone",
    "Copy",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "AsRef",
    "AsMut",
    "Iterator",
    "IntoIterator",
    "DoubleEndedIterator",
    "ExactSizeIterator",
    "Extend",
    "Drop",
    "Fn",
    "FnMut",
    "FnOnce",
    "Send",
    "Sync",
    "Sized",
    "Unpin",
    "ToOwned",
    "ToString",
    "Option",
    "Some",
    "None",
    "Result",
    "Ok",
    "Err",
    "Vec",
    "String",
    "Box",
];

/// The primitive types and the keywords a signature may hold, which every
/// module spells alike.
#[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
const PRIMITIVES: [&str; 20] = [
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
    "i128", "isize", "f32", "f64", "mut", "dyn", "as",
];

/// A concrete type a crate-source module names, by the name it is in
/// scope by there, the traits of [`DERIVES`] its definition derives, and
/// a call of each method of its concrete trait impls in the defining
/// module that the crate-source module can spell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Derived {
    name: String,
    derives: Vec<&'static str>,
    impls: Vec<Binding>,
    /// Each concrete trait impl of the type that has no binding, by its
    /// header, with the rule that leaves it out.
    excluded: Vec<(String, &'static str)>,
}

/// Why a concrete trait impl of a type a crate-source module names has no
/// binding: it names an item the naming module has no path to, as a
/// private type of another module.
#[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
pub(crate) const UNNAMED: &str = "it names an item the opening module has no path to";
/// Why an impl has no binding: a method's parameter is of an associated
/// type of `Self`, which a binding's signature can't spell yet.
#[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
pub(crate) const PROJECTED: &str = "a method's parameter is of an associated type of `Self`";
/// Why an impl has no binding: a method is unsafe, async, extern or
/// generic, so no call of it is written as it is.
#[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
pub(crate) const UNCALLED: &str = "a method is unsafe, async, extern or generic";

/// A binding function's signature and body, its name left to the module
/// it is written in.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    /// Its lifetime parameters, with their `<..>`, or nothing.
    lifetimes: String,
    parameters: Vec<String>,
    /// Its output type, or nothing for `()`.
    output: String,
    /// The call's value is dropped, its type unspelled.
    discarded: bool,
    /// The path of the item it calls.
    path: String,
    arguments: Vec<String>,
}

/// What the concrete type `defined` that `text` defines gives a
/// crate-source module that names it (`located::imported`): its derives
/// of [`DERIVES`], and its concrete trait impls in `text`, which the crate
/// compiles once whatever the program instantiates, as iced's `impl
/// From<u16> for Padding` and `impl From<Pixels> for Padding`. `spell`
/// gives the path by which the naming module writes a name the defining
/// module's text starts a path with, `defined` included, or `None` when it
/// has none. An impl that has a name with no such path, or a method no
/// call can be written of, is left out with its rule (`excluded`). A
/// generic impl is left to the program's uses. `None` when `text` defines
/// no such type, it is generic, or it gives neither.
#[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
pub(crate) fn derived(
    text: &str,
    defined: &str,
    spell: &dyn Fn(&str) -> Option<String>,
) -> Option<Derived> {
    let tokens = polyglot::placed(text);
    let items = polyglot::module_items(text, &tokens);
    let item = items
        .iter()
        .find(|item| matches!(item.keyword, "struct" | "enum") && item.name == Some(defined))?;
    let header = &tokens[item.header.clone()];
    let parameters = header
        .first()
        .is_some_and(|(token, _)| *token == Token::Punct(b'<'));
    let name = spell(defined)?;
    // A type whose every parameter has a default is named alone at those
    // defaults, as iced's `Rectangle` is `Rectangle<f32>`: its derives are
    // that instance's, which the crate compiles once the module names it.
    if parameters {
        return defaulted(text, header).then(|| Derived {
            name,
            derives: derived_traits(&tokens, item),
            impls: Vec::new(),
            excluded: Vec::new(),
        });
    }
    let derives = derives_of(&tokens, item).unwrap_or_default();
    let bound = Bound {
        text,
        written: String::new(),
        count: 0,
    };
    let mut impls = Vec::new();
    let mut excluded = Vec::new();
    for item in items
        .iter()
        .filter(|item| item.keyword == "impl" && !item.generic)
    {
        match bound.trait_impl_bindings(&tokens, item, defined, spell) {
            Some(Ok(mut bindings)) => impls.append(&mut bindings),
            Some(Err(rule)) => {
                let start = tokens[item.header.start].1;
                let header = text[start..tokens[item.header.end].1].trim().to_owned();
                excluded.push((header, rule));
            }
            None => {}
        }
    }
    (!derives.is_empty() || !impls.is_empty() || !excluded.is_empty()).then_some(Derived {
        name,
        derives,
        impls,
        excluded,
    })
}

impl Derived {
    /// Each concrete trait impl of the type with no binding, by its header,
    /// with the rule that leaves it out.
    #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
    pub(crate) fn excluded(&self) -> &[(String, &'static str)] {
        &self.excluded
    }
}

/// `written`, text of the defining module, with each name that starts a
/// path spelled as `spell` gives it; a primitive's, a keyword's, the
/// prelude's and a lifetime's are kept as they are. `None` when a name
/// has no spelling.
#[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
fn respelled(written: &str, spell: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let bytes = written.as_bytes();
    let mut spelled = String::with_capacity(written.len());
    let mut at = 0;
    while at < bytes.len() {
        if !(bytes[at].is_ascii_alphabetic() || bytes[at] == b'_') {
            let character = written[at..].chars().next()?;
            spelled.push(character);
            at += character.len_utf8();
            continue;
        }
        let start = at;
        while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
            at += 1;
        }
        let word = &written[start..at];
        let lifetime = start > 0 && bytes[start - 1] == b'\'';
        let inner = written[..start].ends_with("::");
        if lifetime || inner || PRIMITIVES.contains(&word) || PRELUDE.contains(&word) {
            spelled.push_str(word);
        } else {
            spelled.push_str(&spell(word)?);
        }
    }
    Some(spelled)
}

/// Whether every parameter of the generic list `header` starts with is a
/// type parameter with a default, as `<T = f32>`: no lifetime, and no
/// parameter left to the program.
#[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
fn defaulted(text: &str, header: &[(Token<'_>, usize)]) -> bool {
    let Some(close) = closing(header, 0, b'<', b'>') else {
        return false;
    };
    let parameters = split(header, 1, close);
    !parameters.is_empty()
        && parameters.iter().all(|parameter| {
            let written = text[parameter.clone()].trim();
            !written.starts_with('\'') && written.contains('=')
        })
}

/// The traits of [`DERIVES`] the concrete struct or enum `item` derives.
fn derives_of(tokens: &[(Token<'_>, usize)], item: &ModuleItem<'_>) -> Option<Vec<&'static str>> {
    let lifetimes = tokens[item.header.clone()]
        .first()
        .is_some_and(|(token, _)| *token == Token::Punct(b'<'));
    if item.generic || lifetimes {
        return None;
    }
    let derives = derived_traits(tokens, item);
    (!derives.is_empty()).then_some(derives)
}

/// The traits of [`DERIVES`] the attributes of `item` derive, whatever its
/// parameters.
fn derived_traits(tokens: &[(Token<'_>, usize)], item: &ModuleItem<'_>) -> Vec<&'static str> {
    let attributes = tokens
        .iter()
        .filter(|(_, at)| item.bytes.contains(at))
        .take_while(|(token, _)| !matches!(token, Token::Ident("struct" | "enum", _)))
        .map(|(token, _)| *token)
        .collect::<Vec<_>>();
    let mut derives = Vec::new();
    let mut inside = false;
    for (index, token) in attributes.iter().enumerate() {
        match token {
            Token::Ident("derive", _) => inside = true,
            Token::Punct(b']') => inside = false,
            // The path's last segment names the trait: `std::default::Default`.
            Token::Ident(name, _)
                if inside && attributes.get(index + 1) != Some(&Token::Punct(b':')) =>
            {
                if let Some((derive, ..)) = DERIVES.iter().find(|(derive, ..)| derive == name) {
                    derives.push(*derive);
                }
            }
            _ => {}
        }
    }
    derives
}

/// The functions that bind `text`'s concrete items, one per item, and the
/// derives of its concrete types and of those it names from elsewhere,
/// `imported`.
pub(crate) fn functions(text: &str, imported: &[Derived]) -> String {
    let tokens = polyglot::placed(text);
    let mut bound = Bound {
        text,
        written: String::new(),
        count: 0,
    };
    for item in polyglot::module_items(text, &tokens) {
        if item.generic {
            continue;
        }
        match item.keyword {
            "fn" => bound.function(&tokens, &item, None),
            "static" => bound.static_item(&tokens, &item),
            "struct" => bound.literal(&tokens, &item),
            "impl" => bound.methods(&tokens, &item),
            _ => {}
        }
        if let (Some(name), "struct" | "enum") = (item.name, item.keyword)
            && let Some(derives) = derives_of(&tokens, &item)
        {
            // The module's own trait impls are bound as its items.
            bound.derives(&Derived {
                name: name.to_owned(),
                derives,
                impls: Vec::new(),
                excluded: Vec::new(),
            });
        }
    }
    for derived in imported {
        bound.derives(derived);
    }
    bound.written
}

/// How an impl's method is called: through its owner, or its trait.
struct Owner {
    /// The type the impl is for, as written.
    ty: String,
    /// `<Ty as Trait>`, for a trait impl.
    qualified: Option<String>,
    /// The associated types the impl gives: each name, its lifetime
    /// parameters and its value, as `type Class<'a> = StyleFn<'a, Self>;`.
    associated: Vec<Associated>,
}

/// An associated type a concrete impl gives (`Owner::associated`).
struct Associated {
    name: String,
    parameters: Vec<String>,
    value: String,
}

/// The binding functions written so far.
struct Bound<'t> {
    text: &'t str,
    written: String,
    count: usize,
}

impl Bound<'_> {
    /// `text` of `range`, with `Self` written as the owner.
    fn spelled(&self, range: Range<usize>, owner: Option<&Owner>) -> String {
        let written = self.normalized(range, owner);
        let Some(owner) = owner else {
            return written;
        };
        let associated = owner.qualified.as_deref().unwrap_or(&owner.ty);
        written
            .replace("Self::", &format!("{associated}::"))
            .replace("Self", &owner.ty)
    }

    /// `text` of `range` with each `Self::Name<..>` the impl gives a value
    /// of written as that value at its arguments, as rustc normalizes a
    /// projection on a concrete impl: `&Self::Class<'_>` of an impl whose
    /// `type Class<'a> = StyleFn<'a, Self>` is `&StyleFn<'_, Self>`. A
    /// `Self::` the impl gives no value of is left as it is.
    fn normalized(&self, range: Range<usize>, owner: Option<&Owner>) -> String {
        let mut written = self.text[range].to_owned();
        let Some(owner) = owner else {
            return written;
        };
        for associated in &owner.associated {
            let projection = format!("Self::{}", associated.name);
            let mut at = 0;
            while let Some(found) = written[at..].find(&projection) {
                let start = at + found;
                let mut end = start + projection.len();
                if written[end..]
                    .chars()
                    .next()
                    .is_some_and(|next| next.is_alphanumeric() || next == '_')
                {
                    at = end;
                    continue;
                }
                let mut arguments = Vec::new();
                if written[end..].starts_with('<') {
                    let Some(close) = matching_angle(&written[end..]) else {
                        break;
                    };
                    arguments = written[end + 1..end + close]
                        .split(',')
                        .map(|argument| argument.trim().to_owned())
                        .collect();
                    end += close + 1;
                }
                let mut value = associated.value.clone();
                for (parameter, argument) in associated.parameters.iter().zip(&arguments) {
                    value = value.replace(parameter.as_str(), argument);
                }
                written.replace_range(start..end, &value);
                at = start + value.len();
            }
        }
        written
    }

    /// A call of each derived method of `derived`.
    fn derives(&mut self, derived: &Derived) {
        for derive in &derived.derives {
            let Some((_, signature, call)) = DERIVES.iter().find(|(name, ..)| name == derive)
            else {
                continue;
            };
            let binding = self.next_name();
            let signature = signature.replace("{}", &derived.name);
            let call = call.replace("{}", &derived.name);
            let _ = writeln!(
                self.written,
                "\n#[allow(dead_code)]\nfn {binding}{signature} {{\n    {call}\n}}"
            );
        }
        for binding in &derived.impls {
            self.write(binding);
        }
    }

    fn next_name(&mut self) -> String {
        self.count += 1;
        format!("{BINDING}{}", self.count)
    }

    /// A call of the function `item`, a method of `owner` when given.
    fn function(
        &mut self,
        tokens: &[(Token<'_>, usize)],
        item: &ModuleItem<'_>,
        owner: Option<&Owner>,
    ) {
        if let Some(binding) = self.binding(tokens, item, owner) {
            self.write(&binding);
        }
    }

    /// The binding function of `binding`, named next.
    fn write(&mut self, binding: &Binding) {
        let name = self.next_name();
        let output = if binding.output.is_empty() {
            String::new()
        } else {
            format!(" -> {}", binding.output)
        };
        let discard = if binding.discarded { "let _ = " } else { "" };
        let end = if binding.discarded { ";" } else { "" };
        let _ = writeln!(
            self.written,
            "\n#[allow(dead_code)]\nfn {name}{}({}){output} {{\n    {discard}{}({}){end}\n}}",
            binding.lifetimes,
            binding.parameters.join(", "),
            binding.path,
            binding.arguments.join(", "),
        );
    }

    /// The binding of the function `item`, a method of `owner` when given:
    /// `None` for one that isn't called as written.
    fn binding(
        &self,
        tokens: &[(Token<'_>, usize)],
        item: &ModuleItem<'_>,
        owner: Option<&Owner>,
    ) -> Option<Binding> {
        let name = item.name?;
        let header = &tokens[item.header.clone()];
        // Only a safe, synchronous function is called as written.
        let qualified = tokens.iter().any(|(token, at)| {
            item.bytes.contains(at)
                && *at < header.first().map_or(0, |(_, at)| *at)
                && matches!(token, Token::Ident("unsafe" | "async" | "extern", _))
        });
        if qualified || header.is_empty() {
            return None;
        }
        // The lifetimes it has, and its parameters' list.
        let (lifetimes, open) = match header.first() {
            Some((Token::Punct(b'<'), start)) => {
                let close = closing(header, 0, b'<', b'>')?;
                (self.text[*start..header[close].1 + 1].to_owned(), close + 1)
            }
            _ => (String::new(), 0),
        };
        if header.get(open).map(|(token, _)| *token) != Some(Token::Punct(b'(')) {
            return None;
        }
        let close = closing(header, open, b'(', b')')?;
        let output_start = header[close].1 + 1;
        let output_end = header
            .iter()
            .skip(close + 1)
            .find(|(token, _)| matches!(token, Token::Ident("where", _)))
            .map_or_else(|| tokens[item.header.end].1, |(_, at)| *at);
        let output = self.spelled(output_start..output_end, owner);
        let output = output.trim().trim_start_matches("->").trim();
        // An output that names one of `Self`'s associated types, as a
        // catalog's `default<'a>() -> Self::Class<'a>`, is spelled through
        // a projection rawr can't name in a signature yet: the call is
        // made and its value dropped, which binds the item all the same.
        let raw = self.normalized(output_start..output_end, owner);
        let discarded = raw.contains("Self::");
        let output = if discarded { "" } else { output };
        let mut parameters = Vec::new();
        let mut arguments = Vec::new();
        for parameter in split(header, open + 1, close) {
            let written = self.text[parameter.clone()].trim();
            let receiver = written
                .trim_start_matches('&')
                .trim_start_matches("mut ")
                .trim();
            let receiver = receiver.strip_prefix('\'').map_or(receiver, |rest| {
                rest.split_once(' ').map_or("", |(_, self_)| self_.trim())
            });
            let argument = format!("__a{}", arguments.len());
            if receiver == "self" || receiver == "mut self" {
                let owner = owner?;
                let borrow = if written.starts_with("&mut") {
                    "&mut "
                } else if written.starts_with('&') {
                    "&"
                } else {
                    ""
                };
                parameters.push(format!("{argument}: {borrow}{}", owner.ty));
            } else {
                let colon = top_colon(&self.text[parameter.clone()])?;
                // A parameter of one of `Self`'s associated types has no
                // spelling a binding's signature can hold yet.
                if self.normalized(parameter.clone(), owner).contains("Self::") {
                    return None;
                }
                let ty = self.spelled(parameter.start + colon + 1..parameter.end, owner);
                parameters.push(format!("{argument}: {}", ty.trim()));
            }
            arguments.push(argument);
        }
        let path = match owner {
            Some(Owner {
                qualified: Some(qualified),
                ..
            }) => format!("{qualified}::{name}"),
            Some(owner) => format!("{}::{name}", owner.ty),
            None => name.to_owned(),
        };
        Some(Binding {
            lifetimes,
            parameters,
            output: output.to_owned(),
            discarded,
            path,
            arguments,
        })
    }

    /// A borrow of the static `item`.
    fn static_item(&mut self, tokens: &[(Token<'_>, usize)], item: &ModuleItem<'_>) {
        let Some(name) = item.name.filter(|name| *name != "mut") else {
            return;
        };
        let header = &tokens[item.header.clone()];
        let Some((_, colon)) = header
            .first()
            .filter(|(token, _)| *token == Token::Punct(b':'))
        else {
            return;
        };
        let Some((_, equals)) = header
            .iter()
            .find(|(token, _)| *token == Token::Punct(b'='))
        else {
            return;
        };
        let ty = self.text[colon + 1..*equals].trim().to_owned();
        let binding = self.next_name();
        let _ = writeln!(
            self.written,
            "\n#[allow(dead_code)]\nfn {binding}() -> &'static {ty} {{\n    &{name}\n}}"
        );
    }

    /// A literal of the named-field struct `item`.
    fn literal(&mut self, tokens: &[(Token<'_>, usize)], item: &ModuleItem<'_>) {
        let Some(name) = item.name else {
            return;
        };
        if !tokens[item.header.clone()].is_empty() || item.header.end >= item.end {
            return;
        }
        let body = &tokens[item.header.end..=item.end];
        if body.first().map(|(token, _)| *token) != Some(Token::Punct(b'{')) {
            return;
        }
        let mut parameters = Vec::new();
        let mut fields = Vec::new();
        for field in split(body, 1, body.len() - 1) {
            let written = &self.text[field.clone()];
            let Some(colon) = top_colon(written) else {
                return;
            };
            let Some(field_name) = written[..colon].split_whitespace().last() else {
                return;
            };
            let argument = format!("__a{}", fields.len());
            parameters.push(format!("{argument}: {}", written[colon + 1..].trim()));
            fields.push(format!("{field_name}: {argument}"));
        }
        let binding = self.next_name();
        let _ = writeln!(
            self.written,
            "\n#[allow(dead_code)]\nfn {binding}({}) -> {name} {{\n    {name} {{ {} }}\n}}",
            parameters.join(", "),
            fields.join(", "),
        );
    }

    /// A call of each function of the concrete impl `item`.
    fn methods(&mut self, tokens: &[(Token<'_>, usize)], item: &ModuleItem<'_>) {
        let Some(owner) = self.impl_owner(tokens, item) else {
            return;
        };
        let body = &tokens[item.header.end + 1..item.end];
        for method in polyglot::module_items(self.text, body) {
            if method.keyword == "fn" && !method.generic {
                self.function(body, &method, Some(&owner));
            }
        }
    }

    /// The bindings of the methods of `item`, a concrete trait impl for the
    /// type `defined` of this text, each name spelled by `spell`
    /// ([`respelled`]). `None` for an impl of another type or of no trait;
    /// the rule that leaves it out where no call of a method can be
    /// written.
    #[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
    fn trait_impl_bindings(
        &self,
        tokens: &[(Token<'_>, usize)],
        item: &ModuleItem<'_>,
        defined: &str,
        spell: &dyn Fn(&str) -> Option<String>,
    ) -> Option<Result<Vec<Binding>, &'static str>> {
        let owner = self.impl_owner(tokens, item)?;
        if owner.qualified.is_none() || owner.ty != defined {
            return None;
        }
        let body = &tokens[item.header.end + 1..item.end];
        let mut bindings = Vec::new();
        for method in polyglot::module_items(self.text, body) {
            if method.keyword != "fn" {
                continue;
            }
            let projected = split_parameters(body, &method).iter().any(|parameter| {
                self.normalized(parameter.clone(), Some(&owner))
                    .contains("Self::")
            });
            if projected {
                return Some(Err(PROJECTED));
            }
            let binding = if method.generic {
                None
            } else {
                self.binding(body, &method, Some(&owner))
            };
            let Some(binding) = binding else {
                return Some(Err(UNCALLED));
            };
            let spelled = |written: &str| respelled(written, spell);
            let parameters = binding
                .parameters
                .iter()
                .map(|parameter| {
                    let (argument, ty) = parameter.split_once(": ")?;
                    Some(format!("{argument}: {}", spelled(ty)?))
                })
                .collect::<Option<Vec<_>>>();
            let (Some(parameters), Some(output), Some(path)) =
                (parameters, spelled(&binding.output), spelled(&binding.path))
            else {
                return Some(Err(UNNAMED));
            };
            bindings.push(Binding {
                lifetimes: binding.lifetimes,
                parameters,
                output,
                discarded: binding.discarded,
                path,
                arguments: binding.arguments,
            });
        }
        Some(Ok(bindings))
    }

    /// The type the concrete impl `item` is for, and its trait: `None` for
    /// an impl with lifetime parameters, or of a generic type's instance,
    /// which is left to the program's uses.
    fn impl_owner(&self, tokens: &[(Token<'_>, usize)], item: &ModuleItem<'_>) -> Option<Owner> {
        let header = &tokens[item.header.clone()];
        let owner_tokens = header
            .iter()
            .position(|(token, _)| matches!(token, Token::Ident("for", _)))
            .map_or(header, |at| &header[at + 1..]);
        let arguments = header
            .first()
            .is_some_and(|(token, _)| *token == Token::Punct(b'<'))
            || owner_tokens
                .iter()
                .any(|(token, _)| *token == Token::Punct(b'<'));
        if arguments || item.header.end >= item.end {
            return None;
        }
        let start = header.first().map_or(0, |(_, at)| *at);
        let written = self.text[start..tokens[item.header.end].1].trim();
        let written = written
            .split_once(" where ")
            .map_or(written, |(before, _)| before)
            .trim();
        let associated = self.associated(&tokens[item.header.end + 1..item.end]);
        Some(match written.split_once(" for ") {
            Some((trait_name, ty)) => Owner {
                ty: ty.trim().to_owned(),
                qualified: Some(format!("<{} as {}>", ty.trim(), trait_name.trim())),
                associated,
            },
            None => Owner {
                ty: written.to_owned(),
                qualified: None,
                associated,
            },
        })
    }

    /// The associated types an impl's `body` gives, each with only
    /// lifetime parameters: a type parameter's is left unnormalized.
    fn associated(&self, body: &[(Token<'_>, usize)]) -> Vec<Associated> {
        polyglot::module_items(self.text, body)
            .into_iter()
            .filter(|item| item.keyword == "type" && !item.generic)
            .filter_map(|item| {
                let name = item.name?.to_owned();
                let start = body.get(item.header.start).map(|(_, at)| *at)?;
                let end = body.get(item.end).map(|(_, at)| *at)?;
                let written = self.text[start..end].trim();
                let (head, value) = written.split_once('=')?;
                let parameters = head
                    .trim()
                    .strip_prefix('<')
                    .and_then(|head| head.strip_suffix('>'))
                    .map(|list| {
                        list.split(',')
                            .map(|parameter| parameter.trim().to_owned())
                            .collect()
                    })
                    .unwrap_or_default();
                Some(Associated {
                    name,
                    parameters,
                    value: value.trim().trim_end_matches(';').trim().to_owned(),
                })
            })
            .collect()
    }
}

/// Where the `>` that closes the `<` `written` starts with is, past any
/// `->`.
fn matching_angle(written: &str) -> Option<usize> {
    let bytes = written.as_bytes();
    let mut depth = 0_usize;
    for (at, byte) in bytes.iter().enumerate() {
        match byte {
            b'<' => depth += 1,
            b'>' if at > 0 && bytes[at - 1] == b'-' => {}
            b'>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The byte ranges of the parameters of the function `item`, whose tokens
/// are among `tokens`: none where its header isn't a function's.
#[cfg(any(test, all(feature = "rust-import", not(target_arch = "wasm32"))))]
fn split_parameters(tokens: &[(Token<'_>, usize)], item: &ModuleItem<'_>) -> Vec<Range<usize>> {
    let header = &tokens[item.header.clone()];
    let open = match header.first() {
        Some((Token::Punct(b'<'), _)) => match closing(header, 0, b'<', b'>') {
            Some(close) => close + 1,
            None => return Vec::new(),
        },
        _ => 0,
    };
    if header.get(open).map(|(token, _)| *token) != Some(Token::Punct(b'(')) {
        return Vec::new();
    }
    match closing(header, open, b'(', b')') {
        Some(close) => split(header, open + 1, close),
        None => Vec::new(),
    }
}

/// The index of the token that closes the `open` at `at` in `tokens`.
fn closing(tokens: &[(Token<'_>, usize)], at: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, (token, _)) in tokens.iter().enumerate().skip(at) {
        match *token {
            Token::Punct(byte) if byte == open => depth += 1,
            Token::Punct(byte) if byte == close => {
                if close == b'>' && index > 0 && tokens[index - 1].0 == Token::Punct(b'-') {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// The byte ranges of the comma-separated parts of `tokens[from..to]`.
fn split(tokens: &[(Token<'_>, usize)], from: usize, to: usize) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = None;
    for index in from..to {
        let (token, at) = tokens[index];
        if token == Token::Punct(b'#') && depth == 0 && start.is_none() {
            continue;
        }
        match token {
            Token::Punct(b'(' | b'[' | b'{' | b'<') => depth += 1,
            Token::Punct(b'>') if index > 0 && tokens[index - 1].0 == Token::Punct(b'-') => {}
            Token::Punct(b')' | b']' | b'}' | b'>') => depth = depth.saturating_sub(1),
            Token::Punct(b',') if depth == 0 => {
                if let Some(start) = start.take() {
                    parts.push(start..at);
                }
                continue;
            }
            _ => {}
        }
        start.get_or_insert(at);
    }
    if let Some(start) = start {
        parts.push(start..tokens[to].1);
    }
    parts
}

/// Where the `:` that ends a parameter's pattern or a field's name is in
/// `written`, past any `::`.
fn top_colon(written: &str) -> Option<usize> {
    let bytes = written.as_bytes();
    let mut depth = 0_usize;
    for (at, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' | b'>' => depth = depth.saturating_sub(1),
            b':' if depth == 0
                && bytes.get(at + 1) != Some(&b':')
                && (at == 0 || bytes[at - 1] != b':') =>
            {
                return Some(at);
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    /// A module's static, struct, function, inherent method and trait
    /// impl's method each get a function that names it, and so does each
    /// derive of its concrete types and of a type it names from elsewhere,
    /// and each method of that type's concrete trait impls, its names
    /// spelled by the module's paths to them. An impl with a name the
    /// module has no path to, or a method no binding can spell, is left
    /// out with its rule.
    #[test]
    fn each_concrete_item_is_named_by_a_function() {
        let text = "static SHADED: Counter = Counter { count: 0 };\n\
                    #[derive(Debug, Clone, Default)]\n\
                    struct Style { tint: Tint, order: usize }\n\
                    fn darken(tint: Tint) -> Tint { tint }\n\
                    impl Style { pub fn order(&self) -> usize { self.order } }\n\
                    impl From<[u8; 3]> for Tint { fn from([r, g, b]: [u8; 3]) -> Self { Tint { r, g, b } } }\n";
        let border = "#[derive(Debug, Clone, Copy, PartialEq, Default)]\n\
                      pub struct Border { pub width: f32 }\n\
                      impl From<f32> for Border { fn from(width: f32) -> Self { Border { width } } }\n\
                      impl From<[u16; 2]> for Border { fn from(p: [u16; 2]) -> Border { Border { width: 0.0 } } }\n\
                      impl From<Pixels> for Border { fn from(p: Pixels) -> Self { Border { width: p.0 } } }\n\
                      impl fmt::Display for Border { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { Ok(()) } }\n\
                      impl From<Hidden> for Border { fn from(_: Hidden) -> Self { Border { width: 0.0 } } }\n\
                      impl Catalog for Border { type Class = u8; fn style(&self, class: &Self::Class) -> u8 { *class } }\n\
                      impl Palette for Border { type Class<'a> = &'a u8; fn default<'a>() -> Self::Class<'a> { &0 } }\n\
                      impl Shade for Border { fn shade(&self, class: &Self::Tone) -> u8 { 0 } }\n\
                      impl<T> From<Vec<T>> for Border { fn from(_: Vec<T>) -> Self { Border { width: 0.0 } } }\n\
                      #[derive(Default)]\n\
                      pub struct Wrap<T>(T);\n\
                      #[derive(Default, Clone)]\n\
                      pub struct Frame<T = f32> { pub x: T }\n";
        // The naming module writes `Border` as `Edge`, and reaches `Pixels`
        // and `fmt` by paths of its own; it has none to `Hidden`.
        let spell = |name: &str| match name {
            "Border" => Some("Edge".to_owned()),
            "Pixels" => Some("crate::core::Pixels".to_owned()),
            "fmt" => Some("std::fmt".to_owned()),
            "Catalog" => Some("crate::Catalog".to_owned()),
            "Palette" => Some("crate::Palette".to_owned()),
            "Shade" => Some("crate::Shade".to_owned()),
            _ => None,
        };
        let imported = super::derived(border, "Border", &spell).unwrap();
        assert!(super::derived(border, "Wrap", &spell).is_none());
        // `Frame` alone is `Frame<f32>`, whose derives the module binds.
        let frame = |name: &str| (name == "Frame").then(|| "Frame".to_owned());
        let framed = super::functions("", &[super::derived(border, "Frame", &frame).unwrap()]);
        assert!(
            framed.contains("() -> Frame {\n    <Frame as Default>::default()\n}"),
            "{framed}"
        );
        assert_eq!(
            imported.excluded(),
            [
                ("From<Hidden> for Border".to_owned(), super::UNNAMED),
                ("Shade for Border".to_owned(), super::PROJECTED),
            ]
        );
        let bound = super::functions(text, &[imported]);
        for expected in [
            "() -> &'static Counter {\n    &SHADED\n}",
            "(__a0: Tint, __a1: usize) -> Style {\n    Style { tint: __a0, order: __a1 }\n}",
            "(__a0: Tint) -> Tint {\n    darken(__a0)\n}",
            "(__a0: &Style) -> usize {\n    Style::order(__a0)\n}",
            "(__a0: &Style) -> Style {\n    <Style as Clone>::clone(__a0)\n}",
            "() -> Style {\n    <Style as Default>::default()\n}",
            "(__a0: [u8; 3]) -> Tint {\n    <Tint as From<[u8; 3]>>::from(__a0)\n}",
            "() -> Edge {\n    <Edge as Default>::default()\n}",
            "(__a0: &Edge) -> Edge {\n    <Edge as Clone>::clone(__a0)\n}",
            "(__a0: &Edge, __a1: &Edge) -> bool {\n    <Edge as PartialEq>::eq(__a0, __a1)\n}",
            "(__a0: f32) -> Edge {\n    <Edge as From<f32>>::from(__a0)\n}",
            "(__a0: [u16; 2]) -> Edge {\n    <Edge as From<[u16; 2]>>::from(__a0)\n}",
            "(__a0: crate::core::Pixels) -> Edge {\n    <Edge as From<crate::core::Pixels>>::from(__a0)\n}",
            "<'a>() -> &'a u8 {\n    <Edge as crate::Palette>::default()\n}",
            "(__a0: &Edge, __a1: &u8) -> u8 {\n    <Edge as crate::Catalog>::style(__a0, __a1)\n}",
            "(__a0: &Edge, __a1: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n    <Edge as std::fmt::Display>::fmt(__a0, __a1)\n}",
        ] {
            assert!(bound.contains(expected), "{expected}\nin\n{bound}");
        }
        // A generic impl is left to the program's uses.
        assert!(!bound.contains("Vec"), "{bound}");
    }
}
