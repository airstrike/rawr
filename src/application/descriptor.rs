//! Compiler-normalized application callback contracts.

use crate::{
    application, capability, host, item, program, receiver, ImmutableString, Shared, Span,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// One normalized symbolic application callback.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Callback {
    item: item::Value,
    receiver: Option<receiver::Mode>,
    parameters: Box<[application::Type]>,
    output: application::Type,
}

/// The normalized application boot contract.
// Boxing `Explicit` would change this public, exhaustively matched contract.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Boot {
    /// `iced::run` recreates state through its proven Default capability.
    Default,
    /// `iced::application` invokes one explicit boot item.
    Explicit {
        /// Stable symbolic boot callback.
        callback: Callback,
        /// Optional startup task normalized from the boot output.
        task: Task,
    },
}

/// A normalized callback task output using the literal `iced::Task` facade.
// Boxing `Facade` would change this public, exhaustively matched contract.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Task {
    /// A unit callback output normalized to no task.
    None,
    /// The callback returns the exact registered `iced::Task<Message>` type.
    Facade(application::Type),
}

impl Task {
    pub(crate) fn runtime_type(&self) -> Option<host::runtime::type_ref::Type> {
        match self {
            Self::None => None,
            Self::Facade(ty) => ty.exact_runtime_type(),
        }
    }
}

/// A normalized update callback and its task output contract.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Update {
    callback: Callback,
    task: Task,
}

/// A normalized view callback and its optional direct output projection.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct View {
    callback: Callback,
    runtime_output: host::runtime::type_ref::Type,
    projection: Option<host::projection::Site>,
}

/// `main`'s statements before the application entry, which a runner runs
/// once, before the first boot.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Prelude {
    callback: Callback,
    text: Box<str>,
}

impl Prelude {
    /// Return the function of the statements.
    #[must_use]
    pub const fn callback(&self) -> &Callback {
        &self.callback
    }

    /// Return the statements as written.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A normalized state-derived application title callback.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Title {
    callback: Callback,
    runtime_output: host::runtime::type_ref::Type,
}

/// A normalized state-derived application theme callback.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Theme {
    callback: Callback,
    runtime_output: host::runtime::type_ref::Type,
}

/// A normalized state-derived application subscription callback.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Subscription {
    callback: Callback,
    runtime_output: host::runtime::type_ref::Type,
}

/// A deterministic exact proof of the normalized application contract.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Fingerprint(Shared<Proof>);

/// Non-authoritative application semantics normalized across one authenticated
/// profile self-seal cycle.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticFingerprint([u8; 32]);

/// Compiler-only normalized application contract with no executable authority.
#[doc(hidden)]
pub struct CheckedContract {
    fingerprint: SemanticFingerprint,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) struct Proof {
    facade: host::facade::Fingerprint,
    pub(super) dependencies: Box<[host::facade::Dependency]>,
    root: host::item::Fingerprint,
    application_title: Option<host::item::Fingerprint>,
    application_theme: Option<host::item::Fingerprint>,
    application_subscription: Option<host::item::Fingerprint>,
    application_centered: Option<host::item::Fingerprint>,
    terminal_run: Option<host::item::Fingerprint>,
    pub(super) state: application::Type,
    pub(super) message: application::Type,
    pub(super) boot: Boot,
    pub(super) prelude: Option<Prelude>,
    pub(super) update: Update,
    pub(super) view: View,
    pub(super) title: Option<Title>,
    pub(super) theme: Option<Theme>,
    pub(super) subscription: Option<Subscription>,
    pub(super) centered: bool,
    pub(super) previews: crate::preview::Fingerprint,
}

#[cfg(feature = "rust-import")]
impl Proof {
    fn normalize_profile(
        &mut self,
        profile: &host::facade::ProfileFinalization<'_>,
    ) -> (usize, usize) {
        let mut completed = profile.normalize_fingerprint(&mut self.facade);
        let mut direct_dependencies = 0_usize;
        for dependency in &mut self.dependencies {
            let (direct, nested) = profile.normalize_dependency(dependency);
            direct_dependencies = direct_dependencies.saturating_add(usize::from(direct));
            completed = completed.saturating_add(nested);
        }
        for item in [
            Some(&mut self.root),
            self.application_title.as_mut(),
            self.application_theme.as_mut(),
            self.application_subscription.as_mut(),
            self.application_centered.as_mut(),
            self.terminal_run.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            completed = completed.saturating_add(item.normalize_profile(profile));
        }
        completed = completed.saturating_add(self.state.normalize_profile(profile));
        completed = completed.saturating_add(self.message.normalize_profile(profile));
        completed = completed.saturating_add(self.boot.normalize_profile(profile));
        if let Some(prelude) = &mut self.prelude {
            completed = completed.saturating_add(prelude.callback.normalize_profile(profile));
        }
        completed = completed.saturating_add(self.update.normalize_profile(profile));
        completed = completed.saturating_add(self.view.normalize_profile(profile));
        if let Some(title) = &mut self.title {
            completed = completed.saturating_add(title.normalize_profile(profile));
        }
        if let Some(theme) = &mut self.theme {
            completed = completed.saturating_add(theme.normalize_profile(profile));
        }
        if let Some(subscription) = &mut self.subscription {
            completed = completed.saturating_add(subscription.normalize_profile(profile));
        }
        completed = completed.saturating_add(self.previews.normalize_profile(profile));
        (direct_dependencies, completed)
    }
}

