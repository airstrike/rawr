//! Literals and constants: number literals, `const` uses, and the
//! compile-time macros (`concat!`, `env!`, `include_str!`, `file!`) rustc
//! expands to literals.

use super::*;

impl<'a> Parser<'a> {
    /// A use of `const NAME` evaluates its initializer, as Rust inlines it.
    pub(super) fn constant_use(&mut self, function: &ScriptFunction, span: Span) -> ParsedExpr {
        let ty = function
            .definition
            .output
            .as_ref()
            .map_or(Ty::Unit, Ty::from_resolved);
        // A `const` or `static` of `&str` is `&'static str`, as a literal
        // is: it borrows nothing of the body.
        if ty == Ty::Str {
            self.flow().set_origins(span, Vec::new());
        }
        let source = origin::Fragment::call(function.generated.as_str(), []);
        self.operator_expr(source, ty, span)
    }

    /// `Type::member(..)` for a builtin type: `String::new()`, the
    /// prelude's and the crate's `From` conversions (`String::from(x)`,
    /// `i64::from(true)`), and crate trait functions without `self`.
    /// `concat!`, `env!`, `include_str!`, `include_bytes!` and `stringify!`,
    /// which rustc expands while it compiles: their text (or bytes) as a
    /// literal. `env!("CARGO_MANIFEST_DIR")` is the project's directory and
    /// `include_str!`'s path is relative to the file that names it.
    pub(super) fn parse_compile_time_macro(
        &mut self,
        name: &str,
        name_span: Span,
    ) -> Result<ParsedExpr, Error> {
        let (text, end) = self.compile_time_text(name, name_span)?;
        let at = span(name_span.start(), end);
        if matches!(name, "line" | "column") {
            return Ok(self.literal(text, Ty::Int(Int::U32), at));
        }
        // `option_env!("NAME")`: `Some` of its value at compile time, or
        // `None`.
        if name == "option_env" {
            let value = self.environment_variable(&text);
            let ty = Ty::Option(Box::new(Ty::Str));
            let source = match value {
                Some(value) => self.prelude_op(PreludeOp::Some, [format!("{value:?}").into()]),
                None => self.prelude_op(PreludeOp::None, []),
            };
            return Ok(self.operator_expr(source, ty, at));
        }
        if name == "include_bytes" {
            // A file the host ships is read from its bundle's module, as the
            // constant the host built once: its bytes are the program's, as
            // its text is, and no array literal of them is parsed.
            if let Some((index, bytes)) = crate::resolve::bundled(&text) {
                let ty = Ty::Reference {
                    mutable: false,
                    target: Box::new(Ty::Array {
                        element: Box::new(Ty::Int(Int::U8)),
                        kind: sequence::Kind::Array(bytes.len()),
                    }),
                };
                return Ok(self.literal(crate::resolve::bundled_constant(index), ty, at));
            }
            let bytes = self.include(name, &text, at)?;
            return Ok(self.byte_literal(&bytes, at));
        }
        Ok(self.literal(format!("{text:?}"), Ty::Str, at))
    }

