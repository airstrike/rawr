//! Complete include-list extraction before any include path is resolved.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::super::super::super::path;
use super::*;
use crate::input::membership::host;

/// Borrowed from admitted documents and their source definitions. Construction
/// validates the complete list; it does not observe or load any included file.
pub(in super::super::super) struct Include<'o, 'd> {
    pub(in super::super::super) path: &'d str,
    pub(in super::super::super) definition: Definition<'o>,
    pub(in super::super::super) optional: bool,
    span: Option<Span>,
}

impl Include<'_, '_> {
    fn normalized<A: document::Admission>(
        &self,
        cwd: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<std::path::PathBuf> {
        session.check()?;
        let root = match self.definition {
            Definition::Path(file) | Definition::Cli(Some(file)) => {
                let Some(parent) = file.parent() else {
                    return session.fail(Error::State);
                };
                parent
            }
            Definition::BuiltIn | Definition::Environment(_) | Definition::Cli(None) => cwd,
        };
        let joined = path::join(root, Path::new(self.path), session)?;
        path::normalize(&joined, session)
    }

    pub(in super::super::super) fn resolve(
        &self,
        cwd: &Path,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Option<std::path::PathBuf>> {
        let path = self.normalized(cwd, session)?;
        if self.optional {
            let bytes = path.as_os_str().as_bytes();
            session.string(bytes.len())?;
            // std rejects an interior NUL before issuing stat. Its exists()
            // maps that InvalidInput to false; no filesystem query occurred.
            if bytes.contains(&0) || !session.exists(bytes)? {
                return Ok(None);
            }
        }
        Ok(Some(path))
    }
}

impl<'o, 'd> Tree<'o, 'd> {
    pub(in super::super::super) fn includes<A: document::Admission>(
        &mut self,
        remove: bool,
        session: &mut Session<'_, A>,
    ) -> Step<storage::Buffer<Include<'o, 'd>>> {
        session.check()?;
        let Some(Kind::Table(table)) = self.nodes.as_mut_slice()[0].kind.as_mut() else {
            return session.fail(Error::State);
        };
        // Removal precedes conversion, including unsuccessful conversion.
        let index = if remove {
            table.remove("include", session)?
        } else {
            table.get("include", session)?
        };
        let mut includes = storage::Buffer::new();
        let Some(index) = index else {
            return Ok(includes);
        };
        let node = &self.nodes.as_slice()[index];
        let Some(Kind::List(elements)) = node.kind.as_ref() else {
            return rejected(node.span, "a list of strings or a list of tables", session);
        };
        for element in elements.as_slice() {
            session.items(1)?;
            let node = &self.nodes.as_slice()[element.node];
            let (path, optional) = match node.kind.as_ref() {
                Some(Kind::String(text)) => (*text, false),
                Some(Kind::Table(table)) => {
                    let Some(path) = table.get("path", session)? else {
                        return rejected(node.span, "field `path`", session);
                    };
                    let path = &self.nodes.as_slice()[path];
                    let Some(Kind::String(text)) = path.kind.as_ref() else {
                        return rejected(path.span, "a string", session);
                    };
                    let optional = match table.get("optional", session)? {
                        None => false,
                        Some(index) => {
                            let optional = &self.nodes.as_slice()[index];
                            let Some(Kind::Boolean(value)) = optional.kind.as_ref() else {
                                return rejected(optional.span, "a boolean", session);
                            };
                            *value
                        }
                    };
                    (*text, optional)
                }
                Some(_) => return rejected(node.span, "a string or table", session),
                None => return session.fail(Error::State),
            };
            includes.push(
                Include {
                    path,
                    definition: node.definition,
                    optional,
                    span: node.span,
                },
                session,
            )?;
        }
        // Cargo converts every row first. A later row's type error wins over
        // an earlier row's invalid extension, glob or template syntax.
        for include in includes.as_slice() {
            session.items(1)?;
            session.string(include.path.len())?;
            if Path::new(include.path).extension() != Some(std::ffi::OsStr::new("toml")) {
                return rejected(include.span, "a path ending with `.toml`", session);
            }
            if include.path.contains(&['*', '?', '[', ']'][..]) {
                return rejected(include.span, "a path without glob patterns", session);
            }
            if include.path.contains(&['{', '}'][..]) {
                return rejected(include.span, "a path without template braces", session);
            }
        }
        Ok(includes)
    }
}

fn rejected<T, A: document::Admission>(
    span: Option<Span>,
    expected: &'static str,
    session: &mut Session<'_, A>,
) -> Step<T> {
    session.reject(Diagnostic {
        tag: Tag::ConfigInclude,
        span,
        expected: Some(expected),
    })
}

#[cfg(test)]
mod tests;