#[cfg(feature = "rust-import")]
impl Callback {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        let mut completed = self.item.normalize_profile(profile);
        for parameter in &mut self.parameters {
            completed = completed.saturating_add(parameter.normalize_profile(profile));
        }
        completed.saturating_add(self.output.normalize_profile(profile))
    }
}

#[cfg(feature = "rust-import")]
impl Task {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        match self {
            Self::None => 0,
            Self::Facade(ty) => ty.normalize_profile(profile),
        }
    }
}

#[cfg(feature = "rust-import")]
impl Boot {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        match self {
            Self::Default => 0,
            Self::Explicit { callback, task } => callback
                .normalize_profile(profile)
                .saturating_add(task.normalize_profile(profile)),
        }
    }
}

#[cfg(feature = "rust-import")]
impl Update {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        self.callback
            .normalize_profile(profile)
            .saturating_add(self.task.normalize_profile(profile))
    }
}

#[cfg(feature = "rust-import")]
impl View {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        let mut completed = self.callback.normalize_profile(profile);
        completed = completed.saturating_add(self.runtime_output.normalize_profile(profile));
        if let Some(projection) = &mut self.projection {
            completed = completed.saturating_add(projection.normalize_profile(profile));
        }
        completed
    }
}

#[cfg(feature = "rust-import")]
impl Title {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        self.callback
            .normalize_profile(profile)
            .saturating_add(self.runtime_output.normalize_profile(profile))
    }
}

#[cfg(feature = "rust-import")]
impl Theme {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        self.callback
            .normalize_profile(profile)
            .saturating_add(self.runtime_output.normalize_profile(profile))
    }
}

#[cfg(feature = "rust-import")]
impl Subscription {
    fn normalize_profile(&mut self, profile: &host::facade::ProfileFinalization<'_>) -> usize {
        self.callback
            .normalize_profile(profile)
            .saturating_add(self.runtime_output.normalize_profile(profile))
    }
}

#[derive(Clone)]
pub(crate) struct Contract {
    pub(super) proof: Shared<Proof>,
}

/// One fully validated application descriptor paired with its exact executable.
///
/// A Descriptor can only be constructed by consuming an
/// [`application::Candidate`]. It does not expose the inner executable; a
/// matching shared Engine must consume it into an [`application::Launch`].
///
/// ```compile_fail
/// # use rawr::Engine;
/// # let engine = Engine::new();
/// # let descriptor = engine.compile_application_descriptor("40 + 2").unwrap();
/// let _ = descriptor.into_executable();
/// ```
pub struct Descriptor {
    executable: program::Executable,
    contract: Contract,
    fingerprint: Fingerprint,
}

#[derive(Clone, Debug)]
pub(crate) struct Signature {
    owner: Option<application::Type>,
    receiver: Option<receiver::Mode>,
    parameters: Box<[application::Type]>,
    output: application::Type,
    declaration: Span,
    receiver_span: Option<Span>,
    parameter_spans: Box<[Span]>,
    output_span: Span,
}

impl fmt::Debug for Descriptor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Descriptor")
            .field("state", self.state())
            .field("message", self.message())
            .field("fingerprint", &self.fingerprint)
            .field("executable", &"<private>")
            .finish()
    }
}

impl Descriptor {
    pub(crate) fn from_candidate(
        candidate: application::Candidate,
    ) -> Result<Self, application::Error> {
        let application::Candidate { executable } = candidate;
        let entry = executable
            .application()
            .cloned()
            .expect("application candidates retain one compiler-selected entry");
        let contract = Contract::validate(&entry, &executable)?;
        let fingerprint = Fingerprint(contract.proof.clone());
        Ok(Self {
            executable,
            contract,
            fingerprint,
        })
    }

    #[cfg(not(feature = "no_std"))]
    pub(crate) fn from_package_candidate(
        candidate: application::Candidate,
    ) -> Result<Self, (program::source::Id, application::Error)> {
        let application::Candidate { executable } = candidate;
        let entry = executable
            .application()
            .cloned()
            .expect("application candidates retain one compiler-selected entry");
        let contract = Contract::validate(&entry, &executable).map_err(|error| {
            let source = executable.application_error_source(&error);
            (source, error)
        })?;
        let fingerprint = Fingerprint(contract.proof.clone());
        Ok(Self {
            executable,
            contract,
            fingerprint,
        })
    }

    /// Return the exact stable application state type.
    #[must_use]
    pub fn state(&self) -> &application::Type {
        &self.contract.proof.state
    }

    /// Return the exact stable application message type.
    #[must_use]
    pub fn message(&self) -> &application::Type {
        &self.contract.proof.message
    }

    /// Return the normalized boot contract.
    #[must_use]
    pub fn boot(&self) -> &Boot {
        &self.contract.proof.boot
    }

    /// Return `main`'s statements before the entry, when present.
    #[must_use]
    pub fn prelude(&self) -> Option<&Prelude> {
        self.contract.proof.prelude.as_ref()
    }

    /// Return the normalized update callback.
    #[must_use]
    pub fn update(&self) -> &Update {
        &self.contract.proof.update
    }

    /// Return the normalized view callback.
    #[must_use]
    pub fn view(&self) -> &View {
        &self.contract.proof.view
    }

    /// Return the optional normalized state-derived title callback.
    #[must_use]
    pub fn title(&self) -> Option<&Title> {
        self.contract.proof.title.as_ref()
    }

