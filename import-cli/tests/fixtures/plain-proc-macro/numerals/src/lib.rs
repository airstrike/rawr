//! `double!(expr)`: the expression times two. A function-like proc macro
//! a program names directly, as `rust_decimal_macros`'s `dec!` is.

use proc_macro::TokenStream;

#[proc_macro]
pub fn double(input: TokenStream) -> TokenStream {
    format!("(({input}) * 2)")
        .parse()
        .expect("the product parses")
}
