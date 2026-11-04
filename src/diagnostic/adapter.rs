//! Lossless transformations from existing error families.

use crate::{program, resolve, EvalAltResult, ParseError, ParseErrorType, Span};

use super::{code, Builder, Error, Report, Source};

impl Report {
    /// Transform one positioned compiler error into the canonical report model.
    ///
    /// # Errors
    ///
    /// Returns an error only when compiler evidence falls outside the supplied
    /// exact source snapshot.
    pub fn from_parse(source: Source, error: &ParseError) -> Result<Self, Error> {
        match error.err_type() {
            ParseErrorType::Resolution(error) => resolution_report(source, error),
            ParseErrorType::Unexpected { .. } => unexpected_report(source, error.err_type(), None),
            error_type => {
                let (stable_code, help) = parse_code(error_type);
                let mut builder = Report::error(stable_code, error_type.to_string());
                if let Some(help) = help {
                    builder = builder.help(help);
                }
                attach(builder, source, error.primary_span(), None, None)
            }
        }
    }

    /// Transform a parser error whose compiler-generated position has already
    /// been proven to originate at one exact user-source span.
    pub(crate) fn from_parse_at(
        source: Source,
        error: &ParseError,
        span: Span,
    ) -> Result<Self, Error> {
        match error.err_type() {
            ParseErrorType::Resolution(error) => resolution_report_at(source, error, span),
            ParseErrorType::Unexpected { .. } => {
                unexpected_report(source, error.err_type(), Some(span))
            }
            error_type => {
                let (stable_code, help) = parse_code(error_type);
                let mut builder = Report::error(stable_code, error_type.to_string());
                if let Some(help) = help {
                    builder = builder.help(help);
                }
                attach(builder, source, span, None, None)
            }
        }
    }

    /// Retain a compiler failure whose generated position could not be proven
    /// against user source without inventing a source label.
    pub(crate) fn from_parse_unpositioned(
        error: &ParseError,
        reason: impl Into<crate::ImmutableString>,
    ) -> Self {
        let builder = match error.err_type() {
            ParseErrorType::Resolution(error) => resolution_builder(error),
            error_type => {
                let (stable_code, help) = parse_code(error_type);
                let builder = Report::error(stable_code, error_type.to_string());
                match help {
                    Some(help) => builder.help(help),
                    None => builder,
                }
            }
        };
        builder.note(reason).finish()
    }

    /// Transform a source-qualified aggregate resolution failure.
    pub(crate) fn from_resolution(
        primary: Source,
        secondary: Option<Source>,
        error: &resolve::Error,
    ) -> Result<Self, Error> {
        resolution_report_with_sources(primary, secondary, error)
    }

    /// Transform an evaluator error, recursively retaining its deepest source
    /// position and call/module frames.
    ///
    /// # Errors
    ///
    /// Returns an error only when positioned evaluator evidence falls outside
    /// the supplied source snapshot.
    pub fn from_evaluation(source: Option<Source>, error: &EvalAltResult) -> Result<Self, Error> {
        let builder = evaluation_builder(error);
        let position = deepest_position(error);
        match (source, position.is_none()) {
            (Some(source), false) => Ok(builder
                .primary(source, Span::new(position, position), None)?
                .finish()),
            (Some(source), true) => Ok(builder.source(source).finish()),
            _ => Ok(builder.finish()),
        }
    }

    /// Transform an evaluator error at an exact retained callback declaration.
    ///
    /// This form is used when an aggregate evaluator position is not local to
    /// the callback's source unit. The callback declaration remains exact,
    /// while nested call frames are retained as canonical notes.
    pub fn from_evaluation_at(
        source: Source,
        declaration: Span,
        error: &EvalAltResult,
    ) -> Result<Self, Error> {
        Ok(evaluation_builder(error)
            .primary(source, declaration, Some("callback declared here".into()))?
            .finish())
    }

    /// Transform an evaluator error at `original`, a span in `source` that
    /// [`crate::program::Positions`] gave. With no span, the report names
    /// `source` and no position in it.
    ///
    /// # Errors
    ///
    /// Returns an error only when `original` falls outside `source`.
    #[doc(hidden)]
    pub fn from_evaluation_located(
        source: Source,
        original: Option<Span>,
        error: &EvalAltResult,
    ) -> Result<Self, Error> {
        match original {
            Some(original) => Self::from_evaluation_mapped(source, original, error),
            None => Ok(evaluation_builder(error).source(source).finish()),
        }
    }

    /// Transform an evaluator error at a compiler-proven original source span.
    pub(crate) fn from_evaluation_mapped(
        source: Source,
        original: Span,
        error: &EvalAltResult,
    ) -> Result<Self, Error> {
        Ok(evaluation_builder(error)
            .primary(source, original, None)?
            .finish())
    }

