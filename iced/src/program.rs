//! State, authority, and the native iced surface for one Rawr application.

use std::fmt;
use std::sync::{Arc, Mutex};

use rawr::{application, host, program, timing, Dynamic};

use crate::{diagnostic, effect, runtime, subscription, task, theme, widget};

#[cfg(test)]
mod carry_tests;
pub mod install;
mod live;

/// The result of applying one provenance-checked message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery {
    /// The exact current owner pair admitted the payload to update.
    Applied,
    /// The envelope was host-internal or belonged to a replaced owner.
    Dropped,
}

/// One admitted or dropped delivery paired with its durable follow-up plan.
#[derive(Debug)]
pub struct Transition {
    delivery: Delivery,
    plan: task::Plan,
}

/// A native application callback could not cross the current Step-1 boundary.
#[derive(Debug)]
pub enum Error {
    /// Default State construction failed its executable-owned model proof.
    Boot(application::RuntimeError),
    /// A delivered payload failed the exact application Message proof.
    Message(application::RuntimeError),
    /// A symbolic boot, update, or view callback failed.
    Invocation(program::Error),
    /// A symbolic component message mapper failed before update.
    Mapping(Arc<program::Error>),
    /// A successful view callback returned something other than a widget value.
    ViewOutput(widget::Error),
    /// A successful theme callback returned something other than an inert theme value.
    ThemeOutput(theme::Error),
    /// A callback returned something other than the validated durable Task plan.
    Task(Arc<task::Error>),
    /// A passive callback or bound stream failed its exact subscription contract.
    Subscription(Arc<subscription::Error>),
    /// A runtime failure paired with its exact retained callback source.
    ExactSource {
        /// Original native adapter failure.
        error: Box<Error>,
        /// Fully materialized canonical report from authenticated callback evidence.
        report: rawr::diagnostic::Report,
    },
}

/// The most messages [`Adapter::deliver`] applies, so Tasks that answer
/// each other end.
pub const DELIVERED: usize = 256;

/// One booted application instance with exact launch authority and last-good output.
pub struct Adapter {
    launch: Launch,
    instance: application::Instance,
    lifecycle: Arc<widget::diagnostic::Sink>,
    state: Dynamic,
    display: widget::Value,
    last_good_output: Option<widget::Value>,
    title: String,
    last_good_title: Option<String>,
    theme: theme::Value,
    last_good_theme: Option<theme::Value>,
    diagnostics: Vec<diagnostic::Entry>,
    startup: task::Plan,
    tasks: effect::Tasks,
    subscription: Option<host::stream::Plan>,
    centered: bool,
    source: Option<rawr::diagnostic::Source>,
    timing: Option<timing::Session>,
    pub(crate) prepared: Option<task::Prepared>,
    /// Tasks [`Adapter::deliver`] left running: a recording polls them
    /// again on each frame, as its clock moves.
    running: Vec<iced::futures::stream::BoxStream<'static, iced_runtime::Action<runtime::Message>>>,
}

pub(crate) struct Draft {
    instance: application::Instance,
    lifecycle: Arc<widget::diagnostic::Sink>,
    state: Dynamic,
    display: widget::Value,
    last_good_output: Option<widget::Value>,
    title: String,
    last_good_title: Option<String>,
    theme: theme::Value,
    last_good_theme: Option<theme::Value>,
    diagnostics: Vec<diagnostic::Entry>,
    startup: task::Plan,
    tasks: effect::Tasks,
    subscription: Option<host::stream::Plan>,
    centered: bool,
    source: Option<rawr::diagnostic::Source>,
    timing: Option<timing::Session>,
}

enum Launch {
    Static(Box<rawr::reload::installation::installed::Application>),
    Reload(program::GenerationId),
}

impl Launch {
    fn generation(&self) -> program::GenerationId {
        match self {
            Self::Static(authority) => authority.generation(),
            Self::Reload(generation) => *generation,
        }
    }

    fn static_authority(&self) -> Option<&rawr::reload::installation::installed::Application> {
        match self {
            Self::Static(authority) => Some(authority),
            Self::Reload(_) => None,
        }
    }
}

/// A one-shot native iced program containing one already-booted Rawr adapter.
pub struct Application {
    initial: Mutex<Option<Adapter>>,
    centered: bool,
}

/// One reloadable application host pairing exact generation proof and TEA state.
pub struct Host {
    pub(crate) slot: rawr::reload::installation::current::Slot<Adapter>,
    timing: Option<timing::Session>,
    /// Why this runner's State booted instead of continuing the previous
    /// runner's, shown once the window opens.
    reseeded: Option<String>,
}

impl Host {
    /// The current State as bytes a new runner process can seed from.
    pub(crate) fn export_state(&self) -> Option<Vec<u8>> {
        let launch = self.slot.context().application()?;
        self.slot.state().export_state_authority(&launch)
    }

    /// Continue from the State a previous runner process exported;
    /// `false` keeps the booted State. No `bytes` means the previous
    /// runner's State could not be carried over. The reason is then noted
    /// when it is a field type of the booted State, such as an imported
    /// Rust type whose code changed.
    pub(crate) fn seed_state(&mut self, bytes: &[u8]) -> bool {
        let timing = self.timing.clone();
        let mut mutation = self.slot.mutation();
        let (adapter, context) = mutation.parts();
        let Some(launch) = context.application() else {
            return false;
        };
        if bytes.is_empty() {
            self.reseeded = launch
                .uncarried_state_field(&adapter.state)
                .map(|ty| format!("State reseeded · App: Rust code changed · {ty} does not carry over"));
            return false;
        }
        adapter.seed_state_authority(&launch, bytes, timing.as_ref())
    }

    /// The note for a State that booted instead of continuing the previous
    /// runner's.
    pub(crate) fn take_reseeded(&mut self) -> Option<String> {
        self.reseeded.take()
    }

    /// The note for an edit to `main`'s statements before the entry, which
    /// ran once when the process started.
    pub(crate) fn prelude_note(&self) -> Option<String> {
        let launch = self.slot.context().application()?;
        prelude_note(&launch.borrow())
    }

    /// Bootstrap one reloadable host without exposing candidate authority.
    pub fn bootstrap(
        bootstrap: rawr::reload::installation::Bootstrap,
        source: Option<rawr::diagnostic::Source>,
    ) -> Result<Self, install::Error> {
        Self::bootstrap_with_timing(bootstrap, source, None)
    }

    /// Bootstrap one reloadable host with an explicit timing observation session.
    #[doc(hidden)]
    pub fn bootstrap_observed(
        bootstrap: rawr::reload::installation::Bootstrap,
        source: Option<rawr::diagnostic::Source>,
        timing: rawr::timing::Session,
    ) -> Result<Self, install::Error> {
        Self::bootstrap_with_timing(bootstrap, source, Some(timing))
    }

    fn bootstrap_with_timing(
        bootstrap: rawr::reload::installation::Bootstrap,
        source: Option<rawr::diagnostic::Source>,
        timing: Option<rawr::timing::Session>,
    ) -> Result<Self, install::Error> {
        let state = {
            let preparation = bootstrap.preparation();
            let catalog = preparation.catalog();
            let launch = catalog.application().ok_or(install::Error::Application)?;
            let mut draft =
                Draft::boot(&launch, source, timing.clone()).map_err(install::Error::Adapter)?;
            draft.timing = None;
            let startup = std::mem::take(&mut draft.startup);
            let prepared = startup
                .prepare_application(&launch)
                .map_err(install::Error::Task)?;
            let mut adapter = draft.install(launch.generation());
            adapter.prepared = Some(prepared);
            adapter
        };
        Ok(Self {
            slot: bootstrap.install(state),
            timing,
            reseeded: None,
        })
    }

    /// Return the exact immutable current generation proof.
    #[must_use]
    pub fn generation(&self) -> Arc<rawr::reload::Generation> {
        self.slot.generation()
    }

    /// Borrow the current application adapter without mutation authority.
    #[must_use]
    pub const fn adapter(&self) -> &Adapter {
        self.slot.state()
    }

