//! Restricted, intrinsic-backed application-root resolution.

use super::item;
use crate::{application, host, method, receiver, ImmutableString, Span};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Entry {
    form: application::Form,
    facade: host::facade::Dependency,
    root_intrinsic: host::item::Fingerprint,
    title_intrinsic: Option<host::item::Fingerprint>,
    theme_intrinsic: Option<host::item::Fingerprint>,
    subscription_intrinsic: Option<host::item::Fingerprint>,
    centered_intrinsic: Option<host::item::Fingerprint>,
    run_intrinsic: Option<host::item::Fingerprint>,
    boot: Option<crate::item::Key>,
    update: crate::item::Key,
    view: crate::item::Key,
    title: Option<crate::item::Key>,
    theme: Option<crate::item::Key>,
    subscription: Option<crate::item::Key>,
    centered: bool,
    task: Option<host::type_def::Contract>,
    theme_type: Option<host::type_def::Contract>,
    subscription_type: Option<host::type_def::Contract>,
    widget: Option<host::type_def::Contract>,
    span: Span,
}

impl Entry {
    pub(crate) const fn form(&self) -> application::Form {
        self.form
    }

    pub(crate) const fn dependency(&self) -> &host::facade::Dependency {
        &self.facade
    }

    pub(crate) const fn root_intrinsic(&self) -> &host::item::Fingerprint {
        &self.root_intrinsic
    }

    pub(crate) const fn run_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.run_intrinsic.as_ref()
    }

    pub(crate) const fn title_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.title_intrinsic.as_ref()
    }

    pub(crate) const fn theme_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.theme_intrinsic.as_ref()
    }

    pub(crate) const fn subscription_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.subscription_intrinsic.as_ref()
    }

    pub(crate) const fn centered_intrinsic(&self) -> Option<&host::item::Fingerprint> {
        self.centered_intrinsic.as_ref()
    }

    pub(crate) const fn boot(&self) -> Option<&crate::item::Key> {
        self.boot.as_ref()
    }

    pub(crate) const fn update(&self) -> &crate::item::Key {
        &self.update
    }

    pub(crate) const fn view(&self) -> &crate::item::Key {
        &self.view
    }

    pub(crate) const fn title(&self) -> Option<&crate::item::Key> {
        self.title.as_ref()
    }

    pub(crate) const fn theme(&self) -> Option<&crate::item::Key> {
        self.theme.as_ref()
    }

    pub(crate) const fn subscription(&self) -> Option<&crate::item::Key> {
        self.subscription.as_ref()
    }

    pub(crate) const fn centered(&self) -> bool {
        self.centered
    }

    pub(crate) const fn task(&self) -> Option<&host::type_def::Contract> {
        self.task.as_ref()
    }

    pub(crate) const fn theme_type(&self) -> Option<&host::type_def::Contract> {
        self.theme_type.as_ref()
    }

    pub(crate) const fn subscription_type(&self) -> Option<&host::type_def::Contract> {
        self.subscription_type.as_ref()
    }

    pub(crate) const fn widget(&self) -> Option<&host::type_def::Contract> {
        self.widget.as_ref()
    }

    pub(crate) const fn span(&self) -> Span {
        self.span
    }
}

struct Contracts<'a> {
    facade: host::facade::Dependency,
    run: Option<&'a host::item::Definition>,
    application: Option<&'a host::item::Definition>,
    application_title: Option<&'a host::item::Definition>,
    application_theme: Option<&'a host::item::Definition>,
    application_subscription: Option<&'a host::item::Definition>,
    application_centered: Option<&'a host::item::Definition>,
    application_run: Option<&'a host::item::Definition>,
    task: Option<host::type_def::Contract>,
    theme: Option<host::type_def::Contract>,
    subscription: Option<host::type_def::Contract>,
    widget: Option<host::type_def::Contract>,
}

#[derive(Clone, Copy)]
enum AttemptKind {
    Run,
    Application,
}

struct Attempt<'a> {
    name: &'a crate::item::Name,
    /// A legacy `fn main() { .. }`: Rust's `fn main() -> iced::Result` and
    /// `pub fn main` are items.
    legacy: bool,
    parameter_count: usize,
    has_receiver: bool,
    syntax: &'a crate::item::BodySyntax,
    top_level: bool,
    index: usize,
    kind: AttemptKind,
    span: Span,
}

#[derive(Clone)]
struct Callback {
    segments: Box<[ImmutableString]>,
    span: Span,
    /// `|| value`: the span of `value`, a closure of no parameters that
    /// only a boot may be.
    closure: Option<Span>,
}

struct Tail {
    title: Option<Callback>,
    theme: Option<Callback>,
    subscription: Option<Callback>,
    centered: bool,
}