    /// Transform an authority/invocation error into one canonical report.
    ///
    /// # Errors
    ///
    /// Returns an error only when nested evaluator evidence is out of bounds.
    pub fn from_program(source: Option<Source>, error: &program::Error) -> Result<Self, Error> {
        if let program::Error::Evaluation { error, .. } = error {
            return Self::from_evaluation(source, error);
        }
        let stable_code = match error {
            program::Error::Construction { .. } => code::FRAMEWORK_CONSTRUCTION,
            program::Error::EngineOriginMismatch
            | program::Error::MissingItem { .. }
            | program::Error::Mismatch { .. }
            | program::Error::InvalidArity { .. }
            | program::Error::ApplicationBoundary { .. } => code::FRAMEWORK_AUTHORITY,
            program::Error::Evaluation { .. } => unreachable!(),
        };
        let builder = Report::error(stable_code, error.to_string());
        Ok(finish_with_source(builder, source))
    }

    /// Transform an authority/invocation error at its exact retained callback declaration.
    pub fn from_program_at(
        source: Source,
        declaration: Span,
        error: &program::Error,
    ) -> Result<Self, Error> {
        if let program::Error::Evaluation { error, .. } = error {
            return Self::from_evaluation_at(source, declaration, error);
        }
        Self::from_program(Some(source), error)
    }

    /// Transform a project/environment discovery failure.
    #[must_use]
    pub fn from_project(error: &crate::project::Error) -> Self {
        use crate::project::Error as ProjectError;

        let (stable_code, help) = match error {
            ProjectError::CurrentDirectory(..) => (
                code::ENVIRONMENT_CURRENT_DIRECTORY,
                "pass an explicit `.rs` file or crate directory",
            ),
            ProjectError::InputNotFound { .. } => (
                code::ENVIRONMENT_INPUT_NOT_FOUND,
                "check the path, or create a starter with `rawr new <path>`",
            ),
            ProjectError::NotFound { .. } => (
                code::ENVIRONMENT_PROJECT_NOT_FOUND,
                "pass a `.rs` file or a marked crate directory, or run `rawr new <path>`",
            ),
            ProjectError::Ambiguous { .. } => (
                code::ENVIRONMENT_PROJECT_AMBIGUOUS,
                "pass one exact candidate directory or `.rs` file",
            ),
            ProjectError::Unreadable { .. } => (
                code::ENVIRONMENT_UNREADABLE,
                "check resource permissions and ensure Rawr source is valid UTF-8",
            ),
            ProjectError::UnsupportedInput { .. } => (
                code::ENVIRONMENT_UNSUPPORTED_INPUT,
                "pass a directory, a `.rs` file or a `Cargo.toml`",
            ),
            ProjectError::Legacy { .. } => (
                code::ENVIRONMENT_UNSUPPORTED_INPUT,
                "run `rawr new <path>` to see the layout of a Rawr crate",
            ),
            ProjectError::MissingSource { .. } => (
                code::ENVIRONMENT_MISSING_SOURCE,
                "create `src/main.rs` with `// rawr` as its first line, or pass a `.rs` file",
            ),
            ProjectError::Unmarked { .. } => (
                code::ENVIRONMENT_MISSING_SOURCE,
                "add `// rawr` as the first line of the crate root, or of a module it declares, to run it with Rawr",
            ),
            ProjectError::EscapesRoot { .. } => (
                code::ENVIRONMENT_ESCAPES_ROOT,
                "move the resource inside the project root or replace the escaping symlink",
            ),
        };
        Report::error(stable_code, error.to_string())
            .help(help)
            .finish()
    }
}

/// Syntax Rust does not have, with rustc's label under the token and its
/// help, at `span` when the token's position was mapped there.
fn unexpected_report(
    source: Source,
    error: &ParseErrorType,
    span: Option<Span>,
) -> Result<Report, Error> {
    let ParseErrorType::Unexpected {
        message,
        label,
        help,
        span: own,
    } = error
    else {
        unreachable!("only syntax Rust does not have is reported here");
    };
    let mut builder = Report::error(code::PARSE_UNEXPECTED, message.clone());
    if let Some(help) = help {
        builder = builder.help(help.clone());
    }
    let span = span.unwrap_or(*own);
    builder = builder.source(source.clone());
    if !span.is_none() {
        builder = builder.primary(source, span, Some(label.clone().into()))?;
    }
    Ok(builder.finish())
}

fn parse_code(error: &ParseErrorType) -> (code::Code, Option<&'static str>) {
    match error {
        ParseErrorType::BadInput(..) => (code::PARSE_BAD_INPUT, None),
        ParseErrorType::UnexpectedEOF => (
            code::PARSE_UNEXPECTED_EOF,
            Some("finish the incomplete expression or declaration"),
        ),
        ParseErrorType::MissingToken(..) => (code::PARSE_MISSING_TOKEN, None),
        ParseErrorType::MissingSymbol(..) => (code::PARSE_MISSING_SYMBOL, None),
        ParseErrorType::UnknownEnum(..)
        | ParseErrorType::UnknownEnumVariant(..)
        | ParseErrorType::WrongEnumVariantArity(..) => (code::RESOLVE_ENUM_PATTERN, None),
        ParseErrorType::MalformedReceiver(..) => (code::RESOLVE_RECEIVER, None),
        ParseErrorType::MalformedTypeTuple(..) => (code::RESOLVE_TYPE_SHAPE, None),
        ParseErrorType::Unexpected { .. } => (code::PARSE_UNEXPECTED, None),
        ParseErrorType::Resolution(..) => unreachable!(),
        _ => (code::PARSE_SYNTAX, None),
    }
}