    /// Return the optional normalized state-derived theme callback.
    #[must_use]
    pub fn theme(&self) -> Option<&Theme> {
        self.contract.proof.theme.as_ref()
    }

    /// Return the optional normalized state-derived subscription callback.
    #[must_use]
    pub fn subscription(&self) -> Option<&Subscription> {
        self.contract.proof.subscription.as_ref()
    }

    /// Return whether the native application requests centered initial placement.
    #[must_use]
    pub fn centered(&self) -> bool {
        self.contract.proof.centered
    }

    /// Return every compiler-validated local preview in stable key order.
    #[must_use]
    pub const fn previews(&self) -> &[crate::preview::Definition] {
        self.executable.previews()
    }

    /// Return the exact static facade proof used by this application.
    #[must_use]
    pub fn facade(&self) -> &host::facade::Fingerprint {
        &self.contract.proof.facade
    }

    /// Return every exact host-facade dependency required by this contract.
    #[must_use]
    pub fn dependencies(&self) -> &[host::facade::Dependency] {
        &self.contract.proof.dependencies
    }

    /// Return the deterministic exact application proof.
    #[must_use]
    pub const fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    /// Return the executable generation retained by this descriptor.
    #[must_use]
    pub const fn generation(&self) -> program::GenerationId {
        self.executable.generation()
    }

    pub(crate) fn into_parts(self) -> (program::Executable, Contract, Fingerprint) {
        (self.executable, self.contract, self.fingerprint)
    }
}

impl Callback {
    /// Return the stable symbolic callback item.
    #[must_use]
    pub const fn item(&self) -> &item::Value {
        &self.item
    }

    /// Return the callback receiver mode.
    #[must_use]
    pub const fn receiver(&self) -> Option<receiver::Mode> {
        self.receiver
    }

    /// Return positional callback parameter types, excluding the receiver.
    #[must_use]
    pub const fn parameters(&self) -> &[application::Type] {
        &self.parameters
    }

    /// Return the declared callback output type.
    #[must_use]
    pub const fn output(&self) -> &application::Type {
        &self.output
    }
}

impl View {
    /// Return the stable symbolic view callback.
    #[must_use]
    pub const fn callback(&self) -> &Callback {
        &self.callback
    }

    /// Return the authenticated direct output projection, when required.
    #[must_use]
    pub const fn projection(&self) -> Option<&host::type_def::Projection> {
        match &self.projection {
            Some(projection) => Some(projection.projection()),
            None => None,
        }
    }

    pub(crate) const fn boundary_projection(&self) -> Option<&host::projection::Site> {
        self.projection.as_ref()
    }

    pub(crate) const fn runtime_output(&self) -> &host::runtime::type_ref::Type {
        &self.runtime_output
    }
}

impl Title {
    /// Return the stable symbolic title callback.
    #[must_use]
    pub const fn callback(&self) -> &Callback {
        &self.callback
    }

    pub(crate) const fn runtime_output(&self) -> &host::runtime::type_ref::Type {
        &self.runtime_output
    }
}

impl Theme {
    /// Return the stable symbolic theme callback.
    #[must_use]
    pub const fn callback(&self) -> &Callback {
        &self.callback
    }

    pub(crate) const fn runtime_output(&self) -> &host::runtime::type_ref::Type {
        &self.runtime_output
    }
}

impl Subscription {
    /// Return the stable symbolic subscription callback.
    #[must_use]
    pub const fn callback(&self) -> &Callback {
        &self.callback
    }

    pub(crate) const fn runtime_output(&self) -> &host::runtime::type_ref::Type {
        &self.runtime_output
    }
}

impl Update {
    /// Return the stable symbolic update callback.
    #[must_use]
    pub const fn callback(&self) -> &Callback {
        &self.callback
    }

    /// Return the normalized update task output contract.
    #[must_use]
    pub const fn task(&self) -> &Task {
        &self.task
    }
}

