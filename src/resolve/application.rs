//! Restricted, intrinsic-backed application-root resolution.

use super::item;
use crate::{application, host, method, receiver, ImmutableString, Span};

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
    /// `main`'s statements before the entry, which run once.
    prelude: Option<crate::item::Key>,
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
    /// `iced::time::Instant`, the third parameter of a timed update.
    instant: Option<host::type_def::Contract>,
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

    pub(crate) const fn prelude(&self) -> Option<&crate::item::Key> {
        self.prelude.as_ref()
    }

    /// The span of `main`'s statements before the entry.
    pub(crate) fn prelude_span(&self, items: &[item::Shell]) -> Option<Span> {
        let key = self.prelude.as_ref()?;
        items.iter().find_map(|shell| match &shell.kind {
            item::Kind::Function(function) if shell.key.as_ref() == Some(key) => Some(Span::new(
                function.body.span.start(),
                function.body.elide?.start(),
            )),
            _ => None,
        })
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

    pub(crate) const fn instant(&self) -> Option<&host::type_def::Contract> {
        self.instant.as_ref()
    }

    pub(crate) const fn span(&self) -> Span {
        self.span
    }
}

struct Contracts<'a> {
    facade: host::facade::Dependency,
    run: Option<&'a host::item::Definition>,
    application: Option<&'a host::item::Definition>,
    /// `iced::application::timed`, whose update also takes the message's
    /// `Instant`.
    timed: Option<&'a host::item::Definition>,
    application_title: Option<&'a host::item::Definition>,
    application_theme: Option<&'a host::item::Definition>,
    application_subscription: Option<&'a host::item::Definition>,
    application_centered: Option<&'a host::item::Definition>,
    application_run: Option<&'a host::item::Definition>,
    task: Option<host::type_def::Contract>,
    theme: Option<host::type_def::Contract>,
    subscription: Option<host::type_def::Contract>,
    widget: Option<host::type_def::Contract>,
    instant: Option<host::type_def::Contract>,
    registry: &'a host::Registry,
}

#[derive(Clone, Copy)]
enum AttemptKind {
    Run,
    Application,
    /// `iced::application::timed(boot, update, subscription, view)`.
    Timed,
}