    /// Apply one runtime message and rotate the host freshness stamp.
    pub fn update(&mut self, message: runtime::Message) -> Result<Transition, Error> {
        let mut mutation = self.slot.mutation();
        let (adapter, context) = mutation.parts();
        match adapter.authority(&context) {
            Some(authority) => {
                adapter.update_authority_observed(&authority, message, self.timing.as_ref())
            }
            None => Ok(Transition::new(Delivery::Dropped, task::Plan::none())),
        }
    }

    pub(crate) fn update_state(&mut self, message: runtime::Message) -> Result<Transition, Error> {
        let mut mutation = self.slot.mutation();
        let (adapter, context) = mutation.parts();
        match adapter.authority(&context) {
            Some(authority) => adapter.update_state_authority_observed(&authority, message),
            None => Ok(Transition::new(Delivery::Dropped, task::Plan::none())),
        }
    }

    pub(crate) fn settle(
        &mut self,
        instance: application::Instance,
        generation: program::GenerationId,
    ) -> bool {
        let timing = self.timing.clone();
        let mut mutation = self.slot.mutation();
        let (adapter, context) = mutation.parts();
        if adapter.instance() != instance || adapter.generation() != generation {
            return false;
        }
        let Some(authority) = adapter.authority(&context) else {
            return false;
        };
        adapter.settle_authority_observed(&authority, timing.as_ref());
        true
    }

    /// Lower the current durable view under the exact current authority.
    #[must_use]
    pub fn view(&self) -> iced::Element<'static, runtime::Message> {
        let context = self.slot.context();
        let adapter = self.slot.state();
        match adapter.authority(&context) {
            Some(authority) => adapter.view_authority_observed(&authority, self.timing.as_ref()),
            None => widget::diagnostic("Rawr application authority unavailable")
                .lower(&adapter.context()),
        }
    }

    /// Run this reloadable application with its private live-candidate service.
    pub fn run(self, service: crate::reload::Service) -> iced::Result {
        live::Application::new(self, service).run()
    }

    pub(crate) const fn timing(&self) -> Option<&timing::Session> {
        self.timing.as_ref()
    }
}

impl Adapter {
    fn authority(
        &self,
        context: &rawr::reload::installation::installed::Context<'_>,
    ) -> Option<rawr::reload::installation::installed::Application> {
        match &self.launch {
            Launch::Static(authority) => Some(authority.as_ref().clone()),
            Launch::Reload(_) => context.application(),
        }
    }

    /// Boot one bound application into a fresh provenance owner.
    pub fn boot(launch: application::BoundLaunch) -> Result<Self, Error> {
        Self::boot_with_source(launch, None)
    }

    /// Boot while retaining exact source evidence for runtime diagnostics.
    pub fn boot_with_source(
        launch: application::BoundLaunch,
        source: Option<rawr::diagnostic::Source>,
    ) -> Result<Self, Error> {
        Self::boot_with_source_and_timing(launch, source, None)
    }

    /// Boot one bound application with an explicit host timing observation.
    pub fn boot_observed(launch: application::BoundLaunch) -> Result<Self, Error> {
        Self::boot_with_source_observed(launch, None)
    }

    /// Boot with source evidence and an explicit host timing observation.
    pub fn boot_with_source_observed(
        launch: application::BoundLaunch,
        source: Option<rawr::diagnostic::Source>,
    ) -> Result<Self, Error> {
        Self::boot_with_source_and_timing(launch, source, Some(timing::Session::start()))
    }

    pub(crate) fn boot_with_source_and_timing(
        launch: application::BoundLaunch,
        source: Option<rawr::diagnostic::Source>,
        timing: Option<timing::Session>,
    ) -> Result<Self, Error> {
        Self::boot_authority(
            rawr::reload::installation::installed::Application::from_bound(launch),
            source,
            timing,
        )
    }

    pub(crate) fn boot_authority(
        launch: rawr::reload::installation::installed::Application,
        source: Option<rawr::diagnostic::Source>,
        timing: Option<timing::Session>,
    ) -> Result<Self, Error> {
        let authority = launch.borrow();
        let draft = Draft::boot(&authority, source, timing)?;
        Ok(draft.install_authority(launch))
    }
}

/// What the window notes after an edit to `main`'s statements before the
/// entry.
pub(crate) const PRELUDE_NOTE: &str = "This edit to `main` takes effect when `rawr run` restarts.";

/// The text of `main`'s statements before the entry that ran in this
/// process, by package. A package with no such statements keeps "".
static PRELUDES: Mutex<Vec<(String, Box<str>)>> = Mutex::new(Vec::new());

fn prelude_package(launch: &rawr::reload::installation::Application<'_>) -> String {
    launch.update().callback().item().key().package().to_owned()
}

fn prelude_text<'a>(launch: &'a rawr::reload::installation::Application<'_>) -> &'a str {
    launch.prelude().map_or("", application::Prelude::text)
}

/// Run `main`'s statements before the entry before the package's first
/// boot in this process, as Rust runs them once. A reset or a reload boots
/// again without them.
fn run_prelude(launch: &rawr::reload::installation::Application<'_>) -> Result<(), Error> {
    let package = prelude_package(launch);
    {
        let mut ran = PRELUDES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ran.iter().any(|(name, _)| *name == package) {
            return Ok(());
        }
        ran.push((package, prelude_text(launch).into()));
    }
    launch.invoke_prelude().map_err(Error::Boot)
}

/// The note for `main`'s statements before the entry when they differ from
/// the ones that ran in this process.
pub(crate) fn prelude_note(
    launch: &rawr::reload::installation::Application<'_>,
) -> Option<String> {
    let package = prelude_package(launch);
    PRELUDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|(name, text)| *name == package && **text != *prelude_text(launch))
        .then(|| PRELUDE_NOTE.to_owned())
}

impl Draft {
    pub(crate) fn boot(
        launch: &rawr::reload::installation::Application<'_>,
        source: Option<rawr::diagnostic::Source>,
        timing: Option<timing::Session>,
    ) -> Result<Self, Error> {
        run_prelude(launch)?;
        let (state, startup) = match launch.boot() {
            application::Boot::Default => (
                launch.boot_default().map_err(Error::Boot)?,
                task::Plan::none(),
            ),
            application::Boot::Explicit { callback, .. } => {
                let startup = launch
                    .invoke_boot()
                    .map_err(Error::Boot)
                    .map_err(|error| error.with_callback(launch, callback))?;
                let (state, task) = startup.into_parts();
                let plan = task
                    .map(task::Plan::from_dynamic)
                    .transpose()
                    .map_err(|error| Error::Task(Arc::new(error)))
                    .map_err(|error| error.with_callback(launch, callback))?
                    .unwrap_or_default();
                (state, plan)
            }
        };
        let output = observe_result(timing.as_ref(), timing::Phase::ViewEvaluation, || {
            evaluate_view(launch, &state)
        });
        let title = evaluate_title(launch, &state);
        let (title, last_good_title, mut diagnostics) = match title {
            Ok(Some(title)) => (title.clone(), Some(title), Vec::new()),
            Ok(None) => {
                let title = "Rawr application".to_owned();
                (title.clone(), Some(title), Vec::new())
            }
            Err(error) => (
                "Rawr application".to_owned(),
                None,
                vec![diagnostic::Entry::new(
                    diagnostic::Stage::Title,
                    callback_report(
                        &error,
                        launch,
                        launch.title().map(application::Title::callback),
                        source.clone(),
                    ),
                )],
            ),
        };
        let theme = evaluate_theme(launch, &state);
        let (theme, last_good_theme) = match theme {
            Ok(Some(theme)) => (theme.clone(), Some(theme)),
            Ok(None) => {
                let theme = theme::Value::default();
                (theme.clone(), Some(theme))
            }
            Err(error) => {
                diagnostics.push(diagnostic::Entry::new(
                    diagnostic::Stage::Theme,
                    callback_report(
                        &error,
                        launch,
                        launch.theme().map(application::Theme::callback),
                        source.clone(),
                    ),
                ));
                (theme::Value::default(), None)
            }
        };
        let subscription = match evaluate_subscription(launch, &state) {
            Ok(plan) => plan,
            Err(error) => {
                diagnostics.push(diagnostic::Entry::new(
                    diagnostic::Stage::Subscription,
                    callback_report(
                        &error,
                        launch,
                        launch
                            .subscription()
                            .map(application::Subscription::callback),
                        source.clone(),
                    ),
                ));
                None
            }
        };
        let (display, last_good_output) = match output {
            Ok(value) => (value.clone(), Some(value)),
            Err(error) => {
                let report = callback_report(
                    &error,
                    launch,
                    Some(launch.view().callback()),
                    source.clone(),
                );
                diagnostics.push(diagnostic::Entry::new(diagnostic::Stage::View, report));
                (widget::unavailable(error.to_string()), None)
            }
        };
        let instance = application::Instance::new();
        let lifecycle = Arc::new(widget::diagnostic::Sink::new(instance, launch.generation()));
        Ok(Self {
            instance,
            lifecycle,
            state,
            display,
            last_good_output,
            title,
            last_good_title,
            theme,
            last_good_theme,
            diagnostics,
            startup,
            tasks: effect::Tasks::default(),
            subscription,
            centered: launch.centered(),
            source,
            timing,
        })
    }