impl Contract {
    pub(crate) fn validate<E: program::ApplicationEvidence>(
        entry: &application::Entry,
        executable: &E,
    ) -> Result<Self, application::Error> {
        let task = entry
            .task()
            .ok_or(application::Error::InvalidFacadeContract {
                required: "iced::Task<Message>",
                entry: entry.span(),
            })?;
        let widget = entry
            .widget()
            .ok_or(application::Error::InvalidFacadeContract {
                required: "iced::Element<Message>",
                entry: entry.span(),
            })?;
        let update = checked_callback(executable, entry.update(), "update")?;
        require_kind(&update, &[item::Kind::Method], "a method")?;
        require_receiver(&update, Some(receiver::Mode::Mutable), "`&mut State`")?;
        require_arity(&update, 1)?;

        let state = update
            .signature
            .owner
            .as_ref()
            .expect("validated methods retain their stable owner type")
            .clone();
        let message = update.signature.parameters[0].clone();
        let message_span = update.signature.parameter_spans[0];
        // State may hold tuples: they are arrays at runtime, proven and
        // carried member by member.
        require_runtime_type("message", &message, message_span)?;
        require_runtime_type(
            "update output",
            &update.signature.output,
            update.signature.output_span,
        )?;
        let update_task = normalize_task(
            "update",
            &update.signature.output,
            &message,
            task,
            update.signature.output_span,
            true,
        )?;

        for capability in [
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ] {
            if !message.provides(capability) {
                return Err(application::Error::MissingCapability {
                    message: message.display_name().into(),
                    capability: capability.name(),
                    declaration: message_span,
                });
            }
        }

        let update_declaration = update.signature.declaration;
        let update = Update {
            callback: update.normalized(),
            task: update_task,
        };

        let view = checked_callback(executable, entry.view(), "view")?;
        require_kind(&view, &[item::Kind::Method], "a method")?;
        require_receiver(&view, Some(receiver::Mode::Shared), "`&State`")?;
        require_arity(&view, 0)?;
        require_same_type(
            "view state",
            &state,
            view.signature
                .owner
                .as_ref()
                .expect("validated methods retain their stable owner type"),
            view.signature
                .receiver_span
                .unwrap_or(view.signature.declaration),
        )?;
        let projection = normalize_view(
            &view.signature.output,
            &message,
            widget,
            view.signature.output_span,
        )?;
        let runtime_output = match &projection {
            Some(projection) => projection.target().clone(),
            None => view.signature.output.exact_runtime_type().ok_or(
                application::Error::UnsupportedRuntimeType {
                    role: "view output",
                    ty: view.signature.output.display_name().into(),
                    signature: view.signature.output_span,
                },
            )?,
        };
        require_runtime_type(
            "view output",
            &view.signature.output,
            view.signature.output_span,
        )?;
        let view = View {
            callback: view.normalized(),
            runtime_output,
            projection,
        };

        // `iced::run`, or `iced::application(State::default, ..)` with a
        // derived `Default`, boots from `Default`.
        let boot = match entry.boot() {
            None => {
                if !state.provides(capability::Kind::Default) {
                    return Err(application::Error::MissingDefault {
                        state: state.display_name().into(),
                        declaration: update_declaration,
                    });
                }
                Boot::Default
            }
            Some(boot) => {
                let boot = checked_callback(executable, boot, "boot")?;
                require_kind(
                    &boot,
                    &[item::Kind::Function, item::Kind::AssociatedFunction],
                    "a free or associated function",
                )?;
                require_receiver(&boot, None, "no receiver")?;
                require_arity(&boot, 0)?;
                if boot.item.kind() == item::Kind::AssociatedFunction {
                    require_same_type(
                        "boot owner",
                        &state,
                        boot.signature
                            .owner
                            .as_ref()
                            .expect("associated functions retain their stable owner type"),
                        boot.signature.declaration,
                    )?;
                }
                let task = normalize_boot_output(
                    &boot.signature.output,
                    &state,
                    &message,
                    task,
                    boot.signature.output_span,
                )?;
                Boot::Explicit {
                    callback: boot.normalized(),
                    task,
                }
            }
        };

        let prelude = match entry.prelude() {
            None => None,
            Some((item, text)) => {
                let prelude = checked_callback(executable, item, "prelude")?;
                require_kind(&prelude, &[item::Kind::Function], "a free function")?;
                require_arity(&prelude, 0)?;
                Some(Prelude {
                    callback: prelude.normalized(),
                    text: text.into(),
                })
            }
        };

        let title = match entry.title() {
            Some(item) => {
                let title = checked_callback(executable, item, "title")?;
                require_kind(&title, &[item::Kind::Method], "a method")?;
                require_receiver(&title, Some(receiver::Mode::Shared), "`&State`")?;
                require_arity(&title, 0)?;
                require_same_type(
                    "title state",
                    &state,
                    title
                        .signature
                        .owner
                        .as_ref()
                        .expect("validated methods retain their stable owner type"),
                    title
                        .signature
                        .receiver_span
                        .unwrap_or(title.signature.declaration),
                )?;
                let expected = application::Type::builtin("String");
                if title.signature.output != expected {
                    return Err(application::Error::InvalidOutput {
                        role: "title",
                        expected: "`String`".into(),
                        actual: title.signature.output.display_name().into(),
                        signature: title.signature.output_span,
                    });
                }
                let runtime_output = title.signature.output.exact_runtime_type().ok_or(
                    application::Error::UnsupportedRuntimeType {
                        role: "title output",
                        ty: title.signature.output.display_name().into(),
                        signature: title.signature.output_span,
                    },
                )?;
                Some(Title {
                    callback: title.normalized(),
                    runtime_output,
                })
            }
            None => None,
        };

        let theme = match entry.theme() {
            Some(item) => {
                let contract =
                    entry
                        .theme_type()
                        .ok_or(application::Error::InvalidFacadeContract {
                            required: "iced::Theme",
                            entry: entry.span(),
                        })?;
                let theme = checked_callback(executable, item, "theme")?;
                require_kind(&theme, &[item::Kind::Method], "a method")?;
                require_receiver(&theme, Some(receiver::Mode::Shared), "`&State`")?;
                require_arity(&theme, 0)?;
                require_same_type(
                    "theme state",
                    &state,
                    theme
                        .signature
                        .owner
                        .as_ref()
                        .expect("validated methods retain their stable owner type"),
                    theme
                        .signature
                        .receiver_span
                        .unwrap_or(theme.signature.declaration),
                )?;
                // `-> Option<Theme>`: `None` keeps the default theme.
                let output = theme
                    .signature
                    .output
                    .option_element()
                    .unwrap_or(&theme.signature.output);
                if !output.matches_host(contract, &[]) {
                    return Err(application::Error::InvalidOutput {
                        role: "theme",
                        expected: "`iced::Theme` or `Option<iced::Theme>`".into(),
                        actual: theme.signature.output.display_name().into(),
                        signature: theme.signature.output_span,
                    });
                }
                require_runtime_type(
                    "theme output",
                    &theme.signature.output,
                    theme.signature.output_span,
                )?;
                let runtime_output = theme.signature.output.exact_runtime_type().ok_or(
                    application::Error::UnsupportedRuntimeType {
                        role: "theme output",
                        ty: theme.signature.output.display_name().into(),
                        signature: theme.signature.output_span,
                    },
                )?;
                Some(Theme {
                    callback: theme.normalized(),
                    runtime_output,
                })
            }
            None => None,
        };

        let subscription = match entry.subscription() {
            Some(item) => {
                let contract =
                    entry
                        .subscription_type()
                        .ok_or(application::Error::InvalidFacadeContract {
                            required: "iced::Subscription<Message>",
                            entry: entry.span(),
                        })?;
                let subscription = checked_callback(executable, item, "subscription")?;
                require_kind(&subscription, &[item::Kind::Method], "a method")?;
                require_receiver(&subscription, Some(receiver::Mode::Shared), "`&State`")?;
                require_arity(&subscription, 0)?;
                require_same_type(
                    "subscription state",
                    &state,
                    subscription
                        .signature
                        .owner
                        .as_ref()
                        .expect("validated methods retain their stable owner type"),
                    subscription
                        .signature
                        .receiver_span
                        .unwrap_or(subscription.signature.declaration),
                )?;
                if !subscription
                    .signature
                    .output
                    .matches_host(contract, std::slice::from_ref(&message))
                {
                    return Err(application::Error::InvalidOutput {
                        role: "subscription",
                        expected: format!("`iced::Subscription<{}>`", message.display_name())
                            .into(),
                        actual: subscription.signature.output.display_name().into(),
                        signature: subscription.signature.output_span,
                    });
                }
                require_runtime_type(
                    "subscription output",
                    &subscription.signature.output,
                    subscription.signature.output_span,
                )?;
                let runtime_output = subscription.signature.output.exact_runtime_type().ok_or(
                    application::Error::UnsupportedRuntimeType {
                        role: "subscription output",
                        ty: subscription.signature.output.display_name().into(),
                        signature: subscription.signature.output_span,
                    },
                )?;
                Some(Subscription {
                    callback: subscription.normalized(),
                    runtime_output,
                })
            }
            None => None,
        };

        let dependencies = collect_dependencies(
            entry,
            executable,
            &state,
            &message,
            &boot,
            &update,
            &view,
            title.as_ref(),
            theme.as_ref(),
            subscription.as_ref(),
            prelude.as_ref(),
        )?;

        Ok(Self {
            proof: Shared::new(Proof {
                facade: entry.facade().clone(),
                dependencies,
                root: entry.root_intrinsic().clone(),
                application_title: entry.title_intrinsic().cloned(),
                application_theme: entry.theme_intrinsic().cloned(),
                application_subscription: entry.subscription_intrinsic().cloned(),
                application_centered: entry.centered_intrinsic().cloned(),
                terminal_run: entry.run_intrinsic().cloned(),
                state,
                message,
                boot,
                prelude,
                update,
                view,
                title,
                theme,
                subscription,
                centered: entry.centered(),
                previews: executable.preview_fingerprint().clone(),
            }),
        })
    }
}