fn resolution_report(source: Source, error: &resolve::Error) -> Result<Report, Error> {
    resolution_report_with_sources(source, None, error)
}

fn resolution_report_at(
    source: Source,
    error: &resolve::Error,
    primary: Span,
) -> Result<Report, Error> {
    Ok(sourced_builder(error, &source, primary)
        .source(source.clone())
        .primary(
            source.clone(),
            primary,
            primary_label(error, &source, primary),
        )?
        .finish())
}

/// The builder for `error`, which reads `source` where rustc's error for it
/// depends on how the source names the item.
fn sourced_builder(error: &resolve::Error, source: &Source, primary: Span) -> Builder {
    // rustc's E0425 names what the path is used as: a call's function, or
    // a value.
    if let resolve::Error::NotInModule { module, item, .. } = error {
        let kind = match (
            called(source, primary, item),
            item.starts_with(char::is_uppercase),
        ) {
            (true, false) => "function",
            (true, true) => "function, tuple struct or tuple variant",
            (false, _) => "value",
        };
        return Report::error(
            code::RESOLVE_NOT_IN_MODULE,
            format!(
                "cannot find {kind} `{item}` in module `{}`",
                written_module(source, primary, module)
            ),
        );
    }
    let resolve::Error::UnknownMember { owner, member, .. } = error else {
        return resolution_builder(error);
    };
    // `print` as the source writes it, not Rawr's spelling of it.
    let member = crate::tokenizer::demangle_names(member);
    let member = member.as_ref();
    let Some(kind) = member_kind(source, primary, member) else {
        return resolution_builder(error);
    };
    let builder = match kind {
        Member::Field => Report::error(
            code::RESOLVE_NO_FIELD,
            format!("no field `{member}` on type `{}`", short(owner)),
        ),
        Member::Method => Report::error(
            code::RESOLVE_NO_METHOD,
            format!(
                "no method named `{member}` found for `{}` in the current scope",
                short(owner)
            ),
        ),
        Member::Associated => {
            let item = if member.starts_with(char::is_uppercase) {
                "variant or associated item"
            } else {
                "function or associated item"
            };
            Report::error(
                code::RESOLVE_NO_METHOD,
                format!(
                    "no {item} named `{member}` found for `{}` in the current scope",
                    short(owner)
                ),
            )
        }
    };
    // rustc adds no help to these: the name is misspelled or the item
    // isn't there.
    builder
}

/// The module's path as the source writes it before the name at `at`,
/// as rustc names it: `label` for `label::describe`, which resolves to the
/// program's `crate::label`. `module` when the source has no path there.
fn written_module<'a>(source: &'a Source, at: Span, module: &'a str) -> &'a str {
    let start = at.start();
    let (Some(line), Some(column)) = (start.line(), start.position()) else {
        return module;
    };
    let Some(text) = source.text().split('\n').nth(line.saturating_sub(1)) else {
        return module;
    };
    let end = text
        .char_indices()
        .nth(column.saturating_sub(1))
        .map_or(text.len(), |(index, _)| index);
    let Some(before) = text[..end].strip_suffix("::") else {
        return module;
    };
    let begin = before
        .rfind(|character: char| {
            !(character.is_alphanumeric() || character == '_' || character == ':')
        })
        .map_or(0, |index| index + 1);
    match &before[begin..] {
        "" => module,
        written => written,
    }
}

/// Whether the name at `at` in `source` is called: `(` follows it.
fn called(source: &Source, at: Span, name: &str) -> bool {
    let start = at.start();
    let (Some(line), Some(column)) = (start.line(), start.position()) else {
        return false;
    };
    let Some(text) = source.text().split('\n').nth(line.saturating_sub(1)) else {
        return false;
    };
    text.chars()
        .skip(column.saturating_sub(1) + name.chars().count())
        .find(|character| !character.is_whitespace())
        .is_some_and(|next| next == '(')
}

/// An unknown member's span, which covers its name as rustc underlines it.
fn member_span(error: &resolve::Error) -> Span {
    let primary = error.primary_span();
    let (resolve::Error::UnknownMember { member, .. }
    | resolve::Error::MemberKind { member, .. }
    | resolve::Error::NotInModule { item: member, .. }) = error
    else {
        return primary;
    };
    let start = primary.start();
    match (
        start.line(),
        start.position(),
        u16::try_from(crate::tokenizer::demangle_names(member).chars().count()),
    ) {
        (Some(line), Some(column), Ok(length)) if primary.end() == start && length > 0 => {
            let line = u16::try_from(line).unwrap_or(u16::MAX);
            let column = u16::try_from(column).unwrap_or(u16::MAX);
            Span::new(
                start,
                crate::Position::new(line, column.saturating_add(length)),
            )
        }
        _ => primary,
    }
}

