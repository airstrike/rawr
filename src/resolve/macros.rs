//! The program's own `macro_rules!` macros and where Rust's textual scope
//! makes each one visible: after its definition in its module and in the
//! modules declared after it there, and, through `#[macro_use]` on a
//! module, after that module in its parent. A `#[macro_export]` macro is
//! also in the crate root's path scope, so the root's code calls it from
//! anywhere.

use std::collections::BTreeMap;

use crate::program::source;
use crate::{item, ImmutableString, Position, Span};

/// rustc's default recursion limit, which bounds how deeply macro calls
/// nest.
const RECURSION_LIMIT: usize = 128;

/// The program's macros.
#[derive(Clone, Debug)]
pub(crate) struct Table {
    definitions: Box<[Definition]>,
    /// Each module source's parent, with the span of the `mod` item that
    /// declares it there.
    parents: BTreeMap<source::Id, (source::Id, Span)>,
    limit: usize,
}

/// One `macro_rules!` definition.
#[derive(Clone, Debug)]
pub(crate) struct Definition {
    source: source::Id,
    name: ImmutableString,
    /// Where its name is written.
    name_span: Span,
    span: Span,
    exported: bool,
    /// Where it is in scope: each source from its position on, with the
    /// modules that source declares after the position. The first is its
    /// own module; each next is the parent a `#[macro_use]` module lets it
    /// into.
    frames: Box<[(source::Id, Position)]>,
}

/// What a call of a name finds.
pub(crate) enum Lookup<'a> {
    /// The definition in scope, the latest when several are.
    Found(&'a Definition),
    /// None in scope: one of the name is defined later in the same module,
    /// its name at that span.
    Later(Span),
    /// None in scope, though the program defines one of the name elsewhere.
    Elsewhere,
    /// The program defines none of the name.
    Missing,
}

/// One source's part in building the table.
pub(crate) struct Module<'a> {
    pub(crate) source: source::Id,
    /// The crate it is a module of.
    pub(crate) package: &'a str,
    /// Its path below the crate root.
    pub(crate) path: &'a [ImmutableString],
    pub(crate) items: &'a [item::Parsed],
    pub(crate) macros: &'a [item::MacroRules],
    pub(crate) inner_attributes: &'a [item::Attribute],
}

