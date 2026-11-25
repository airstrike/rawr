//! A crate module's file as the program module that holds its generic
//! items (`crate_source`).

/// The crate module's file `text` with only its generic items, its `use`
/// declarations and its `macro_rules!` definitions at their places
/// (`polyglot::generic_items`), everything else blank but its line breaks,
/// and a glob import of the crate module at `path` after them, then a
/// function that binds each concrete item (`bound`). The generic inherent
/// methods of the crate's concrete types are their trait's, which is
/// declared after the module's text (`crate_source::inherent`). Rawr
/// reads the concrete items only as far as their bounds, so a concrete
/// item it can't parse is no error. The binding functions bind the
/// derives of the types the module names from elsewhere too, `imported`.
/// A runner's program, whose runner is built, takes no binding functions
/// ([`Bindings::Left`]). A generic item stays only when `reached` has its
/// name (`polyglot::reached_names`).
pub(crate) fn text<S: AsRef<str>>(
    text: &str,
    path: &[S],
    bindings: Bindings,
    imported: &[super::bound::Derived],
    reached: &dyn Fn(&str) -> bool,
) -> String {
    let items = super::super::polyglot::generic_items(text, reached);
    let mut generic = String::with_capacity(text.len());
    for (at, character) in text.char_indices() {
        let inside = items.kept.iter().any(|range| range.contains(&at));
        if inside || matches!(character, '\n' | '\r') {
            generic.push(character);
        } else {
            generic.push_str(&" ".repeat(character.len_utf8()));
        }
    }
    let mut replaced = items.replaced;
    replaced.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, written) in replaced {
        generic.replace_range(range, &written);
    }
    generic.push_str(&items.declared);
    generic.push_str(&glob(path));
    if bindings == Bindings::Written {
        generic.push_str(&super::bound::functions(text, imported));
    }
    generic
}

/// Whether a program module of a crate module's generic items has the
/// functions that bind its concrete items.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Bindings {
    /// The check that builds the runner reads them, so the runner binds
    /// every concrete item.
    Written,
    /// The runner's own compile of the program, whose runner is built.
    Left,
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
    /// A module's generic function, generic type with each of its impls,
    /// and generic type alias stay at their places with its imports; its
    /// concrete types, their impls, an alias with only lifetime parameters
    /// and its other concrete items go, one rawr can't parse included, the
    /// glob of the crate module comes after them, and functions that bind
    /// the concrete items.
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
                    impl Panel<u8> { pub fn first(&self) -> u8 { self.items[0] } }\n\
                    pub type Shading<'a, T> = Box<dyn Fn(&T) -> Tint + 'a>;\n\
                    pub type Named<'a> = &'a str;\n";
        let generic = super::text(
            text,
            &["events", "tinted"],
            super::Bindings::Written,
            &[],
            &|_| true,
        );
        let (kept, bound) = generic
            .split_once("\n#[allow(dead_code)]")
            .unwrap_or((&generic, ""));
        let lines = kept.lines().map(str::trim_end).collect::<Vec<_>>();
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
                "pub type Shading<'a, T> = Box<dyn Fn(&T) -> Tint + 'a>;",
                "",
                "",
                "pub use events::tinted::*;",
            ]
        );
        assert!(bound.contains("darken(__a0)"), "{bound}");
    }

    /// A generic method of a concrete type's inherent impl is its trait's,
    /// declared after the text, its visibility blank, and the impl's
    /// concrete method goes, named by a binding function instead.
    #[test]
    fn a_concrete_types_generic_method_is_its_inherent_traits() {
        let text = "impl Style {\n\
                    pub fn tint(&self) -> Tint { self.tint }\n\
                    pub fn retinted(self, tint: impl Into<Tint>) -> Self { Self { tint: tint.into(), ..self } }\n\
                    }\n";
        let generic = super::text(
            text,
            &["events", "tinted"],
            super::Bindings::Written,
            &[],
            &|_| true,
        );
        let (kept, bound) = generic
            .split_once("\n#[allow(dead_code)]")
            .unwrap_or((&generic, ""));
        let lines = kept.lines().map(str::trim_end).collect::<Vec<_>>();
        assert_eq!(
            lines,
            [
                "impl __rawr_inherent__Style for Style {",
                "",
                "    fn retinted(self, tint: impl Into<Tint>) -> Self { Self { tint: tint.into(), ..self } }",
                "}",
                "",
                "pub trait __rawr_inherent__Style {",
                "    fn retinted(self, tint: impl Into<Tint>) -> Self;",
                "}",
                "",
                "pub use events::tinted::*;",
            ]
        );
        assert!(bound.contains("Style::tint(__a0)"), "{bound}");
    }

    /// Each inherent impl of a type that gives generic methods is a trait
    /// of its own, as two `impl Tally` blocks are, so the two never clash.
    #[test]
    fn two_inherent_impls_of_a_type_are_two_traits() {
        let text = "impl Tally {\n\
                    pub fn diff(&mut self, items: &mut [impl Shown]) {}\n\
                    }\n\
                    impl Tally {\n\
                    pub fn marked<M>(&mut self, items: &mut [impl Diffed<M>]) {}\n\
                    }\n";
        let generic = super::text(
            text,
            &["events", "tinted"],
            super::Bindings::Left,
            &[],
            &|_| true,
        );
        for declared in [
            "impl __rawr_inherent__Tally for Tally {",
            "impl __rawr_inherent__Tally__1 for Tally {",
            "pub trait __rawr_inherent__Tally {",
            "pub trait __rawr_inherent__Tally__1 {",
        ] {
            assert!(generic.contains(declared), "{declared}:\n{generic}");
        }
    }

    /// A crate trait's generic impl for a crate type runs from source, and
    /// one for a reference or a primitive stays native, as iced's `impl<..>
    /// Widget<..> for &str` beside `Text`'s.
    #[test]
    fn a_trait_impl_runs_from_source_only_for_a_named_type() {
        let text = "impl<M> Diffed<M> for Label {\n\
                    fn label(&mut self) -> String { String::new() }\n\
                    }\n\
                    impl<M> Diffed<M> for &str {\n\
                    fn label(&mut self) -> String { String::new() }\n\
                    }\n";
        let generic = super::text(
            text,
            &["events", "tinted"],
            super::Bindings::Left,
            &[],
            &|_| true,
        );
        assert!(
            generic.contains("impl<M> Diffed<M> for Label {"),
            "{generic}"
        );
        assert!(!generic.contains("for &str"), "{generic}");
    }
}