/// The application entry; a boot closure (`iced::application(|| false, ..)`)
/// joins `items` as the function it is.
pub(super) fn resolve(
    items: &mut Vec<item::Shell>,
    methods: &[method::Definition],
    registry: &host::Registry,
    source: Option<crate::program::source::Id>,
) -> Result<Option<Entry>, application::Error> {
    let Some((entry, boot)) = resolve_entry(items, methods, registry, source)? else {
        return Ok(None);
    };
    items.extend(boot);
    closure_view_output(items, methods, registry, &entry);
    Ok(Some(entry))
}

/// A `view` closure returns `impl Widget<Message>` of the Message its
/// application's `update` takes, as Rust infers its return type.
fn closure_view_output(
    items: &mut [item::Shell],
    methods: &[method::Definition],
    registry: &host::Registry,
    entry: &Entry,
) {
    let message = items
        .iter()
        .find(|shell| shell.key.as_ref() == Some(&entry.update))
        .and_then(|shell| match &shell.kind {
            item::Kind::Function(function) => function.parameters.get(1),
            _ => None,
        })
        .or_else(|| {
            methods
                .iter()
                .find(|method| method.key() == &entry.update)
                .and_then(|method| method.parameters().first())
        })
        .map(|parameter| parameter.ty.clone());
    let element = "iced::Element";
    let (Some(message), Some(definition)) = (message, registry.type_(element)) else {
        return;
    };
    for shell in items.iter_mut() {
        if shell.key.as_ref() != Some(&entry.view) {
            continue;
        }
        let item::Kind::Function(function) = &mut shell.kind else {
            continue;
        };
        if function.output.is_none()
            && function
                .name
                .text
                .starts_with(crate::parser::CALLBACK_CLOSURE_PREFIX)
        {
            function.output = Some(super::type_ref::Type::Path(super::type_ref::Path {
                name: element.into(),
                target: super::type_ref::Target::Host {
                    definition: definition.clone(),
                    owner: registry.type_owner(element).cloned(),
                },
                arguments: Box::new([super::type_ref::Argument::Type(message.clone())]),
                span: function.body.span,
            }));
        }
    }
}

fn resolve_entry(
    items: &[item::Shell],
    methods: &[method::Definition],
    registry: &host::Registry,
    source: Option<crate::program::source::Id>,
) -> Result<Option<(Entry, Option<item::Shell>)>, application::Error> {
    let Some(contracts) = Contracts::from_registry(registry) else {
        return Ok(None);
    };
    let attempts = collect_attempts(items, &contracts, source);

    if attempts.is_empty() {
        return Ok(None);
    }
    if attempts.len() > 1 {
        return Err(application::Error::MultipleEntries {
            first: attempts[0].span,
            additional: attempts[1].span,
        });
    }

    let attempt = &attempts[0];
    if !attempt.top_level || attempt.name.text != "main" {
        return Err(application::Error::HiddenEntry {
            function: attempt.name.text.clone(),
            declaration: attempt.name.span,
            entry: attempt.span,
        });
    }
    if attempt.legacy {
        return Err(application::Error::PrivateMain {
            declaration: attempt.name.span,
            entry: attempt.span,
        });
    }
    if attempt.has_receiver || attempt.parameter_count > 0 {
        return Err(application::Error::InvalidMain {
            reason: "application `main` cannot declare a receiver or parameters".into(),
            declaration: attempt.name.span,
        });
    }

    resolve_main(attempt, &contracts, items, methods, source)
}