/// A type as rustc names one the script imported: its last segment.
fn short(owner: &str) -> &str {
    if owner.contains('<') {
        return owner;
    }
    owner.rsplit("::").next().unwrap_or(owner)
}

/// How source names a member: `value.member()`, `value.member` or
/// `Type::member`.
enum Member {
    Method,
    Field,
    Associated,
}

/// `None` for a struct literal or a pattern, which rustc reports apart.
fn member_kind(source: &Source, at: Span, member: &str) -> Option<Member> {
    let start = at.start();
    let line = start.line()?;
    let column = start.position()?;
    let text = source.text().split('\n').nth(line.saturating_sub(1))?;
    let characters: Vec<char> = text.chars().collect();
    let column = column.saturating_sub(1).min(characters.len());
    let before: String = characters[..column].iter().collect();
    let before = before.trim_end();
    if before.ends_with("::") {
        return Some(Member::Associated);
    }
    if !before.ends_with('.') {
        return None;
    }
    let end = (column + member.chars().count()).min(characters.len());
    let after: String = characters[end..].iter().collect();
    let after = after.trim_start();
    Some(if after.starts_with('(') || after.starts_with("::") {
        Member::Method
    } else {
        Member::Field
    })
}

fn resolution_report_with_sources(
    source: Source,
    secondary_source: Option<Source>,
    error: &resolve::Error,
) -> Result<Report, Error> {
    let builder = sourced_builder(error, &source, error.primary_span());
    let secondary_message = error.secondary_span().map(|_| match error {
        resolve::Error::MovedValue { .. } => "value moved here",
        resolve::Error::BorrowConflict { .. } => "borrow made here",
        resolve::Error::ArgumentCount { callee, .. } => callee.defined_here(),
        resolve::Error::PrivateItem { item, .. } => item.defined_here(),
        _ => "first declaration or related definition is here",
    });
    let mut builder = builder.source(source.clone());
    if !error.primary_span().is_none() {
        let label = primary_label(error, &source, error.primary_span());
        builder = builder.primary(source.clone(), member_span(error), label)?;
    }
    if let resolve::Error::MovedValue {
        name,
        cause: Some(cause),
        ..
    } = error
    {
        builder = builder.secondary(
            source.clone(),
            cause.declaration,
            Some(
                format!(
                    "move occurs because `{name}` has type `{}`, which does not implement the \
                     `Copy` trait",
                    cause.ty
                )
                .into(),
            ),
        )?;
    }
    if let Some(secondary) = error.secondary_span().filter(|span| !span.is_none()) {
        builder = builder.secondary(
            secondary_source.unwrap_or(source),
            secondary,
            secondary_message.map(Into::into),
        )?;
    }
    Ok(builder.finish())
}

/// The label under the primary span, where rustc writes one.
fn primary_label(
    error: &resolve::Error,
    source: &Source,
    primary: Span,
) -> Option<crate::ImmutableString> {
    match error {
        resolve::Error::Syntax { label, .. } => {
            Some(label.clone()).filter(|label| !label.is_empty())
        }
        resolve::Error::UnlabeledJumpInWhileCondition { keyword, .. } => {
            Some(format!("unlabeled `{keyword}` in the condition of a `while` loop").into())
        }
        resolve::Error::AsyncMain { .. } => Some(resolve::ASYNC_MAIN.into()),
        resolve::Error::StdPath { label, .. } => label.clone(),
        resolve::Error::CannotCompare { lhs, rhs, .. } => {
            Some(format!("no implementation for `{lhs} == {rhs}`").into())
        }
        resolve::Error::ArgumentSignature { .. } => Some("found signature defined here".into()),
        resolve::Error::PrivateItem { item, .. } => Some(format!("private {}", item.noun()).into()),
        resolve::Error::NotInModule { module, .. } => {
            Some(format!("not found in `{}`", written_module(source, primary, module)).into())
        }
        resolve::Error::UnknownFunction { .. } | resolve::Error::UnknownVariable { .. } => {
            Some("not found in this scope".into())
        }
        resolve::Error::MovedValue {
            previous_iteration: true,
            ..
        } => Some("value moved here, in previous iteration of loop".into()),
        resolve::Error::MovedValue { borrowed: true, .. } => {
            Some("value borrowed here after move".into())
        }
        resolve::Error::MovedValue { .. } => Some("value used here after move".into()),
        _ => None,
    }
}

