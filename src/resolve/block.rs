//! Items declared in blocks (#1631, #1429). rustc names a block's item
//! throughout its block, before its declaration too, and in the items
//! nested there, and nowhere outside it. A name read in a block is the
//! innermost enclosing block's item of that name, else the module's.
//!
//! Each block's items are their own: two bodies' `const MAX` and `struct
//! Point` are two items, keyed apart by their place among the module's
//! block items of their name (`program::Path::block`), and still named
//! `Point` wherever rustc prints the name.

use crate::item::Parsed;
use crate::{ImmutableString, Position, Span};
use std::collections::BTreeMap;

/// Whether `at` is inside `block`, its braces included.
pub(crate) fn contains(block: Span, at: Position) -> bool {
    block.start() <= at && at <= block.end()
}

/// The blocks `items` are declared in, each once, outermost first: a block
/// comes after every block it is nested in.
pub(crate) fn scopes<'a>(items: impl IntoIterator<Item = &'a Parsed>) -> Vec<Span> {
    let mut blocks = items
        .into_iter()
        .filter_map(Parsed::scope)
        .collect::<Vec<_>>();
    blocks.sort_by(|a, b| a.start().cmp(&b.start()).then(b.end().cmp(&a.end())));
    blocks.dedup();
    blocks
}

/// Whether `outer` is `inner` or holds it.
pub(crate) fn encloses(outer: Span, inner: Span) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}

/// The value of the innermost block of `blocks` that encloses `block`,
/// `block` itself first: of the blocks enclosing one, the innermost
/// starts last.
pub(crate) fn innermost<T>(blocks: &BTreeMap<Span, T>, block: Span) -> Option<&T> {
    blocks
        .range(..=block)
        .rev()
        .find(|(outer, _)| encloses(**outer, block))
        .map(|(_, value)| value)
}

/// Each item's place among the block items of its name in `items`, from 1
/// in source order; `None` for a module's item and an unnamed one.
pub(crate) fn ordinals(items: &[Parsed]) -> Vec<Option<u32>> {
    let mut counts = BTreeMap::<&ImmutableString, u32>::new();
    items
        .iter()
        .map(|parsed| {
            parsed.scope()?;
            let count = counts.entry(name(parsed)?).or_default();
            *count += 1;
            Some(*count)
        })
        .collect()
}

/// The name an item declares, in either namespace.
fn name(parsed: &Parsed) -> Option<&ImmutableString> {
    match parsed {
        Parsed::Module(item) => Some(&item.name.text),
        Parsed::Struct(item) => Some(&item.name.text),
        Parsed::Enum(item) => Some(&item.name.text),
        Parsed::Trait(item) => Some(&item.name.text),
        Parsed::Function(item) => Some(&item.name.text),
        Parsed::TypeAlias(item) => Some(&item.name.text),
        Parsed::Use(_) | Parsed::Impl(_) => None,
    }
}