impl<'a> Contracts<'a> {
    fn from_registry(registry: &'a host::Registry) -> Option<Self> {
        let run = registry
            .item("iced::run")
            .filter(|item| item.kind() == host::item::Kind::IntrinsicFunction);
        let application = registry
            .item("iced::application")
            .filter(|item| item.kind() == host::item::Kind::IntrinsicFunction);

        if run.is_none() && application.is_none() {
            return None;
        }

        let facade_item = run
            .or(application)
            .expect("one root intrinsic was resolved");
        let facade = registry
            .item_owner(&facade_item.path())
            .cloned()
            .expect("static intrinsic items retain their installed facade owner proof");

        Some(Self {
            facade,
            run,
            application,
            application_run: registry.item("iced::Application::run").filter(|item| {
                item.kind() == host::item::Kind::IntrinsicMethod
                    && item.receiver() == Some(receiver::Mode::Owned)
                    && item.signature().parameters().is_empty()
            }),
            application_title: registry.item("iced::Application::title").filter(|item| {
                item.kind() == host::item::Kind::IntrinsicMethod
                    && item.receiver() == Some(receiver::Mode::Owned)
                    && item.signature().parameters().len() == 1
            }),
            application_theme: registry.item("iced::Application::theme").filter(|item| {
                item.kind() == host::item::Kind::IntrinsicMethod
                    && item.receiver() == Some(receiver::Mode::Owned)
                    && item.signature().parameters().len() == 1
            }),
            application_subscription: registry.item("iced::Application::subscription").filter(
                |item| {
                    item.kind() == host::item::Kind::IntrinsicMethod
                        && item.receiver() == Some(receiver::Mode::Owned)
                        && item.signature().parameters().len() == 1
                },
            ),
            application_centered: registry.item("iced::Application::centered").filter(|item| {
                item.kind() == host::item::Kind::IntrinsicMethod
                    && item.receiver() == Some(receiver::Mode::Owned)
                    && item.signature().parameters().is_empty()
            }),
            task: registry
                .type_("iced::Task")
                .filter(|definition| definition.parameters().len() == 1)
                .map(host::type_def::Definition::contract),
            theme: registry
                .type_("iced::Theme")
                .filter(|definition| definition.parameters().is_empty())
                .map(host::type_def::Definition::contract),
            subscription: registry
                .type_("iced::Subscription")
                .filter(|definition| definition.parameters().len() == 1)
                .map(host::type_def::Definition::contract),
            widget: registry
                .type_("iced::Element")
                .filter(|definition| definition.parameters().len() == 1)
                .map(host::type_def::Definition::contract),
        })
    }

    fn attempt(&self, tokens: &[crate::item::BodyToken], index: usize) -> Option<AttemptKind> {
        if !is_identifier(tokens.get(index), "iced")
            || !is_kind(
                tokens.get(index + 1),
                &crate::item::BodyTokenKind::DoubleColon,
            )
        {
            return None;
        }

        if is_identifier(tokens.get(index + 2), "run") && self.run.is_some() {
            Some(AttemptKind::Run)
        } else if is_identifier(tokens.get(index + 2), "application") && self.application.is_some()
        {
            Some(AttemptKind::Application)
        } else {
            None
        }
    }
}

fn collect_attempts<'a>(
    items: &'a [item::Shell],
    contracts: &Contracts<'_>,
    source: Option<crate::program::source::Id>,
) -> Vec<Attempt<'a>> {
    let mut attempts = Vec::new();

    for shell in items {
        if source.is_some_and(|source| shell.source != source) {
            continue;
        }
        match &shell.kind {
            item::Kind::Function(function) => {
                collect_function_attempts(
                    &function.name,
                    false,
                    function.parameters.len(),
                    function.receiver.is_some(),
                    &function.body.syntax,
                    true,
                    contracts,
                    &mut attempts,
                );
            }
            item::Kind::LegacyFunction(function) => {
                collect_function_attempts(
                    &function.name,
                    true,
                    function.parameter_count,
                    false,
                    &function.body.syntax,
                    true,
                    contracts,
                    &mut attempts,
                );
            }
            item::Kind::Impl(item) => {
                for function in &item.functions {
                    collect_function_attempts(
                        &function.name,
                        false,
                        function.parameters.len(),
                        function.receiver.is_some(),
                        &function.body.syntax,
                        false,
                        contracts,
                        &mut attempts,
                    );
                }
            }
            item::Kind::Use(_)
            | item::Kind::Trait(_)
            | item::Kind::Struct(_)
            | item::Kind::Enum(_) => {}
        }
    }

    attempts
}

fn collect_function_attempts<'a>(
    name: &'a crate::item::Name,
    legacy: bool,
    parameter_count: usize,
    has_receiver: bool,
    syntax: &'a crate::item::BodySyntax,
    top_level: bool,
    contracts: &Contracts<'_>,
    attempts: &mut Vec<Attempt<'a>>,
) {
    let tokens = syntax.tokens();
    for index in 0..tokens.len() {
        let Some(kind) = contracts.attempt(tokens, index) else {
            continue;
        };
        let end = tokens
            .get(index + 2)
            .map(crate::item::BodyToken::span)
            .unwrap_or_else(|| tokens[index].span());
        attempts.push(Attempt {
            name,
            legacy,
            parameter_count,
            has_receiver,
            syntax,
            top_level,
            index,
            kind,
            span: Span::new(tokens[index].span().start(), end.end()),
        });
    }
}

