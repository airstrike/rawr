//! Lossless transformations from existing error families.

use crate::{application, program, resolve, EvalAltResult, ParseError, ParseErrorType, Span};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

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
            ParseErrorType::Resolution(resolve::Error::Application(error)) => {
                application_report(source, error)
            }
            ParseErrorType::Resolution(error) => resolution_report(source, error),
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
            ParseErrorType::Resolution(resolve::Error::Application(error)) => {
                application_report_at(source, error, span)
            }
            ParseErrorType::Resolution(error) => resolution_report_at(source, error, span),
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
            ParseErrorType::Resolution(resolve::Error::Application(error)) => {
                application_builder(error)
            }
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
    #[cfg(not(feature = "no_std"))]
    pub(crate) fn from_resolution(
        primary: Source,
        secondary: Option<Source>,
        error: &resolve::Error,
    ) -> Result<Self, Error> {
        match error {
            resolve::Error::Application(error) => application_report(primary, error),
            error => resolution_report_with_sources(primary, secondary, error),
        }
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

    /// Transform an invocation error through compiler-retained original-position evidence.
    pub(crate) fn from_program_mapped(
        source: Source,
        original: Span,
        error: &program::Error,
    ) -> Result<Self, Error> {
        if let program::Error::Evaluation { error, .. } = error {
            return Self::from_evaluation_mapped(source, original, error);
        }
        Self::from_program(Some(source), error)
    }

    /// Transform a bound application runtime failure.
    ///
    /// # Errors
    ///
    /// Returns an error only when nested evaluator evidence is out of bounds.
    pub fn from_application_runtime(
        source: Option<Source>,
        error: &application::RuntimeError,
    ) -> Result<Self, Error> {
        match error {
            application::RuntimeError::Invocation(error)
            | application::RuntimeError::Task(error) => Self::from_program(source, error),
            application::RuntimeError::StateConstruction(..)
            | application::RuntimeError::State(..)
            | application::RuntimeError::Message(..)
            | application::RuntimeError::Startup(..)
            | application::RuntimeError::TaskRepresentation { .. } => {
                let builder = Report::error(code::RUNTIME_VALUE, error.to_string());
                Ok(finish_with_source(builder, source))
            }
            application::RuntimeError::DefaultBoot | application::RuntimeError::ExplicitBoot => {
                let builder = Report::error(code::RUNTIME_BOOT, error.to_string());
                Ok(finish_with_source(builder, source))
            }
        }
    }

    /// Transform a bound application runtime failure at its exact callback declaration.
    pub fn from_application_runtime_at(
        source: Source,
        declaration: Span,
        error: &application::RuntimeError,
    ) -> Result<Self, Error> {
        match error {
            application::RuntimeError::Invocation(error)
            | application::RuntimeError::Task(error) => {
                Self::from_program_at(source, declaration, error)
            }
            application::RuntimeError::StateConstruction(..)
            | application::RuntimeError::State(..)
            | application::RuntimeError::Message(..)
            | application::RuntimeError::Startup(..)
            | application::RuntimeError::TaskRepresentation { .. } => {
                Ok(Report::error(code::RUNTIME_VALUE, error.to_string())
                    .primary(source, declaration, Some("callback declared here".into()))?
                    .finish())
            }
            application::RuntimeError::DefaultBoot | application::RuntimeError::ExplicitBoot => {
                Ok(Report::error(code::RUNTIME_BOOT, error.to_string())
                    .primary(source, declaration, Some("callback declared here".into()))?
                    .finish())
            }
        }
    }

    /// Transform an application callback failure through original-position evidence.
    pub(crate) fn from_application_runtime_mapped(
        source: Source,
        original: Span,
        error: &application::RuntimeError,
    ) -> Result<Self, Error> {
        match error {
            application::RuntimeError::Invocation(error)
            | application::RuntimeError::Task(error) => {
                Self::from_program_mapped(source, original, error)
            }
            _ => Self::from_application_runtime(Some(source), error),
        }
    }

    /// Transform a project/environment discovery failure.
    #[cfg(feature = "std")]
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
                "add `// rawr` as the first line of the crate root to run it with Rawr",
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
        .primary(source, primary, None)?
        .finish())
}