impl Contract {
    pub(crate) fn dependencies(&self) -> &[host::facade::Dependency] {
        &self.proof.dependencies
    }

    pub(crate) fn fingerprint(&self) -> Fingerprint {
        Fingerprint(self.proof.clone())
    }

    #[cfg(feature = "rust-import")]
    pub(crate) fn semantic_fingerprint(
        &self,
        cycle: &host::facade::ProfileCycle,
    ) -> Result<SemanticFingerprint, application::Error> {
        use std::hash::Hash as _;
        let finalization = cycle.finalization(&self.proof.facade).ok_or(
            application::Error::InvalidFacadeContract {
                required: "one exact profile self-cycle application facade",
                entry: Span::NONE,
            },
        )?;
        let mut proof = Proof::clone(&self.proof);
        let (dependencies, completed) = proof.normalize_profile(&finalization);
        if dependencies != 1 {
            return Err(application::Error::InvalidFacadeContract {
                required: "one exact profile self-cycle application dependency",
                entry: Span::NONE,
            });
        }
        if !finalization.runtime_representations_are_valid() {
            return Err(application::Error::InvalidFacadeContract {
                required: "authenticated semantic runtime representation",
                entry: Span::NONE,
            });
        }
        let mut digest = SemanticHasher::new();
        completed.hash(&mut digest);
        Fingerprint(Shared::new(proof)).hash(&mut digest);
        Ok(SemanticFingerprint(digest.seal()))
    }

    pub(crate) fn reload_fingerprint(&self) -> Fingerprint {
        let mut proof = Proof::clone(&self.proof);
        proof.previews = crate::preview::Fingerprint::default();
        // The statements before the entry run once, so an edit to them
        // keeps the State.
        proof.prelude = None;
        Fingerprint(Shared::new(proof))
    }

