//! Rawr's attributes, for the cargo build of a crate whose files are marked
//! `// rawr`.
//!
//! Rawr reads these attributes itself when it runs the crate. Under cargo
//! each one leaves its item as written, so a release build carries nothing
//! of Rawr. The one exception is that `#[rawr::preview]` keeps cargo from
//! warning about functions that only previews use.
//!
//! ```toml
//! [dependencies]
//! rawr-macros = "0.1"
//! ```
//!
//! ```ignore
//! // rawr
//! #[rawr::preview(name = "empty", size = (320, 200))]
//! fn view(state: &State) -> Element<'_, Message> {
//!     text("Nothing yet").into()
//! }
//! ```
//!
//! `use rawr::preview;` lets a file write `#[preview]` instead.

use proc_macro::{Delimiter, Group, Punct, Spacing, TokenStream, TokenTree};

/// A preview of a view, which `rawr run` shows beside the app.
///
/// Under cargo the function keeps its signature and its body. It may be
/// called only by previews, so it is allowed to be dead code. The functions
/// named by `seed`, `update`, `theme` and `themes` are used by its body, so
/// cargo does not warn that they are never used.
#[proc_macro_attribute]
pub fn preview(arguments: TokenStream, item: TokenStream) -> TokenStream {
    let used = named_values(arguments);
    let mut tokens: Vec<TokenTree> = item.into_iter().collect();

    if !used.is_empty() {
        let body = tokens.iter().rposition(
            |token| matches!(token, TokenTree::Group(group) if group.delimiter() == Delimiter::Brace),
        );

        if let Some(index) = body {
            if let TokenTree::Group(group) = &tokens[index] {
                let mut stream = TokenStream::new();
                for value in used {
                    stream.extend(statement(value));
                }
                stream.extend(group.stream());

                let mut replaced = Group::new(Delimiter::Brace, stream);
                replaced.set_span(group.span());
                tokens[index] = TokenTree::Group(replaced);
            }
        }
    }

    let mut output: TokenStream = "#[allow(dead_code)]".parse().expect("an attribute");
    output.extend(tokens);
    output
}

/// A widget whose lifecycle an inherent impl defines, which `rawr run`
/// drives. Under cargo the impl is left as written.
#[proc_macro_attribute]
pub fn widget(_arguments: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// The values of the `seed`, `update`, `theme` and `themes` arguments, as
/// written. Each element of a `themes` list is its own value.
fn named_values(arguments: TokenStream) -> Vec<TokenStream> {
    let tokens: Vec<TokenTree> = arguments.into_iter().collect();
    let mut values = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        let named = matches!(&tokens[index], TokenTree::Ident(ident)
            if ["seed", "update", "theme", "themes"].contains(&ident.to_string().as_str()));
        let assigned = matches!(tokens.get(index + 1), Some(TokenTree::Punct(punct)) if punct.as_char() == '=');

        if named && assigned {
            let mut value = TokenStream::new();
            index += 2;
            while index < tokens.len() && !is_comma(&tokens[index]) {
                value.extend([tokens[index].clone()]);
                index += 1;
            }
            values.extend(elements(value));
        }
        index += 1;
    }

    values
}

/// The elements of a `[a, b]` list one by one, since functions of
/// different types cannot share an array. Any other value is one element.
fn elements(value: TokenStream) -> Vec<TokenStream> {
    let tokens: Vec<TokenTree> = value.clone().into_iter().collect();
    let [TokenTree::Group(group)] = tokens.as_slice() else {
        return vec![value];
    };
    if group.delimiter() != Delimiter::Bracket {
        return vec![value];
    }

    let mut elements = Vec::new();
    let mut element = TokenStream::new();
    for token in group.stream() {
        if is_comma(&token) {
            elements.push(std::mem::take(&mut element));
        } else {
            element.extend([token]);
        }
    }
    elements.push(element);
    elements.retain(|element| !element.is_empty());
    elements
}

fn is_comma(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == ',')
}

/// `let _ = value;`
fn statement(value: TokenStream) -> TokenStream {
    let mut stream: TokenStream = "let _ =".parse().expect("a let");
    stream.extend(value);
    stream.extend([TokenTree::Punct(Punct::new(';', Spacing::Alone))]);
    stream
}
