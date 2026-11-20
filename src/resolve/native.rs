//! Bounded provenance for a terminal miss against retained native metadata.

use super::Error;
use crate::{host, package, program, ImmutableString, Span};
use std::collections::BTreeMap;
use std::mem::{discriminant, size_of, Discriminant};
use std::sync::{Arc, Mutex};

/// One compilation's exact declared Rust roots and at most one failed lookup.
/// This is diagnostic provenance, never executable or reuse authority.
pub(crate) struct Capture {
    aliases: Box<[ImmutableString]>,
    terminal: Mutex<Terminal>,
}

/// The misses recorded so far: how many, and the last.
#[derive(Default)]
struct Terminal {
    recorded: usize,
    last: Option<Miss>,
}

/// A failed lookup's place and kind.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Miss {
    source: program::source::Id,
    span: Span,
    kind: Discriminant<Error>,
}

#[derive(Clone, Copy)]
pub(crate) struct Site<'a> {
    capture: &'a Capture,
    source: program::source::Id,
    shadowed: Option<&'a BTreeMap<ImmutableString, (package::source::Id, Span)>>,
}

impl Capture {
    pub(crate) fn from_graph(
        graph: &package::Graph,
        allocation: &mut impl host::ImportAllocation,
    ) -> Result<Arc<Self>, rawr_import::surface::Error> {
        let dependencies = graph
            .entry_package()
            .rust
            .as_ref()
            .map_or(&[][..], |config| config.dependencies.as_ref());
        let bytes = dependencies
            .len()
            .checked_mul(size_of::<ImmutableString>())
            .and_then(|bytes| bytes.checked_add(size_of::<Self>() + 2 * size_of::<usize>()))
            .and_then(|bytes| {
                dependencies.iter().try_fold(bytes, |bytes, dependency| {
                    bytes.checked_add(dependency.alias.len())
                })
            });
        allocation.retain_checked(dependencies.len().checked_add(1), bytes)?;
        let mut aliases = Vec::new();
        aliases
            .try_reserve_exact(dependencies.len())
            .map_err(|_| rawr_import::surface::Error::Allocation)?;
        aliases.extend(
            dependencies
                .iter()
                .map(|dependency| dependency.alias.clone()),
        );
        Ok(Arc::new(Self {
            aliases: aliases.into_boxed_slice(),
            terminal: Mutex::new(Terminal::default()),
        }))
    }

    pub(crate) fn at(&self, source: program::source::Id) -> Site<'_> {
        Site {
            capture: self,
            source,
            shadowed: None,
        }
    }

    /// Only the error that actually escapes this same pass can classify a miss.
    /// Successful/speculative lookups and different diagnostics cannot inherit it.
    pub(crate) fn matches(&self, source: program::source::Id, error: &Error) -> bool {
        self.terminal.lock().ok().is_some_and(|terminal| {
            terminal
                .last
                .as_ref()
                .is_some_and(|miss| miss.is(source, error))
        })
    }

    /// The miss recorded for `error` is the miss of `restated`, the error
    /// the pass reports for it instead: rustc names a path's unknown type
    /// at the path's last segment.
    pub(crate) fn restate(&self, source: program::source::Id, error: &Error, restated: &Error) {
        if let Ok(mut terminal) = self.terminal.lock()
            && let Some(miss) = terminal.last.as_mut()
            && miss.is(source, error)
        {
            miss.span = restated.primary_span();
            miss.kind = discriminant(restated);
        }
    }

    /// The crates the entry package's manifest names.
    pub(crate) fn roots(&self) -> &[ImmutableString] {
        &self.aliases
    }

    /// How many misses were recorded so far.
    pub(crate) fn recorded(&self) -> usize {
        self.terminal.lock().map_or(0, |terminal| terminal.recorded)
    }

    /// The last miss, when one was recorded after the first `recorded`.
    pub(crate) fn since(&self, recorded: usize) -> Option<Miss> {
        self.terminal
            .lock()
            .ok()
            .filter(|terminal| terminal.recorded > recorded)
            .and_then(|terminal| terminal.last.clone())
    }

    /// Record `miss` again, as the resolution that recorded it does when a
    /// later one reuses what it resolved (#1876).
    pub(crate) fn replay(&self, miss: &Miss) {
        if let Ok(mut terminal) = self.terminal.lock() {
            terminal.recorded += 1;
            terminal.last = Some(miss.clone());
        }
    }
}

impl Miss {
    /// Whether this is the miss of `error` in `source`.
    fn is(&self, source: program::source::Id, error: &Error) -> bool {
        self.source == source
            && self.span == error.primary_span()
            && self.kind == discriminant(error)
    }
}

impl<'a> Site<'a> {
    pub(crate) fn with_names(
        mut self,
        names: &'a BTreeMap<ImmutableString, (package::source::Id, Span)>,
    ) -> Self {
        self.shadowed = Some(names);
        self
    }
    pub(crate) fn declared_root(&self, root: &str) -> bool {
        !self.shadowed.is_some_and(|names| names.contains_key(root))
            && self.capture.aliases.iter().any(|alias| alias == root)
    }

    /// The input is a resolver-produced canonical binding, not source text or
    /// an error message. Its first component is an exact manifest alias.
    pub(crate) fn declared_binding(&self, qualified: &str) -> bool {
        qualified
            .split("::")
            .next()
            .is_some_and(|root| self.declared_root(root))
    }

    /// Validate the namespace route of a canonical parsed/resolved path. A
    /// present non-namespace prefix is a wrong-kind source error, not absence.
    pub(crate) fn missing_binding(&self, host: &host::Registry, qualified: &str) -> bool {
        self.declared_binding(qualified)
            && host.entry(qualified).is_none()
            && qualified.match_indices("::").all(|(end, _)| {
                matches!(
                    host.entry(&qualified[..end]),
                    None | Some(
                        host::Entry::Module | host::Entry::ModuleItem(_) | host::Entry::Type(_)
                    )
                )
            })
    }

    /// Call only at an actual terminal absence after establishing native origin.
    pub(crate) fn record(&self, error: &Error) {
        if let Ok(mut terminal) = self.capture.terminal.lock() {
            terminal.recorded += 1;
            terminal.last = Some(Miss {
                source: self.source,
                span: error.primary_span(),
                kind: discriminant(error),
            });
        }
    }
}