/// The builder for `error`, which reads `source` where rustc's error for it
/// depends on how the source names the item.
fn sourced_builder(error: &resolve::Error, source: &Source, primary: Span) -> Builder {
    let resolve::Error::UnknownMember { owner, member, .. } = error else {
        return resolution_builder(error);
    };
    let builder = match member_kind(source, primary, member) {
        Member::Field => Report::error(
            code::RESOLVE_NO_FIELD,
            format!("no field `{member}` on type `{owner}`"),
        ),
        Member::Method => Report::error(
            code::RESOLVE_NO_METHOD,
            format!("no method named `{member}` found for `{owner}` in the current scope"),
        ),
        Member::Associated => {
            let item = if member.starts_with(char::is_uppercase) {
                "variant or associated item"
            } else {
                "function or associated item"
            };
            Report::error(
                code::RESOLVE_NO_METHOD,
                format!("no {item} named `{member}` found for `{owner}` in the current scope"),
            )
        }
    };
    match resolution_builder(error).finish().help() {
        Some(help) => builder.help(help),
        None => builder,
    }
}

/// How source names a member: `value.member()`, `value.member` or
/// `Type::member`.
enum Member {
    Method,
    Field,
    Associated,
}

fn member_kind(source: &Source, at: Span, member: &str) -> Member {
    let start = at.start();
    let (Some(line), Some(column)) = (start.line(), start.position()) else {
        return Member::Method;
    };
    let Some(text) = source.text().split('\n').nth(line.saturating_sub(1)) else {
        return Member::Method;
    };
    let characters: Vec<char> = text.chars().collect();
    let column = column.saturating_sub(1).min(characters.len());
    let before: String = characters[..column].iter().collect();
    if before.trim_end().ends_with("::") {
        return Member::Associated;
    }
    let end = (column + member.chars().count()).min(characters.len());
    let after: String = characters[end..].iter().collect();
    let after = after.trim_start();
    if after.starts_with('(') || after.starts_with("::") {
        Member::Method
    } else {
        Member::Field
    }
}

