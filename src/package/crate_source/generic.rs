//! A crate module's file as the program module that holds its generic
//! items (`crate_source`).

/// The crate module's file `text` with only its generic items, its `use`
/// declarations and its `macro_rules!` definitions at their places
/// (`polyglot::generic_items`), everything else blank but its line breaks,
/// and a glob import of the crate module at `path` at its end. Rawr reads
/// the concrete items only as far as their bounds, so a concrete item it
/// can't parse is no error.
pub(crate) fn text<S: AsRef<str>>(text: &str, path: &[S]) -> String {
    let kept = super::super::polyglot::generic_items(text);
    let mut generic = String::with_capacity(text.len());
    for (at, character) in text.char_indices() {
        let inside = kept.iter().any(|range| range.contains(&at));
        if inside || matches!(character, '\n' | '\r') {
            generic.push(character);
        } else {
            generic.push_str(&" ".repeat(character.len_utf8()));
        }
    }
    let glob = glob(path);
    if !text.contains(&glob) {
        generic.push_str(&glob);
    }
    generic
}

/// The public glob import of the native crate module at `path` that a
/// program module of its generic items ends with. In that module the
/// crate's name is the native crate's (`resolve::package`).
fn glob<S: AsRef<str>>(path: &[S]) -> String {
    let path = path
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join("::");
    format!("\npub use {path}::*;\n")
}

#[cfg(test)]
mod tests {
    /// A module's generic function, generic type and its impls stay at
    /// their places with its imports, its concrete items go, one rawr
    /// can't parse included, and the glob of the crate module ends it.
    #[test]
    fn a_module_keeps_its_generic_items_and_imports_at_their_places() {
        let text = "use std::fmt::Debug;\n\
                    pub struct Tint { pub red: u8 }\n\
                    static SHADED: Counter = Counter { count: 0 };\n\
                    fn darken<'a>(tint: &'a Tint) -> &'a Tint { tint }\n\
                    pub fn shade(tint: impl Into<Tint>) -> Tint { darken(tint.into()) }\n\
                    #[derive(Clone)]\n\
                    pub struct Panel<M> { items: Vec<M> }\n\
                    impl<M: Debug> Panel<M> { pub fn len(&self) -> usize { self.items.len() } }\n\
                    impl From<[u8; 1]> for Tint { fn from([red]: [u8; 1]) -> Self { Tint { red } } }\n\
                    impl Panel<u8> { pub fn first(&self) -> u8 { self.items[0] } }\n";
        let generic = super::text(text, &["events", "tinted"]);
        let lines = generic.lines().map(str::trim_end).collect::<Vec<_>>();
        assert_eq!(
            lines,
            [
                "use std::fmt::Debug;",
                "",
                "",
                "",
                "pub fn shade(tint: impl Into<Tint>) -> Tint { darken(tint.into()) }",
                "#[derive(Clone)]",
                "pub struct Panel<M> { items: Vec<M> }",
                "impl<M: Debug> Panel<M> { pub fn len(&self) -> usize { self.items.len() } }",
                "",
                "impl Panel<u8> { pub fn first(&self) -> u8 { self.items[0] } }",
                "",
                "pub use events::tinted::*;",
            ]
        );
        assert_eq!(super::text(&generic, &["events", "tinted"]), generic);
    }
}