    pub(crate) fn install(self, generation: program::GenerationId) -> Adapter {
        self.attach(Launch::Reload(generation))
    }

    pub(crate) fn install_authority(
        self,
        authority: rawr::reload::installation::installed::Application,
    ) -> Adapter {
        self.attach(Launch::Static(Box::new(authority)))
    }

    fn attach(self, launch: Launch) -> Adapter {
        let Self {
            instance,
            lifecycle,
            state,
            display,
            last_good_output,
            title,
            last_good_title,
            theme,
            last_good_theme,
            diagnostics,
            startup,
            tasks,
            subscription,
            centered,
            source,
            timing,
        } = self;
        Adapter {
            launch,
            instance,
            lifecycle,
            state,
            display,
            last_good_output,
            title,
            last_good_title,
            theme,
            last_good_theme,
            diagnostics,
            startup,
            tasks,
            subscription,
            centered,
            source,
            timing,
            prepared: None,
            running: Vec::new(),
        }
    }

    pub(crate) const fn theme(&self) -> &theme::Value {
        &self.theme
    }

    pub(crate) fn diagnostics(&self) -> &[diagnostic::Entry] {
        &self.diagnostics
    }

    pub(crate) fn take_startup_plan(&mut self) -> task::Plan {
        std::mem::take(&mut self.startup)
    }

    pub(crate) fn clear_timing(&mut self) {
        self.timing = None;
    }
}

impl Adapter {
    /// Return the current application-instance identity.
    #[must_use]
    pub const fn instance(&self) -> application::Instance {
        self.instance
    }

    /// Return the current executable generation.
    #[must_use]
    pub fn generation(&self) -> program::GenerationId {
        self.launch.generation()
    }

    /// Return the current opaque Rawr State value.
    #[must_use = "the opaque Rawr state belongs to the live adapter"]
    pub const fn state(&self) -> &Dynamic {
        &self.state
    }

    /// Return the last successful durable widget output, if one exists.
    #[must_use]
    pub const fn last_good_output(&self) -> Option<&widget::Value> {
        self.last_good_output.as_ref()
    }

    /// Return diagnostics retained for the later native overlay layer.
    #[must_use]
    pub fn diagnostics(&self) -> &[diagnostic::Entry] {
        &self.diagnostics
    }