fn resolve_main(
    attempt: &Attempt<'_>,
    contracts: &Contracts<'_>,
    items: &[item::Shell],
    methods: &[method::Definition],
    source: Option<crate::program::source::Id>,
) -> Result<Option<(Entry, Option<item::Shell>)>, application::Error> {
    let tokens = attempt.syntax.tokens();
    let mut boot_closure = None;

    if tokens
        .iter()
        .any(|token| matches!(token.kind(), crate::item::BodyTokenKind::If))
    {
        return Err(application::Error::ConditionalEntry {
            entry: attempt.span,
        });
    }
    if attempt.index != 0 {
        return Err(if delimiter_depth(tokens, attempt.index) > 0 {
            application::Error::NestedEntry {
                entry: attempt.span,
            }
        } else {
            application::Error::NonTailEntry {
                entry: attempt.span,
            }
        });
    }

    let mut cursor = Cursor::new(tokens, attempt.span);
    cursor.consume_intrinsic()?;
    let result = match attempt.kind {
        AttemptKind::Run => {
            let callbacks = cursor.callbacks(&["update", "view"])?;
            let update = resolve_callback(&callbacks[0], "update", items, methods, source)?;
            let view = resolve_callback(&callbacks[1], "view", items, methods, source)?;
            // `iced::run` boots from `State::default()`: a hand-written
            // `impl Default` runs; a derived one has no method item.
            let boot = match callbacks[0].segments.as_ref() {
                [owner, _] => resolve_callback(
                    &Callback {
                        segments: Box::new([owner.clone(), "default".into()]),
                        span: callbacks[0].span,
                        closure: None,
                    },
                    "boot",
                    items,
                    methods,
                    source,
                )
                .ok(),
                _ => None,
            };
            Entry {
                form: application::Form::Run,
                facade: contracts.facade.clone(),
                root_intrinsic: contracts
                    .run
                    .expect("run attempts retain their intrinsic contract")
                    .fingerprint(),
                title_intrinsic: None,
                theme_intrinsic: None,
                subscription_intrinsic: None,
                centered_intrinsic: None,
                run_intrinsic: None,
                boot,
                update,
                view,
                title: None,
                theme: None,
                subscription: None,
                centered: false,
                task: contracts.task.clone(),
                theme_type: contracts.theme.clone(),
                subscription_type: contracts.subscription.clone(),
                widget: contracts.widget.clone(),
                span: Span::new(attempt.span.start(), cursor.last_end()),
            }
        }
        AttemptKind::Application => {
            let callbacks = cursor.callbacks(&["boot", "update", "view"])?;
            let tail = cursor.application_tail(
                contracts.application_title,
                contracts.application_theme,
                contracts.application_subscription,
                contracts.application_centered,
                contracts.application_run,
            )?;
            let update = resolve_callback(&callbacks[1], "update", items, methods, source)?;
            // `State::default` from a derived `Default` has no method item:
            // boot from `Default` as `iced::run` does.
            let boot = match callbacks[0].closure {
                Some(body) => {
                    let shell = boot_function(attempt, body, &update, items)?;
                    let key = shell.key.clone();
                    boot_closure = Some(shell);
                    key
                }
                None => match resolve_callback(&callbacks[0], "boot", items, methods, source) {
                Ok(boot) => Some(boot),
                Err(_) if matches!(callbacks[0].segments.as_ref(), [_, name] if name.as_str() == "default") => {
                    None
                }
                Err(error) => return Err(error),
                },
            };
            let view = resolve_callback(&callbacks[2], "view", items, methods, source)?;
            let title = tail
                .title
                .as_ref()
                .map(|callback| resolve_callback(callback, "title", items, methods, source))
                .transpose()?;
            let theme = tail
                .theme
                .as_ref()
                .map(|callback| resolve_callback(callback, "theme", items, methods, source))
                .transpose()?;
            let subscription = tail
                .subscription
                .as_ref()
                .map(|callback| resolve_callback(callback, "subscription", items, methods, source))
                .transpose()?;
            Entry {
                form: application::Form::Application,
                facade: contracts.facade.clone(),
                root_intrinsic: contracts
                    .application
                    .expect("application attempts retain their intrinsic contract")
                    .fingerprint(),
                title_intrinsic: tail.title.map(|_| {
                    contracts
                        .application_title
                        .expect("accepted title chains retain their intrinsic contract")
                        .fingerprint()
                }),
                theme_intrinsic: tail.theme.map(|_| {
                    contracts
                        .application_theme
                        .expect("accepted theme chains retain their intrinsic contract")
                        .fingerprint()
                }),
                subscription_intrinsic: tail.subscription.map(|_| {
                    contracts
                        .application_subscription
                        .expect("accepted subscription chains retain their intrinsic contract")
                        .fingerprint()
                }),
                centered_intrinsic: tail.centered.then(|| {
                    contracts
                        .application_centered
                        .expect("accepted centered chains retain their intrinsic contract")
                        .fingerprint()
                }),
                run_intrinsic: Some(
                    contracts
                        .application_run
                        .expect("accepted application chains retain their run contract")
                        .fingerprint(),
                ),
                boot,
                update,
                view,
                title,
                theme,
                subscription,
                centered: tail.centered,
                task: contracts.task.clone(),
                theme_type: contracts.theme.clone(),
                subscription_type: contracts.subscription.clone(),
                widget: contracts.widget.clone(),
                span: Span::new(attempt.span.start(), cursor.last_end()),
            }
        }
    };

    if !cursor.is_end() {
        return Err(application::Error::NonTailEntry {
            entry: attempt.span,
        });
    }
    Ok(Some((result, boot_closure)))
}

