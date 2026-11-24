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

/// The functions that bind `text`'s concrete items, one per item.
pub(crate) fn functions(text: &str) -> String {
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
    }
    bound.written
}

/// How an impl's method is called: through its owner, or its trait.
struct Owner {
    /// The type the impl is for, as written.
    ty: String,
    /// `<Ty as Trait>`, for a trait impl.
    qualified: Option<String>,
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
        let written = &self.text[range];
        let Some(owner) = owner else {
            return written.to_owned();
        };
        let associated = owner.qualified.as_deref().unwrap_or(&owner.ty);
        written
            .replace("Self::", &format!("{associated}::"))
            .replace("Self", &owner.ty)
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
        let Some(name) = item.name else {
            return;
        };
        let header = &tokens[item.header.clone()];
        // Only a safe, synchronous function is called as written.
        let qualified = tokens.iter().any(|(token, at)| {
            item.bytes.contains(at)
                && *at < header.first().map_or(0, |(_, at)| *at)
                && matches!(token, Token::Ident("unsafe" | "async" | "extern", _))
        });
        if qualified || header.is_empty() {
            return;
        }
        // The lifetimes it has, and its parameters' list.
        let (lifetimes, open) = match header.first() {
            Some((Token::Punct(b'<'), start)) => {
                let Some(close) = closing(header, 0, b'<', b'>') else {
                    return;
                };
                (self.text[*start..header[close].1 + 1].to_owned(), close + 1)
            }
            _ => (String::new(), 0),
        };
        if header.get(open).map(|(token, _)| *token) != Some(Token::Punct(b'(')) {
            return;
        }
        let Some(close) = closing(header, open, b'(', b')') else {
            return;
        };
        // A trait impl's method whose signature names one of `Self`'s
        // associated types, such as a catalog's `Self::Class<'a>`, is
        // spelled through a projection the binding can't name yet.
        let signature = &self.text[header[open].1..tokens[item.header.end].1];
        if owner.is_some_and(|owner| owner.qualified.is_some()) && signature.contains("Self::") {
            return;
        }
        let output_start = header[close].1 + 1;
        let output_end = header
            .iter()
            .skip(close + 1)
            .find(|(token, _)| matches!(token, Token::Ident("where", _)))
            .map_or_else(|| tokens[item.header.end].1, |(_, at)| *at);
        let output = self.spelled(output_start..output_end, owner);
        let output = output.trim().trim_start_matches("->").trim();
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
                let Some(owner) = owner else {
                    return;
                };
                let borrow = if written.starts_with("&mut") {
                    "&mut "
                } else if written.starts_with('&') {
                    "&"
                } else {
                    ""
                };
                parameters.push(format!("{argument}: {borrow}{}", owner.ty));
            } else {
                let Some(colon) = top_colon(&self.text[parameter.clone()]) else {
                    return;
                };
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
        let binding = self.next_name();
        let output = if output.is_empty() {
            String::new()
        } else {
            format!(" -> {output}")
        };
        let _ = writeln!(
            self.written,
            "\n#[allow(dead_code)]\nfn {binding}{lifetimes}({}){output} {{\n    {path}({})\n}}",
            parameters.join(", "),
            arguments.join(", "),
        );
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
        let header = &tokens[item.header.clone()];
        // An impl with lifetime parameters, or of a generic type's instance,
        // is left to the program's uses.
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
            return;
        }
        let start = header.first().map_or(0, |(_, at)| *at);
        let written = self.text[start..tokens[item.header.end].1].trim();
        let written = written
            .split_once(" where ")
            .map_or(written, |(before, _)| before)
            .trim();
        let owner = match written.split_once(" for ") {
            Some((trait_name, ty)) => Owner {
                ty: ty.trim().to_owned(),
                qualified: Some(format!("<{} as {}>", ty.trim(), trait_name.trim())),
            },
            None => Owner {
                ty: written.to_owned(),
                qualified: None,
            },
        };
        let body = &tokens[item.header.end + 1..item.end];
        for method in polyglot::module_items(self.text, body) {
            if method.keyword == "fn" && !method.generic {
                self.function(body, &method, Some(&owner));
            }
        }
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
    /// impl's method each get a function that names it.
    #[test]
    fn each_concrete_item_is_named_by_a_function() {
        let text = "static SHADED: Counter = Counter { count: 0 };\n\
                    struct Style { tint: Tint, order: usize }\n\
                    fn darken(tint: Tint) -> Tint { tint }\n\
                    impl Style { pub fn order(&self) -> usize { self.order } }\n\
                    impl From<[u8; 3]> for Tint { fn from([r, g, b]: [u8; 3]) -> Self { Tint { r, g, b } } }\n";
        let bound = super::functions(text);
        for expected in [
            "fn __rawr_bind_1() -> &'static Counter {\n    &SHADED\n}",
            "fn __rawr_bind_2(__a0: Tint, __a1: usize) -> Style {\n    Style { tint: __a0, order: __a1 }\n}",
            "fn __rawr_bind_3(__a0: Tint) -> Tint {\n    darken(__a0)\n}",
            "fn __rawr_bind_4(__a0: &Style) -> usize {\n    Style::order(__a0)\n}",
            "fn __rawr_bind_5(__a0: [u8; 3]) -> Tint {\n    <Tint as From<[u8; 3]>>::from(__a0)\n}",
        ] {
            assert!(bound.contains(expected), "{expected}\nin\n{bound}");
        }
    }
}
