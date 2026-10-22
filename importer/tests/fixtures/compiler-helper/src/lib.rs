use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn identity(_attribute: TokenStream, item: TokenStream) -> TokenStream {
    item
}