    pub(crate) fn schema_roots(&self) -> (BTreeSet<crate::type_def::Identity>, bool) {
        let mut roots = BTreeSet::new();
        self.proof.state.collect_schema_roots(&mut roots);
        self.proof.message.collect_schema_roots(&mut roots);

        match &self.proof.boot {
            Boot::Default => {}
            Boot::Explicit { callback, task } => {
                collect_callback_schema_roots(callback, &mut roots);
                collect_task_schema_roots(task, &mut roots);
            }
        }
        collect_callback_schema_roots(self.proof.update.callback(), &mut roots);
        collect_task_schema_roots(self.proof.update.task(), &mut roots);
        collect_callback_schema_roots(self.proof.view.callback(), &mut roots);
        if let Some(title) = &self.proof.title {
            collect_callback_schema_roots(title.callback(), &mut roots);
        }
        if let Some(theme) = &self.proof.theme {
            collect_callback_schema_roots(theme.callback(), &mut roots);
        }
        if let Some(subscription) = &self.proof.subscription {
            collect_callback_schema_roots(subscription.callback(), &mut roots);
        }

        (roots, self.proof.state.retained_root_is_opaque())
    }

    pub(crate) fn boundary_projections(&self) -> impl Iterator<Item = &host::projection::Site> {
        self.proof.view.boundary_projection().into_iter()
    }

    pub(crate) fn boundary_types(&self) -> Vec<host::runtime::type_ref::Type> {
        let mut types = vec![self.proof.view.runtime_output().clone()];
        types.extend(self.proof.state.exact_runtime_type());
        types.extend(self.proof.message.exact_runtime_type());
        types.extend(match &self.proof.boot {
            Boot::Default => None,
            Boot::Explicit { task, .. } => task.runtime_type(),
        });
        types.extend(self.proof.update.task().runtime_type());
        types.extend(
            self.proof
                .title
                .as_ref()
                .map(Title::runtime_output)
                .cloned(),
        );
        types.extend(
            self.proof
                .theme
                .as_ref()
                .map(Theme::runtime_output)
                .cloned(),
        );
        types.extend(
            self.proof
                .subscription
                .as_ref()
                .map(Subscription::runtime_output)
                .cloned(),
        );
        types
    }
}

impl CheckedContract {
    #[cfg(feature = "rust-import")]
    pub(crate) fn transform(
        evidence: &program::ContractEvidence,
        cycle: &host::facade::ProfileCycle,
    ) -> Result<Self, application::Error> {
        // A program whose `main` is not an application runs `main` once,
        // as a plain program does.
        let Some(entry) = program::ApplicationEvidence::application(evidence) else {
            return Ok(Self {
                fingerprint: SemanticFingerprint::plain(),
            });
        };
        let contract = Contract::validate(entry, evidence)?;
        Ok(Self {
            fingerprint: contract.semantic_fingerprint(cycle)?,
        })
    }

    /// Borrow the exact normalized application-semantic fingerprint.
    #[doc(hidden)]
    #[must_use]
    pub const fn fingerprint(&self) -> &SemanticFingerprint {
        &self.fingerprint
    }
}

impl SemanticFingerprint {
    /// The semantics of a plain program: no application, a `main` run once.
    #[doc(hidden)]
    #[must_use]
    pub fn plain() -> Self {
        use sha2::Digest as _;
        Self(sha2::Sha256::digest(b"rawr.application.plain-program/v1").into())
    }

    /// Borrow the canonical domain-separated semantic contract bytes.
    #[doc(hidden)]
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[cfg(feature = "rust-import")]
struct SemanticHasher(sha2::Sha256);

#[cfg(feature = "rust-import")]
impl SemanticHasher {
    fn new() -> Self {
        use sha2::Digest as _;
        let mut digest = sha2::Sha256::new();
        digest.update(b"rawr.application.semantic-contract/v3");
        Self(digest)
    }

    fn seal(self) -> [u8; 32] {
        use sha2::Digest as _;
        self.0.finalize().into()
    }
}

#[cfg(feature = "rust-import")]
impl std::hash::Hasher for SemanticHasher {
    fn finish(&self) -> u64 {
        0
    }

    fn write(&mut self, bytes: &[u8]) {
        use sha2::Digest as _;
        self.0.update(bytes);
    }
}

fn collect_callback_schema_roots(
    callback: &Callback,
    roots: &mut BTreeSet<crate::type_def::Identity>,
) {
    for parameter in callback.parameters() {
        parameter.collect_schema_roots(roots);
    }
    callback.output().collect_schema_roots(roots);
}

fn collect_task_schema_roots(task: &Task, roots: &mut BTreeSet<crate::type_def::Identity>) {
    match task {
        Task::None => {}
        Task::Facade(ty) => ty.collect_schema_roots(roots),
    }
}

#[derive(Default)]
pub(crate) struct Dependencies(BTreeMap<ImmutableString, host::facade::Dependency>);

impl Dependencies {
    pub(crate) fn insert(
        &mut self,
        dependency: host::facade::Dependency,
    ) -> Result<(), application::Error> {
        if let Some(existing) = self.0.get(dependency.root()) {
            if existing != &dependency {
                return Err(application::Error::FacadeDependencyConflict {
                    root: dependency.root().into(),
                });
            }
            return Ok(());
        }
        self.0.insert(dependency.root().into(), dependency);
        Ok(())
    }

    fn type_(&mut self, ty: &application::Type) -> Result<(), application::Error> {
        let mut retained = BTreeSet::new();
        ty.collect_dependencies(&mut retained);
        for dependency in retained {
            self.insert(dependency)?;
        }
        Ok(())
    }