    /// Return the authority-limited lowering context for this owner pair.
    #[must_use]
    pub fn context(&self) -> widget::Context<'_> {
        let Some(authority) = self.launch.static_authority() else {
            return widget::Context::unavailable(
                self.instance,
                self.generation(),
                &self.lifecycle,
                &self.theme,
            );
        };
        self.context_authority(authority)
    }

    fn context_authority<'a>(
        &'a self,
        authority: &'a rawr::reload::installation::installed::Application,
    ) -> widget::Context<'a> {
        self.context_authority_observed(authority, self.timing.as_ref())
    }

    fn context_authority_observed<'a>(
        &'a self,
        authority: &'a rawr::reload::installation::installed::Application,
        timing: Option<&'a timing::Session>,
    ) -> widget::Context<'a> {
        let context =
            widget::Context::application(authority, self.instance, &self.lifecycle, &self.theme);
        match timing {
            Some(session) => context.observed(session),
            None => context,
        }
    }

    /// Return the durable descriptor currently shown by the native host.
    #[must_use]
    pub const fn display(&self) -> &widget::Value {
        &self.display
    }

    /// Return the last materialized native window title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Return the last successful state-derived title, if one exists.
    #[must_use]
    pub fn last_good_title(&self) -> Option<&str> {
        self.last_good_title.as_deref()
    }

    /// Return whether the application requests centered initial placement.
    #[must_use]
    pub const fn centered(&self) -> bool {
        self.centered
    }

    /// Return the durable plan produced by explicit boot.
    #[must_use]
    pub const fn startup_plan(&self) -> &task::Plan {
        &self.startup
    }

    /// Return the durable state-derived subscription plan, when configured and valid.
    #[must_use]
    pub const fn subscription_plan(&self) -> Option<&host::stream::Plan> {
        self.subscription.as_ref()
    }

    /// Return the currently retained fully materialized theme.
    #[must_use]
    pub const fn theme(&self) -> &theme::Value {
        &self.theme
    }

    /// Return the last successful state-derived theme, if one exists.
    #[must_use]
    pub const fn last_good_theme(&self) -> Option<&theme::Value> {
        self.last_good_theme.as_ref()
    }

    /// Clone the immutable compiler and runtime timing observations so far.
    #[must_use]
    pub fn timing(&self) -> timing::Snapshot {
        self.timing
            .as_ref()
            .map_or_else(timing::Snapshot::default, timing::Session::snapshot)
    }

    pub(crate) fn clear_timing(&mut self) {
        self.timing = None;
    }

    fn task_authority_from(
        &self,
        authority: &rawr::reload::installation::installed::Application,
    ) -> task::Authority {
        task::Authority::application(authority.clone())
    }

    #[cfg(test)]
    pub(crate) fn task_authority(&self) -> task::Authority {
        task::Authority::application(
            self.launch
                .static_authority()
                .expect("test adapter must retain static application authority")
                .clone(),
        )
    }

    pub(crate) fn lower_task(
        &mut self,
        plan: task::Plan,
    ) -> Result<iced::Task<runtime::Message>, task::Error> {
        let Some(authority) = self.launch.static_authority().cloned() else {
            return Ok(iced::Task::none());
        };
        self.lower_task_authority(plan, &authority)
    }

    pub(crate) fn lower_task_authority(
        &mut self,
        plan: task::Plan,
        authority: &rawr::reload::installation::installed::Application,
    ) -> Result<iced::Task<runtime::Message>, task::Error> {
        if plan.is_none() {
            return Ok(iced::Task::none());
        }
        let owner = effect::Owner::new(self.instance(), self.generation());
        let authority = self.task_authority_from(authority);
        let (task, aborts) = task::collect_aborts(|| {
            plan.lower(owner.instance(), owner.generation(), &authority)
        });
        Ok(self.tasks.track_with(owner, task?, aborts))
    }

    pub(crate) fn lower_prepared_task(
        &mut self,
        prepared: task::Prepared,
        context: &rawr::reload::installation::installed::Context<'_>,
        authority: &rawr::reload::installation::installed::Application,
    ) -> iced::Task<runtime::Message> {
        if prepared.is_none() {
            return iced::Task::none();
        }
        let owner = effect::Owner::new(self.instance(), self.generation());
        let authority = self.task_authority_from(authority);
        let (task, aborts) = task::collect_aborts(|| {
            prepared.lower(owner.instance(), owner.generation(), context, authority)
        });
        self.tasks.track_with(owner, task, aborts)
    }

    /// A task this owner started is still running.
    pub(crate) fn has_running_tasks(&self) -> bool {
        self.tasks.is_running()
    }

    pub(crate) fn activate_prepared(
        &mut self,
        context: &rawr::reload::installation::installed::Context<'_>,
        authority: &rawr::reload::installation::installed::Application,
    ) -> iced::Task<runtime::Message> {
        match self.prepared.take() {
            Some(prepared) => self.lower_prepared_task(prepared, context, authority),
            None => iced::Task::none(),
        }
    }

    pub(crate) fn complete_task(&mut self, completion: &effect::Completion) -> bool {
        self.tasks.complete(completion)
    }

    #[cfg(test)]
    pub(crate) fn task_handles(&self) -> Vec<iced::task::Handle> {
        self.tasks.handles()
    }

    pub(crate) fn lower_subscription<Message, F>(&self, emit: F) -> iced::Subscription<Message>
    where
        Message: Send + 'static,
        F: Fn(runtime::Message) -> Message + Clone + Send + 'static,
    {
        let Some(authority) = self.launch.static_authority() else {
            return iced::Subscription::none();
        };
        self.lower_subscription_authority(authority, emit)
    }

    pub(crate) fn lower_subscription_authority<Message, F>(
        &self,
        authority: &rawr::reload::installation::installed::Application,
        emit: F,
    ) -> iced::Subscription<Message>
    where
        Message: Send + 'static,
        F: Fn(runtime::Message) -> Message + Clone + Send + 'static,
    {
        let Some(plan) = self.subscription.clone() else {
            return iced::Subscription::none();
        };
        subscription::lower(
            authority.bind_stream_plan(plan),
            self.instance(),
            self.generation(),
            emit,
        )
    }

    /// The Message the crate's argument-free function `name` returns,
    /// stamped for this instance; `None` without such a function.
    pub fn message_of(&self, name: &str) -> Option<Result<runtime::Message, Error>> {
        let authority = self.launch.static_authority()?;
        let function = authority.function(name)?;
        Some(
            authority
                .invoke(&function, &mut [])
                .map(|payload| runtime::Message::new(self.instance, self.generation(), payload))
                .map_err(Error::Invocation),
        )
    }

    /// Apply the boot's Task and then `messages` as iced's runtime does:
    /// each Task output ready at once (`Task::done`) is applied after them,
    /// and at most [`DELIVERED`] are applied. A task still waiting keeps
    /// running: each call polls it again, so a timer on a recording's clock
    /// ends as the clock passes it. A failing update stays in the
    /// diagnostics.
    pub fn deliver(&mut self, messages: impl IntoIterator<Item = runtime::Message>) {
        let startup = self.take_startup_plan();
        let mut queue = self.ready(startup);
        queue.extend(messages);
        let mut delivered = 0;
        loop {
            while let Some(message) = queue.pop_front() {
                if delivered == DELIVERED {
                    return;
                }
                delivered += 1;
                if let Ok(transition) = self.update(message) {
                    let ready = self.ready(transition.into_plan());
                    queue.extend(ready);
                }
            }
            // A script step that just ran may have let a running task go on.
            queue.extend(self.poll_running());
            if queue.is_empty() {
                return;
            }
        }
    }

    /// Every output the running tasks have ready. A task that ended is
    /// dropped.
    fn poll_running(&mut self) -> std::collections::VecDeque<runtime::Message> {
        use iced::futures::{FutureExt as _, StreamExt as _};

        let mut ready = std::collections::VecDeque::new();
        self.running.retain_mut(|stream| loop {
            match stream.next().now_or_never() {
                Some(Some(iced_runtime::Action::Output(message))) => ready.push_back(message),
                Some(Some(_)) => {}
                Some(None) => return false,
                None => return true,
            }
        });
        ready
    }

    /// Apply `message` here when it is a script step a task handed back,
    /// through [`Adapter::update`]; otherwise return it. For host tests that
    /// step a task's stream by hand.
    #[doc(hidden)]
    pub fn take_script_step(&mut self, message: runtime::Message) -> Option<runtime::Message> {
        if message.resumption().is_some() {
            let _ = self.update(message);
            None
        } else {
            Some(message)
        }
    }

    /// Drive `task` to its end on this thread, as iced's runtime would with
    /// this adapter. Script steps the task hands back run through
    /// [`Adapter::update`], after its instance and generation check. Every
    /// other output is returned in order. For host tests.
    #[doc(hidden)]
    pub fn drain(&mut self, task: iced::Task<runtime::Message>) -> Option<Vec<runtime::Message>> {
        use iced::futures::StreamExt as _;

        let mut stream = iced_runtime::task::into_stream(task)?;
        let mut outputs = Vec::new();
        iced::futures::executor::block_on(async {
            while let Some(action) = stream.next().await {
                if let iced_runtime::Action::Output(message) = action {
                    if message.resumption().is_some() {
                        let _ = self.update(message);
                    } else {
                        outputs.push(message);
                    }
                }
            }
        });
        Some(outputs)
    }

    fn ready(&mut self, plan: task::Plan) -> std::collections::VecDeque<runtime::Message> {
        use iced::futures::{FutureExt as _, StreamExt as _};

        let mut ready = std::collections::VecDeque::new();
        let Some(mut stream) = self
            .lower_task(plan)
            .ok()
            .and_then(iced_runtime::task::into_stream)
        else {
            return ready;
        };
        loop {
            match stream.next().now_or_never() {
                Some(Some(iced_runtime::Action::Output(message))) => ready.push_back(message),
                Some(Some(_)) => {}
                Some(None) => break,
                None => {
                    self.running.push(stream);
                    break;
                }
            }
        }
        ready
    }

    /// Consume host lifecycle messages and apply only current-provenance payloads.
    pub fn update(&mut self, message: runtime::Message) -> Result<Transition, Error> {
        let Some(authority) = self.launch.static_authority().cloned() else {
            return Ok(Transition::new(Delivery::Dropped, task::Plan::none()));
        };
        self.update_authority(&authority, message)
    }

    /// The State as bytes a new runner process can seed from; `None`
    /// when it holds anything but plain script data.
    pub(crate) fn export_state_authority(
        &self,
        launch: &rawr::reload::installation::installed::Application,
    ) -> Option<Vec<u8>> {
        launch.export_state(&self.state)
    }

    /// Continue from the State a previous runner process exported:
    /// the boot's State and startup tasks give way to it, and view, title,
    /// theme and subscription are evaluated from it. `false` keeps the boot.
    pub(crate) fn seed_state_authority(
        &mut self,
        launch: &rawr::reload::installation::installed::Application,
        bytes: &[u8],
        timing: Option<&timing::Session>,
    ) -> bool {
        let Some(state) = launch.import_state(bytes) else {
            return false;
        };
        self.state = state;
        self.startup = task::Plan::none();
        self.prepared = None;
        self.settle_authority_observed(launch, timing);
        true
    }

    pub(crate) fn update_authority(
        &mut self,
        launch: &rawr::reload::installation::installed::Application,
        message: runtime::Message,
    ) -> Result<Transition, Error> {
        let timing = self.timing.clone();
        self.update_authority_observed(launch, message, timing.as_ref())
    }

    pub(crate) fn update_authority_observed(
        &mut self,
        launch: &rawr::reload::installation::installed::Application,
        message: runtime::Message,
        timing: Option<&timing::Session>,
    ) -> Result<Transition, Error> {
        let transition = self.update_state_authority_observed(launch, message)?;
        if transition.delivery() == Delivery::Applied {
            self.settle_authority_observed(launch, timing);
        }
        Ok(transition)
    }

    pub(crate) fn update_state_authority_observed(
        &mut self,
        launch: &rawr::reload::installation::installed::Application,
        message: runtime::Message,
    ) -> Result<Transition, Error> {
        if let Some(completion) = message.task_completion() {
            let _ = self.complete_task(completion);
            return Ok(Transition::new(Delivery::Dropped, task::Plan::none()));
        }
        if message.instance() != self.instance || message.generation() != self.generation() {
            return Ok(Transition::new(Delivery::Dropped, task::Plan::none()));
        }
        // Script work a native task left for the UI thread runs here, after
        // the freshness check. A stale request was dropped above, which ends
        // the task that sent it.
        if let Some(resume) = message.resumption() {
            return match resume.run() {
                Some(message) => self.update_state_authority_observed(launch, message),
                None => Ok(Transition::new(Delivery::Dropped, task::Plan::none())),
            };
        }
        if let Some(error) = message.mapping_error() {
            let error = Error::Mapping(error.clone());
            self.record_update_error_authority(launch, &error);
            return Err(error);
        }
        if let Some(error) = message.task_failure() {
            let error = error.clone();
            self.record_task_error_authority(launch, Error::Task(error.clone()));
            return Err(Error::Task(error));
        }
        if let Some(error) = message.subscription_failure() {
            let error = error.clone();
            self.record_subscription_error_authority(launch, Error::Subscription(error.clone()));
            return Err(Error::Subscription(error));
        }
        if let Err(error) = launch.validate_message(message.payload()) {
            let error = Error::Message(error);
            self.record_update_error_authority(launch, &error);
            return Err(error);
        }

        let previous = self.state.clone();
        let mut args = [self.state.take(), message.into_payload()];
        let plan = match launch.invoke_update(&mut args) {
            Ok(Some(plan)) => match task::Plan::from_dynamic(plan) {
                Ok(plan) => plan,
                Err(error) => {
                    self.state = previous;
                    let error = Error::Task(Arc::new(error));
                    self.record_update_error_authority(launch, &error);
                    return Err(error);
                }
            },
            Ok(None) => task::Plan::none(),
            Err(error) => {
                self.state = previous;
                let error = Error::Invocation(error);
                self.record_update_error_authority(launch, &error);
                return Err(error);
            }
        };
        if let Err(error) = launch.validate_state(&args[0]) {
            self.state = previous;
            let error = Error::Boot(error);
            self.record_update_error_authority(launch, &error);
            return Err(error);
        }
        self.state = args[0].take();
        self.diagnostics
            .retain(|entry| entry.stage() != diagnostic::Stage::Update);
        Ok(Transition::new(Delivery::Applied, plan))
    }

    pub(crate) fn settle_authority_observed(
        &mut self,
        launch: &rawr::reload::installation::installed::Application,
        timing: Option<&timing::Session>,
    ) {
        let _ = self.refresh_subscription_authority(launch);
        let _ = self.refresh_title_authority(launch);
        let _ = self.refresh_theme_authority(launch);
        let _ = self.refresh_view_authority_observed(launch, timing);
    }

    /// Evaluate view through the bound authority and replace only on success.
    pub fn refresh_view(&mut self) -> Result<&widget::Value, Error> {
        let Some(launch) = self.launch.static_authority().cloned() else {
            return Ok(&self.display);
        };
        self.refresh_view_authority(&launch)
    }

    fn refresh_view_authority(
        &mut self,
        authority: &rawr::reload::installation::installed::Application,
    ) -> Result<&widget::Value, Error> {
        let timing = self.timing.clone();
        self.refresh_view_authority_observed(authority, timing.as_ref())
    }

    fn refresh_view_authority_observed(
        &mut self,
        authority: &rawr::reload::installation::installed::Application,
        timing: Option<&timing::Session>,
    ) -> Result<&widget::Value, Error> {
        let launch = authority.borrow();
        match observe_result(timing, timing::Phase::ViewEvaluation, || {
            evaluate_view(&launch, &self.state)
        }) {
            Ok(value) => {
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::View);
                self.display = value.clone();
                Ok(self.last_good_output.insert(value))
            }
            Err(error) => {
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::View);
                let report = callback_report(
                    &error,
                    &launch,
                    Some(launch.view().callback()),
                    self.source.clone(),
                );
                self.diagnostics.push(diagnostic::Entry::new(
                    diagnostic::Stage::View,
                    report.clone(),
                ));
                if self.last_good_output.is_none() {
                    self.display = widget::diagnostic(report.message());
                }
                Err(error)
            }
        }
    }

    /// Evaluate title through bound authority and replace only on success.
    pub fn refresh_title(&mut self) -> Result<&str, Error> {
        let Some(launch) = self.launch.static_authority().cloned() else {
            return Ok(&self.title);
        };
        self.refresh_title_authority(&launch)
    }

    fn refresh_title_authority(
        &mut self,
        authority: &rawr::reload::installation::installed::Application,
    ) -> Result<&str, Error> {
        let launch = authority.borrow();
        match evaluate_title(&launch, &self.state) {
            Ok(Some(value)) => {
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::Title);
                self.title = value.clone();
                Ok(self.last_good_title.insert(value))
            }
            Ok(None) => {
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::Title);
                Ok(&self.title)
            }
            Err(error) => {
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::Title);
                self.diagnostics.push(diagnostic::Entry::new(
                    diagnostic::Stage::Title,
                    callback_report(
                        &error,
                        &launch,
                        launch.title().map(application::Title::callback),
                        self.source.clone(),
                    ),
                ));
                Err(error)
            }
        }
    }

    /// Evaluate theme through bound authority and replace only on success.
    pub fn refresh_theme(&mut self) -> Result<&theme::Value, Error> {
        let Some(launch) = self.launch.static_authority().cloned() else {
            return Ok(&self.theme);
        };
        self.refresh_theme_authority(&launch)
    }

    fn refresh_theme_authority(
        &mut self,
        authority: &rawr::reload::installation::installed::Application,
    ) -> Result<&theme::Value, Error> {
        let launch = authority.borrow();
        match evaluate_theme(&launch, &self.state) {
            Ok(Some(value)) => {
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::Theme);
                self.theme = value.clone();
                Ok(self.last_good_theme.insert(value))
            }
            Ok(None) => {
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::Theme);
                Ok(&self.theme)
            }
            Err(error) => {
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::Theme);
                self.diagnostics.push(diagnostic::Entry::new(
                    diagnostic::Stage::Theme,
                    callback_report(
                        &error,
                        &launch,
                        launch.theme().map(application::Theme::callback),
                        self.source.clone(),
                    ),
                ));
                Err(error)
            }
        }
    }

    /// Evaluate the state-derived passive subscription once at the TEA boundary.
    pub fn refresh_subscription(&mut self) -> Result<Option<&host::stream::Plan>, Error> {
        let Some(launch) = self.launch.static_authority().cloned() else {
            return Ok(self.subscription.as_ref());
        };
        self.refresh_subscription_authority(&launch)
    }

    fn refresh_subscription_authority(
        &mut self,
        authority: &rawr::reload::installation::installed::Application,
    ) -> Result<Option<&host::stream::Plan>, Error> {
        let launch = authority.borrow();
        match evaluate_subscription(&launch, &self.state) {
            Ok(plan) => {
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::Subscription);
                self.subscription = plan;
                Ok(self.subscription.as_ref())
            }
            Err(error) => {
                self.subscription = None;
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::Subscription);
                self.diagnostics.push(diagnostic::Entry::new(
                    diagnostic::Stage::Subscription,
                    callback_report(
                        &error,
                        &launch,
                        launch
                            .subscription()
                            .map(application::Subscription::callback),
                        self.source.clone(),
                    ),
                ));
                Err(error)
            }
        }
    }

    /// Lower the retained descriptor without invoking Rawr.
    #[must_use]
    pub fn view(&self) -> iced::Element<'static, runtime::Message> {
        let Some(authority) = self.launch.static_authority() else {
            let context = self.context();
            return widget::diagnostic("reload host view requires installed slot authority")
                .lower(&context);
        };
        self.view_authority(authority)
    }

    pub(crate) fn view_authority(
        &self,
        authority: &rawr::reload::installation::installed::Application,
    ) -> iced::Element<'static, runtime::Message> {
        self.view_authority_observed(authority, self.timing.as_ref())
    }

    /// The view without its diagnostic overlays, which fill the window: what
    /// a fitted render measures.
    pub(crate) fn content(&self) -> iced::Element<'static, runtime::Message> {
        self.display.lower(&self.context())
    }

    pub(crate) fn view_authority_observed(
        &self,
        authority: &rawr::reload::installation::installed::Application,
        timing: Option<&timing::Session>,
    ) -> iced::Element<'static, runtime::Message> {
        let context = self.context_authority_observed(authority, timing);
        let content = observe(timing, timing::Phase::DescriptorLowering, || {
            self.display.lower(&context)
        });
        let content = diagnostic::overlay(content, &self.diagnostics);
        widget::diagnostic::overlay(content, self.lifecycle.clone())
    }

    pub(crate) fn take_startup_plan(&mut self) -> task::Plan {
        std::mem::take(&mut self.startup)
    }

    fn record_update_error_authority(
        &mut self,
        authority: &rawr::reload::installation::installed::Application,
        error: &Error,
    ) {
        let launch = authority.borrow();
        self.diagnostics
            .retain(|entry| entry.stage() != diagnostic::Stage::Update);
        self.diagnostics.push(diagnostic::Entry::new(
            diagnostic::Stage::Update,
            callback_report(
                error,
                &launch,
                Some(launch.update().callback()),
                self.source.clone(),
            ),
        ));
    }

    pub(crate) fn record_task_error(&mut self, error: Error) {
        let Some(authority) = self.launch.static_authority().cloned() else {
            return;
        };
        self.record_task_error_authority(&authority, error);
    }

    pub(crate) fn record_task_error_authority(
        &mut self,
        authority: &rawr::reload::installation::installed::Application,
        error: Error,
    ) {
        let launch = authority.borrow();
        self.diagnostics
            .retain(|entry| entry.stage() != diagnostic::Stage::Task);
        self.diagnostics.push(diagnostic::Entry::new(
            diagnostic::Stage::Task,
            callback_report(&error, &launch, None, self.source.clone()),
        ));
    }

    fn record_subscription_error_authority(
        &mut self,
        authority: &rawr::reload::installation::installed::Application,
        error: Error,
    ) {
        let launch = authority.borrow();
        self.subscription = None;
        self.diagnostics
            .retain(|entry| entry.stage() != diagnostic::Stage::Subscription);
        self.diagnostics.push(diagnostic::Entry::new(
            diagnostic::Stage::Subscription,
            callback_report(
                &error,
                &launch,
                launch
                    .subscription()
                    .map(application::Subscription::callback),
                self.source.clone(),
            ),
        ));
    }
}