struct Attempt<'a> {
    name: &'a crate::item::Name,
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
    /// `()` as `iced::run`'s update: iced's update of a view whose Message
    /// is `Never`, which never runs.
    unit: bool,
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
) -> Result<Option<(Entry, Vec<item::Shell>)>, application::Error> {
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
            registry,
            facade,
            run,
            application,
            timed: registry
                .item("iced::application::timed")
                .filter(|item| item.kind() == host::item::Kind::IntrinsicFunction),
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
            instant: registry
                .type_("iced::time::Instant")
                .filter(|definition| definition.parameters().is_empty())
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

        let path = is_kind(
            tokens.get(index + 3),
            &crate::item::BodyTokenKind::DoubleColon,
        );
        if is_identifier(tokens.get(index + 2), "run") && self.run.is_some() {
            Some(AttemptKind::Run)
        } else if !is_identifier(tokens.get(index + 2), "application") {
            None
        } else if !path {
            self.application
                .is_some()
                .then_some(AttemptKind::Application)
        } else if is_identifier(tokens.get(index + 4), "timed") && self.timed.is_some() {
            Some(AttemptKind::Timed)
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
                    function.parameters.len(),
                    function.receiver.is_some(),
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
        let last = match kind {
            AttemptKind::Timed => index + 4,
            AttemptKind::Run | AttemptKind::Application => index + 2,
        };
        let end = tokens
            .get(last)
            .map(crate::item::BodyToken::span)
            .unwrap_or_else(|| tokens[index].span());
        attempts.push(Attempt {
            name,
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
) -> Result<Option<(Entry, Vec<item::Shell>)>, application::Error> {
    let tokens = attempt.syntax.tokens();
    let mut boot_closure = Vec::new();

    let nested = delimiter_depth(tokens, attempt.index) > 0;
    // Statements may come before the entry, as Rust's `main` runs them
    // first. Each one ends where the entry starts.
    let after = if nested { 0 } else { attempt.index };
    if tokens[after..]
        .iter()
        .any(|token| matches!(token.kind(), crate::item::BodyTokenKind::If))
    {
        return Err(application::Error::ConditionalEntry {
            entry: attempt.span,
        });
    }
    if nested {
        return Err(application::Error::NestedEntry {
            entry: attempt.span,
        });
    }
    if attempt.index != 0
        && !matches!(
            tokens[attempt.index - 1].kind(),
            crate::item::BodyTokenKind::Semicolon | crate::item::BodyTokenKind::RightBrace
        )
    {
        return Err(application::Error::NonTailEntry {
            entry: attempt.span,
        });
    }

    let mut cursor = Cursor::new(&tokens[attempt.index..], attempt.span);
    cursor.consume_intrinsic(matches!(attempt.kind, AttemptKind::Timed))?;
    let result = match attempt.kind {
        AttemptKind::Run => {
            let callbacks = cursor.callbacks(&["update", "view"])?;
            let view = resolve_callback(&callbacks[1], "view", items, methods, source)?;
            let update = if callbacks[0].unit {
                let shell =
                    unit_update(attempt, callbacks[0].span, &view, items, methods, contracts)
                        .ok_or(application::Error::NonSymbolicCallback {
                            role: "update".into(),
                            callback: callbacks[0].span,
                        })?;
                let key = shell.key.clone().expect("an update has its key");
                boot_closure.push(shell);
                key
            } else {
                resolve_callback(&callbacks[0], "update", items, methods, source)?
            };
            // `iced::run` boots from `State::default()`: a hand-written
            // `impl Default` runs; a derived one has no method item.
            let boot = match callbacks[0].segments.as_ref() {
                [owner, _] => resolve_callback(
                    &Callback {
                        segments: Box::new([owner.clone(), "default".into()]),
                        span: callbacks[0].span,
                        closure: None,
                        unit: false,
                    },
                    "boot",
                    items,
                    methods,
                    source,
                )
                .ok(),
                _ => None,
            };
            let boot = boot.or_else(|| {
                let shell = default_boot(attempt, &update, items, methods, boot_closure.len())?;
                let key = shell.key.clone();
                boot_closure.push(shell);
                key
            });
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
                prelude: None,
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
                instant: None,
                span: Span::new(attempt.span.start(), cursor.last_end()),
            }
        }
        AttemptKind::Application | AttemptKind::Timed => {
            let timed = matches!(attempt.kind, AttemptKind::Timed);
            // A timed application takes its subscription as its third
            // argument, where `.subscription(..)` would set it.
            let (callbacks, subscription) = if timed {
                let mut callbacks =
                    cursor.callbacks(&["boot", "update", "subscription", "view"])?;
                let subscription = callbacks.remove(2);
                (callbacks, Some(subscription))
            } else {
                (cursor.callbacks(&["boot", "update", "view"])?, None)
            };
            let mut tail = cursor.application_tail(
                contracts.application_title,
                contracts.application_theme,
                contracts.application_subscription,
                contracts.application_centered,
                contracts.application_run,
            )?;
            if let Some(subscription) = subscription {
                if tail.subscription.is_some() {
                    return Err(application::Error::MalformedEntry {
                        reason: "`iced::application::timed` takes its subscription as its third argument, not `.subscription(..)`".into(),
                        entry: attempt.span,
                    });
                }
                tail.subscription = Some(subscription);
            }
            let update = resolve_callback(&callbacks[1], "update", items, methods, source)?;
            // `State::default` from a derived `Default` has no method item:
            // boot from `Default` as `iced::run` does.
            let boot = match callbacks[0].closure {
                Some(body) => {
                    let shell = boot_function(attempt, body, &update, items)?;
                    let key = shell.key.clone();
                    boot_closure.push(shell);
                    key
                }
                None => match resolve_callback(&callbacks[0], "boot", items, methods, source) {
                    Ok(boot) => Some(boot),
                    Err(_) if matches!(callbacks[0].segments.as_ref(), [_, name] if name.as_str() == "default") => {
                        default_boot(attempt, &update, items, methods, boot_closure.len()).and_then(
                            |shell| {
                                let key = shell.key.clone();
                                boot_closure.push(shell);
                                key
                            },
                        )
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
            let theme = match &tail.theme {
                None => None,
                Some(callback) => Some(
                    match resolve_callback(callback, "theme", items, methods, source) {
                        Ok(theme) => theme,
                        // `.theme(Theme::Dark)`: iced's theme value, which
                        // is the function of the State that returns it.
                        Err(error) => {
                            let Some(shell) = theme_function(
                                attempt,
                                callback.span,
                                &update,
                                items,
                                methods,
                                contracts,
                                boot_closure.len(),
                            ) else {
                                return Err(error);
                            };
                            let key = shell.key.clone().expect("a theme function has its key");
                            boot_closure.push(shell);
                            key
                        }
                    },
                ),
            };
            let subscription = tail
                .subscription
                .as_ref()
                .map(|callback| resolve_callback(callback, "subscription", items, methods, source))
                .transpose()?;
            Entry {
                form: if timed {
                    application::Form::Timed
                } else {
                    application::Form::Application
                },
                facade: contracts.facade.clone(),
                root_intrinsic: if timed {
                    contracts
                        .timed
                        .expect("timed attempts retain their intrinsic contract")
                } else {
                    contracts
                        .application
                        .expect("application attempts retain their intrinsic contract")
                }
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
                // A timed application's subscription is its root's argument.
                subscription_intrinsic: tail.subscription.filter(|_| !timed).map(|_| {
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
                prelude: None,
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
                instant: contracts.instant.clone().filter(|_| timed),
                span: Span::new(attempt.span.start(), cursor.last_end()),
            }
        }
    };

    if !cursor.is_end() {
        return Err(application::Error::NonTailEntry {
            entry: attempt.span,
        });
    }
    let mut result = result;
    if attempt.index != 0 {
        let shell = prelude_function(attempt, result.span, items, boot_closure.len()).ok_or(
            application::Error::NonTailEntry {
                entry: attempt.span,
            },
        )?;
        result.prelude = shell.key.clone();
        boot_closure.push(shell);
    }
    Ok(Some((result, boot_closure)))
}

/// The statements of `main` before its entry, as a function of no
/// parameters returning `()`: `main`'s body with the entry left out. The
/// runner calls it once, before the first boot.
fn prelude_function(
    attempt: &Attempt<'_>,
    entry: Span,
    items: &[item::Shell],
    synthesized: usize,
) -> Option<item::Shell> {
    let (main, function) = items.iter().find_map(|shell| match &shell.kind {
        item::Kind::Function(function) if function.name.span == attempt.name.span => {
            Some((shell, function))
        }
        _ => None,
    })?;
    let key = main.key.as_ref()?;
    let name = crate::item::Name {
        text: PRELUDE.into(),
        span: function.name.span,
    };
    Some(item::Shell {
        id: crate::item::Id::from_index(items.len() + synthesized),
        source: main.source,
        facades: Box::default(),
        key: Some(crate::item::Key::new(
            key.package().into(),
            key.modules().into(),
            name.text.clone(),
            crate::item::Kind::Function,
        )),
        span: main.span,
        kind: item::Kind::Function(item::Function {
            header: function.header.clone(),
            name,
            generics: Box::default(),
            lifetimes: item::Lifetimes::default(),
            receiver: None,
            parameters: Box::default(),
            output: None,
            body: item::Body {
                span: function.body.span,
                lowering: item::Lowering::NominalFunction,
                syntax: crate::item::BodySyntax::new(Vec::new()),
                constant: false,
                asynchronous: false,
                text: None,
                elide: Some(entry),
            },
        }),
    })
}

/// The name of the function of `main`'s statements before its entry.
pub(crate) const PRELUDE: &str = "__rawr_main";

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
    let main = items
        .iter()
        .find(|shell| match &shell.kind {
            item::Kind::Function(function) => function.name.span == attempt.name.span,
            _ => false,
        })
        .ok_or_else(non_symbolic)?;
    let state = items
        .iter()
        .filter(|shell| shell.key.as_ref() == Some(update))
        .find_map(function)
        .and_then(
            |update| match update.parameters.first().map(|parameter| &parameter.ty) {
                Some(super::type_ref::Type::Reference { target, .. }) => Some((**target).clone()),
                _ => None,
            },
        )
        .ok_or_else(non_symbolic)?;
    let name = crate::item::Name {
        text: "__rawr_boot".into(),
        span: body,
    };
    Ok(item::Shell {
        id: crate::item::Id::from_index(items.len()),
        source: main.source,
        facades: Box::default(),
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
            lifetimes: item::Lifetimes::default(),
            receiver: None,
            parameters: Box::default(),
            output: Some(state),
            // The closure's value is the body, as a `const` initializer is.
            body: item::Body {
                span: body,
                lowering: item::Lowering::NominalFunction,
                syntax: crate::item::BodySyntax::new(Vec::new()),
                constant: true,
                asynchronous: false,
                text: None,
                elide: None,
            },
        }),
    })
}

/// `()` as `iced::run`'s update, for a view whose Message is iced's
/// `Never`: `fn(&mut State, Never)` of the view's State, which never runs,
/// beside `main`.
fn unit_update(
    attempt: &Attempt<'_>,
    span: Span,
    view: &crate::item::Key,
    items: &[item::Shell],
    methods: &[method::Definition],
    contracts: &Contracts<'_>,
) -> Option<item::Shell> {
    let main = items.iter().find(|shell| match &shell.kind {
        item::Kind::Function(function) => function.name.span == attempt.name.span,
        _ => false,
    })?;
    let state = update_state(view, items, methods).or_else(|| {
        items
            .iter()
            .filter(|shell| shell.key.as_ref() == Some(view))
            .find_map(|shell| match &shell.kind {
                item::Kind::Function(function) if function.parameters.is_empty() => {
                    Some(super::type_ref::Type::Unit(span))
                }
                _ => None,
            })
    })?;
    let never = contracts.registry.type_("iced::Never")?.clone();
    let owner = contracts.registry.type_owner("iced::Never").cloned();
    let message = super::type_ref::Type::Path(super::type_ref::Path {
        name: "iced::Never".into(),
        target: super::type_ref::Target::Host {
            definition: never,
            owner,
        },
        arguments: Box::default(),
        span,
    });
    let parameter = |name: &str, ty| item::Parameter {
        name: crate::item::Name {
            text: name.into(),
            span,
        },
        ty,
        span,
        pattern: None,
    };
    let name = crate::item::Name {
        text: "__rawr_update".into(),
        span,
    };
    let start = span.start();
    let (line, column) = (start.line()?, start.position()?);
    let text = format!("{}{}()", "\n".repeat(line - 1), " ".repeat(column - 1));
    Some(item::Shell {
        id: crate::item::Id::from_index(items.len()),
        source: main.source,
        facades: Box::default(),
        key: Some(crate::item::Key::new(
            view.package().into(),
            view.modules().into(),
            name.text.clone(),
            crate::item::Kind::Function,
        )),
        span,
        kind: item::Kind::Function(item::Function {
            header: crate::item::Header {
                attributes: Box::default(),
                visibility: None,
                span,
            },
            name,
            generics: Box::default(),
            lifetimes: item::Lifetimes::default(),
            receiver: None,
            parameters: Box::new([
                parameter(
                    "state",
                    super::type_ref::Type::Reference {
                        mutable: true,
                        lifetime: None,
                        target: Box::new(state),
                        span,
                    },
                ),
                parameter("_message", message),
            ]),
            output: None,
            body: item::Body {
                span,
                lowering: item::Lowering::NominalFunction,
                syntax: crate::item::BodySyntax::new(Vec::new()),
                constant: true,
                asynchronous: false,
                text: Some(text.into()),
                elide: None,
            },
        }),
    })
}

/// A boot from a derived `Default` the model cannot build when the program
/// is compiled: a field's type is an imported Rust type, or has its own
/// `impl Default`, so building the State runs code. It is the function of
/// no parameters returning `State::default()`, beside `main`,
/// written where the `iced::run` call starts.
fn default_boot(
    attempt: &Attempt<'_>,
    update: &crate::item::Key,
    items: &[item::Shell],
    methods: &[method::Definition],
    synthesized: usize,
) -> Option<item::Shell> {
    let main = items.iter().find(|shell| match &shell.kind {
        item::Kind::Function(function) => function.name.span == attempt.name.span,
        _ => false,
    })?;
    let state = update_state(update, items, methods)?;
    if !default_runs_code(&state, items, methods, &mut Vec::new()) {
        return None;
    }
    let super::type_ref::Type::Path(super::type_ref::Path {
        target:
            super::type_ref::Target::Script {
                identity: crate::type_def::Identity::Script(key),
                ..
            },
        ..
    }) = &state
    else {
        return None;
    };
    // `main` is at the crate root, where a root State is named as it is.
    let mut body = String::from(if key.modules().is_empty() {
        ""
    } else {
        "crate::"
    });
    for module in key.modules() {
        body.push_str(module);
        body.push_str("::");
    }
    body.push_str(key.name());
    body.push_str("::default()");
    let start = attempt.span.start();
    let (line, column) = (start.line()?, start.position()?);
    let text = format!("{}{}{body}", "\n".repeat(line - 1), " ".repeat(column - 1));
    let end = crate::Position::new(
        u16::try_from(line).ok()?,
        u16::try_from(column + body.len()).ok()?,
    );
    let body = Span::new(start, end);
    let name = crate::item::Name {
        text: "__rawr_default".into(),
        span: body,
    };
    Some(item::Shell {
        id: crate::item::Id::from_index(items.len() + synthesized),
        source: main.source,
        facades: Box::default(),
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
            lifetimes: item::Lifetimes::default(),
            receiver: None,
            parameters: Box::default(),
            output: Some(state),
            body: item::Body {
                span: body,
                lowering: item::Lowering::NominalFunction,
                syntax: crate::item::BodySyntax::new(Vec::new()),
                constant: true,
                asynchronous: false,
                text: Some(text.into()),
                elide: None,
            },
        }),
    })
}

/// Whether building `ty`'s `Default` runs code: an imported Rust type, a
/// type with its own `impl Default`, or a struct deriving `Default` with
/// such a field. Rawr builds any other `Default` once, when it compiles the
/// program.
pub(crate) fn default_runs_code(
    ty: &super::type_ref::Type,
    items: &[item::Shell],
    methods: &[method::Definition],
    visiting: &mut Vec<crate::type_def::Id>,
) -> bool {
    use super::type_ref::{Target, Type};
    match ty {
        Type::Unit(_) | Type::Reference { .. } => false,
        Type::Tuple { members, .. } => members
            .iter()
            .any(|member| default_runs_code(member, items, methods, visiting)),
        Type::Path(_)
            if ty.option_element().is_some()
                || ty.vec_element().is_some()
                || ty.keyed_arguments().is_some() =>
        {
            false
        }
        Type::Path(_) if let Some(inner) = ty.box_element().or_else(|| ty.reverse_element()) => {
            default_runs_code(inner, items, methods, visiting)
        }
        Type::Path(_) if let Some((element, _)) = ty.array_element() => {
            default_runs_code(element, items, methods, visiting)
        }
        Type::Path(path) => match &path.target {
            Target::Host { definition, .. } => definition.native_identity().is_none(),
            Target::Script { definition, .. } => {
                let definition = *definition;
                if visiting.contains(&definition) {
                    return false;
                }
                if methods.iter().any(|method| {
                    method.owner() == definition
                        && method.trait_name().is_some_and(|name| name == "Default")
                }) {
                    return true;
                }
                visiting.push(definition);
                let runs = items.iter().any(|shell| match &shell.kind {
                    item::Kind::Struct(item) if item.definition == definition => item
                        .fields
                        .iter()
                        .any(|field| default_runs_code(&field.ty, items, methods, visiting)),
                    _ => false,
                });
                visiting.pop();
                runs
            }
            Target::Builtin | Target::Parameter | Target::Projection(..) | Target::Alias(_) => {
                false
            }
        },
    }
}

/// The State an `update` callback changes: the `&mut` first parameter of a
/// free function, or the owner of a method.
fn update_state(
    update: &crate::item::Key,
    items: &[item::Shell],
    methods: &[method::Definition],
) -> Option<super::type_ref::Type> {
    if let Some(method) = methods.iter().find(|method| method.key() == update) {
        return Some(super::type_ref::Type::Path(super::type_ref::Path {
            name: method.owner_name().into(),
            target: super::type_ref::Target::Script {
                definition: method.owner(),
                identity: method.owner_identity().clone(),
            },
            arguments: Box::default(),
            span: Span::NONE,
        }));
    }
    items
        .iter()
        .filter(|shell| shell.key.as_ref() == Some(update))
        .find_map(|shell| match &shell.kind {
            item::Kind::Function(function) => function.parameters.first().cloned(),
            _ => None,
        })
        .and_then(|parameter| match parameter.ty {
            super::type_ref::Type::Reference { target, .. } => Some(*target),
            _ => None,
        })
}

/// `.theme(value)` with a theme value rather than a function: the function
/// of `&State` returning it, beside `main`.
fn theme_function(
    attempt: &Attempt<'_>,
    value: Span,
    update: &crate::item::Key,
    items: &[item::Shell],
    methods: &[method::Definition],
    contracts: &Contracts<'_>,
    synthesized: usize,
) -> Option<item::Shell> {
    let main = items.iter().find(|shell| match &shell.kind {
        item::Kind::Function(function) => function.name.span == attempt.name.span,
        _ => false,
    })?;
    let state = update_state(update, items, methods)?;
    let theme = contracts.registry.type_("iced::Theme")?.clone();
    let owner = contracts.registry.type_owner("iced::Theme").cloned();
    let output = super::type_ref::Type::Path(super::type_ref::Path {
        name: "Theme".into(),
        target: super::type_ref::Target::Host {
            definition: theme,
            owner,
        },
        arguments: Box::default(),
        span: value,
    });
    let name = crate::item::Name {
        text: "__rawr_theme".into(),
        span: value,
    };
    let parameter = item::Parameter {
        name: crate::item::Name {
            text: "__rawr_state".into(),
            span: value,
        },
        ty: super::type_ref::Type::Reference {
            mutable: false,
            lifetime: None,
            target: Box::new(state),
            span: value,
        },
        span: value,
        pattern: None,
    };
    Some(item::Shell {
        id: crate::item::Id::from_index(items.len() + synthesized),
        source: main.source,
        facades: Box::default(),
        key: Some(crate::item::Key::new(
            update.package().into(),
            update.modules().into(),
            name.text.clone(),
            crate::item::Kind::Function,
        )),
        span: value,
        kind: item::Kind::Function(item::Function {
            header: crate::item::Header {
                attributes: Box::default(),
                visibility: None,
                span: value,
            },
            name,
            generics: Box::default(),
            lifetimes: item::Lifetimes::default(),
            receiver: None,
            parameters: Box::new([parameter]),
            output: Some(output),
            // The theme value is the body, as a `const` initializer is.
            body: item::Body {
                span: value,
                lowering: item::Lowering::NominalFunction,
                syntax: crate::item::BodySyntax::new(Vec::new()),
                constant: true,
                asynchronous: false,
                text: None,
                elide: None,
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
                    | item::Kind::Struct(_)
                    | item::Kind::Enum(_) => None,
                });
            match owner {
                // An inherent method before a trait's of the same name, as
                // Rust resolves `State::update` when State is also a canvas
                // Program.
                Some(owner) => {
                    let candidates = methods.iter().filter(|method| {
                        method.owner() == owner
                            && method.name() == name.as_str()
                            && (source.is_none_or(|source| method.source() == source)
                                || method.is_public())
                    });
                    candidates
                        .clone()
                        .find(|method| method.trait_name().is_none())
                        .or_else(|| candidates.clone().next())
                        .map(|method| method.key().clone())
                }
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

    /// Consume `iced::run`, `iced::application` or, when `timed`,
    /// `iced::application::timed`.
    fn consume_intrinsic(&mut self, timed: bool) -> Result<(), application::Error> {
        for _ in 0..if timed { 5 } else { 3 } {
            self.consume();
        }
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
        if role == "update" && first.kind() == &crate::item::BodyTokenKind::Unit {
            let span = first.span();
            self.consume();
            return Ok(Callback {
                segments: Box::default(),
                span,
                closure: None,
                unit: true,
            });
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
            unit: false,
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
            unit: false,
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
                        self.tokens
                            .get(self.index + 3)
                            .map(crate::item::BodyToken::kind),
                        Some(crate::item::BodyTokenKind::Identifier(_))
                    ))
            {
                let operation =
                    Span::new(self.tokens[self.index].span().start(), method.span().end());
                self.consume();
                self.consume();
                self.skip_argument_list()
                    .ok_or_else(|| application::Error::MalformedEntry {
                        reason: format!("`.{name}(..)` expects its arguments in parentheses")
                            .into(),
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
