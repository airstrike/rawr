//! Rawr's attributes, for the cargo build of a crate whose files are marked
//! `// rawr`.
//!
//! Rawr reads these attributes itself when it runs the crate. Under cargo
//! each one leaves its item exactly as written, so a release build carries
//! nothing of Rawr.
//!
//! ```toml
//! [dependencies]
//! rawr-attributes = "0.1"
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

use proc_macro::TokenStream;

/// A preview of a view, which `rawr run` shows beside the app. Under cargo
/// the function is left as written.
#[proc_macro_attribute]
pub fn preview(_arguments: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// A widget whose lifecycle an inherent impl defines, which `rawr run`
/// drives. Under cargo the impl is left as written.
#[proc_macro_attribute]
pub fn widget(_arguments: TokenStream, item: TokenStream) -> TokenStream {
    item
}