fn resolution_builder(error: &resolve::Error) -> Builder {
    // A member used as the other kind, which rustc reports apart.
    match error {
        resolve::Error::MemberKind {
            owner,
            member,
            field: true,
            ..
        } => {
            return Report::error(
                code::RESOLVE_NO_METHOD,
                format!(
                    "no method named `{member}` found for `{}` in the current scope",
                    short(owner)
                ),
            )
            .help(format!(
                "to use the field `{member}`, remove the parentheses"
            ));
        }
        resolve::Error::MemberKind {
            owner,
            member,
            field: false,
            ..
        } => {
            return Report::error(
                code::RESOLVE_METHOD_VALUE,
                format!(
                    "attempted to take value of method `{member}` on type `{}`",
                    short(owner)
                ),
            )
            .help("use parentheses to call the method");
        }
        _ => {}
    }
    let stable_code = match error.category() {
        resolve::Category::DuplicateItem => code::RESOLVE_DUPLICATE_ITEM,
        resolve::Category::UnknownType => code::RESOLVE_UNKNOWN_TYPE,
        resolve::Category::UnknownImport => code::RESOLVE_UNKNOWN_IMPORT,
        resolve::Category::InvalidImport => code::RESOLVE_INVALID_IMPORT,
        resolve::Category::InvalidTypeArguments => code::RESOLVE_TYPE_ARGUMENTS,
        resolve::Category::LoweringUnavailable => code::RESOLVE_LOWERING,
        resolve::Category::InvalidDerive => code::RESOLVE_DERIVE,
        resolve::Category::DuplicateMember => match error {
            resolve::Error::DuplicateMember { owner, .. } if owner.is_empty() => {
                code::RESOLVE_DUPLICATE_GENERIC
            }
            _ => code::RESOLVE_DUPLICATE_MEMBER,
        },
        resolve::Category::UnknownMember => code::RESOLVE_UNKNOWN_MEMBER,
        resolve::Category::NotInModule => code::RESOLVE_NOT_IN_MODULE,
        resolve::Category::UnknownFunction => code::RESOLVE_UNKNOWN_FUNCTION,
        resolve::Category::UnknownVariable => code::RESOLVE_UNKNOWN_VARIABLE,
        resolve::Category::MovedValue => code::RESOLVE_MOVED_VALUE,
        resolve::Category::BorrowConflict => match error {
            resolve::Error::BorrowConflict { code, .. } => code::borrow(code),
            _ => unreachable!("a borrow conflict's category"),
        },
        resolve::Category::InvalidConstruction => code::RESOLVE_CONSTRUCTION,
        resolve::Category::TypeMismatch => code::RESOLVE_TYPE_MISMATCH,
        resolve::Category::ImmutableBinding => code::RESOLVE_IMMUTABLE_BINDING,
        resolve::Category::InvalidImplOwner => code::RESOLVE_IMPL_OWNER,
        resolve::Category::DuplicateMethod => code::RESOLVE_DUPLICATE_METHOD,
        resolve::Category::InvalidMethodCall => code::RESOLVE_METHOD_CALL,
        resolve::Category::InvalidHostOperation => code::RESOLVE_HOST_OPERATION,
        resolve::Category::UnusedValue => code::RESOLVE_UNUSED_VALUE,
        resolve::Category::SelfOutsideImpl => code::RESOLVE_SELF,
        resolve::Category::ImmutableReceiver => code::RESOLVE_RECEIVER_MUTATION,
        resolve::Category::InvalidMethodSignature => code::RESOLVE_METHOD_SIGNATURE,
        resolve::Category::MissingCapability => code::RESOLVE_CAPABILITY,
        resolve::Category::InvalidMatch => code::RESOLVE_MATCH,
        resolve::Category::UnsupportedNominalSyntax => code::RESOLVE_UNSUPPORTED_SYNTAX,
        resolve::Category::Syntax => code::PARSE_UNEXPECTED,
        resolve::Category::UnlabeledJumpInWhileCondition => code::RESOLVE_JUMP_IN_WHILE_CONDITION,
        resolve::Category::AsyncMain => code::RESOLVE_ASYNC_MAIN,
        resolve::Category::StdPath => match error {
            resolve::Error::StdPath { code, .. } => code::std_path(code),
            _ => unreachable!("a std path's category"),
        },
        resolve::Category::NotYet => code::RESOLVE_NOT_YET,
        resolve::Category::TypeAnnotationsNeeded => code::RESOLVE_TYPE_ANNOTATIONS,
        resolve::Category::CannotCompare => code::RESOLVE_CANNOT_COMPARE,
        resolve::Category::ArgumentSignature => code::RESOLVE_ARGUMENT_SIGNATURE,
        resolve::Category::ArgumentCount => match error {
            resolve::Error::ArgumentCount {
                callee: resolve::Callee::Closure,
                ..
            } => code::RESOLVE_CLOSURE_ARGUMENT_COUNT,
            _ => code::RESOLVE_ARGUMENT_COUNT,
        },
        resolve::Category::PrivateItem => match error {
            resolve::Error::PrivateItem { item, .. } => code::private(*item),
            _ => unreachable!("a private item's category"),
        },
    };
    // Names Rawr keeps under another spelling read as Rust's.
    let message = resolution_message(error);
    let builder = Report::error(
        stable_code,
        crate::tokenizer::demangle_names(&message).into_owned(),
    );
    match error {
        resolve::Error::Syntax {
            help: Some(help), ..
        } => builder.help(help.clone()),
        resolve::Error::DuplicateItem { .. }
        | resolve::Error::DuplicateMember { .. }
        | resolve::Error::DuplicateMethod { .. } => {
            builder.help("rename or remove one of the duplicate declarations")
        }
        resolve::Error::UnknownType { name, .. } if name.contains("::") => builder.help(format!(
            "`{name}` isn't available in Rawr yet, or the path is misspelled"
        )),
        resolve::Error::UnknownType { name, .. } => {
            builder.help(format!("declare `{name}`, or import it with `use`"))
        }
        resolve::Error::UnknownMember { owner, member, .. } if owner.contains("::") => builder
            .help(format!(
                "`{owner}::{member}` isn't available in Rawr yet, or the name is misspelled"
            )),
        resolve::Error::UnknownImport { path, .. } => builder.help(format!(
            "`{path}` isn't available in Rawr yet, or the path is misspelled"
        )),
        resolve::Error::StdPath {
            note: Some(note), ..
        } => builder.note(note.clone()),
        resolve::Error::UnknownFunction {
            import: Some(path), ..
        } => builder.help(format!("import it with `use {path};`")),
        resolve::Error::UnknownFunction { name, .. } => {
            builder.help(format!("declare `fn {name}`, or import it with `use`"))
        }
        resolve::Error::UnknownVariable { name, .. } => builder.help(format!(
            "declare `{name}` with `let`, or import it with `use`"
        )),
        resolve::Error::MovedValue { name, .. } => builder.help(format!(
            "`{name}` was moved before this use; clone it before the move to keep a copy"
        )),
        resolve::Error::MissingCapability {
            owner,
            capability,
            derive,
            required_by,
            ..
        } => {
            let builder = match capability.as_str() {
                "Display" => builder.help(format!(
                    "the trait `std::fmt::Display` is not implemented for `{owner}`"
                )),
                "Sized" => builder.help(format!(
                    "the trait `Sized` is not implemented for `{owner}`"
                )),
                _ => builder,
            };
            let builder = match required_by {
                Some(item) => builder.note(format!("required by a bound in `{item}`")),
                None => builder,
            };
            if *derive && resolve::derivable(capability) {
                builder.help(format!(
                    "consider annotating `{owner}` with `#[derive({capability})]`"
                ))
            } else {
                builder
            }
        }
        resolve::Error::CannotCompare {
            lhs, rhs, required, ..
        } => {
            let builder = builder.help(format!(
                "the trait `PartialEq<{rhs}>` is not implemented for `{lhs}`"
            ));
            match required {
                Some((lhs, rhs)) => builder.note(format!(
                    "required for `{lhs}` to implement `PartialEq<{rhs}>`"
                )),
                None => builder,
            }
        }
        resolve::Error::ArgumentSignature {
            closure,
            expected,
            found,
            ..
        } => {
            let kind = resolve::ArgumentSignature(*closure).kind();
            builder.note(format!(
                "expected {kind} signature `{expected}`\n   found {kind} signature `{found}`"
            ))
        }
        resolve::Error::ArgumentCount {
            expected, found, ..
        } => builder.help(match (found < expected, expected.abs_diff(*found)) {
            (true, 1) => "provide the argument",
            (true, _) => "provide the arguments",
            (false, 1) => "remove the extra argument",
            (false, _) => "remove the extra arguments",
        }),
        _ => builder,
    }
}