impl Transition {
    fn new(delivery: Delivery, plan: task::Plan) -> Self {
        Self { delivery, plan }
    }

    /// Return whether the message was applied or dropped.
    #[must_use]
    pub const fn delivery(&self) -> Delivery {
        self.delivery
    }

    /// Return the durable follow-up plan.
    #[must_use]
    pub const fn plan(&self) -> &task::Plan {
        &self.plan
    }

    fn into_plan(self) -> task::Plan {
        self.plan
    }
}

impl PartialEq<Delivery> for Transition {
    fn eq(&self, other: &Delivery) -> bool {
        self.delivery == *other
    }
}

impl Application {
    /// Transform one booted adapter into a native iced program.
    #[must_use]
    pub fn from_adapter(adapter: Adapter) -> Self {
        let centered = adapter.centered();
        Self {
            initial: Mutex::new(Some(adapter)),
            centered,
        }
    }

    /// Run this application in iced's native single-window event loop.
    pub fn run(self) -> iced::Result {
        let centered = self.centered;
        let application = iced::application(
            move || self.boot_native(),
            Self::update_native,
            Self::view_native,
        )
        .title(Self::title_native)
        .theme(Self::theme_native)
        .subscription(Self::subscription_native);
        if centered {
            application.centered().run()
        } else {
            application.run()
        }
    }