fn resolution_report_with_sources(
    source: Source,
    secondary_source: Option<Source>,
    error: &resolve::Error,
) -> Result<Report, Error> {
    let builder = sourced_builder(error, &source, error.primary_span());
    let secondary_message = error.secondary_span().map(|_| match error {
        resolve::Error::MovedValue { .. } => "value moved here",
        _ => "first declaration or related definition is here",
    });
    let mut builder = builder.source(source.clone());
    if !error.primary_span().is_none() {
        builder = builder.primary(source.clone(), error.primary_span(), None)?;
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

fn resolution_builder(error: &resolve::Error) -> Builder {
    let stable_code = match error.category() {
        resolve::Category::DuplicateItem => code::RESOLVE_DUPLICATE_ITEM,
        resolve::Category::UnknownType => code::RESOLVE_UNKNOWN_TYPE,
        resolve::Category::UnknownImport => code::RESOLVE_UNKNOWN_IMPORT,
        resolve::Category::InvalidImport => code::RESOLVE_INVALID_IMPORT,
        resolve::Category::InvalidTypeArguments => code::RESOLVE_TYPE_ARGUMENTS,
        resolve::Category::LoweringUnavailable => code::RESOLVE_LOWERING,
        resolve::Category::InvalidDerive => code::RESOLVE_DERIVE,
        resolve::Category::DuplicateMember => code::RESOLVE_DUPLICATE_MEMBER,
        resolve::Category::UnknownMember => code::RESOLVE_UNKNOWN_MEMBER,
        resolve::Category::UnknownFunction => code::RESOLVE_UNKNOWN_FUNCTION,
        resolve::Category::UnknownVariable => code::RESOLVE_UNKNOWN_VARIABLE,
        resolve::Category::MovedValue => code::RESOLVE_MOVED_VALUE,
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
        resolve::Category::InvalidPreview => code::RESOLVE_PREVIEW,
        resolve::Category::InvalidWidget => code::RESOLVE_WIDGET,
        resolve::Category::InvalidApplication => unreachable!(),
    };
    let builder = Report::error(stable_code, resolution_message(error));
    match error {
        resolve::Error::DuplicateItem { .. }
        | resolve::Error::DuplicateMember { .. }
        | resolve::Error::DuplicateMethod { .. } => {
            builder.help("rename or remove one of the duplicate declarations")
        }
        resolve::Error::UnknownType { name, .. } if name.contains("::") => builder.help(
            format!("`{name}` isn't available in Rawr yet, or the path is misspelled"),
        ),
        resolve::Error::UnknownType { name, .. } => {
            builder.help(format!("declare `{name}`, or import it with `use`"))
        }
        resolve::Error::UnknownMember { owner, member, .. } if owner.contains("::") => {
            builder.help(format!(
                "`{owner}::{member}` isn't available in Rawr yet, or the name is misspelled"
            ))
        }
        resolve::Error::UnknownImport { path, .. } => builder.help(format!(
            "`{path}` isn't available in Rawr yet, or the path is misspelled"
        )),
        resolve::Error::UnknownFunction {
            import: Some(path), ..
        } => builder.help(format!("import it with `use {path};`")),
        resolve::Error::UnknownFunction { name, .. } => {
            builder.help(format!("declare `fn {name}`, or import it with `use`"))
        }
        resolve::Error::UnknownVariable { name, .. } => {
            builder.help(format!("declare `{name}` with `let`, or import it with `use`"))
        }
        resolve::Error::MovedValue { name, .. } => builder.help(format!(
            "`{name}` was moved before this use; clone it before the move to keep a copy"
        )),
        resolve::Error::MissingCapability {
            owner, capability, ..
        } => builder.help(format!("derive `{capability}` for `{owner}`")),
        _ => builder,
    }
}

fn application_report(source: Source, error: &application::Error) -> Result<Report, Error> {
    let builder = application_builder(error);
    let secondary_message = error
        .secondary_span()
        .map(|_| "first application entry or related declaration is here");
    attach(
        builder,
        source,
        error.primary_span(),
        error.secondary_span(),
        secondary_message,
    )
}

fn application_report_at(
    source: Source,
    error: &application::Error,
    primary: Span,
) -> Result<Report, Error> {
    Ok(application_builder(error)
        .source(source.clone())
        .primary(source, primary, None)?
        .finish())
}

fn application_builder(error: &application::Error) -> Builder {
    use application::Category as ApplicationCategory;

    let stable_code = match error.category() {
        ApplicationCategory::MissingEntry => code::FRAMEWORK_MISSING_ENTRY,
        ApplicationCategory::PrivateMain => code::FRAMEWORK_PRIVATE_MAIN,
        ApplicationCategory::InvalidMain => code::FRAMEWORK_INVALID_MAIN,
        ApplicationCategory::MultipleEntries => code::FRAMEWORK_MULTIPLE_ENTRIES,
        ApplicationCategory::ConditionalEntry => code::FRAMEWORK_CONDITIONAL_ENTRY,
        ApplicationCategory::NestedEntry => code::FRAMEWORK_NESTED_ENTRY,
        ApplicationCategory::HiddenEntry => code::FRAMEWORK_HIDDEN_ENTRY,
        ApplicationCategory::NonTailEntry => code::FRAMEWORK_NON_TAIL_ENTRY,
        ApplicationCategory::MalformedEntry => code::FRAMEWORK_MALFORMED_ENTRY,
        ApplicationCategory::NonSymbolicCallback => code::FRAMEWORK_NON_SYMBOLIC_CALLBACK,
        ApplicationCategory::UnknownCallback => code::FRAMEWORK_UNKNOWN_CALLBACK,
        ApplicationCategory::InvalidCallbackKind => code::FRAMEWORK_CALLBACK_KIND,
        ApplicationCategory::InvalidReceiver => code::FRAMEWORK_CALLBACK_RECEIVER,
        ApplicationCategory::InvalidArity => code::FRAMEWORK_CALLBACK_ARITY,
        ApplicationCategory::TypeMismatch => code::FRAMEWORK_TYPE_MISMATCH,
        ApplicationCategory::InvalidOutput => code::FRAMEWORK_INVALID_OUTPUT,
        ApplicationCategory::MissingDefault => code::FRAMEWORK_MISSING_DEFAULT,
        ApplicationCategory::MissingCapability => code::FRAMEWORK_CAPABILITY,
        ApplicationCategory::InvalidFacadeContract => code::FRAMEWORK_FACADE,
        ApplicationCategory::FacadeDependencyConflict => code::FRAMEWORK_FACADE_DEPENDENCY,
        ApplicationCategory::UnsupportedRuntimeType => code::FRAMEWORK_RUNTIME_TYPE,
    };
    let builder = Report::error(stable_code, application_message(error));
    match error {
        application::Error::MissingEntry => builder.help(
            "define one public `main` whose tail is `iced::run(...)` or `iced::application(...)`",
        ),
        application::Error::UnknownCallback { path, .. } => builder.help(format!(
            "define `{path}` or pass an existing symbolic callback"
        )),
        application::Error::MissingDefault { state, .. } => builder.help(format!(
            "derive `Default` for `{state}` or use an explicit boot callback"
        )),
        application::Error::MissingCapability {
            message,
            capability,
            ..
        } => builder.help(format!(
            "derive `{capability}` for application message `{message}`"
        )),
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
            let mut note = format!("in call to function `{function}`");
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
        resolve::Error::DuplicateMember { owner, member, .. } => {
            format!("duplicate member `{owner}::{member}`")
        }
        resolve::Error::UnknownMember { owner, member, .. } => {
            format!("unknown member `{owner}::{member}`")
        }
        resolve::Error::UnknownFunction { name, .. } => format!("unknown function `{name}`"),
        resolve::Error::UnknownVariable { name, .. } => format!("unknown variable `{name}`"),
        resolve::Error::MovedValue { name, .. } => format!("use of moved value: `{name}`"),
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
        } => format!("invalid call to `{owner}::{method}`: {reason}"),
        resolve::Error::InvalidHostOperation { item, reason, .. } => {
            format!("cannot use `{item}` here: {reason}")
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
        } => format!("invalid signature for `{owner}::{method}`: {reason}"),
        resolve::Error::MissingCapability {
            owner, capability, ..
        } => format!("`{owner}` does not provide `{capability}`"),
        resolve::Error::InvalidMatch { owner, reason, .. } => {
            format!("invalid match for `{owner}`: {reason}")
        }
        resolve::Error::UnsupportedNominalSyntax { construct, .. } => {
            format!("unsupported syntax: {construct}")
        }
        resolve::Error::Application(error) => application_message(error),
        resolve::Error::InvalidPreview {
            preview, reason, ..
        } => format!("invalid preview `{preview}`: {reason}"),
        resolve::Error::InvalidWidget { widget, reason, .. } => {
            format!("invalid widget protocol for `{widget}`: {reason}")
        }
    }
}

fn application_message(error: &application::Error) -> String {
    match error {
        application::Error::MissingEntry => "application compilation requires one public `main` tail entry using registered `iced` intrinsics".into(),
        application::Error::PrivateMain { .. } => {
            "application entry must be declared by `fn main() -> iced::Result` or `pub fn main`".into()
        }
        application::Error::InvalidMain { reason, .. } => {
            format!("invalid application `main`: {reason}")
        }
        application::Error::MultipleEntries { .. } => "multiple application entries".into(),
        application::Error::ConditionalEntry { .. } => {
            "application entry cannot be conditional".into()
        }
        application::Error::NestedEntry { .. } => {
            "application entry cannot be nested in another expression or block".into()
        }
        application::Error::HiddenEntry { function, .. } => {
            format!("application entry cannot be hidden in helper `{function}`")
        }
        application::Error::NonTailEntry { .. } => {
            "application entry must be the complete tail expression of `pub fn main`".into()
        }
        application::Error::MalformedEntry { reason, .. } => {
            format!("malformed application entry: {reason}")
        }
        application::Error::NonSymbolicCallback { role, .. } => {
            format!("application {role} callback must be a symbolic script item")
        }
        application::Error::UnknownCallback { role, path, .. } => {
            format!("unknown application {role} callback `{path}`")
        }
        application::Error::InvalidCallbackKind {
            role,
            callback,
            expected,
            actual,
            ..
        } => format!(
            "application {role} callback `{callback}` must be {expected}, found {actual:?}"
        ),
        application::Error::InvalidReceiver {
            role,
            callback,
            expected,
            actual,
            ..
        } => format!(
            "application {role} callback `{callback}` requires {expected}, found {actual:?}"
        ),
        application::Error::InvalidArity {
            role,
            callback,
            expected,
            actual,
            ..
        } => format!(
            "application {role} callback `{callback}` expects {expected} parameter(s), found {actual}"
        ),
        application::Error::TypeMismatch {
            role,
            expected,
            actual,
            ..
        } => format!("application {role} type must be `{expected}`, found `{actual}`"),
        application::Error::InvalidOutput {
            role,
            expected,
            actual,
            ..
        } => format!(
            "application {role} output must be {expected}, found `{actual}`"
        ),
        application::Error::MissingDefault { state, .. } => {
            format!("application state `{state}` does not provide `Default`")
        }
        application::Error::MissingCapability {
            message,
            capability,
            ..
        } => format!("application message `{message}` does not provide `{capability}`"),
        application::Error::InvalidFacadeContract { required, .. } => {
            format!("application requires canonical facade type `{required}`")
        }
        application::Error::FacadeDependencyConflict { root } => format!(
            "application dependency root `{root}` has conflicting exact facade fingerprints"
        ),
        application::Error::UnsupportedRuntimeType { role, ty, .. } => format!(
            "application {role} type `{ty}` reaches a structural tuple, which has no runtime value semantics"
        ),
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
}
