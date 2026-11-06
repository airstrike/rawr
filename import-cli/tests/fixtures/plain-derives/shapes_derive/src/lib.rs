//! `#[derive(Shape)]`: implements `shapes::Shape` with the type's name.

use proc_macro::{TokenStream, TokenTree};

#[proc_macro_derive(Shape)]
pub fn derive_shape(input: TokenStream) -> TokenStream {
    let mut tokens = input.into_iter();
    let name = loop {
        match tokens.next() {
            Some(TokenTree::Ident(ident))
                if matches!(ident.to_string().as_str(), "struct" | "enum") =>
            {
                break tokens.next().expect("a type's name").to_string();
            }
            Some(_) => {}
            None => panic!("`Shape` derives on a struct or an enum"),
        }
    };
    format!(
        "impl ::shapes::Shape for {name} {{ fn name(&self) -> &'static str {{ {name:?} }} }}"
    )
    .parse()
    .expect("the impl parses")
}
