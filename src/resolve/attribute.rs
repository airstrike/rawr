//! Whether an attribute names something in scope, as rustc resolves it: a
//! built-in attribute, a tool's (`rustfmt::skip`), Rawr's own, an attribute
//! macro a `use` brings, or a helper of a derive on the item. Any other is
//! rustc's "cannot find attribute".

use crate::resolve::{item, Error};
use crate::ImmutableString;

/// rustc's built-in attributes, and the helpers of its built-in derives.
const BUILT_IN: &[&str] = &[
    "allow",
    "automatically_derived",
    "bench",
    "cfg",
    "cfg_attr",
    "cold",
    "collapse_debuginfo",
    "crate_name",
    "crate_type",
    "debugger_visualizer",
    "default",
    "deny",
    "deprecated",
    "derive",
    "diagnostic",
    "doc",
    "expect",
    "export_name",
    "forbid",
    "global_allocator",
    "ignore",
    "inline",
    "instruction_set",
    "link",
    "link_name",
    "link_ordinal",
    "link_section",
    "macro_export",
    "macro_use",
    "must_use",
    "naked",
    "no_builtins",
    "no_implicit_prelude",
    "no_link",
    "no_main",
    "no_mangle",
    "no_std",
    "non_exhaustive",
    "panic_handler",
    "path",
    "proc_macro",
    "proc_macro_attribute",
    "proc_macro_derive",
    "recursion_limit",
    "repr",
    "should_panic",
    "target_feature",
    "test",
    "track_caller",
    "type_length_limit",
    "unsafe",
    "used",
    "warn",
    "windows_subsystem",
];

/// The attributes Rawr reads itself: `#[preview]`, which a marked file
/// imports from the `rawr` crate for rustc.
const RAWR: &[&str] = &["preview"];

/// rustc's error for the first of `attributes` that names nothing in
/// scope, where `uses` are the file's imports and `helpers` the helper
/// attributes of the crate derives on the item. An attribute whose path has
/// more than one segment is a tool's or a crate's, which rustc resolves
/// through that crate, and is not checked here.
pub(crate) fn unknown<'a>(
    attributes: impl IntoIterator<Item = &'a crate::item::Attribute>,
    uses: &[&item::Use],
    helpers: &[ImmutableString],
) -> Option<Error> {
    attributes.into_iter().find_map(|attribute| {
        let [segment] = attribute.path.segments.as_ref() else {
            return None;
        };
        let name = segment.name.text.as_str();
        // A `#[cfg(..)]` that does not hold is read as `cfg(false)`.
        let known = BUILT_IN.contains(&name)
            || RAWR.contains(&name)
            || name.starts_with("cfg")
            || name.starts_with("rustc_")
            || helpers.iter().any(|helper| helper.as_str() == name)
            || uses.iter().any(|used| {
                used.bindings
                    .iter()
                    .any(|binding| binding.name.text.as_str() == name)
            });
        (!known).then(|| Error::Syntax {
            message: ImmutableString::from(format!("cannot find attribute `{name}` in this scope")),
            label: ImmutableString::new(),
            help: None,
            at: segment.name.span,
        })
    })
}