    fn boot_native(&self) -> (Adapter, iced::Task<runtime::Message>) {
        let mut adapter = self
            .initial
            .lock()
            .expect("Rawr native application boot state lock must remain available")
            .take()
            .expect("iced must boot a Rawr native application exactly once");
        let plan = adapter.take_startup_plan();
        let task = match adapter.lower_task(plan) {
            Ok(task) => task,
            Err(error) => {
                adapter.record_task_error(Error::Task(Arc::new(error)));
                iced::Task::none()
            }
        };
        (adapter, task)
    }

    fn update_native(
        state: &mut Adapter,
        message: runtime::Message,
    ) -> iced::Task<runtime::Message> {
        state.update(message).map_or_else(
            |_| iced::Task::none(),
            |transition| match state.lower_task(transition.into_plan()) {
                Ok(task) => task,
                Err(error) => {
                    state.record_task_error(Error::Task(Arc::new(error)));
                    iced::Task::none()
                }
            },
        )
    }

    fn view_native(state: &Adapter) -> iced::Element<'_, runtime::Message> {
        state.view()
    }

    fn title_native(state: &Adapter) -> String {
        state.title.clone()
    }

    fn theme_native(state: &Adapter) -> iced::Theme {
        state.theme.native().clone()
    }

    fn subscription_native(state: &Adapter) -> iced::Subscription<runtime::Message> {
        state.lower_subscription(std::convert::identity)
    }
}

impl iced::Program for Application {
    type State = Adapter;
    type Message = runtime::Message;
    type Theme = iced::Theme;
    type Renderer = iced::Renderer;
    type Executor = iced::executor::Default;

    fn name() -> &'static str {
        "rawr_application"
    }

    fn settings(&self) -> iced::Settings {
        iced::Settings::default()
    }

    fn window(&self) -> Option<iced::window::Settings> {
        Some(iced::window::Settings {
            position: if self.centered {
                iced::window::Position::Centered
            } else {
                iced::window::Position::Default
            },
            ..iced::window::Settings::default()
        })
    }

    fn boot(&self) -> (Self::State, iced::Task<Self::Message>) {
        self.boot_native()
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> iced::Task<Self::Message> {
        Self::update_native(state, message)
    }

    fn view<'a>(
        &self,
        state: &'a Self::State,
        _window: iced::window::Id,
    ) -> impl iced::advanced::Widget<Self::Message, Self::Theme, Self::Renderer> + 'a {
        Self::view_native(state)
    }

    fn title(&self, state: &Self::State, _window: iced::window::Id) -> String {
        Self::title_native(state)
    }

    fn theme(&self, state: &Self::State, _window: iced::window::Id) -> Option<Self::Theme> {
        Some(Self::theme_native(state))
    }

    fn subscription(&self, state: &Self::State) -> iced::Subscription<Self::Message> {
        Self::subscription_native(state)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boot(error) => error.fmt(formatter),
            Self::Message(error) => error.fmt(formatter),
            Self::Invocation(error) => error.fmt(formatter),
            Self::Mapping(error) => error.fmt(formatter),
            Self::ViewOutput(error) => error.fmt(formatter),
            Self::ThemeOutput(error) => error.fmt(formatter),
            Self::Task(error) => error.fmt(formatter),
            Self::Subscription(error) => error.fmt(formatter),
            Self::ExactSource { error, .. } => error.fmt(formatter),
        }
    }
}

impl Error {
    /// Transform this callback failure into the canonical shared report.
    #[must_use]
    pub fn report(&self, source: Option<rawr::diagnostic::Source>) -> rawr::diagnostic::Report {
        use rawr::diagnostic::{code, Report};

        let fallback_source = source.clone();
        let report = match self {
            Self::Boot(error) | Self::Message(error) => {
                Report::from_application_runtime(source, error)
            }
            Self::Invocation(error) => Report::from_program(source, error),
            Self::Mapping(error) => Report::from_program(source, error),
            Self::ViewOutput(error) => Ok(report_with_source(
                code::FRAMEWORK_VIEW_OUTPUT,
                error.to_string(),
                source,
            )),
            Self::ThemeOutput(error) => Ok(report_with_source(
                code::FRAMEWORK_THEME_OUTPUT,
                error.to_string(),
                source,
            )),
            Self::Task(error) => Ok(report_with_source(
                code::FRAMEWORK_TASK_OUTPUT,
                error.to_string(),
                source,
            )),
            Self::Subscription(error) => Ok(report_with_source(
                code::FRAMEWORK_SUBSCRIPTION_OUTPUT,
                error.to_string(),
                source,
            )),
            Self::ExactSource { report, .. } => return report.clone(),
        };
        report.unwrap_or_else(|error| {
            let builder = Report::error(
                code::ENVIRONMENT_DIAGNOSTIC,
                format!("cannot retain exact diagnostic evidence: {error}"),
            );
            match fallback_source {
                Some(source) => builder.source(source).finish(),
                None => builder.finish(),
            }
        })
    }