/// `|| value` as a boot: a function of no parameters returning `value`, of
/// the State the free `update` function takes as `&mut`, beside `main`.
fn boot_function(
    attempt: &Attempt<'_>,
    body: Span,
    update: &crate::item::Key,
    items: &[item::Shell],
) -> Result<item::Shell, application::Error> {
    let non_symbolic = || application::Error::NonSymbolicCallback {
        role: "boot".into(),
        callback: body,
    };
    let function = |shell: &item::Shell| match &shell.kind {
        item::Kind::Function(function) => Some(function.clone()),
        _ => None,
    };
    // `fn main` is a private legacy function in a script.
    let main = items
        .iter()
        .find(|shell| match &shell.kind {
            item::Kind::Function(function) => function.name.span == attempt.name.span,
            item::Kind::LegacyFunction(function) => function.name.span == attempt.name.span,
            _ => false,
        })
        .ok_or_else(non_symbolic)?;
    let state = items
        .iter()
        .filter(|shell| shell.key.as_ref() == Some(update))
        .find_map(function)
        .and_then(|update| match update.parameters.first().map(|parameter| &parameter.ty) {
            Some(super::type_ref::Type::Reference { target, .. }) => Some((**target).clone()),
            _ => None,
        })
        .ok_or_else(non_symbolic)?;
    let name = crate::item::Name {
        text: "__rawr_boot".into(),
        span: body,
    };
    Ok(item::Shell {
        id: crate::item::Id::from_index(items.len()),
        source: main.source,
        key: Some(crate::item::Key::new(
            update.package().into(),
            update.modules().into(),
            name.text.clone(),
            crate::item::Kind::Function,
        )),
        span: body,
        kind: item::Kind::Function(item::Function {
            header: crate::item::Header {
                attributes: Box::default(),
                visibility: None,
                span: body,
            },
            name,
            generics: Box::default(),
            receiver: None,
            parameters: Box::default(),
            output: Some(state),
            // The closure's value is the body, as a `const` initializer is.
            body: item::Body {
                span: body,
                lowering: item::Lowering::NominalFunction,
                syntax: crate::item::BodySyntax::new(Vec::new()),
                constant: true,
            },
        }),
    })
}

fn resolve_callback(
    callback: &Callback,
    role: &str,
    items: &[item::Shell],
    methods: &[method::Definition],
    source: Option<crate::program::source::Id>,
) -> Result<crate::item::Key, application::Error> {
    let resolved = match callback.segments.as_ref() {
        [name] => items
            .iter()
            .filter(|shell| source.is_none_or(|source| shell.source == source))
            .find_map(|shell| match &shell.kind {
                item::Kind::Function(function)
                    if function.name.text == *name && function.name.text != "main" =>
                {
                    shell.key.clone()
                }
                item::Kind::Use(item) => item.bindings.iter().find_map(|binding| {
                    (binding.name.text == *name)
                        .then(|| match &binding.target {
                            item::Import::ScriptItem { key } => Some(key.clone()),
                            _ => None,
                        })
                        .flatten()
                }),
                item::Kind::Struct(_)
                | item::Kind::Enum(_)
                | item::Kind::Impl(_)
                | item::Kind::Trait(_)
                | item::Kind::LegacyFunction(_)
                | item::Kind::Function(_) => None,
            }),
        [owner, name] => {
            let module = owner;
            let owner = items
                .iter()
                .filter(|shell| source.is_none_or(|source| shell.source == source))
                .find_map(|shell| match &shell.kind {
                    item::Kind::Struct(item) if item.name.text == *owner => Some(item.definition),
                    item::Kind::Enum(item) if item.name.text == *owner => Some(item.definition),
                    item::Kind::Use(item) => item.bindings.iter().find_map(|binding| {
                        (binding.name.text == *owner)
                            .then_some(match binding.target {
                                item::Import::ScriptType { definition, .. } => Some(definition),
                                _ => None,
                            })
                            .flatten()
                    }),
                    item::Kind::Impl(_)
                    | item::Kind::Trait(_)
                    | item::Kind::Function(_)
                    | item::Kind::LegacyFunction(_)
                    | item::Kind::Struct(_)
                    | item::Kind::Enum(_) => None,
                });
            match owner {
                Some(owner) => methods
                    .iter()
                    .find(|method| {
                        method.owner() == owner
                            && method.name() == name.as_str()
                            && (source.is_none_or(|source| method.source() == source)
                                || method.is_public())
                    })
                    .map(|method| method.key().clone()),
                // `view::view`: a function of the crate's module `view`.
                None => items.iter().find_map(|shell| match &shell.kind {
                    item::Kind::Function(function)
                        if function.name.text == *name
                            && shell
                                .key
                                .as_ref()
                                .is_some_and(|key| key.modules().last() == Some(module)) =>
                    {
                        shell.key.clone()
                    }
                    _ => None,
                }),
            }
        }
        _ => None,
    };

    resolved.ok_or_else(|| application::Error::UnknownCallback {
        role: role.into(),
        path: callback.segments.join("::").into(),
        callback: callback.span,
    })
}