    /// `bytes` as a `&[u8; N]` literal: an array of integers at runtime.
    pub(super) fn byte_literal(&self, bytes: &[u8], at: Span) -> ParsedExpr {
        let source = format!(
            "[{}]",
            bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let ty = Ty::Reference {
            mutable: false,
            target: Box::new(Ty::Array {
                element: Box::new(Ty::Int(Int::U8)),
                kind: sequence::Kind::Array(bytes.len()),
            }),
        };
        self.literal(source, ty, at)
    }

    /// The text of the compile-time macro `name`, after its `!`, and where
    /// it ends.
    pub(super) fn compile_time_text(
        &mut self,
        name: &str,
        name_span: Span,
    ) -> Result<(String, Position), Error> {
        // `line!()`, `column!()`, `file!()` and `module_path!()`: where the
        // macro is written.
        if matches!(name, "line" | "column" | "file" | "module_path") {
            let end = match self.next() {
                (Token::Unit, position) => position,
                (Token::LeftParen, _) => self.expect(Token::RightParen, "`)` after `(`")?,
                (_, position) => {
                    return Err(Error::UnsupportedNominalSyntax {
                        construct: format!("`{name}!` without `()`").into(),
                        source: point_span(position),
                    })
                }
            };
            let at = name_span.start();
            let text = match name {
                "line" => at.line().unwrap_or_default().to_string(),
                "column" => at.position().unwrap_or_default().to_string(),
                "file" => match self.source_file() {
                    Some(file) => cargo_env::file_path(&file),
                    None => self.source_identity(),
                },
                _ => self.module_path(),
            };
            return Ok((text, end));
        }
        // `stringify!()` and `concat!()`: an empty string.
        if matches!(name, "stringify" | "concat") && matches!(self.peek(), Token::Unit) {
            let (_, position) = self.next();
            return Ok((String::new(), position));
        }
        let open = match self.next() {
            (Token::LeftParen | Token::LeftBracket | Token::LeftBrace, position) => position,
            (_, position) => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: format!("`{name}!` without `(`").into(),
                    source: point_span(position),
                })
            }
        };
        if name == "stringify" {
            // The tokens as written, between the delimiters.
            let mut depth = 0usize;
            let mut tokens = Vec::new();
            let end = loop {
                match self.next() {
                    (token @ (Token::LeftParen | Token::LeftBracket | Token::LeftBrace), at) => {
                        depth += 1;
                        tokens.push((token, at));
                    }
                    (Token::RightParen | Token::RightBracket | Token::RightBrace, position)
                        if depth == 0 =>
                    {
                        break position;
                    }
                    (token @ (Token::RightParen | Token::RightBracket | Token::RightBrace), at) => {
                        depth -= 1;
                        tokens.push((token, at));
                    }
                    (Token::EOF, position) => {
                        return Err(Error::UnsupportedNominalSyntax {
                            construct: "`stringify!` without its closing delimiter".into(),
                            source: span(open, position),
                        })
                    }
                    (token, at) => tokens.push((token, at)),
                }
            };
            let key = self.models.sources.key(self.models.context.index());
            let text = self
                .models
                .sources
                .resolve(key)
                .ok()
                .and_then(|source| stringify_tokens(source.text(), &tokens, end));
            let text = text.unwrap_or_else(|| {
                let pieces: Vec<String> =
                    tokens.iter().map(|(token, _)| token.to_string()).collect();
                pieces.join(" ")
            });
            return Ok((text, end));
        }
        let mut arguments = Vec::new();
        let end = loop {
            if let (Token::RightParen | Token::RightBracket | Token::RightBrace, position) =
                (self.peek(), self.position())
            {
                self.next();
                break position;
            }
            arguments.push(self.compile_time_argument()?);
            if matches!(self.peek(), Token::Comma) {
                self.next();
            } else if !matches!(
                self.peek(),
                Token::RightParen | Token::RightBracket | Token::RightBrace
            ) {
                return Err(self.unsupported("compile-time macro arguments without ','"));
            }
        };
        let at = span(name_span.start(), end);
        let one = |arguments: &[String]| -> Result<String, Error> {
            match arguments {
                [text] => Ok(text.clone()),
                _ => Err(Error::InvalidHostOperation {
                    item: format!("{name}!").into(),
                    reason: "takes one string literal".into(),
                    operation: at,
                }),
            }
        };
        let text = match name {
            "concat" => arguments.concat(),
            "env" => {
                let Some(variable) = arguments.first() else {
                    return Err(Error::InvalidHostOperation {
                        item: "env!".into(),
                        reason: "takes the name of an environment variable".into(),
                        operation: at,
                    });
                };
                match self.environment_variable(variable) {
                    Some(value) => value,
                    None => {
                        return Err(Error::InvalidHostOperation {
                            item: "env!".into(),
                            reason: arguments
                                .get(1)
                                .cloned()
                                .unwrap_or_else(|| {
                                    format!(
                                    "environment variable `{variable}` not defined at compile time"
                                )
                                })
                                .into(),
                            operation: at,
                        })
                    }
                }
            }
            "include_str" => {
                let path = one(&arguments)?;
                let bytes = self.include(name, &path, at)?;
                String::from_utf8(bytes).map_err(|_| Error::InvalidHostOperation {
                    item: "include_str!".into(),
                    reason: format!("`{path}` is not UTF-8").into(),
                    operation: at,
                })?
            }
            // `include_bytes!`'s path; its caller reads the file.
            _ => one(&arguments)?,
        };
        Ok((text, end))
    }

    /// One argument of `concat!` or another compile-time macro: a literal
    /// or a nested `concat!`, `env!` or `include_str!`.
    pub(super) fn compile_time_argument(&mut self) -> Result<String, Error> {
        let negative = matches!(self.peek(), Token::Minus | Token::UnaryMinus);
        if negative {
            self.next();
        }
        let sign = if negative { "-" } else { "" };
        // `::core::line!()`: a path from the crate root, as an expansion's
        // `$crate` writes one.
        if !negative && matches!(self.peek(), Token::DoubleColon) {
            self.next();
        }
        let text = match self.next() {
            // `$crate::__macro_support::line!()`: std's macro by a path.
            (Token::Identifier(first), position)
                if let Some(name) = self.std_macro_path(&first, point_span(position)) =>
            {
                (Token::Identifier(Box::new(name.as_str().into())), position)
            }
            text => text,
        };
        // `1u8`: `concat!` writes the number without its suffix.
        if matches!(text.0, Token::IntegerConstant(_) | Token::FloatConstant(_))
            && let Token::Identifier(suffix) = self.peek()
            && numeric_type(suffix.as_str()).is_some()
        {
            self.next();
        }
        Ok(match text {
            (Token::StringConstant(text), _) if !negative => text.to_string(),
            (Token::IntegerConstant(value), _) => format!("{sign}{value}"),
            (Token::FloatConstant(value), _) => format!("{sign}{}", value.1),
            (Token::CharConstant(value), _) if !negative => value.to_string(),
            (Token::True, _) if !negative => "true".into(),
            (Token::False, _) if !negative => "false".into(),
            (Token::Identifier(name), name_position)
                if !negative
                    && matches!(
                        name.as_str(),
                        "concat"
                            | "env"
                            | "include_str"
                            | "stringify"
                            | "line"
                            | "column"
                            | "file"
                            | "module_path"
                    )
                    && matches!(self.peek(), Token::Bang) =>
            {
                self.next();
                let name_span = point_span(name_position);
                self.compile_time_text(name.as_str(), name_span)?.0
            }
            // `$crate::__macro_support::file!()` before the crate's
            // re-export is known: the path missed, so it is asked for.
            (Token::Identifier(first), position)
                if !negative && let Some(miss) = self.macro_path_miss(&first, position) =>
            {
                return Err(miss);
            }
            (_, position) => {
                return Err(Error::UnsupportedNominalSyntax {
                    construct: "a compile-time macro argument other than a literal".into(),
                    source: point_span(position),
                })
            }
        })
    }

    /// The name of the file this code is in, as its source names it.
    pub(super) fn source_identity(&self) -> String {
        let key = self.models.sources.key(self.models.context.index());
        self.models
            .sources
            .resolve(key)
            .map(|source| source.identity().to_owned())
            .unwrap_or_default()
    }

    /// The program's root file, when it is a file on disk.
    pub(super) fn entry_file(&self) -> Option<std::path::PathBuf> {
        let key = self.models.sources.key(self.models.entry.index());
        let source = self.models.sources.resolve(key).ok()?;
        let path = std::path::PathBuf::from(source.identity());
        path.is_file().then_some(path)
    }

    /// `module_path!()`: the crate's name and the modules of the item
    /// being read, as `app::geometry`.
    pub(super) fn module_path(&self) -> String {
        let root = match self.entry_file() {
            Some(entry) => cargo_env::crate_name(&entry),
            None => std::path::Path::new(&self.source_identity())
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .replace('-', "_"),
        };
        let modules = self
            .callable
            .as_ref()
            .map(|key| key.modules())
            .unwrap_or_default();
        std::iter::once(root.as_str())
            .chain(modules.iter().map(ImmutableString::as_str))
            .collect::<Vec<_>>()
            .join("::")
    }

    /// The file this code is in, when it is a file on disk.
    pub(super) fn source_file(&self) -> Option<std::path::PathBuf> {
        let key = self.models.sources.key(self.models.context.index());
        let source = self.models.sources.resolve(key).ok()?;
        let path = std::path::PathBuf::from(source.identity());
        path.is_file().then_some(path)
    }

    /// A compile-time environment variable (`Place::variable`). A runner's
    /// compiler keeps it with the lowering that read it (#1646).
    pub(super) fn environment_variable(&self, name: &str) -> Option<String> {
        let place = Place {
            file: self.source_file(),
            entry: self.entry_file(),
        };
        let value = place.variable(&self.models.access, name);
        #[cfg(feature = "rust-import")]
        self.models
            .read(super::kept::Read::variable(place, name, value.as_deref()));
        value
    }

    /// The bytes of the file `path` names, relative to this code's file.
    pub(super) fn include(&self, macro_name: &str, path: &str, at: Span) -> Result<Vec<u8>, Error> {
        let error = |reason: String| Error::InvalidHostOperation {
            item: format!("{macro_name}!").into(),
            reason: reason.into(),
            operation: at,
        };
        // A file the host ships with its programs, as `?play` ships its
        // examples' assets.
        if let Some((_, bytes)) = crate::resolve::bundled(path) {
            return Ok(bytes.to_vec());
        }
        let Some(file) = self.source_file() else {
            return Err(error(format!(
                "`{path}` is read relative to a file on disk; this code is not in one"
            )));
        };
        let target = file.parent().map_or_else(
            || std::path::PathBuf::from(path),
            |directory| directory.join(path),
        );
        let bytes = included(&self.models.access, &target)
            .map_err(|failure| error(format!("couldn't read `{}`: {failure}", target.display())))?;
        // A runner's compiler keeps it with the lowering that read it.
        #[cfg(feature = "rust-import")]
        self.models.read(super::kept::Read::file(target, &bytes));
        Ok(bytes)
    }

    pub(super) fn float_constant(&mut self, value: f64, single: bool, at: Span) -> ParsedExpr {
        let ty = Ty::Float(if single { Float::F32 } else { Float::F64 });
        self.literal(format!("{value:?}"), ty, at)
    }

    /// A number literal's type: its suffix's (`3usize`, `2.5f32`, `1f64`,
    /// the name the tokenizer leaves after the digits), else a fresh
    /// `{integer}` or `{float}`; whether an integer is written as a float.
    /// A suffixed integer must fit its kind, as Rust's lint denies.
    pub(super) fn literal_kind(
        &mut self,
        value: INT,
        integer: bool,
        span: Span,
    ) -> Result<(Ty, bool), Error> {
        if let Token::Identifier(name) = self.peek()
            && let Some(ty) = numeric_type(name.as_str())
            && (integer || matches!(ty, Ty::Float(_)))
        {
            self.next();
            if let Ty::Int(kind) = ty
                && Wide::Signed(value.into()).fits(kind).is_err()
            {
                return Err(self.overflowing(kind, Wide::Signed(value.into()), span));
            }
            let float = integer && matches!(ty, Ty::Float(_));
            return Ok((ty, float));
        }
        let id = self.inference.fresh_id();
        self.literal_kinds.push((id, integer));
        if integer {
            self.literal_values.push((id, value, span));
        }
        Ok((
            if integer {
                Ty::Int(Int::Var(id))
            } else {
                Ty::Float(Float::Var(id))
            },
            false,
        ))
    }

    pub(super) fn literal(&self, source: impl Into<String>, ty: Ty, span: Span) -> ParsedExpr {
        ParsedExpr {
            prelude: origin::Fragment::default(),
            source: origin::Fragment::plain(source.into()),
            ty,
            span,
            lvalue: None,
            outcome: crate::host::usage::Outcome::Conservative,
            evaluation: crate::host::usage::Evaluation::Required,
        }
        .retain_origin()
    }
}