    fn with_callback(
        self,
        launch: &rawr::reload::installation::Application<'_>,
        callback: &application::Callback,
    ) -> Self {
        match launch.callback_evidence(callback) {
            Some(evidence) => {
                let report = report_with_callback_evidence(&self, &evidence);
                Self::ExactSource {
                    error: Box::new(self),
                    report,
                }
            }
            None => self,
        }
    }

    fn report_with_evidence(
        &self,
        evidence: &application::callback::Evidence<'_>,
    ) -> Result<rawr::diagnostic::Report, rawr::diagnostic::Error> {
        use rawr::diagnostic::code;

        match self {
            Self::Boot(error) | Self::Message(error) => evidence.runtime_report(error),
            Self::Invocation(error) => evidence.program_report(error),
            Self::Mapping(error) => evidence.program_report(error),
            Self::ViewOutput(error) => report_with_location(
                code::FRAMEWORK_VIEW_OUTPUT,
                error.to_string(),
                evidence.source().clone(),
                evidence.declaration(),
            ),
            Self::ThemeOutput(error) => report_with_location(
                code::FRAMEWORK_THEME_OUTPUT,
                error.to_string(),
                evidence.source().clone(),
                evidence.declaration(),
            ),
            Self::Task(error) => report_with_location(
                code::FRAMEWORK_TASK_OUTPUT,
                error.to_string(),
                evidence.source().clone(),
                evidence.declaration(),
            ),
            Self::Subscription(error) => report_with_location(
                code::FRAMEWORK_SUBSCRIPTION_OUTPUT,
                error.to_string(),
                evidence.source().clone(),
                evidence.declaration(),
            ),
            Self::ExactSource { report, .. } => Ok(report.clone()),
        }
    }
}

fn callback_report(
    error: &Error,
    launch: &rawr::reload::installation::Application<'_>,
    callback: Option<&application::Callback>,
    fallback: Option<rawr::diagnostic::Source>,
) -> rawr::diagnostic::Report {
    callback
        .and_then(|callback| launch.callback_evidence(callback))
        .map_or_else(
            || error.report(fallback),
            |evidence| report_with_callback_evidence(error, &evidence),
        )
}

fn report_with_callback_evidence(
    error: &Error,
    evidence: &application::callback::Evidence<'_>,
) -> rawr::diagnostic::Report {
    error
        .report_with_evidence(evidence)
        .unwrap_or_else(|error| {
            rawr::diagnostic::Report::error(
                rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                format!("cannot retain exact diagnostic evidence: {error}"),
            )
            .source(evidence.source().clone())
            .finish()
        })
}

fn report_with_source(
    code: rawr::diagnostic::code::Code,
    message: impl Into<rawr::ImmutableString>,
    source: Option<rawr::diagnostic::Source>,
) -> rawr::diagnostic::Report {
    let builder = rawr::diagnostic::Report::error(code, message);
    match source {
        Some(source) => builder.source(source).finish(),
        None => builder.finish(),
    }
}

fn report_with_location(
    code: rawr::diagnostic::code::Code,
    message: impl Into<rawr::ImmutableString>,
    source: rawr::diagnostic::Source,
    declaration: rawr::Span,
) -> Result<rawr::diagnostic::Report, rawr::diagnostic::Error> {
    Ok(rawr::diagnostic::Report::error(code, message)
        .primary(source, declaration, Some("callback declared here".into()))?
        .finish())
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Boot(error) => Some(error),
            Self::Message(error) => Some(error),
            Self::Invocation(error) => Some(error),
            Self::Mapping(error) => Some(error.as_ref()),
            Self::ViewOutput(error) => Some(error),
            Self::ThemeOutput(error) => Some(error),
            Self::Task(error) => Some(error),
            Self::Subscription(error) => Some(error),
            Self::ExactSource { error, .. } => Some(error.as_ref()),
        }
    }
}

fn evaluate_view(
    launch: &rawr::reload::installation::Application<'_>,
    state: &Dynamic,
) -> Result<widget::Value, Error> {
    launch.validate_state(state).map_err(Error::Boot)?;
    let mut args = [state.clone()];
    let output = launch.invoke_view(&mut args).map_err(Error::Invocation)?;
    widget::Value::from_dynamic(output).map_err(Error::ViewOutput)
}

fn evaluate_title(
    launch: &rawr::reload::installation::Application<'_>,
    state: &Dynamic,
) -> Result<Option<String>, Error> {
    launch.validate_state(state).map_err(Error::Boot)?;
    let mut args = [state.clone()];
    launch
        .invoke_title(&mut args)
        .map_err(Error::Invocation)
        .map(|title| {
            title.map(|title| {
                title
                    .into_immutable_string()
                    .expect("bound title output retains its exact String runtime proof")
                    .to_string()
            })
        })
}

fn evaluate_theme(
    launch: &rawr::reload::installation::Application<'_>,
    state: &Dynamic,
) -> Result<Option<theme::Value>, Error> {
    launch.validate_state(state).map_err(Error::Boot)?;
    let mut args = [state.clone()];
    launch
        .invoke_theme(&mut args)
        .map_err(Error::Invocation)?
        // `theme() -> Option<Theme>`: `None` is the default theme.
        .and_then(|mut value| match rawr::option::take(&mut value) {
            Some(option) => option,
            None => Some(value),
        })
        .map(theme::Value::from_dynamic)
        .transpose()
        .map_err(Error::ThemeOutput)
}

fn evaluate_subscription(
    launch: &rawr::reload::installation::Application<'_>,
    state: &Dynamic,
) -> Result<Option<host::stream::Plan>, Error> {
    launch.validate_state(state).map_err(Error::Boot)?;
    let mut args = [state.clone()];
    launch
        .invoke_subscription(&mut args)
        .map_err(Error::Invocation)?
        .map(subscription::from_dynamic)
        .transpose()
        .map_err(|error| Error::Subscription(Arc::new(error)))
}

fn observe<T>(
    session: Option<&timing::Session>,
    phase: timing::Phase,
    operation: impl FnOnce() -> T,
) -> T {
    match session {
        Some(session) => session.observe(phase, operation),
        None => operation(),
    }
}

fn observe_result<T, E>(
    session: Option<&timing::Session>,
    phase: timing::Phase,
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    match session {
        Some(session) => session.observe_result(phase, operation),
        None => operation(),
    }
}

#[cfg(test)]
mod tests {
    use iced_test::futures::futures::StreamExt;
    use rawr::timing::{Outcome, Phase};
    use rawr::{application, host, program, Engine, EvalAltResult, Position, Shared};

    use super::{callback_report, Adapter, Application, Delivery, Error};
    use crate::{diagnostic, runtime};

    #[test]
    fn boot_update_and_repeated_lowering_keep_phase_ownership_separate() {
        let mut adapter = boot_observed();
        assert_eq!(adapter.timing().aggregate(Phase::ViewEvaluation).count(), 1);
        assert_eq!(
            adapter
                .timing()
                .aggregate(Phase::DescriptorLowering)
                .count(),
            0
        );

        drop(adapter.view());
        drop(adapter.view());
        let after_lowering = adapter.timing();
        assert_eq!(after_lowering.aggregate(Phase::ViewEvaluation).count(), 1);
        assert_eq!(
            after_lowering.aggregate(Phase::DescriptorLowering).count(),
            2
        );

        let increment = click(&adapter, "Increment");
        adapter.update(increment).unwrap();
        let after_update = adapter.timing();
        assert_eq!(after_update.aggregate(Phase::ViewEvaluation).count(), 2);
        assert_eq!(after_update.aggregate(Phase::DescriptorLowering).count(), 3);
        assert_eq!(
            after_update.aggregate(Phase::ScriptedHookDispatch).count(),
            0
        );
        assert!(after_update
            .attempts()
            .iter()
            .filter(|attempt| attempt.phase() == Phase::ViewEvaluation)
            .all(|attempt| attempt.outcome() == Outcome::Success));
    }

    #[test]
    fn centered_shell_proof_becomes_the_native_initial_window_position() {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(centered_application_source())
            .unwrap();
        let launch = bind(engine.launch_application(descriptor).unwrap());
        let application = Application::from_adapter(Adapter::boot(launch).unwrap());

        assert!(matches!(
            <Application as iced::Program>::window(&application)
                .expect("the Rawr application must configure one native window")
                .position,
            iced::window::Position::Centered
        ));
    }

