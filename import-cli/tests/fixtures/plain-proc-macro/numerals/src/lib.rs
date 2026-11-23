//! `double!(expr)`: the expression times two. A function-like proc macro
//! a program names directly, as `rust_decimal_macros`'s `dec!` is.

use proc_macro::TokenStream;

#[proc_macro]
pub fn double(input: TokenStream) -> TokenStream {
    format!("(({input}) * 2)")
        .parse()
        .expect("the product parses")
}

/// `verbatim!(tokens)`: the tokens as they are, so a call expands to a
/// type in a type's place and a pattern in a pattern's.
#[proc_macro]
pub fn verbatim(input: TokenStream) -> TokenStream {
    input
}

/// `located!(tokens)`: `(line!(), column!(), tokens)`, the first two at
/// the call and the tokens where the input writes them, as a macro that
/// reports where it is called does.
#[proc_macro]
pub fn located(input: TokenStream) -> TokenStream {
    let mut tuple: TokenStream = "line!(), column!(),".parse().expect("the calls parse");
    tuple.extend(input);
    let group = proc_macro::Group::new(proc_macro::Delimiter::Parenthesis, tuple);
    TokenStream::from(proc_macro::TokenTree::Group(group))
}