impl Table {
    /// The table of the crates whose modules are `modules`, one source
    /// each, the entry crate's root first.
    pub(crate) fn new<'a>(modules: impl IntoIterator<Item = Module<'a>>) -> Self {
        let modules = modules.into_iter().collect::<Vec<_>>();
        let by_path = modules
            .iter()
            .map(|module| ((module.package, module.path), module))
            .collect::<BTreeMap<_, _>>();
        // Each module's parent, with its `mod` item there.
        let mut declarations = BTreeMap::new();
        for module in &modules {
            let Some((name, parent)) = module.path.split_last() else {
                continue;
            };
            let Some(parent) = by_path.get(&(module.package, parent)) else {
                continue;
            };
            let declared = parent.items.iter().find_map(|item| match item {
                item::Parsed::Module(declared) if declared.name.text == *name => Some(declared),
                _ => None,
            });
            if let Some(declared) = declared {
                declarations.insert(module.source, (parent.source, declared));
            }
        }
        let definitions = modules
            .iter()
            .flat_map(|module| {
                module.macros.iter().map(|definition| {
                    let mut frames = vec![(module.source, definition.span.end())];
                    let mut current = module.source;
                    while let Some((parent, declared)) = declarations.get(&current)
                        && declared
                            .header
                            .attributes
                            .iter()
                            .any(|attribute| is(attribute, "macro_use"))
                    {
                        frames.push((*parent, declared.span.end()));
                        current = *parent;
                    }
                    Definition {
                        source: module.source,
                        name: definition.name.text.clone(),
                        name_span: definition.name.span,
                        span: definition.span,
                        exported: definition
                            .header
                            .attributes
                            .iter()
                            .any(|attribute| is(attribute, "macro_export")),
                        frames: frames.into_boxed_slice(),
                    }
                })
            })
            .collect();
        let limit = modules
            .first()
            .filter(|module| module.path.is_empty())
            .and_then(|module| {
                module
                    .inner_attributes
                    .iter()
                    .find_map(|attribute| recursion_limit(&attribute.text))
            })
            .unwrap_or(RECURSION_LIMIT);
        Self {
            definitions,
            parents: declarations
                .into_iter()
                .map(|(child, (parent, declared))| (child, (parent, declared.span)))
                .collect(),
            limit,
        }
    }

    /// Every definition.
    pub(crate) fn definitions(&self) -> impl Iterator<Item = &Definition> {
        self.definitions.iter()
    }

    /// How deeply macro calls may nest: rustc's limit, or the crate's
    /// `#![recursion_limit = ".."]`.
    pub(crate) const fn limit(&self) -> usize {
        self.limit
    }

    /// The macro `name` names at `at` in `source`.
    pub(crate) fn lookup(&self, name: &str, source: source::Id, at: Position) -> Lookup<'_> {
        let mut best: Option<(&Definition, (usize, Position))> = None;
        let mut later = None;
        let mut elsewhere = false;
        for definition in self
            .definitions
            .iter()
            .filter(|definition| definition.name == name)
        {
            let Some(rank) = self.visible(definition, source, at) else {
                if definition.source == source && definition.span.start() > at {
                    later = later.or(Some(definition.name_span));
                }
                elsewhere = true;
                continue;
            };
            // The nearer module wins, then the later definition there.
            if best
                .is_none_or(|(_, (hops, from))| rank.0 < hops || (rank.0 == hops && rank.1 >= from))
            {
                best = Some((definition, rank));
            }
        }
        match (best, later) {
            (Some((definition, _)), _) => Lookup::Found(definition),
            (None, Some(span)) => Lookup::Later(span),
            (None, None) if elsewhere => Lookup::Elsewhere,
            (None, None) => Lookup::Missing,
        }
    }

    /// Whether `definition` is in scope at `at` in `source`: how many
    /// modules up from `source` the frame it is in scope through is, and
    /// where in that frame's source it is.
    fn visible(
        &self,
        definition: &Definition,
        source: source::Id,
        at: Position,
    ) -> Option<(usize, Position)> {
        for &(frame, from) in &definition.frames {
            let mut current = source;
            let mut position = at;
            let mut hops = 0;
            loop {
                if current == frame {
                    if position >= from {
                        return Some((hops, from));
                    }
                    break;
                }
                let Some((parent, declared)) = self.parents.get(&current) else {
                    break;
                };
                current = *parent;
                position = declared.start();
                hops += 1;
            }
        }
        // A `#[macro_export]` macro is an item of its crate's root.
        let root = self.root_of(definition.source);
        (definition.exported && source == root).then_some((usize::MAX, Position::NONE))
    }

    /// Write what a lookup reads of the table (`lookup`): each definition,
    /// the limit, and each module's parent with, in place of where the
    /// parent declares it, which of the frames in the parent it comes
    /// after. A declaration a save adds before a `mod` item moves the item,
    /// and changes no lookup, nor what this writes (#1726).
    #[cfg(feature = "rust-import")]
    pub(crate) fn write_interface(&self, out: &mut impl std::fmt::Write) -> std::fmt::Result {
        write!(out, "{:?}|{}|", self.definitions, self.limit)?;
        for (child, (parent, declared)) in &self.parents {
            write!(out, "{child:?}<{parent:?}:")?;
            for (frame, from) in self
                .definitions
                .iter()
                .flat_map(|definition| &definition.frames)
            {
                if frame == parent {
                    out.write_char(if declared.start() >= *from { '1' } else { '0' })?;
                }
            }
            out.write_char('|')?;
        }
        Ok(())
    }

    /// The root source of the crate `source` is a module of.
    fn root_of(&self, mut source: source::Id) -> source::Id {
        while let Some((parent, _)) = self.parents.get(&source) {
            source = *parent;
        }
        source
    }
}

impl Definition {
    /// The definition, from `macro_rules` to its end.
    pub(crate) const fn span(&self) -> Span {
        self.span
    }

    /// The source that defines it.
    pub(crate) const fn source(&self) -> source::Id {
        self.source
    }
}

/// Whether `attribute` is `#[name]`.
fn is(attribute: &item::Attribute, name: &str) -> bool {
    matches!(&*attribute.path.segments, [segment] if segment.name.text == name)
}

/// The limit `recursion_limit = "256"` sets.
fn recursion_limit(text: &str) -> Option<usize> {
    let text = text.trim_start_matches(['!', '[']).trim_start();
    let value = text.strip_prefix("recursion_limit")?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    value.strip_prefix('"')?.strip_suffix('"')?.parse().ok()
}