/// Where code reads its compile-time variables: its file and its
/// program's root file, each when it is a file on disk.
#[derive(Clone)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(super) struct Place {
    file: Option<std::path::PathBuf>,
    entry: Option<std::path::PathBuf>,
}

impl Place {
    /// The compile-time environment variable `name` here: what Cargo sets
    /// for the crate the file is in, `CARGO_MANIFEST_DIR` being the
    /// directory of the project the file is in, and else the
    /// environment's. A program granted no environment has none: every
    /// variable is undefined, as rustc reports one.
    pub(super) fn variable(&self, access: &crate::Access, name: &str) -> Option<String> {
        if !access.env() {
            return None;
        }
        if name.starts_with("CARGO_")
            && let (Some(file), Some(entry)) = (&self.file, &self.entry)
            && let Some(value) = cargo_env::variable(file, entry, name)
        {
            return value;
        }
        if name == "CARGO_MANIFEST_DIR" {
            let file = self.file.as_deref()?;
            let manifest = file
                .ancestors()
                .skip(1)
                .find(|directory| directory.join("Cargo.toml").is_file())
                .or_else(|| {
                    file.ancestors()
                        .skip(1)
                        .find(|directory| directory.file_name().is_some_and(|name| name == "src"))
                        .and_then(std::path::Path::parent)
                })
                .or_else(|| file.parent())?;
            return Some(manifest.to_string_lossy().into_owned());
        }
        std::env::var(name).ok()
    }
}

/// The bytes of the file at `target`, when the program may read it.
pub(super) fn included(
    access: &crate::Access,
    target: &std::path::Path,
) -> std::io::Result<Vec<u8>> {
    access.read(target).and_then(|()| std::fs::read(target))
}