fn delimiter_depth(tokens: &[crate::item::BodyToken], end: usize) -> usize {
    let mut depth = 0_usize;
    for token in &tokens[..end] {
        match token.kind() {
            crate::item::BodyTokenKind::LeftParen
            | crate::item::BodyTokenKind::LeftBrace
            | crate::item::BodyTokenKind::LeftBracket => depth += 1,
            crate::item::BodyTokenKind::RightParen
            | crate::item::BodyTokenKind::RightBrace
            | crate::item::BodyTokenKind::RightBracket => depth = depth.saturating_sub(1),
            crate::item::BodyTokenKind::Identifier(_)
            | crate::item::BodyTokenKind::DoubleColon
            | crate::item::BodyTokenKind::DoubleArrow
            | crate::item::BodyTokenKind::Match
            | crate::item::BodyTokenKind::Unit
            | crate::item::BodyTokenKind::Comma
            | crate::item::BodyTokenKind::Dot
            | crate::item::BodyTokenKind::Semicolon
            | crate::item::BodyTokenKind::If
            | crate::item::BodyTokenKind::OrOr
            | crate::item::BodyTokenKind::Integer(_)
            | crate::item::BodyTokenKind::Bang
            | crate::item::BodyTokenKind::Other => {}
        }
    }
    depth
}

struct Cursor<'a> {
    tokens: &'a [crate::item::BodyToken],
    index: usize,
    entry: Span,
    last_end: crate::Position,
}

impl<'a> Cursor<'a> {
    fn new(tokens: &'a [crate::item::BodyToken], entry: Span) -> Self {
        Self {
            tokens,
            index: 0,
            entry,
            last_end: entry.end(),
        }
    }

    fn consume_intrinsic(&mut self) -> Result<(), application::Error> {
        self.consume();
        self.consume();
        self.consume();
        Ok(())
    }

    fn callbacks(&mut self, roles: &[&str]) -> Result<Vec<Callback>, application::Error> {
        if !self.consume_kind(&crate::item::BodyTokenKind::LeftParen) {
            return Err(self.malformed(format!(
                "intrinsic expects {} callback argument(s)",
                roles.len()
            )));
        }
        let mut callbacks = Vec::with_capacity(roles.len());

        for (index, role) in roles.iter().enumerate() {
            callbacks.push(self.callback(role)?);
            if index + 1 < roles.len() && !self.consume_kind(&crate::item::BodyTokenKind::Comma) {
                return Err(self.malformed(format!(
                    "intrinsic expects {} callback argument(s)",
                    roles.len()
                )));
            }
        }

        self.consume_kind(&crate::item::BodyTokenKind::Comma);
        if !self.consume_kind(&crate::item::BodyTokenKind::RightParen) {
            return Err(self.malformed(format!(
                "intrinsic expects {} callback argument(s)",
                roles.len()
            )));
        }
        Ok(callbacks)
    }

    fn callback(&mut self, role: &str) -> Result<Callback, application::Error> {
        let Some(first) = self.peek() else {
            return Err(application::Error::NonSymbolicCallback {
                role: role.into(),
                callback: self.entry,
            });
        };
        if role == "boot" && first.kind() == &crate::item::BodyTokenKind::OrOr {
            return self.closure(role);
        }
        let crate::item::BodyTokenKind::Identifier(first_name) = first.kind() else {
            return Err(application::Error::NonSymbolicCallback {
                role: role.into(),
                callback: first.span(),
            });
        };
        let start = first.span().start();
        let mut segments = vec![first_name.clone()];
        let mut end = first.span().end();
        self.consume();

        if self.consume_kind(&crate::item::BodyTokenKind::DoubleColon) {
            let Some(second) = self.peek() else {
                return Err(application::Error::NonSymbolicCallback {
                    role: role.into(),
                    callback: Span::new(start, end),
                });
            };
            let crate::item::BodyTokenKind::Identifier(second_name) = second.kind() else {
                return Err(application::Error::NonSymbolicCallback {
                    role: role.into(),
                    callback: Span::new(start, second.span().end()),
                });
            };
            segments.push(second_name.clone());
            end = second.span().end();
            self.consume();
        }

        if !self.peek().is_some_and(|token| {
            matches!(
                token.kind(),
                crate::item::BodyTokenKind::Comma | crate::item::BodyTokenKind::RightParen
            )
        }) {
            return Err(application::Error::NonSymbolicCallback {
                role: role.into(),
                callback: Span::new(start, end),
            });
        }

        Ok(Callback {
            segments: segments.into_boxed_slice(),
            span: Span::new(start, end),
            closure: None,
        })
    }