fn attach(
    mut builder: Builder,
    source: Source,
    primary: Span,
    secondary: Option<Span>,
    secondary_message: Option<&str>,
) -> Result<Report, Error> {
    builder = builder.source(source.clone());
    if !primary.is_none() {
        builder = builder.primary(source.clone(), primary, None)?;
    }
    if let Some(secondary) = secondary.filter(|span| !span.is_none()) {
        builder = builder.secondary(source, secondary, secondary_message.map(Into::into))?;
    }
    Ok(builder.finish())
}

fn finish_with_source(builder: Builder, source: Option<Source>) -> Report {
    match source {
        Some(source) => builder.source(source).finish(),
        None => builder.finish(),
    }
}

fn evaluation_leaf<'a>(error: &'a EvalAltResult, notes: &mut Vec<String>) -> &'a EvalAltResult {
    match error {
        EvalAltResult::ErrorInFunctionCall(function, source, inner, ..) => {
            let leaf = evaluation_leaf(inner, notes);
            let mut note = format!(
                "in call to function `{}`",
                crate::resolve::lower_nominal::function_display_name(function)
            );
            if !source.is_empty() {
                note.push_str(&format!(" from `{source}`"));
            }
            notes.push(note);
            leaf
        }
        EvalAltResult::ErrorInModule(module, inner, ..) => {
            let leaf = evaluation_leaf(inner, notes);
            if module.is_empty() {
                notes.push("in module".into());
            } else {
                notes.push(format!("in module `{module}`"));
            }
            leaf
        }
        leaf => leaf,
    }
}

fn evaluation_builder(error: &EvalAltResult) -> Builder {
    let mut notes = Vec::new();
    let leaf = evaluation_leaf(error, &mut notes);
    let stable_code = match leaf {
        EvalAltResult::ErrorRuntime(..) => code::RUNTIME_THROW,
        _ => code::RUNTIME_EVALUATION,
    };
    notes.into_iter().fold(
        Report::error(stable_code, evaluation_message(leaf)),
        Builder::note,
    )
}

