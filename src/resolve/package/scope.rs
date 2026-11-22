//! What the blocks and `use`s around one item bind, which the module's
//! names don't hold: a block's items and imports, and the std trait each
//! `use` the item sees names (#758).

use super::{sees, Address, BlockItems, Target};
use crate::resolve::item;
use crate::{std_table, ImmutableString, Span};

/// The blocks and `use`s of a unit, as the item declared in the block
/// `scope` (or the module's, `None`) sees them.
pub(super) struct Around<'a> {
    pub(super) blocks: &'a BlockItems,
    pub(super) uses: &'a [Option<item::Use>],
    pub(super) scope: Option<Span>,
}

impl Around<'_> {
    /// Whether a block around the item declares or imports `name`.
    pub(super) fn binds(&self, name: &ImmutableString) -> bool {
        let Some(here) = self.scope else {
            return false;
        };
        self.blocks.iter().any(|(block, declared)| {
            crate::resolve::block::encloses(*block, here) && declared.contains_key(name)
        }) || self.uses.iter().flatten().any(|used| {
            used.header.scope.is_some()
                && sees(used.header.scope, self.scope)
                && (used.traits.contains(name)
                    || used
                        .bindings
                        .iter()
                        .any(|binding| binding.name.text == *name))
        })
    }

    /// The trait the innermost block around the item that declares `name`
    /// declares, by its address: a block's trait is no module's item, so
    /// only its block names it. `None` when that block's `name` is another
    /// kind of item, or no block around the item declares one.
    pub(super) fn block_trait(&self, name: &ImmutableString) -> Option<&Address> {
        let here = self.scope?;
        let (_, target) = self
            .blocks
            .range(..=here)
            .rev()
            .filter(|(block, _)| crate::resolve::block::encloses(**block, here))
            .find_map(|(_, declared)| declared.get(name))?;
        match target {
            Target::Trait(address) => Some(address),
            _ => None,
        }
    }

    /// The stable std trait the innermost `use` the item sees binds `name`
    /// to, by its definition: `core::io::write::Write` of `Write` after
    /// `use std::io::Write;`, or after `use std::io::*;`, whose names the
    /// glob binds one by one.
    pub(super) fn std_trait(&self, name: &ImmutableString) -> Option<&'static str> {
        let mut visible = self
            .uses
            .iter()
            .flatten()
            .filter(|used| sees(used.header.scope, self.scope))
            .collect::<Vec<_>>();
        // A block's `use` shadows the module's, an inner block's an outer's.
        visible.sort_by_key(|used| std::cmp::Reverse(used.header.scope.map(|block| block.start())));
        let binding = visible.into_iter().find_map(|used| {
            used.bindings
                .iter()
                .find(|binding| binding.name.text == *name)
        })?;
        std_table::stable_trait(&binding.qualified)
    }
}