    /// `|| value` up to the argument's end: its value's span.
    fn closure(&mut self, role: &str) -> Result<Callback, application::Error> {
        let start = self.peek().map_or(self.entry, |token| token.span());
        self.consume();
        let body = self.peek().map(|token| token.span().start());
        let mut depth = 0_usize;
        while let Some(token) = self.peek() {
            match token.kind() {
                crate::item::BodyTokenKind::LeftParen
                | crate::item::BodyTokenKind::LeftBrace
                | crate::item::BodyTokenKind::LeftBracket => depth += 1,
                crate::item::BodyTokenKind::RightParen if depth == 0 => break,
                crate::item::BodyTokenKind::Comma if depth == 0 => break,
                crate::item::BodyTokenKind::RightParen
                | crate::item::BodyTokenKind::RightBrace
                | crate::item::BodyTokenKind::RightBracket => depth = depth.saturating_sub(1),
                _ => {}
            }
            self.consume();
        }
        let (Some(body), Some(end)) = (body, self.peek().map(|token| token.span().start())) else {
            return Err(application::Error::NonSymbolicCallback {
                role: role.into(),
                callback: start,
            });
        };
        Ok(Callback {
            segments: Box::default(),
            span: Span::new(start.start(), end),
            closure: Some(Span::new(body, end)),
        })
    }

    fn application_tail(
        &mut self,
        title: Option<&host::item::Definition>,
        theme: Option<&host::item::Definition>,
        subscription: Option<&host::item::Definition>,
        centered: Option<&host::item::Definition>,
        run: Option<&host::item::Definition>,
    ) -> Result<Tail, application::Error> {
        if run.is_none() {
            return Err(self.malformed(
                "registered facade lacks intrinsic owned `iced::Application::run`".to_string(),
            ));
        }
        let mut tail = Tail {
            title: None,
            theme: None,
            subscription: None,
            centered: false,
        };
        loop {
            let method = if is_kind(
                self.tokens.get(self.index),
                &crate::item::BodyTokenKind::Dot,
            ) {
                self.tokens.get(self.index + 1)
            } else {
                None
            };
            let Some(method) = method else {
                break;
            };
            let crate::item::BodyTokenKind::Identifier(name) = method.kind() else {
                break;
            };
            if name == "centered" {
                let operation =
                    Span::new(self.tokens[self.index].span().start(), method.span().end());
                self.consume();
                self.consume();
                if centered.is_none() {
                    return Err(application::Error::MalformedEntry {
                        reason:
                            "registered facade lacks intrinsic owned `iced::Application::centered`"
                                .into(),
                        entry: operation,
                    });
                }
                if tail.centered {
                    return Err(application::Error::MalformedEntry {
                        reason: "intrinsic `.centered()` may appear only once".into(),
                        entry: operation,
                    });
                }
                if !(self.consume_kind(&crate::item::BodyTokenKind::Unit)
                    || (self.consume_kind(&crate::item::BodyTokenKind::LeftParen)
                        && self.consume_kind(&crate::item::BodyTokenKind::RightParen)))
                {
                    return Err(application::Error::MalformedEntry {
                        reason: "intrinsic `.centered()` expects no arguments".into(),
                        entry: operation,
                    });
                }
                tail.centered = true;
                continue;
            }
            // iced's window and renderer settings (`.window_size(..)`,
            // `.font(..)`, `.style(..)`, ...) and a `.title("literal")`:
            // the preview window does not apply them, so they are accepted
            // and skipped.
            if WINDOW_SETTINGS.contains(&name.as_str())
                || (name == "title"
                    && !matches!(
                        self.tokens.get(self.index + 3).map(crate::item::BodyToken::kind),
                        Some(crate::item::BodyTokenKind::Identifier(_))
                    ))
            {
                let operation =
                    Span::new(self.tokens[self.index].span().start(), method.span().end());
                self.consume();
                self.consume();
                self.skip_argument_list().ok_or_else(|| application::Error::MalformedEntry {
                    reason: format!("`.{name}(..)` expects its arguments in parentheses").into(),
                    entry: operation,
                })?;
                continue;
            }
            let (role, contract, slot) = match name.as_str() {
                "title" => ("title", title, &mut tail.title),
                "theme" => ("theme", theme, &mut tail.theme),
                "subscription" => ("subscription", subscription, &mut tail.subscription),
                "run" => break,
                _ => break,
            };
            let operation = Span::new(self.tokens[self.index].span().start(), method.span().end());
            self.consume();
            self.consume();
            if contract.is_none() {
                return Err(application::Error::MalformedEntry {
                    reason: format!(
                        "registered facade lacks intrinsic owned `iced::Application::{role}`"
                    )
                    .into(),
                    entry: operation,
                });
            }
            if slot.is_some() {
                return Err(application::Error::MalformedEntry {
                    reason: format!("intrinsic `.{role}` may appear only once").into(),
                    entry: operation,
                });
            }
            if !self.consume_kind(&crate::item::BodyTokenKind::LeftParen) {
                return Err(application::Error::MalformedEntry {
                    reason: format!("intrinsic `.{role}` expects one symbolic callback").into(),
                    entry: operation,
                });
            }
            let callback = self.callback(role)?;
            if !self.consume_kind(&crate::item::BodyTokenKind::RightParen) {
                return Err(application::Error::MalformedEntry {
                    reason: format!("intrinsic `.{role}` expects one symbolic callback").into(),
                    entry: operation,
                });
            }
            *slot = Some(callback);
        }
        if !self.consume_kind(&crate::item::BodyTokenKind::Dot)
            || !self.consume_identifier("run")
            || !(self.consume_kind(&crate::item::BodyTokenKind::Unit)
                || (self.consume_kind(&crate::item::BodyTokenKind::LeftParen)
                    && self.consume_kind(&crate::item::BodyTokenKind::RightParen)))
        {
            return Err(self.malformed(
                "`iced::application` must end with intrinsic `.run()` after optional `.title(callback)`, `.theme(callback)`, `.subscription(callback)`, and `.centered()` builders"
                    .to_string(),
            ));
        }
        Ok(tail)
    }