    fn callback(&mut self, callback: &Callback) -> Result<(), application::Error> {
        for parameter in &callback.parameters {
            self.type_(parameter)?;
        }
        self.type_(&callback.output)
    }

    fn task(&mut self, task: &Task) -> Result<(), application::Error> {
        match task {
            Task::None => Ok(()),
            Task::Facade(ty) => self.type_(ty),
        }
    }

    pub(crate) fn preview(
        &mut self,
        preview: &crate::preview::Definition,
    ) -> Result<(), application::Error> {
        self.preview_callback(preview.subject())?;
        self.type_(preview.message())?;
        match preview.input() {
            crate::preview::Input::Stateless => {}
            crate::preview::Input::Shared(state) | crate::preview::Input::Owned(state) => {
                self.type_(state)?;
            }
        }
        if let crate::preview::Seed::Callback(seed) = preview.seed() {
            self.preview_callback(seed)?;
        }
        if let Some(update) = preview.update() {
            self.preview_callback(update)?;
        }
        for theme in preview.themes() {
            self.preview_callback(theme.callback())?;
        }
        if let Some(projection) = preview.projection_site() {
            for dependency in projection.dependencies() {
                self.insert(dependency)?;
            }
        }
        Ok(())
    }

    fn preview_callback(
        &mut self,
        callback: &crate::preview::Callback,
    ) -> Result<(), application::Error> {
        for parameter in callback.parameters() {
            self.type_(parameter)?;
        }
        self.type_(callback.output())
    }

    pub(crate) fn finish(self) -> Box<[host::facade::Dependency]> {
        self.0.into_values().collect()
    }
}

fn collect_dependencies(
    entry: &application::Entry,
    executable: &impl program::ApplicationEvidence,
    state: &application::Type,
    message: &application::Type,
    boot: &Boot,
    update: &Update,
    view: &View,
    title: Option<&Title>,
    theme: Option<&Theme>,
    subscription: Option<&Subscription>,
    prelude: Option<&Prelude>,
) -> Result<Box<[host::facade::Dependency]>, application::Error> {
    let mut dependencies = Dependencies::default();
    dependencies.insert(entry.dependency().clone())?;
    for dependency in executable.host_dependencies() {
        dependencies.insert(dependency.clone())?;
    }
    dependencies.type_(state)?;
    dependencies.type_(message)?;
    dependencies.callback(&update.callback)?;
    dependencies.task(&update.task)?;
    dependencies.callback(&view.callback)?;
    if let Some(title) = title {
        dependencies.callback(&title.callback)?;
    }
    if let Some(theme) = theme {
        dependencies.callback(&theme.callback)?;
    }
    if let Some(subscription) = subscription {
        dependencies.callback(&subscription.callback)?;
    }
    if let Some(prelude) = prelude {
        dependencies.callback(&prelude.callback)?;
    }
    if let Some(projection) = view.projection.as_ref() {
        for dependency in projection.dependencies() {
            dependencies.insert(dependency)?;
        }
    }
    if let Boot::Explicit { callback, task } = boot {
        dependencies.callback(callback)?;
        dependencies.task(task)?;
    }
    Ok(dependencies.finish())
}

struct Selected<'a> {
    role: &'static str,
    item: &'a item::Value,
    kind: item::Kind,
    signature: Signature,
}

impl Selected<'_> {
    fn normalized(&self) -> Callback {
        Callback {
            item: self.item.clone(),
            receiver: self.signature.receiver,
            parameters: self.signature.parameters.clone(),
            output: self.signature.output.clone(),
        }
    }
}

fn checked_callback<'a>(
    executable: &'a impl program::ApplicationEvidence,
    item: &'a item::Value,
    role: &'static str,
) -> Result<Selected<'a>, application::Error> {
    let callable = executable
        .callable(item.key())
        .expect("application entries select callables from the same executable");
    let signature = callable.application_signature().ok_or_else(|| {
        application::Error::InvalidCallbackKind {
            role,
            callback: item.key().to_string().into(),
            expected: "a script function or method",
            actual: item.kind(),
            declaration: entryless_span(),
        }
    })?;
    // A free `fn update(state: &mut State, ..)` or `fn view(state: &State)`,
    // as iced master's examples write them, is the method of its State.
    if item.kind() == item::Kind::Function
        && let Some((mutable, state)) = signature
            .parameters
            .first()
            .and_then(application::Type::reference)
    {
        return Ok(Selected {
            role,
            item,
            kind: item::Kind::Method,
            signature: Signature {
                owner: Some(state.clone()),
                receiver: Some(if mutable {
                    receiver::Mode::Mutable
                } else {
                    receiver::Mode::Shared
                }),
                parameters: signature.parameters[1..].into(),
                receiver_span: signature.parameter_spans.first().copied(),
                parameter_spans: signature.parameter_spans[1..].into(),
                ..signature.clone()
            },
        });
    }
    Ok(Selected {
        role,
        item,
        kind: item.kind(),
        signature: signature.clone(),
    })
}

fn require_kind(
    selected: &Selected<'_>,
    accepted: &[item::Kind],
    expected: &'static str,
) -> Result<(), application::Error> {
    if accepted.contains(&selected.kind) {
        Ok(())
    } else {
        Err(application::Error::InvalidCallbackKind {
            role: selected.role,
            callback: selected.item.key().to_string().into(),
            expected,
            actual: selected.kind,
            declaration: selected.signature.declaration,
        })
    }
}

