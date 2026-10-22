//! Validated `rawr.toml` package metadata.

use std::ops::Range;
use std::path::{Path, PathBuf};

use toml_edit::{Document, Item};

use crate::{ImmutableString, Position, Span};

pub(crate) mod cargo;
pub(crate) mod rust;
pub(crate) mod workspace;

#[derive(Clone, Debug)]
pub(crate) struct Manifest {
    pub(crate) name: Option<Name>,
    pub(crate) dependencies: Box<[Dependency]>,
    #[allow(
        dead_code,
        reason = "Rust import requests are parsed before the resolution slice consumes them"
    )]
    pub(crate) rust: Option<rust::Config>,
}

#[derive(Clone, Debug)]
pub(crate) struct Name {
    pub(crate) value: ImmutableString,
    pub(crate) span: Span,
}

#[derive(Clone, Debug)]
pub(crate) struct Dependency {
    pub(crate) alias: ImmutableString,
    pub(crate) path: PathBuf,
    pub(crate) span: Span,
}

#[derive(Debug)]
pub(crate) struct Error {
    pub(crate) message: ImmutableString,
    pub(crate) span: Span,
}

impl Manifest {
    pub(crate) fn parse(text: &str, root: &Path) -> Result<Self, Error> {
        let document = Document::parse(text).map_err(|error| Error {
            message: error.to_string().into(),
            span: error
                .span()
                .map(|range| byte_span(text, range))
                .unwrap_or_else(|| Span::new(Position::START, Position::START)),
        })?;
        let table = document.as_table();

        for (key, item) in table.iter() {
            if key != "package" && key != "dependencies" && key != "rust" {
                return Err(Error {
                    message: format!("unknown manifest section or key `{key}`").into(),
                    span: item_span(text, item),
                });
            }
        }

        let name = match table.get("package") {
            None => None,
            Some(item) => {
                let package = item.as_table().ok_or_else(|| Error {
                    message: "`package` must be a table".into(),
                    span: item_span(text, item),
                })?;
                for (key, value) in package.iter() {
                    if key != "name" {
                        return Err(Error {
                            message: format!("unknown package key `{key}`").into(),
                            span: item_span(text, value),
                        });
                    }
                }
                let item = package.get("name").ok_or_else(|| Error {
                    message: "`[package]` requires `name`".into(),
                    span: item_span(text, item),
                })?;
                let value = item.as_str().ok_or_else(|| Error {
                    message: "`package.name` must be a string".into(),
                    span: item_span(text, item),
                })?;
                if !valid_package_name(value) {
                    return Err(Error {
                        message: format!("invalid package name `{value}`").into(),
                        span: item_span(text, item),
                    });
                }
                Some(Name {
                    value: value.into(),
                    span: item_span(text, item),
                })
            }
        };

        let mut dependencies = Vec::new();
        if let Some(item) = table.get("dependencies") {
            let table = item.as_table().ok_or_else(|| Error {
                message: "`dependencies` must be a table".into(),
                span: item_span(text, item),
            })?;
            for (alias, item) in table.iter() {
                if !valid_alias(alias) {
                    return Err(Error {
                        message: format!("invalid dependency alias `{alias}`").into(),
                        span: item_span(text, item),
                    });
                }
                let value = item.as_value().ok_or_else(|| Error {
                    message: format!("dependency `{alias}` must be an inline table").into(),
                    span: item_span(text, item),
                })?;
                let inline = value.as_inline_table().ok_or_else(|| Error {
                    message: format!("dependency `{alias}` must use `{{ path = \"...\" }}`").into(),
                    span: item_span(text, item),
                })?;
                for (key, _) in inline.iter() {
                    if key != "path" {
                        return Err(Error {
                            message: format!("unknown key `{key}` for dependency `{alias}`").into(),
                            span: value
                                .span()
                                .map(|range| byte_span(text, range))
                                .unwrap_or_else(|| item_span(text, item)),
                        });
                    }
                }
                let path = inline.get("path").ok_or_else(|| Error {
                    message: format!("dependency `{alias}` requires `path`").into(),
                    span: item_span(text, item),
                })?;
                let path = path.as_str().ok_or_else(|| Error {
                    message: format!("dependency `{alias}` path must be a string").into(),
                    span: value
                        .span()
                        .map(|range| byte_span(text, range))
                        .unwrap_or_else(|| item_span(text, item)),
                })?;
                dependencies.push(Dependency {
                    alias: alias.into(),
                    path: root.join(path),
                    span: item_span(text, item),
                });
            }
        }

        let rust = table
            .get("rust")
            .map(|item| rust::Config::parse(text, root, item))
            .transpose()?;

        Ok(Self {
            name,
            dependencies: dependencies.into_boxed_slice(),
            rust,
        })
    }
}

fn item_span(text: &str, item: &Item) -> Span {
    item.span()
        .map(|range| byte_span(text, range))
        .unwrap_or_else(|| Span::new(Position::START, Position::START))
}

pub(super) fn byte_span(text: &str, range: Range<usize>) -> Span {
    Span::new(
        byte_position(text, range.start),
        byte_position(text, range.end),
    )
}

fn byte_position(text: &str, byte: usize) -> Position {
    let mut line = 1_u16;
    let mut column = 0_u16;
    for character in text[..byte.min(text.len())].chars() {
        if character == '\n' {
            line = line.saturating_add(1);
            column = 0;
        } else {
            column = column.saturating_add(1);
        }
    }
    Position::new(line, column.saturating_add(1))
}

fn valid_package_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

fn valid_alias(alias: &str) -> bool {
    crate::tokenizer::is_valid_identifier(alias)
        && crate::tokenizer::Token::lookup_symbol_from_syntax(alias).is_none()
        && alias != "crate"
        && alias != "super"
}