fn evaluation_message(error: &EvalAltResult) -> String {
    match error {
        EvalAltResult::ErrorRuntime(value, ..) if value.is_unit() => "runtime error".into(),
        EvalAltResult::ErrorRuntime(value, ..) => format!("runtime error: {value}"),
        EvalAltResult::ErrorParsing(error, ..) => format!("syntax error: {error}"),
        // Rust's wording for an index past the end.
        EvalAltResult::ErrorArrayBounds(len, index, ..) if *index >= 0 => {
            format!("index out of bounds: the len is {len} but the index is {index}")
        }
        _ => error.to_string(),
    }
}

fn resolution_message(error: &resolve::Error) -> String {
    match error {
        resolve::Error::DuplicateItem { name, .. } => format!("duplicate item `{name}`"),
        resolve::Error::UnknownType { name, .. } => format!("unknown type `{name}`"),
        resolve::Error::UnknownImport { path, .. } => format!("unknown import `{path}`"),
        resolve::Error::InvalidImport { path, reason, .. } => {
            format!("invalid import `{path}`: {reason}")
        }
        resolve::Error::InvalidTypeArguments {
            ty,
            expected,
            actual,
            ..
        } => format!("invalid type arguments for `{ty}`: expected {expected}, found {actual}"),
        resolve::Error::LoweringUnavailable { item, .. } => {
            format!("item body for `{item}` cannot be lowered yet")
        }
        resolve::Error::InvalidDerive {
            owner,
            capability,
            reason,
            ..
        } => format!("cannot derive `{capability}` for `{owner}`: {reason}"),
        resolve::Error::DuplicateMember { owner, member, .. } if owner.is_empty() => {
            format!("the name `{member}` is already used for a generic parameter")
        }
        resolve::Error::DuplicateMember { owner, member, .. } => {
            format!("duplicate member `{owner}::{member}`")
        }
        resolve::Error::UnknownMember { owner, member, .. } => {
            format!("unknown member `{owner}::{member}`")
        }
        resolve::Error::NotInModule { module, item, .. } => {
            format!("cannot find `{item}` in module `{module}`")
        }
        resolve::Error::PrivateItem { item, name, .. } => {
            format!("{} `{name}` is private", item.noun())
        }
        resolve::Error::MemberKind { owner, member, .. } => {
            format!("member `{owner}::{member}` used as the other kind")
        }
        resolve::Error::UnknownFunction { name, .. } => {
            format!("cannot find function `{name}` in this scope")
        }
        resolve::Error::UnknownVariable { name, .. } => {
            format!("cannot find value `{name}` in this scope")
        }
        resolve::Error::MovedValue {
            name,
            borrowed: true,
            ..
        } => format!("borrow of moved value: `{name}`"),
        resolve::Error::MovedValue { name, .. } => format!("use of moved value: `{name}`"),
        resolve::Error::BorrowConflict { message, .. } => message.to_string(),
        resolve::Error::InvalidConstruction { owner, reason, .. } => {
            format!("invalid construction of `{owner}`: {reason}")
        }
        resolve::Error::TypeMismatch {
            expected, actual, ..
        } => format!("type mismatch: expected `{expected}`, found `{actual}`"),
        resolve::Error::ImmutableBinding { binding, .. } => {
            format!("cannot mutate immutable binding `{binding}`")
        }
        resolve::Error::InvalidImplOwner { owner, .. } => {
            format!("cannot define inherent methods for `{owner}`")
        }
        resolve::Error::DuplicateMethod { owner, method, .. } => {
            format!("duplicate method `{owner}::{method}`")
        }
        resolve::Error::InvalidMethodCall {
            owner,
            method,
            reason,
            ..
        } => format!(
            "invalid call to `{}`: {reason}",
            resolve::Qualified(owner, method)
        ),
        resolve::Error::InvalidHostOperation { item, reason, .. } => {
            format!("cannot use `{item}` here: {reason}")
        }
        resolve::Error::ImportedApplicationRequired { item, .. } => {
            format!("`{item}` needs a proof of which imported method applies in its scope")
        }
        resolve::Error::UnusedValue { value, .. } => {
            format!("unused `{value}` must be returned, bound, or explicitly consumed")
        }
        resolve::Error::SelfOutsideImpl { .. } => {
            "`self` is only available inside an impl method".into()
        }
        resolve::Error::ImmutableReceiver { owner, .. } => {
            format!("cannot mutate `{owner}` through shared receiver `&self`")
        }
        resolve::Error::InvalidMethodSignature {
            owner,
            method,
            reason,
            ..
        } => format!(
            "invalid signature for `{}`: {reason}",
            resolve::Qualified(owner, method)
        ),
        resolve::Error::MissingCapability {
            owner, capability, ..
        } => resolve::Unsatisfied(owner, capability).to_string(),
        resolve::Error::InvalidMatch { owner, reason, .. } => {
            format!("invalid match for `{owner}`: {reason}")
        }
        resolve::Error::UnsupportedNominalSyntax { construct, .. } => {
            resolve::Unsupported(construct).to_string()
        }
        resolve::Error::Syntax { message, .. } => message.to_string(),
        resolve::Error::UnlabeledJumpInWhileCondition { .. } => {
            "`break` or `continue` with no label in the condition of a `while` loop".to_string()
        }
        resolve::Error::AsyncMain { .. } => resolve::ASYNC_MAIN.to_string(),
        resolve::Error::StdPath { message, .. } => message.to_string(),
        resolve::Error::NotYet { path, .. } => format!("rawr doesn't run `{path}` yet"),
        resolve::Error::TypeAnnotationsNeeded { parameter, .. } => {
            format!("type annotations needed: cannot infer the type of the type parameter `{parameter}`")
        }
        resolve::Error::ArgumentCount {
            callee,
            expected,
            found,
            ..
        } => resolve::ArgumentCount {
            callee: *callee,
            expected: *expected,
            found: *found,
        }
        .to_string(),
        resolve::Error::CannotCompare { lhs, rhs, .. } => {
            resolve::CannotCompare(lhs, rhs).to_string()
        }
        resolve::Error::ArgumentSignature { closure, .. } => {
            resolve::ArgumentSignature(*closure).to_string()
        }
    }
}

