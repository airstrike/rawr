//! The provided methods of a crate trait a crate-source module declares,
//! each as a generic function of the program module (D1118, #2698). The
//! trait stays native, and a native method rustc selects runs natively.
//! A method the impl rustc selects leaves to the trait, whose signature
//! can't cross, runs from the trait's own body instead, as iced's
//! `Widget::overlay` does for a native `Text`: the function is that body
//! over a type parameter in place of `Self`, bounded by the trait at its
//! own parameters, so its calls go through the trait's bound as the
//! body's own calls through `Self` do.

use super::super::polyglot::{module_items, ModuleItem, Token};

/// The name of the receiver parameter that stands for `self`.
const RECEIVER: &str = "__rawr_self";

/// The type parameter that stands for `Self`.
pub(crate) const SELF_TYPE: &str = "__RawrSelf";

/// The template functions of the provided methods of `item`, a trait of
/// `text` whose tokens are `tokens`, that `reached` names: those with a
/// receiver, as a method call reaches them.
pub(crate) fn templates(
    text: &str,
    tokens: &[(Token<'_>, usize)],
    item: &ModuleItem<'_>,
    reached: &dyn Fn(&str) -> bool,
) -> String {
    let Some(trait_name) = item.name else {
        return String::new();
    };
    let header = &tokens[item.header.clone()];
    let Some(open) = tokens.get(item.header.end).map(|(_, at)| *at) else {
        return String::new();
    };
    let (parameters, after) = match header.first() {
        Some((Token::Punct(b'<'), at)) => match closing(header, 0) {
            Some(close) => (&text[at + 1..header[close].1], close + 1),
            None => return String::new(),
        },
        _ => ("", 0),
    };
    let trait_where = header[after..]
        .iter()
        .position(|(token, _)| matches!(token, Token::Ident("where", _)))
        .map_or("", |offset| {
            let at = header[after + offset].1 + "where".len();
            &text[at..open]
        });
    let parameters = split(parameters)
        .into_iter()
        .map(without_default)
        .collect::<Vec<_>>();
    let arguments = parameters
        .iter()
        .map(|parameter| parameter_name(parameter))
        .collect::<Vec<_>>();
    let bound = if arguments.is_empty() {
        trait_name.to_owned()
    } else {
        format!("{trait_name}<{}>", arguments.join(", "))
    };
    let body = &tokens[item.header.end + 1..item.end];
    let mut written = String::new();
    for method in module_items(text, body) {
        let provided = body
            .get(method.end)
            .is_some_and(|(token, _)| *token == Token::Punct(b'}'));
        let Some(name) = method.name.filter(|name| reached(name)) else {
            continue;
        };
        if method.keyword != "fn" || !provided {
            continue;
        }
        if let Some(template) = template(
            text,
            body,
            &method,
            &bound,
            &parameters,
            trait_where,
            &super::provided(trait_name, name),
        ) {
            written.push('\n');
            written.push_str(&template);
            written.push('\n');
        }
    }
    written
}

/// The template of the provided method `method`, whose tokens are among
/// `tokens`, named `name`, over a `Self` bounded by `bound`, with the
/// trait's `parameters` and where-clause: `None` for a function with no
/// receiver.
fn template(
    text: &str,
    tokens: &[(Token<'_>, usize)],
    method: &ModuleItem<'_>,
    bound: &str,
    parameters: &[&str],
    trait_where: &str,
    name: &str,
) -> Option<String> {
    let header = &tokens[method.header.clone()];
    let (own, inputs_at) = match header.first() {
        Some((Token::Punct(b'<'), at)) => {
            let close = closing(header, 0)?;
            (&text[at + 1..header[close].1], close + 1)
        }
        _ => ("", 0),
    };
    if header.get(inputs_at).map(|(token, _)| *token) != Some(Token::Punct(b'(')) {
        return None;
    }
    let inputs_close = closing(header, inputs_at)?;
    let open = tokens.get(method.header.end)?.1;
    let close = tokens.get(method.end)?.1;
    let inputs = split(&text[header[inputs_at].1 + 1..header[inputs_close].1]);
    let (&receiver, rest) = inputs.split_first()?;
    let receiver = written_receiver(receiver)?;
    let rest_start = header[inputs_close].1 + 1;
    let where_at = header[inputs_close..]
        .iter()
        .find(|(token, _)| matches!(token, Token::Ident("where", _)))
        .map(|(_, at)| *at);
    let output = rewritten(text, tokens, rest_start..where_at.unwrap_or(open));
    let own_where = where_at.map_or_else(String::new, |at| {
        rewritten(text, tokens, at + "where".len()..open)
    });
    let rest = rest
        .iter()
        .map(|input| {
            let start = offset_in(text, input);
            rewritten(text, tokens, start..start + input.len())
        })
        .collect::<Vec<_>>();
    let body = rewritten(text, tokens, open..close + 1);
    let own = split(own);
    let (lifetimes, types): (Vec<&str>, Vec<&str>) = parameters
        .iter()
        .copied()
        .chain(own)
        .partition(|parameter| parameter.starts_with('\''));
    let self_bound = format!("{SELF_TYPE}: {bound}");
    let generics = lifetimes
        .into_iter()
        .map(str::to_owned)
        .chain(std::iter::once(self_bound))
        .chain(types.into_iter().map(str::to_owned))
        .collect::<Vec<_>>()
        .join(", ");
    let wheres = [trait_where.trim(), own_where.trim()]
        .into_iter()
        .map(|clause| clause.trim_end_matches(','))
        .filter(|clause| !clause.trim().is_empty())
        .collect::<Vec<_>>();
    let wheres = if wheres.is_empty() {
        String::new()
    } else {
        format!(" where {}", wheres.join(", "))
    };
    let inputs = std::iter::once(receiver)
        .chain(rest)
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "pub fn {name}<{generics}>({inputs}) {}{wheres} {body}",
        output.trim()
    ))
}

/// The receiver `self`, `mut self`, `&self`, `&'a mut self` or `self: T`
/// as the template's first parameter, of the type parameter in place of
/// `Self`: `None` when `receiver` is no receiver.
fn written_receiver(receiver: &str) -> Option<String> {
    let receiver = receiver.trim();
    let words = |text: &str| replaced_words(text);
    if let Some(ty) = receiver
        .strip_prefix("self")
        .and_then(|rest| rest.trim_start().strip_prefix(':'))
    {
        return Some(format!("{RECEIVER}: {}", words(ty).trim()));
    }
    if let Some(ty) = receiver.strip_prefix("mut ") {
        return (ty.trim() == "self").then(|| format!("mut {RECEIVER}: {SELF_TYPE}"));
    }
    if receiver == "self" {
        return Some(format!("{RECEIVER}: {SELF_TYPE}"));
    }
    let borrowed = receiver.strip_suffix("self")?;
    borrowed
        .starts_with('&')
        .then(|| format!("{RECEIVER}: {borrowed}{SELF_TYPE}"))
}

/// `text` with each word `self` and `Self` replaced, as [`rewritten`]
/// does, for a text with no tokens of its own.
fn replaced_words(text: &str) -> String {
    let tokens = super::super::polyglot::placed(text);
    rewritten(text, &tokens, 0..text.len())
}

/// The bytes `range` of `text` with each `self` that is no path's first
/// segment written as the receiver parameter and each `Self` as the type
/// parameter, by the tokens `tokens` that start in the range.
fn rewritten(text: &str, tokens: &[(Token<'_>, usize)], range: std::ops::Range<usize>) -> String {
    let mut written = String::with_capacity(range.len());
    let mut from = range.start;
    for (index, (token, at)) in tokens.iter().enumerate() {
        if !range.contains(at) || *at < from {
            continue;
        }
        let path = matches!(
            tokens.get(index + 1..index + 3),
            Some([(Token::Punct(b':'), _), (Token::Punct(b':'), _)])
        );
        let replacement = match token {
            Token::Ident("self", _) if !path => RECEIVER,
            Token::Ident("Self", _) => SELF_TYPE,
            _ => continue,
        };
        written.push_str(&text[from..*at]);
        written.push_str(replacement);
        from = at + "self".len();
    }
    written.push_str(&text[from..range.end]);
    written
}

/// The index of the `>` or `)` that closes the `<` or `(` at `open` of
/// `tokens`.
fn closing(tokens: &[(Token<'_>, usize)], open: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, (token, _)) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::Punct(b'>') if index > 0 && tokens[index - 1].0 == Token::Punct(b'-') => {}
            Token::Punct(b'<' | b'(' | b'[') => depth += 1,
            Token::Punct(b'>' | b')' | b']') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// The parameters or inputs of the list `text`, split at its commas
/// outside brackets, each trimmed: none for an empty list.
fn split(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'<' | b'(' | b'[' | b'{' => depth += 1,
            b'>' if index > 0 && bytes[index - 1] == b'-' => {}
            b'>' | b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts.retain(|part| !part.is_empty());
    parts
}

/// The type parameter `parameter` without its default, as a function's
/// parameter takes none: `T = ()` is `T`.
fn without_default(parameter: &str) -> &str {
    let mut depth = 0_usize;
    for (index, byte) in parameter.bytes().enumerate() {
        match byte {
            b'<' | b'(' | b'[' => depth += 1,
            b'>' | b')' | b']' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 => return parameter[..index].trim(),
            _ => {}
        }
    }
    parameter
}

/// The name a parameter declares: `'a` of `'a: 'b`, `T` of `T: Clone` and
/// `N` of `const N: usize`.
fn parameter_name(parameter: &str) -> &str {
    let parameter = parameter.strip_prefix("const ").unwrap_or(parameter).trim();
    let end = parameter
        .find(|character: char| character == ':' || character.is_whitespace())
        .unwrap_or(parameter.len());
    &parameter[..end]
}

/// Where `part`, a slice of `text`, starts in it.
fn offset_in(text: &str, part: &str) -> usize {
    part.as_ptr() as usize - text.as_ptr() as usize
}

#[cfg(test)]
mod tests {
    use super::super::super::polyglot::{module_items, placed};

    fn templates(text: &str) -> String {
        let tokens = placed(text);
        let items = module_items(text, &tokens);
        items
            .iter()
            .filter(|item| item.keyword == "trait")
            .map(|item| super::templates(text, &tokens, item, &|name| name != "skipped"))
            .collect()
    }

    /// A provided method is a function over a parameter in place of
    /// `Self`, bounded by the trait at its own parameters, and a required
    /// one, an associated function and an unreached one are none.
    #[test]
    fn a_traits_provided_methods_are_functions_over_its_self() {
        let text = "pub trait Widget<M, T = ()>: Meta where T: Clone {\n\
                    fn size(&self) -> usize;\n\
                    /// Lifted.\n\
                    fn lifted<'b>(&'b mut self, node: &'b mut Node) -> Vec<Lifted<'b, M>> {\n\
                    node.mark(&self.label()); let _ = Self::size; self::helper(); Vec::new() }\n\
                    fn boxed(self) -> Boxed<Self> where Self: Sized { Boxed(self) }\n\
                    fn new() -> Self { todo!() }\n\
                    fn skipped(&self) {}\n\
                    }\n";
        assert_eq!(
            templates(text),
            "\npub fn __rawr_provided__Widget__lifted<'b, __RawrSelf: Widget<M, T>, M, T>(\
             __rawr_self: &'b mut __RawrSelf, node: &'b mut Node) -> Vec<Lifted<'b, M>> where T: Clone \
             {\nnode.mark(&__rawr_self.label()); let _ = __RawrSelf::size; self::helper(); Vec::new() }\n\
             \npub fn __rawr_provided__Widget__boxed<__RawrSelf: Widget<M, T>, M, T>(\
             __rawr_self: __RawrSelf) -> Boxed<__RawrSelf> where T: Clone, __RawrSelf: Sized \
             { Boxed(__rawr_self) }\n"
        );
    }
}