    /// Skip one balanced `(..)` argument list.
    fn skip_argument_list(&mut self) -> Option<()> {
        if self.consume_kind(&crate::item::BodyTokenKind::Unit) {
            return Some(());
        }
        if !self.consume_kind(&crate::item::BodyTokenKind::LeftParen) {
            return None;
        }
        let mut depth = 0_usize;
        loop {
            let token = self.peek()?;
            match token.kind() {
                crate::item::BodyTokenKind::LeftParen
                | crate::item::BodyTokenKind::LeftBrace
                | crate::item::BodyTokenKind::LeftBracket => depth += 1,
                crate::item::BodyTokenKind::RightParen if depth == 0 => {
                    self.consume();
                    return Some(());
                }
                crate::item::BodyTokenKind::RightParen
                | crate::item::BodyTokenKind::RightBrace
                | crate::item::BodyTokenKind::RightBracket => depth = depth.saturating_sub(1),
                _ => {}
            }
            self.consume();
        }
    }

    fn consume_identifier(&mut self, expected: &str) -> bool {
        if is_identifier(self.peek(), expected) {
            self.consume();
            true
        } else {
            false
        }
    }

    fn consume_kind(&mut self, expected: &crate::item::BodyTokenKind) -> bool {
        if is_kind(self.peek(), expected) {
            self.consume();
            true
        } else {
            false
        }
    }

    fn consume(&mut self) {
        if let Some(token) = self.tokens.get(self.index) {
            self.last_end = token.span().end();
            self.index += 1;
        }
    }

    fn peek(&self) -> Option<&'a crate::item::BodyToken> {
        self.tokens.get(self.index)
    }

    const fn last_end(&self) -> crate::Position {
        self.last_end
    }

    fn is_end(&self) -> bool {
        self.index == self.tokens.len()
    }

    fn malformed(&self, reason: String) -> application::Error {
        application::Error::MalformedEntry {
            reason: reason.into(),
            entry: self.entry,
        }
    }
}

/// iced's application builders that set window or renderer settings.
const WINDOW_SETTINGS: &[&str] = &[
    "antialiasing",
    "decorations",
    "default_font",
    "executor",
    "exit_on_close_request",
    "font",
    "fonts",
    "level",
    "position",
    "resizable",
    "scale_factor",
    "settings",
    "style",
    "transparent",
    "window",
    "window_size",
];

fn is_identifier(token: Option<&crate::item::BodyToken>, expected: &str) -> bool {
    matches!(
        token.map(crate::item::BodyToken::kind),
        Some(crate::item::BodyTokenKind::Identifier(name)) if name == expected
    )
}

fn is_kind(token: Option<&crate::item::BodyToken>, expected: &crate::item::BodyTokenKind) -> bool {
    token.is_some_and(|token| token.kind() == expected)
}