fn deepest_position(error: &EvalAltResult) -> crate::Position {
    match error {
        EvalAltResult::ErrorInFunctionCall(.., inner, outer)
        | EvalAltResult::ErrorInModule(.., inner, outer) => deepest_position(inner).or_else(*outer),
        leaf => leaf.position(),
    }
}

#[cfg(test)]
mod tests {
    use super::Report;
    use crate::diagnostic::{code, Source};
    use crate::{resolve, ParseError, ParseErrorType, Position, Span};

    #[test]
    fn translated_resolution_uses_only_proven_primary_evidence() {
        let generated_declaration = Span::new(Position::new(91, 3), Position::new(91, 9));
        let generated_previous = Span::new(Position::new(12, 3), Position::new(12, 9));
        let error = ParseError(
            Box::new(ParseErrorType::Resolution(resolve::Error::DuplicateItem {
                name: "view".into(),
                declaration: generated_declaration,
                previous_declaration: generated_previous,
            })),
            generated_declaration.start(),
            Default::default(),
        );
        let translated = Span::new(Position::new(2, 8), Position::new(2, 12));
        let source = Source::new("src/lib.rs", "fn one() {}\nfn view() {}\n")
            .expect("test source has a stable identity");

        let report = Report::from_parse_at(source, &error, translated)
            .expect("translated primary evidence belongs to the source");

        assert_eq!(report.code(), code::RESOLVE_DUPLICATE_ITEM);
        assert_eq!(report.message(), "duplicate item `view`");
        assert_eq!(
            report.help(),
            Some("rename or remove one of the duplicate declarations")
        );
        assert_eq!(report.labels().len(), 1);
        assert_eq!(report.labels()[0].span(), translated);
    }

    #[test]
    fn unmapped_generated_error_retains_context_without_a_fabricated_label() {
        let error = ParseError(
            Box::new(ParseErrorType::UnexpectedEOF),
            Position::new(9, 4),
            Default::default(),
        );
        let report =
            Report::from_parse_unpositioned(&error, "generated diagnostic has no origin proof");

        assert_eq!(report.code(), code::PARSE_UNEXPECTED_EOF);
        assert!(report.labels().is_empty());
        assert!(report.sources().is_empty());
        assert_eq!(
            report.notes().collect::<Vec<_>>(),
            vec!["generated diagnostic has no origin proof"]
        );
    }

    /// A missing trait reads as rustc's E0277, and suggests `#[derive]`
    /// only for a derivable trait on the program's own type (#718).
    #[test]
    fn a_missing_trait_reads_as_rustcs_unsatisfied_bound() {
        let source = Source::new(
            "src/main.rs",
            "fn main() { let x: Point = \"\".parse(); }\n",
        )
        .expect("test source has a stable identity");
        let at = Span::new(Position::new(1, 28), Position::new(1, 37));
        let missing = |capability: &str, derive: bool| resolve::Error::MissingCapability {
            owner: "Point".into(),
            capability: capability.into(),
            operation: at,
            derive,
            required_by: None,
        };

        let report = Report::from_resolution(source.clone(), None, &missing("FromStr", true))
            .expect("the span is in the source");
        assert_eq!(report.code(), code::RESOLVE_CAPABILITY);
        assert_eq!(report.code().as_str(), "E0277");
        assert_eq!(
            report.message(),
            "the trait bound `Point: FromStr` is not satisfied"
        );
        assert_eq!(report.help(), None);

        let report = Report::from_resolution(source.clone(), None, &missing("Debug", true))
            .expect("the span is in the source");
        assert_eq!(report.message(), "`Point` doesn't implement `Debug`");
        assert_eq!(
            report.help(),
            Some("consider annotating `Point` with `#[derive(Debug)]`")
        );

        let report = Report::from_resolution(source, None, &missing("Clone", false))
            .expect("the span is in the source");
        assert_eq!(report.help(), None);
    }
}
