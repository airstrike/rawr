// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Pinned cargo-platform parser and diagnostics over an admitted flat tree.

use super::super::{
    super::{session::Session, storage, Step},
    normalize::target,
    Warnings,
};
use rawr_toml::document;
use std::{fmt, path::Path};
mod parser;

pub(in super::super) struct Platform<'a> {
    nodes: storage::Buffer<Node<'a>>,
    name: Option<String>,
}
#[derive(Clone, Copy)]
struct Node<'a> {
    kind: Kind<'a>,
    child: Option<usize>,
    next: Option<usize>,
    parent: Option<usize>,
}
#[derive(Clone, Copy)]
enum Kind<'a> {
    All,
    Any,
    Not,
    True,
    False,
    Value(Atom<'a>),
}
#[derive(Clone, Copy)]
struct Atom<'a> {
    name: &'a str,
    raw: bool,
    value: Option<&'a str>,
}

impl<'a> Platform<'a> {
    /// Copy the admitted flat representation without parsing or repeating
    /// diagnostics. Atom borrows retain the original manifest-text lifetime.
    pub(in super::super) fn copy<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let name = self
            .name
            .as_ref()
            .map(|name| target::formatted(format_args!("{name}"), session))
            .transpose()?;
        let mut nodes = storage::Buffer::new();
        for node in self.nodes.as_slice() {
            if let Kind::Value(atom) = node.kind {
                session.string(atom.name.len())?;
                if let Some(value) = atom.value {
                    session.string(value.len())?;
                }
            }
            nodes.push(*node, session)?;
        }
        Ok(Self { nodes, name })
    }
}
impl fmt::Display for Atom<'_> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.raw {
            out.write_str("r#")?;
        }
        out.write_str(self.name)?;
        if let Some(value) = self.value {
            write!(out, " = \"{value}\"")?;
        }
        Ok(())
    }
}
impl fmt::Display for Platform<'_> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(name) = &self.name {
            return out.write_str(name);
        }
        out.write_str("cfg(")?;
        let mut current = 0;
        loop {
            let node = self.nodes.as_slice().get(current).ok_or(fmt::Error)?;
            match node.kind {
                Kind::All => out.write_str("all(")?,
                Kind::Any => out.write_str("any(")?,
                Kind::Not => out.write_str("not(")?,
                Kind::True => out.write_str("true")?,
                Kind::False => out.write_str("false")?,
                Kind::Value(atom) => write!(out, "{atom}")?,
            }
            if let Some(child) = node.child {
                current = child;
                continue;
            }
            if matches!(node.kind, Kind::All | Kind::Any | Kind::Not) {
                out.write_str(")")?;
            }
            loop {
                let node = self.nodes.as_slice().get(current).ok_or(fmt::Error)?;
                if let Some(next) = node.next {
                    out.write_str(", ")?;
                    current = next;
                    break;
                }
                if let Some(parent) = node.parent {
                    out.write_str(")")?;
                    current = parent;
                } else {
                    return out.write_str(")");
                }
            }
        }
    }
}

pub(in super::super) fn parse<'a, A: document::Admission>(
    text: &'a str,
    session: &mut Session<'_, A>,
) -> Step<Result<Platform<'a>, String>> {
    session.check()?;
    session.string(text.len())?;
    if let Some(inner) = text
        .strip_prefix("cfg(")
        .and_then(|text| text.strip_suffix(')'))
    {
        let nodes = match parser::parse(inner, session)? {
            Ok(nodes) => nodes,
            Err(error) => return Ok(Err(error)),
        };
        return Ok(Ok(Platform { nodes, name: None }));
    }
    if let Some(ch) = text
        .chars()
        .find(|&ch| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.')))
    {
        session.string(text.len())?;
        let cause = if text.contains('(') {
            target::formatted(
                format_args!("unexpected `(` character, cfg expressions must start with `cfg(`"),
                session,
            )?
        } else {
            target::formatted(
                format_args!("unexpected character {ch} in target name"),
                session,
            )?
        };
        return target::formatted(
            format_args!(
                "failed to parse `{text}` as a cfg expression: invalid target specifier: {cause}"
            ),
            session,
        )
        .map(Err);
    }
    let rendered = target::formatted(format_args!("{text}"), session)?;
    Ok(Ok(Platform {
        nodes: storage::Buffer::new(),
        name: Some(rendered),
    }))
}

pub(in super::super) fn check_attributes<A: document::Admission>(
    platform: &Platform<'_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.check()?;
    for node in platform.nodes.as_slice() {
        session.items(1)?;
        if let Kind::Value(atom) = node.kind {
            if atom.value.is_none()
                && matches!(atom.name, "test" | "debug_assertions" | "proc_macro")
            {
                let name = Atom {
                    value: None,
                    ..atom
                };
                let warning=target::formatted(format_args!("Found `{name}` in `target.'cfg(...)'.dependencies`. This value is not supported for selecting dependencies and will not work as expected. To learn more visit https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#platform-specific-dependencies"),session)?;
                warnings.push_formatted(warning, session)?;
            } else if atom.value.is_some() && atom.name == "feature" {
                warnings.push(&["Found `feature = ...` in `target.'cfg(...)'.dependencies`. This key is not supported for selecting dependencies and will not work as expected. Use the [features] section instead: https://doc.rust-lang.org/cargo/reference/features.html"],session)?;
            }
        }
    }
    Ok(())
}

pub(in super::super) fn check_keywords<A: document::Admission>(
    platform: &Platform<'_>,
    path: &Path,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.check()?;
    for node in platform.nodes.as_slice() {
        session.items(1)?;
        if let Kind::Value(atom) = node.kind
            && !atom.raw
            && matches!(atom.name, "true" | "false")
        {
            let name = atom.name;
            let warning=target::formatted(format_args!("[{}] future-incompatibility: `cfg({atom})` is deprecated as `{name}` is a keyword and not an identifier and should not have have been accepted in this position.\n | this was previously accepted by Cargo but is being phased out; it will become a hard error in a future release!\n |\n | help: use raw-idents instead: `cfg(r#{name})`",path.display()),session)?;
            warnings.push_formatted(warning, session)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