    #[test]
    fn stale_instance_and_generation_are_independently_dropped_without_side_effects() {
        let mut adapter = boot();
        let current = click(&adapter, "Increment");
        let display = adapter.display().identity();
        let diagnostics = adapter.diagnostics().to_vec();

        let wrong_instance = application::Instance::new();
        assert_ne!(wrong_instance, adapter.instance());
        let stale = runtime::Message::new(
            wrong_instance,
            adapter.generation(),
            current.payload().clone(),
        );
        let transition = adapter.update(stale).unwrap();
        assert_eq!(transition.delivery(), Delivery::Dropped);
        assert!(transition.plan().is_none());
        assert_eq!(adapter.display().identity(), display);
        assert_eq!(adapter.diagnostics(), diagnostics);
        adapter.refresh_view().unwrap();
        assert_text(&adapter, "0");
        assert_eq!(adapter.diagnostics(), diagnostics);

        let other_generation = boot().generation();
        assert_ne!(other_generation, adapter.generation());
        let stale = runtime::Message::new(
            adapter.instance(),
            other_generation,
            current.payload().clone(),
        );
        let transition = adapter.update(stale).unwrap();
        assert_eq!(transition.delivery(), Delivery::Dropped);
        assert!(transition.plan().is_none());
        assert_eq!(adapter.diagnostics(), diagnostics);
        adapter.refresh_view().unwrap();
        assert_text(&adapter, "0");
        assert_eq!(adapter.diagnostics(), diagnostics);
    }

    #[test]
    fn captured_completion_cannot_reach_a_unit_message_update() {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(unit_message_task_source())
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut adapter = Adapter::boot(bind(launch)).unwrap();
        let plan = adapter.take_startup_plan();
        let task = adapter.lower_task(plan).unwrap();
        assert_eq!(adapter.task_handles().len(), 1);
        let mut messages = task_messages(task);
        assert_eq!(messages.len(), 2);
        let ordinary = messages.remove(0);
        assert!(ordinary.task_completion().is_none());
        let completion = messages
            .pop()
            .expect("the tracked task must finish with a completion message");
        assert!(completion.task_completion().is_some());
        let diagnostics = adapter.diagnostics().to_vec();

        let transition = adapter.update(completion).unwrap();

        assert_eq!(transition.delivery(), Delivery::Dropped);
        assert!(transition.plan().is_none());
        assert!(adapter.task_handles().is_empty());
        assert_text(&adapter, "0");
        assert_eq!(adapter.diagnostics(), diagnostics);
    }

    #[test]
    fn current_stamp_with_a_swapped_payload_is_rejected_before_state_mutation() {
        let mut adapter = boot();
        let display = adapter.display().identity();
        let swapped = adapter.context().message(42_i64.into());
        let view_count = adapter.timing().aggregate(Phase::ViewEvaluation).count();

        let error = adapter.update(swapped).unwrap_err();
        assert!(matches!(error, Error::Message(_)));
        assert_eq!(adapter.display().identity(), display);
        assert_eq!(
            adapter.timing().aggregate(Phase::ViewEvaluation).count(),
            view_count
        );
        adapter.refresh_view().unwrap();
        assert_text(&adapter, "0");
        assert_eq!(adapter.diagnostics().len(), 1);
        assert_eq!(adapter.diagnostics()[0].stage(), diagnostic::Stage::Update);
        assert!(adapter.diagnostics()[0]
            .message()
            .contains("expected `main::Message`"));
        let message = adapter.diagnostics()[0].message().to_owned();
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.find(crate::diagnostic::OVERLAY_TITLE).unwrap();
        simulator.find(message.as_str()).unwrap();
    }

    #[test]
    fn unmapped_callback_position_falls_back_to_exact_declaration() {
        let launch = launch();
        let callback = launch.view().callback();
        let declaration = launch
            .callback_evidence(callback)
            .expect("bound callback must retain declaration evidence")
            .declaration();
        let error = Error::Invocation(program::Error::Evaluation {
            key: callback.item().key().clone(),
            error: EvalAltResult::ErrorRuntime("unmapped".into(), Position::NONE).into(),
        });

        let authority =
            rawr::reload::installation::installed::Application::from_bound(launch.clone());
        let report = callback_report(&error, &authority.borrow(), Some(callback), None);

        assert_eq!(report.labels()[0].span(), declaration);
    }

    #[test]
    fn closures_with_parameters_are_application_callbacks() {
        let state = r#"
use iced::widget::{button, column, text};
#[derive(Default)]
struct State { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Increment }
"#;
        for main in [
            r#"fn main() -> iced::Result {
    iced::run(
        |state: &mut State, message: Message| match message {
            Message::Increment => state.value += 1,
        },
        |state: &State| column![text(state.value), button("Increment").on_press(Message::Increment)],
    )
}"#,
            r#"fn main() -> iced::Result {
    iced::application(State::default, |state: &mut State, message: Message| {
        match message {
            Message::Increment => state.value += 1,
        }
    }, |state: &State| column![text(state.value), button("Increment").on_press(Message::Increment)])
    .run()
}"#,
            // A `_` parameter, as an update that ignores its message.
            r#"fn main() -> iced::Result {
    iced::run(
        |state: &mut State, _: Message| state.value += 1,
        |state: &State| column![text(state.value), button("Increment").on_press(Message::Increment)],
    )
}"#,
        ] {
            let mut engine = Engine::new();
            crate::install(&mut engine).unwrap();
            let engine = Shared::new(engine);
            let descriptor = engine
                .compile_application_descriptor(&format!("{state}{main}"))
                .unwrap();
            let mut adapter =
                Adapter::boot(bind(engine.launch_application(descriptor).unwrap())).unwrap();
            assert_text(&adapter, "0");
            let increment = click(&adapter, "Increment");
            adapter.update(increment).unwrap();
            adapter.refresh_view().unwrap();
            assert_text(&adapter, "1");
        }
    }

    fn boot() -> Adapter {
        Adapter::boot(launch()).unwrap()
    }

    fn launch() -> application::BoundLaunch {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(include_str!("../examples/scripts/application_counter.rs"))
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        bind(launch)
    }

    fn boot_observed() -> Adapter {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(include_str!("../examples/scripts/application_counter.rs"))
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        Adapter::boot_observed(bind(launch)).unwrap()
    }

    fn bind(launch: application::Launch) -> application::BoundLaunch {
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        launch.bind(registry).unwrap()
    }

    fn click(adapter: &Adapter, label: &str) -> runtime::Message {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.click(label).unwrap();
        let mut messages = simulator.into_messages();
        let message = messages
            .next()
            .expect("the enabled button must emit exactly one message");
        assert!(messages.next().is_none());
        message
    }

    fn assert_text(adapter: &Adapter, expected: &str) {
        let mut simulator = iced_test::simulator(adapter.view());
        simulator.find(expected).unwrap();
    }

    fn unit_message_task_source() -> &'static str {
        r#"
use iced::Task;
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State { value: i64 }

impl State {
    fn boot() -> (State, Task<()>) { (State::default(), Task::done(())) }
    fn update(&mut self, message: ()) { self.value += 1 }
    fn view(&self) -> Element<'_, ()> { text(self.value) }
}

pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
    }

    fn centered_application_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

struct State {}
#[derive(Debug, Clone, Copy)] enum Message { Idle }

impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> { text("Centered") }
}

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .centered()
        .run()
}
"#
    }

    fn task_messages(task: iced::Task<runtime::Message>) -> Vec<runtime::Message> {
        let mut stream = iced_test::runtime::task::into_stream(task)
            .expect("a tracked nonempty task must expose a native stream");
        iced_test::futures::futures::executor::block_on(async move {
            let mut messages = Vec::new();
            while let Some(action) = stream.next().await {
                match action {
                    iced_test::runtime::Action::Output(message) => {
                        messages.extend(crate::dispatch::inline(message));
                    }
                    action => panic!("expected a task output, found {action:?}"),
                }
            }
            messages
        })
    }
}