fn require_receiver(
    selected: &Selected<'_>,
    expected: Option<receiver::Mode>,
    expected_name: &'static str,
) -> Result<(), application::Error> {
    if selected.signature.receiver == expected {
        Ok(())
    } else {
        Err(application::Error::InvalidReceiver {
            role: selected.role,
            callback: selected.item.key().to_string().into(),
            expected: expected_name,
            actual: selected.signature.receiver,
            declaration: selected
                .signature
                .receiver_span
                .unwrap_or(selected.signature.declaration),
        })
    }
}

fn require_arity(selected: &Selected<'_>, expected: usize) -> Result<(), application::Error> {
    let actual = selected.signature.parameters.len();
    if actual == expected {
        Ok(())
    } else {
        Err(application::Error::InvalidArity {
            role: selected.role,
            callback: selected.item.key().to_string().into(),
            expected,
            actual,
            declaration: selected.signature.declaration,
        })
    }
}

fn require_same_type(
    role: &'static str,
    expected: &application::Type,
    actual: &application::Type,
    signature: Span,
) -> Result<(), application::Error> {
    if expected == actual {
        Ok(())
    } else {
        Err(application::Error::TypeMismatch {
            role,
            expected: expected.display_name().into(),
            actual: actual.display_name().into(),
            signature,
        })
    }
}

fn require_runtime_type(
    role: &'static str,
    ty: &application::Type,
    signature: Span,
) -> Result<(), application::Error> {
    if ty.is_tuple_free() {
        Ok(())
    } else {
        Err(application::Error::UnsupportedRuntimeType {
            role,
            ty: ty.display_name().into(),
            signature,
        })
    }
}

fn normalize_task(
    role: &'static str,
    output: &application::Type,
    message: &application::Type,
    task: &host::type_def::Contract,
    signature: Span,
    accept_unit: bool,
) -> Result<Task, application::Error> {
    if accept_unit && output.is_unit() {
        return Ok(Task::None);
    }
    if output.matches_host(task, std::slice::from_ref(message)) {
        Ok(Task::Facade(output.clone()))
    } else {
        Err(application::Error::InvalidOutput {
            role,
            expected: if accept_unit {
                "`() or iced::Task<Message>`".into()
            } else {
                "`iced::Task<Message>`".into()
            },
            actual: output.display_name().into(),
            signature,
        })
    }
}

fn normalize_boot_output(
    output: &application::Type,
    state: &application::Type,
    message: &application::Type,
    task: &host::type_def::Contract,
    signature: Span,
) -> Result<Task, application::Error> {
    if output == state {
        return Ok(Task::None);
    }
    let Some(members) = output.tuple_members() else {
        return require_same_type("boot state", state, output, signature).map(|()| Task::None);
    };
    if members.len() != 2 {
        return Err(application::Error::InvalidOutput {
            role: "boot",
            expected: "a two-member `(State, iced::Task<Message>)` tuple".into(),
            actual: output.display_name().into(),
            signature,
        });
    }
    require_same_type("boot state", state, &members[0], signature)?;
    normalize_task("boot", &members[1], message, task, signature, false)
}

fn normalize_view(
    output: &application::Type,
    message: &application::Type,
    widget: &host::type_def::Contract,
    signature: Span,
) -> Result<Option<host::projection::Site>, application::Error> {
    if output.matches_host(widget, std::slice::from_ref(message)) {
        return Ok(None);
    }
    if let Some((projection, arguments)) =
        output.projection_site(widget, Some(std::slice::from_ref(message)), signature)
        && arguments.as_ref() == std::slice::from_ref(message)
    {
        return Ok(Some(projection));
    }
    Err(application::Error::InvalidOutput {
        role: "view",
        expected: "a registered widget output compatible with Message".into(),
        actual: output.display_name().into(),
        signature,
    })
}

const fn entryless_span() -> Span {
    Span::NONE
}

impl Signature {
    pub(crate) const fn declaration(&self) -> Span {
        self.declaration
    }

    pub(crate) fn new(
        owner: Option<application::Type>,
        receiver: Option<receiver::Mode>,
        parameters: Box<[application::Type]>,
        output: application::Type,
        declaration: Span,
        receiver_span: Option<Span>,
        parameter_spans: Box<[Span]>,
        output_span: Span,
    ) -> Self {
        Self {
            owner,
            receiver,
            parameters,
            output,
            declaration,
            receiver_span,
            parameter_spans,
            output_span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Dependencies;
    use crate::{application, host};

    #[test]
    fn canonical_dependencies_reject_one_root_with_unequal_fingerprints() {
        let first = facade(1);
        let second = facade(2);
        let mut dependencies = Dependencies::default();
        dependencies
            .insert(host::facade::Dependency::new(
                first.root_path(),
                first.fingerprint().clone(),
            ))
            .unwrap();
        let error = dependencies
            .insert(host::facade::Dependency::new(
                second.root_path(),
                second.fingerprint().clone(),
            ))
            .unwrap_err();

        assert!(matches!(
            error,
            application::Error::FacadeDependencyConflict { root } if root == "ui"
        ));
    }

    fn facade(abi: u32) -> host::facade::Definition {
        host::facade::Builder::new("ui", host::facade::Abi::new(abi).unwrap())
            .unwrap()
            .finish()
            .unwrap()
    }
}
