//! Transactional app/preview sessions behind one native low-chrome host.

use std::fmt;
use std::sync::{Arc, Mutex};

use iced::widget::Widget as _;
use rawr::{application, preview, timing, Dynamic};

use crate::{diagnostic, effect, program, runtime, task, widget};

use super::{catalog, chrome, theme};

const TITLE: &str = "Rawr preview";

pub(crate) mod install;
mod live;

pub(crate) struct Application {
    initial: Mutex<Option<Host>>,
}

pub(crate) struct Host {
    storage: Storage,
    timing: Option<timing::Session>,
    run: bool,
    /// `<project>/target/rawr/preview`: the preview `rawr preview` opens next.
    memory: Option<std::path::PathBuf>,
}

enum Storage {
    Static {
        catalog: catalog::Authority,
        state: State,
    },
    Reload(rawr::reload::installation::current::Slot<State>),
}

pub(crate) struct State {
    control_previews: Box<[crate::control::Preview]>,
    source: rawr::diagnostic::Source,
    choices: Vec<Choice>,
    selection: Selection,
    application: Option<program::Adapter>,
    preview: Option<Preview>,
    last_good_theme: crate::theme::Value,
    failure: Option<rawr::diagnostic::Report>,
    pending: Option<Pending>,
    ready: Option<iced::Task<Message>>,
    pending_settles: Vec<Stamp>,
}

struct Pending {
    instance: application::Instance,
    plan: task::Plan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Stamp {
    instance: application::Instance,
    generation: rawr::program::GenerationId,
}

enum Owner {
    Application,
    Preview(preview::Key),
}

type ControlAdmission = rawr::reload::scheduler::Admission<
    (),
    (
        Box<[rawr::reload::compatibility::Evidence]>,
        rawr::reload::scheduler::publication::Retired,
    ),
>;

struct Preview {
    key: preview::Key,
    instance: application::Instance,
    lifecycle: Arc<widget::diagnostic::Sink>,
    state: Option<Dynamic>,
    display: widget::Value,
    theme: crate::theme::Value,
    selected_theme: Option<usize>,
    diagnostics: Vec<diagnostic::Entry>,
    tasks: effect::Tasks,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Selection {
    Application,
    Preview(preview::Key),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Choice {
    selection: Selection,
    label: String,
    /// The preview's group; the picker shows one entry per group.
    group: Option<String>,
}

#[derive(Clone, Debug)]
enum Control {
    Select(Selection),
    /// Move to the next (`true`) or previous preview of the selected one's
    /// group, wrapping.
    Step(bool),
    Close,
    Reset,
    Theme(theme::Choice),
    Screenshot,
}

#[derive(Clone, Debug)]
enum Message {
    Control(Control),
    Script(runtime::Message),
    Settle(Stamp),
}

impl Application {
    pub(crate) fn new(host: Host) -> Self {
        Self {
            initial: Mutex::new(Some(host)),
        }
    }

    pub(crate) fn run(self) -> iced::Result {
        iced::application(
            move || self.boot_native(),
            Host::update_native,
            Host::view_native,
        )
        .title(TITLE)
        .theme(Host::theme_native)
        .subscription(|host| {
            iced::Subscription::batch([Host::subscription_native(host), Host::shortcuts()])
        })
        .run()
    }

    fn boot_native(&self) -> (Host, iced::Task<Message>) {
        let mut host = self
            .initial
            .lock()
            .expect("Rawr preview boot state lock must remain available")
            .take()
            .expect("iced must boot a Rawr preview host exactly once");
        let (pending, ready) = host.take_boot();
        let task = iced::Task::batch([host.lower(pending), ready]);
        (host, task)
    }
}

impl Host {
    pub(crate) fn new(
        catalog: Arc<preview::BoundCatalog>,
        source: rawr::diagnostic::Source,
        timing: Option<timing::Session>,
    ) -> Result<Self, rawr::diagnostic::Report> {
        let catalog = catalog::Authority::static_(catalog);
        let state = State::from_catalog(&catalog, source, timing.as_ref())?;
        Ok(Self {
            storage: Storage::Static { catalog, state },
            timing,
            run: false,
            memory: None,
        })
    }

    pub(crate) fn bootstrap(
        bootstrap: rawr::reload::installation::Bootstrap,
        source: rawr::diagnostic::Source,
        timing: Option<timing::Session>,
    ) -> Result<Self, rawr::diagnostic::Report> {
        let state = {
            let preparation = bootstrap.preparation();
            install::Draft::bootstrap(preparation.catalog(), source, timing.as_ref())
                .map(install::Draft::seal)
        }?;
        let mut host = Self {
            storage: Storage::Reload(bootstrap.install(state)),
            timing,
            run: false,
            memory: None,
        };
        let task = host.activate_application();
        host.apply(|state, _catalog, _context| state.ready = Some(task));
        Ok(host)
    }

    /// Present this host as `rawr run`: the application's own window title and
    /// placement, with the preview controls as the only added chrome.
    pub(crate) const fn into_run(mut self) -> Self {
        self.run = true;
        self
    }

    /// Present this host as `rawr preview`: open straight into the preview named
    /// in `<root>/target/rawr/preview` (written by `rawr preview <name>` and by each
    /// later pick), else the first one. A retained application stays booted
    /// behind it.
    pub(crate) fn into_preview(mut self, root: &std::path::Path) -> Self {
        let memory = root.join("target/rawr/preview");
        let wanted = std::fs::read_to_string(&memory).unwrap_or_default();
        let wanted = wanted.trim();
        let previews = self.control_previews();
        let key = previews
            .iter()
            .find(|preview| preview.matches(wanted))
            .or_else(|| {
                if !wanted.is_empty() {
                    eprintln!(
                        "rawr preview: no preview named `{wanted}`; opening the first. Previews: {}",
                        previews
                            .iter()
                            .map(crate::control::Preview::label)
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                previews.first()
            })
            .map(|preview| preview.key().clone());
        if let Some(key) = key {
            let timing = self.timing.clone();
            // The preview's own pending work is empty; the application's
            // startup stays pending so it boots behind the preview.
            self.apply(|state, catalog, _context| {
                state.select_preview(catalog, key, None, timing.as_ref())
            });
        }
        self.memory = Some(memory);
        self
    }

    /// Remember a picked preview for the next `rawr preview`.
    fn remember(&self) {
        let (Some(memory), Selection::Preview(key)) = (&self.memory, &self.selection) else {
            return;
        };
        let Some(preview) = self
            .control_previews()
            .iter()
            .find(|preview| preview.key() == key)
        else {
            return;
        };
        if let Some(parent) = memory.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(memory, preview.label());
    }

    fn title(&self) -> String {
        match (&self.application, self.run) {
            (Some(application), true) => application.title().to_owned(),
            (Some(_) | None, false) | (None, true) => TITLE.to_owned(),
        }
    }

    fn centered(&self) -> bool {
        self.run
            && self
                .application
                .as_ref()
                .is_some_and(program::Adapter::centered)
    }

    /// Name and bottom chrome height for a capture of the visible surface.
    pub(super) fn screenshot_subject(&self) -> (String, f32) {
        let name = match &self.selection {
            Selection::Application => self.title(),
            Selection::Preview(_) => self
                .choices
                .iter()
                .find(|choice| choice.selection == self.selection)
                .map_or_else(|| TITLE.to_owned(), |choice| choice.label.clone()),
        };
        // The footer and its divider are dropped; a failure panel keeps the
        // whole window so the report is captured too.
        let chrome = if self.failure.is_some() {
            0.0
        } else {
            chrome::FOOTER_MAX_HEIGHT + 1.0
        };
        (name, chrome)
    }

    fn inspect<R>(&self, inspect: impl FnOnce(&State, &catalog::Authority) -> R) -> R {
        match &self.storage {
            Storage::Static { catalog, state } => inspect(state, catalog),
            Storage::Reload(slot) => {
                let context = slot.context();
                let catalog = catalog::Authority::reload(context.catalog());
                inspect(slot.state(), &catalog)
            }
        }
    }

    fn apply<R>(
        &mut self,
        apply: impl FnOnce(
            &mut State,
            &catalog::Authority,
            Option<&rawr::reload::installation::installed::Context<'_>>,
        ) -> R,
    ) -> R {
        match &mut self.storage {
            Storage::Static { catalog, state } => apply(state, catalog, None),
            Storage::Reload(slot) => {
                let mut mutation = slot.mutation();
                let (state, context) = mutation.parts();
                let catalog = catalog::Authority::reload(context.catalog());
                apply(state, &catalog, Some(&context))
            }
        }
    }

    fn generation(&self) -> Option<Arc<rawr::reload::Generation>> {
        match &self.storage {
            Storage::Static { .. } => None,
            Storage::Reload(slot) => Some(slot.generation()),
        }
    }

    #[cfg(test)]
    fn catalog(&self) -> catalog::Authority {
        self.inspect(|_state, catalog| catalog.clone())
    }

    fn activate_application(&mut self) -> iced::Task<Message> {
        self.apply(|state, catalog, context| {
            let Some(application) = state.application.as_mut() else {
                return iced::Task::none();
            };
            match (catalog.application(), context) {
                (Some(authority), Some(context)) => application
                    .activate_prepared(context, &authority)
                    .map(Message::Script),
                (Some(_), None) | (None, Some(_)) | (None, None) => iced::Task::none(),
            }
        })
    }

    /// The note for an edit to `main`'s statements before the entry.
    pub(crate) fn prelude_note(&self) -> Option<String> {
        let Storage::Reload(slot) = &self.storage else {
            return None;
        };
        let launch = slot.context().application()?;
        crate::program::prelude_note(&launch.borrow())
    }

    /// The running application's State as bytes a new runner process can
    /// seed from.
    pub(crate) fn export_state(&self) -> Option<Vec<u8>> {
        let Storage::Reload(slot) = &self.storage else {
            return None;
        };
        let launch = slot.context().application()?;
        slot.state()
            .application
            .as_ref()?
            .export_state_authority(&launch)
    }

    /// Continue the running application from the State a previous runner
    /// process exported; its boot tasks give way. `false` keeps the
    /// booted State.
    pub(crate) fn seed_state(&mut self, bytes: &[u8]) -> bool {
        let timing = self.timing.clone();
        let Storage::Reload(slot) = &mut self.storage else {
            return false;
        };
        let mut mutation = slot.mutation();
        let (state, context) = mutation.parts();
        let Some(launch) = context.application() else {
            return false;
        };
        let Some(adapter) = state.application.as_mut() else {
            return false;
        };
        if !adapter.seed_state_authority(&launch, bytes, timing.as_ref()) {
            return false;
        }
        if state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.instance == adapter.instance())
        {
            state.pending = None;
        }
        true
    }

    fn take_boot(&mut self) -> (Option<Pending>, iced::Task<Message>) {
        self.apply(|state, _catalog, _context| {
            (
                state.pending.take(),
                state.ready.take().unwrap_or_else(iced::Task::none),
            )
        })
    }

    fn select(&mut self, selection: Selection, selected_theme: Option<usize>) -> Option<Pending> {
        let timing = self.timing.clone();
        let pending = self.apply(|state, catalog, _context| {
            state.select(catalog, selection, selected_theme, timing.as_ref())
        });
        self.remember();
        pending
    }

    pub(crate) fn select_control(
        &mut self,
        key: &preview::Key,
    ) -> Result<(), rawr::diagnostic::Report> {
        let timing = self.timing.clone();
        self.apply(|state, catalog, _context| state.select_control(catalog, key, timing.as_ref()))
    }

    pub(crate) fn reset_control(&mut self) -> Result<(), rawr::diagnostic::Report> {
        let timing = self.timing.clone();
        self.apply(|state, catalog, _context| state.reset_control(catalog, timing.as_ref()))
    }

    pub(crate) fn close_preview(&mut self) {
        let timing = self.timing.clone();
        self.apply(|state, catalog, _context| state.close_preview(catalog, timing.as_ref()));
    }

    pub(crate) fn install_control(
        &mut self,
        gate: &rawr::reload::scheduler::Gate,
        publication: &rawr::reload::scheduler::publication::Slot,
        candidate: rawr::reload::scheduler::Candidate<(
            rawr::reload::Candidate,
            rawr::reload::scheduler::publication::Staged,
        )>,
        engine: &rawr::Shared<rawr::Engine>,
        runtime: rawr::host::runtime::Registry,
    ) -> Result<ControlAdmission, install::Error> {
        match self.install(gate, publication, candidate, engine, runtime)? {
            rawr::reload::scheduler::Admission::Current(outcome) => {
                let evidence = outcome.evidence().to_vec().into_boxed_slice();
                let (task, inputs) = outcome.into_parts();
                let Some(inputs) = inputs else {
                    return Err(install::Error::Stale);
                };
                self.apply(|state, _catalog, _context| state.ready = Some(task));
                Ok(rawr::reload::scheduler::Admission::Current((
                    evidence, inputs,
                )))
            }
            rawr::reload::scheduler::Admission::Obsolete(()) => {
                Ok(rawr::reload::scheduler::Admission::Obsolete(()))
            }
        }
    }

    #[cfg(test)]
    pub(super) fn plan_for_test(
        &self,
        candidate: rawr::reload::Candidate,
    ) -> Option<rawr::reload::compatibility::Plan> {
        let generation = self.generation()?;
        let owners = install::owners(self).ok()?;
        Some(rawr::reload::compatibility::Plan::compare(
            generation, candidate, owners,
        ))
    }

    /// Step to the next or previous preview of the group. A preview that fails
    /// is still the selection, beside its report, so stepping goes on past it.
    fn step(&mut self, forward: bool) -> Option<Pending> {
        let selection = self.neighbor(forward)?;
        let pending = self.select(selection.clone(), None);
        if pending.is_none() {
            self.apply(|state, _catalog, _context| state.selection = selection);
        }
        pending
    }

    fn reset(&mut self) -> Option<Pending> {
        let timing = self.timing.clone();
        self.apply(|state, catalog, _context| state.reset(catalog, timing.as_ref()))
    }

    fn select_theme(&mut self, choice: theme::Choice) -> bool {
        self.apply(|state, catalog, _context| state.select_theme(catalog, choice))
    }

    fn update_script_pending(
        &mut self,
        message: runtime::Message,
    ) -> (Option<Pending>, Option<Stamp>) {
        self.apply(|state, catalog, _context| state.update_script_pending(catalog, message))
    }

    fn lower(&mut self, pending: Option<Pending>) -> iced::Task<Message> {
        self.apply(|state, catalog, _context| state.lower(catalog, pending))
    }

    fn complete_task(&mut self, completion: &effect::Completion) {
        self.apply(|state, _catalog, _context| state.complete_task(completion));
    }

    fn view(&self) -> iced::Element<'_, Message> {
        self.inspect(|state, catalog| state.view(catalog, self.timing.as_ref()))
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        self.inspect(State::subscription)
    }

    fn update_native(state: &mut Self, message: Message) -> iced::Task<Message> {
        let (pending, settle) = match message {
            Message::Control(Control::Select(selection)) => (state.select(selection, None), None),
            Message::Control(Control::Step(forward)) => (state.step(forward), None),
            Message::Control(Control::Close) => {
                state.close_preview();
                (
                    state.apply(|state, _catalog, _context| state.pending.take()),
                    None,
                )
            }
            Message::Control(Control::Reset) => (state.reset(), None),
            Message::Control(Control::Theme(choice)) => {
                state.select_theme(choice);
                (None, None)
            }
            // Captures need the live host's status owner (`live.rs`).
            Message::Control(Control::Screenshot) => (None, None),
            Message::Script(message) => {
                if let Some(completion) = message.task_completion() {
                    state.complete_task(completion);
                    (None, None)
                } else {
                    state.update_script_pending(message)
                }
            }
            Message::Settle(stamp) => (None, Some(stamp)),
        };
        if let Some(stamp) = settle {
            let timing = state.timing.clone();
            state.apply(|state, catalog, _context| state.settle(catalog, stamp, timing.as_ref()));
        }
        state.lower(pending)
    }

    fn update_state_native(state: &mut Self, message: Message) -> iced::Task<Message> {
        let (pending, settle) = match message {
            Message::Control(Control::Select(selection)) => (state.select(selection, None), None),
            Message::Control(Control::Step(forward)) => (state.step(forward), None),
            Message::Control(Control::Close) => {
                state.close_preview();
                (
                    state.apply(|state, _catalog, _context| state.pending.take()),
                    None,
                )
            }
            Message::Control(Control::Reset) => (state.reset(), None),
            Message::Control(Control::Theme(choice)) => {
                state.select_theme(choice);
                (None, None)
            }
            // Captures need the live host's status owner (`live.rs`).
            Message::Control(Control::Screenshot) => (None, None),
            Message::Script(message) => {
                if let Some(completion) = message.task_completion() {
                    state.complete_task(completion);
                    (None, None)
                } else {
                    state.update_script_pending(message)
                }
            }
            Message::Settle(stamp) => {
                let timing = state.timing.clone();
                state.apply(|state, catalog, _context| {
                    state.settle(catalog, stamp, timing.as_ref())
                });
                (None, None)
            }
        };
        let task = state.lower(pending);
        match settle {
            Some(stamp) => iced::Task::batch([task, iced::Task::done(Message::Settle(stamp))]),
            None => task,
        }
    }

    fn view_native(state: &Self) -> iced::Element<'_, Message> {
        state.view()
    }

    fn theme_native(state: &Self) -> iced::Theme {
        state.visible_theme().map_or_else(
            || crate::theme::Value::default().native().clone(),
            |theme| theme.native().clone(),
        )
    }

    fn subscription_native(state: &Self) -> iced::Subscription<Message> {
        state.subscription()
    }

    fn shortcuts() -> iced::Subscription<Message> {
        iced::event::listen_with(|event, status, _window| shortcut(event, status))
    }
}

impl std::ops::Deref for Host {
    type Target = State;

    fn deref(&self) -> &Self::Target {
        match &self.storage {
            Storage::Static { state, .. } => state,
            Storage::Reload(slot) => slot.state(),
        }
    }
}

impl State {
    fn from_catalog(
        catalog: &catalog::Authority,
        source: rawr::diagnostic::Source,
        timing: Option<&timing::Session>,
    ) -> Result<Self, rawr::diagnostic::Report> {
        let choices = choices(catalog);
        let control_previews = catalog
            .previews()
            .iter()
            .map(crate::control::Preview::from_definition)
            .collect();
        let selection = choices
            .first()
            .map(|choice| choice.selection.clone())
            .ok_or_else(|| {
                plain_report("preview Catalog contains no selectable entries", &source)
            })?;
        let mut application = None;
        let mut preview = None;
        let mut last_good_theme = crate::theme::Value::default();
        let mut failure = None;
        let mut pending = None;
        match &selection {
            Selection::Application => match application_candidate(catalog, &source, timing) {
                Ok((adapter, startup)) => {
                    last_good_theme = adapter.theme().clone();
                    pending = Some(Pending {
                        instance: adapter.instance(),
                        plan: startup,
                    });
                    application = Some(adapter);
                }
                Err(report) => failure = Some(report),
            },
            Selection::Preview(key) => {
                match preview_candidate(catalog, key, None, &last_good_theme, &source, timing) {
                    Ok(session) => {
                        last_good_theme = session.theme.clone();
                        preview = Some(session);
                    }
                    Err(report) => failure = Some(report),
                }
            }
        };
        Ok(Self {
            control_previews,
            source,
            choices,
            selection,
            application,
            preview,
            last_good_theme,
            failure,
            pending,
            ready: None,
            pending_settles: Vec::new(),
        })
    }

    fn select(
        &mut self,
        catalog: &catalog::Authority,
        selection: Selection,
        selected_theme: Option<usize>,
        timing: Option<&timing::Session>,
    ) -> Option<Pending> {
        match selection {
            Selection::Application => self.select_application(catalog, timing),
            Selection::Preview(key) => self.select_preview(catalog, key, selected_theme, timing),
        }
    }

    /// The selected preview's group: its members and the selected index.
    fn group(&self) -> Option<(Vec<&crate::control::Preview>, usize)> {
        let Selection::Preview(key) = &self.selection else {
            return None;
        };
        let group = self
            .control_previews
            .iter()
            .find(|preview| preview.key() == key)?
            .group();
        let members: Vec<_> = self
            .control_previews
            .iter()
            .filter(|preview| preview.group() == group)
            .collect();
        let index = members.iter().position(|preview| preview.key() == key)?;
        Some((members, index))
    }

    /// The next or previous preview in the selected one's group, wrapping.
    fn neighbor(&self, forward: bool) -> Option<Selection> {
        let (members, index) = self.group()?;
        let count = members.len();
        let next = if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        };
        (next != index).then(|| Selection::Preview(members[next].key().clone()))
    }

    pub(crate) const fn selection(&self) -> &Selection {
        &self.selection
    }

    pub(crate) const fn control_previews(&self) -> &[crate::control::Preview] {
        &self.control_previews
    }

    pub(crate) fn is_active(&self) -> bool {
        self.visible_theme().is_some()
    }

    pub(crate) fn theme_name(&self) -> Option<String> {
        self.visible_theme().map(|theme| theme.native().to_string())
    }

    pub(crate) fn reports(&self) -> Vec<&rawr::diagnostic::Report> {
        let mut reports = Vec::new();
        if let Some(report) = &self.failure {
            reports.push(report);
        }
        if let Some(application) = &self.application {
            reports.extend(
                application
                    .diagnostics()
                    .iter()
                    .map(diagnostic::Entry::report),
            );
        }
        if let Some(preview) = &self.preview {
            reports.extend(preview.diagnostics.iter().map(diagnostic::Entry::report));
        }
        reports
    }

    fn select_control(
        &mut self,
        catalog: &catalog::Authority,
        key: &preview::Key,
        timing: Option<&timing::Session>,
    ) -> Result<(), rawr::diagnostic::Report> {
        if let Some(pending) = self.select_preview(catalog, key.clone(), None, timing) {
            self.pending = Some(pending);
            return Ok(());
        }
        Err(self.failure.clone().unwrap_or_else(|| {
            plain_report(
                "preview selection failed without a canonical report",
                &self.source,
            )
        }))
    }

    fn reset_control(
        &mut self,
        catalog: &catalog::Authority,
        timing: Option<&timing::Session>,
    ) -> Result<(), rawr::diagnostic::Report> {
        if matches!(self.selection, Selection::Application) {
            return Err(plain_report("no preview is selected", &self.source));
        }
        if let Some(pending) = self.reset(catalog, timing) {
            self.pending = Some(pending);
            return Ok(());
        }
        Err(self.failure.clone().unwrap_or_else(|| {
            plain_report(
                "preview reset failed without a canonical report",
                &self.source,
            )
        }))
    }

    fn select_application(
        &mut self,
        catalog: &catalog::Authority,
        timing: Option<&timing::Session>,
    ) -> Option<Pending> {
        if let Some(adapter) = &self.application {
            let pending = Pending {
                instance: adapter.instance(),
                plan: task::Plan::none(),
            };
            self.preview = None;
            self.selection = Selection::Application;
            self.failure = None;
            return Some(pending);
        }
        match application_candidate(catalog, &self.source, timing) {
            Ok((adapter, startup)) => {
                self.last_good_theme = adapter.theme().clone();
                let pending = Pending {
                    instance: adapter.instance(),
                    plan: startup,
                };
                self.application = Some(adapter);
                self.preview = None;
                self.selection = Selection::Application;
                self.failure = None;
                Some(pending)
            }
            Err(report) => {
                self.failure = Some(report);
                None
            }
        }
    }

    fn select_preview(
        &mut self,
        catalog: &catalog::Authority,
        key: preview::Key,
        selected_theme: Option<usize>,
        timing: Option<&timing::Session>,
    ) -> Option<Pending> {
        let inherited = self.inherited_theme();
        match preview_candidate(
            catalog,
            &key,
            selected_theme,
            &inherited,
            &self.source,
            timing,
        ) {
            Ok(session) => {
                self.last_good_theme = session.theme.clone();
                let pending = Pending {
                    instance: session.instance,
                    plan: task::Plan::none(),
                };
                self.selection = Selection::Preview(key);
                self.preview = Some(session);
                self.failure = None;
                Some(pending)
            }
            Err(report) => {
                if self.application.is_none() && self.preview.is_none() {
                    self.selection = Selection::Preview(key);
                }
                self.failure = Some(report);
                None
            }
        }
    }

    fn close_preview(&mut self, catalog: &catalog::Authority, timing: Option<&timing::Session>) {
        if self.application.is_some() || catalog.application().is_some() {
            self.pending = self.select_application(catalog, timing);
            return;
        }
        self.preview = None;
        self.pending = None;
        self.failure = None;
    }

    fn reset(
        &mut self,
        catalog: &catalog::Authority,
        timing: Option<&timing::Session>,
    ) -> Option<Pending> {
        match &self.selection {
            Selection::Application => match application_candidate(catalog, &self.source, timing) {
                Ok((adapter, startup)) => {
                    self.last_good_theme = adapter.theme().clone();
                    let pending = Pending {
                        instance: adapter.instance(),
                        plan: startup,
                    };
                    self.application = Some(adapter);
                    self.failure = None;
                    Some(pending)
                }
                Err(report) => {
                    self.failure = Some(report);
                    None
                }
            },
            Selection::Preview(key) => {
                let key = key.clone();
                let selected_theme = self
                    .preview
                    .as_ref()
                    .and_then(|session| session.selected_theme);
                self.select_preview(catalog, key, selected_theme, timing)
            }
        }
    }

    fn select_theme(&mut self, catalog: &catalog::Authority, choice: theme::Choice) -> bool {
        let inherited = self.inherited_theme();
        let Some(session) = &mut self.preview else {
            return false;
        };
        let next = match choice {
            theme::Choice::Inherited => Ok((inherited, None)),
            theme::Choice::Explicit { index, .. } => catalog
                .theme(&session.key, index, session.state.as_ref())
                .map_err(|error| preview_report(catalog, &session.key, Stage::Theme(index), error))
                .and_then(|value| {
                    crate::theme::Value::from_dynamic(value)
                        .map(|theme| (theme, Some(index)))
                        .map_err(|error| {
                            positioned_report(
                                catalog,
                                &session.key,
                                Stage::Theme(index),
                                rawr::diagnostic::code::FRAMEWORK_THEME_OUTPUT,
                                error.to_string(),
                            )
                        })
                }),
        };
        match next {
            Ok((theme, selected)) => {
                session.theme = theme.clone();
                session.selected_theme = selected;
                self.last_good_theme = theme;
                self.failure = None;
                true
            }
            Err(report) => {
                self.failure = Some(report);
                false
            }
        }
    }

    fn update_script(
        &mut self,
        catalog: &catalog::Authority,
        message: runtime::Message,
    ) -> (task::Plan, Option<Stamp>) {
        if message.is_subscription() && !matches!(self.selection, Selection::Application) {
            return (task::Plan::none(), None);
        }
        let instance = message.instance();
        let generation = message.generation();
        if let Some(adapter) = self
            .application
            .as_mut()
            .filter(|adapter| adapter.instance() == instance)
        {
            let Some(authority) = catalog.application() else {
                return (task::Plan::none(), None);
            };
            return adapter
                .update_state_authority_observed(&authority, message)
                .map_or_else(
                    |_| (task::Plan::none(), None),
                    |transition| {
                        let stamp = (transition.delivery() == program::Delivery::Applied)
                            .then_some(Stamp {
                                instance,
                                generation,
                            });
                        (transition.plan().clone(), stamp)
                    },
                );
        }
        let Some(session) = &mut self.preview else {
            return (task::Plan::none(), None);
        };
        if session.instance != instance {
            return (task::Plan::none(), None);
        }
        let (plan, applied) = session.update_state(catalog, message);
        let stamp = applied.then_some(Stamp {
            instance,
            generation,
        });
        (plan, stamp)
    }

    fn update_script_pending(
        &mut self,
        catalog: &catalog::Authority,
        message: runtime::Message,
    ) -> (Option<Pending>, Option<Stamp>) {
        let instance = message.instance();
        let (plan, settle) = self.update_script(catalog, message);
        let generation = catalog.generation();
        let application = self.application.as_ref().map(program::Adapter::instance);
        let preview = self.preview.as_ref().map(|preview| preview.instance);
        self.pending_settles.retain(|stamp| {
            stamp.generation == generation
                && (Some(stamp.instance) == application || Some(stamp.instance) == preview)
        });
        let pending = self
            .owner(instance)
            .is_some()
            .then_some(Pending { instance, plan });
        let settle = settle.filter(|stamp| {
            if self.pending_settles.contains(stamp) {
                false
            } else {
                self.pending_settles.push(*stamp);
                true
            }
        });
        (pending, settle)
    }

    fn settle(
        &mut self,
        catalog: &catalog::Authority,
        stamp: Stamp,
        timing: Option<&timing::Session>,
    ) {
        let Some(index) = self
            .pending_settles
            .iter()
            .position(|pending| *pending == stamp)
        else {
            return;
        };
        self.pending_settles.swap_remove(index);
        if catalog.generation() != stamp.generation {
            return;
        }
        if let Some(adapter) = self
            .application
            .as_mut()
            .filter(|adapter| adapter.instance() == stamp.instance)
        {
            let Some(authority) = catalog.application() else {
                return;
            };
            adapter.settle_authority_observed(&authority, timing);
            if adapter
                .diagnostics()
                .iter()
                .all(|entry| entry.stage() != diagnostic::Stage::Theme)
                && let Some(theme) = adapter.last_good_theme()
            {
                self.last_good_theme = theme.clone();
            }
            return;
        }
        let Some(session) = self
            .preview
            .as_mut()
            .filter(|session| session.instance == stamp.instance)
        else {
            return;
        };
        session.settle(catalog, timing);
        if session
            .diagnostics
            .iter()
            .all(|entry| entry.stage() != diagnostic::Stage::Theme)
        {
            self.last_good_theme = session.theme.clone();
        }
    }

    fn owner(&self, instance: application::Instance) -> Option<Owner> {
        if self
            .application
            .as_ref()
            .is_some_and(|adapter| adapter.instance() == instance)
        {
            return Some(Owner::Application);
        }
        self.preview
            .as_ref()
            .filter(|session| session.instance == instance)
            .map(|session| Owner::Preview(session.key.clone()))
    }

    fn inherited_theme(&self) -> crate::theme::Value {
        self.application.as_ref().map_or_else(
            || self.last_good_theme.clone(),
            |adapter| adapter.theme().clone(),
        )
    }

    fn visible_theme(&self) -> Option<&crate::theme::Value> {
        match &self.selection {
            Selection::Application => self.application.as_ref().map(program::Adapter::theme),
            Selection::Preview(_) => self.preview.as_ref().map(|session| &session.theme),
        }
    }

    fn visible_view(
        &self,
        catalog: &catalog::Authority,
        timing: Option<&timing::Session>,
    ) -> Option<iced::Element<'static, runtime::Message>> {
        match &self.selection {
            Selection::Application => self.application.as_ref().and_then(|application| {
                catalog
                    .application()
                    .map(|authority| application.view_authority_observed(&authority, timing))
            }),
            Selection::Preview(_) => self
                .preview
                .as_ref()
                .map(|session| session.view(catalog, timing)),
        }
    }

    fn active_preview(&self) -> Option<&Preview> {
        match self.selection {
            Selection::Preview(_) => self.preview.as_ref(),
            Selection::Application => None,
        }
    }

    fn subscription(&self, catalog: &catalog::Authority) -> iced::Subscription<Message> {
        match &self.selection {
            Selection::Application => {
                self.application
                    .as_ref()
                    .map_or_else(iced::Subscription::none, |application| {
                        catalog
                            .application()
                            .map_or_else(iced::Subscription::none, |authority| {
                                application
                                    .lower_subscription_authority(&authority, Message::Script)
                            })
                    })
            }
            Selection::Preview(_) => iced::Subscription::none(),
        }
    }

    fn view(
        &self,
        catalog: &catalog::Authority,
        timing: Option<&timing::Session>,
    ) -> iced::Element<'static, Message> {
        let (picker, selected) = self.picker();
        let selector = iced::widget::container(
            iced::widget::pick_list(selected, picker, Choice::to_string)
                .on_select(|choice| Message::Control(Control::Select(choice.selection)))
                .style(chrome::pick_list)
                .text_size(12)
                .padding([2, 6]),
        )
        .id(chrome::SELECTOR_ID);
        let mut selection = iced::widget::row![]
            .spacing(4)
            .align_y(iced::Alignment::Center);
        let group = self.group().filter(|(members, _)| members.len() > 1);
        if group.is_some() {
            selection = selection.push(step("‹", false));
        }
        selection = selection.push(selector.boxed());
        if let Some((members, index)) = group {
            selection = selection.push(step("›", true)).push(
                iced::widget::container(
                    iced::widget::text(format!("{}/{}", index + 1, members.len()))
                        .size(12)
                        .color(chrome::accent()),
                )
                .id(chrome::POSITION_ID)
                .boxed(),
            );
        }
        if self.active_preview().is_some() {
            selection = selection.push(
                iced::widget::container(
                    iced::widget::button(iced::widget::text("Close").size(12))
                        .on_press(Message::Control(Control::Close))
                        .padding([2, 6])
                        .style(chrome::button),
                )
                .id(chrome::CLOSE_ID)
                .boxed(),
            );
        }
        let selection = iced::widget::container(selection)
            .id(chrome::SELECTION_ID)
            .width(iced::Length::FillPortion(1))
            .align_x(iced::alignment::Horizontal::Center);

        let status = iced::widget::container(iced::widget::text(self.status_label()).size(12))
            .id(chrome::STATUS_ID)
            .width(iced::Length::FillPortion(1))
            .align_x(iced::alignment::Horizontal::Left);

        let reset = iced::widget::container(
            iced::widget::button(iced::widget::text("Reset").size(12))
                .on_press(Message::Control(Control::Reset))
                .padding([2, 6])
                .style(chrome::button),
        )
        .id(chrome::RESET_ID);
        let cue = iced::widget::text(self.effective_theme_label(catalog))
            .size(12)
            .color(chrome::accent());
        let mut actions = iced::widget::row![cue]
            .spacing(4)
            .align_y(iced::Alignment::Center);
        if let Some((themes, selected)) = self.theme_choices(catalog) {
            actions = actions.push(
                iced::widget::container(
                    iced::widget::pick_list(selected, themes, theme::Choice::to_string)
                        .on_select(|choice| Message::Control(Control::Theme(choice)))
                        .style(chrome::pick_list)
                        .text_size(12)
                        .padding([2, 6]),
                )
                .id(chrome::THEME_ID)
                .boxed(),
            );
        }
        let screenshot = iced::widget::container(
            iced::widget::tooltip(
                iced::widget::button(iced::widget::text("Shot").size(12))
                    .on_press(Message::Control(Control::Screenshot))
                    .padding([2, 6])
                    .style(chrome::button),
                iced::widget::container(iced::widget::text("Save PNG · Cmd+Shift+S").size(11))
                    .padding([2, 6])
                    .style(chrome::surface),
                iced::widget::tooltip::Position::Top,
            )
            .gap(4),
        )
        .id(chrome::SCREENSHOT_ID);
        let actions = iced::widget::container(actions.push(screenshot.boxed()).push(reset.boxed()))
            .id(chrome::ACTIONS_ID)
            .width(iced::Length::FillPortion(1))
            .align_x(iced::alignment::Horizontal::Right);
        let controls = iced::widget::row![status, selection, actions]
            .width(iced::Length::Fill)
            .align_y(iced::Alignment::Center);
        let content = self.visible_view(catalog, timing).map_or_else(
            || {
                iced::widget::container(iced::widget::text("No preview session is active").size(16))
                    .width(iced::Length::Fill)
                    .height(iced::Length::Fill)
                    .center(iced::Length::Fill)
                    .boxed()
            },
            |content| content.map(Message::Script).boxed(),
        );
        let content = iced::widget::container(content)
            .id(chrome::CONTENT_ID)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill);
        let footer = iced::widget::container(controls)
            .id(chrome::FOOTER_ID)
            .width(iced::Length::Fill)
            .height(iced::Length::Fixed(chrome::FOOTER_MAX_HEIGHT))
            .padding([4, 6])
            .align_y(iced::alignment::Vertical::Center)
            .style(chrome::surface);
        let mut chrome = iced::widget::column![];
        if let Some(report) = &self.failure {
            chrome = chrome.push(
                iced::widget::container(diagnostic::report_detail("preview host", report))
                    .id(chrome::DIAGNOSTIC_ID)
                    .width(iced::Length::Fill)
                    .padding([8, 10])
                    .style(chrome::surface)
                    .boxed(),
            );
        }
        let chrome = chrome
            .push(
                iced::widget::rule::horizontal(1)
                    .style(chrome::divider)
                    .boxed(),
            )
            .push(footer.boxed());
        let chrome = iced::widget::themer(Some(chrome::theme()), chrome);

        iced::widget::column![content, chrome].boxed()
    }

    fn theme_choices(
        &self,
        catalog: &catalog::Authority,
    ) -> Option<(Vec<theme::Choice>, Option<theme::Choice>)> {
        let session = self.active_preview()?;
        let definition = definition(catalog, &session.key)?;
        let mut choices = vec![theme::Choice::Inherited];
        choices.extend(
            definition
                .themes()
                .iter()
                .enumerate()
                .map(|(index, theme)| theme::Choice::Explicit {
                    index,
                    name: theme.name().to_owned(),
                }),
        );
        let selected = match session.selected_theme {
            Some(index) => choices.get(index + 1).cloned(),
            None => Some(theme::Choice::Inherited),
        };
        Some((choices, selected))
    }

    /// The picker's entries, one per group: the selected preview stands
    /// for its group, any other group by its first preview as `group (n)`;
    /// ‹ › step within a group. Then the entry selected.
    fn picker(&self) -> (Vec<Choice>, Option<Choice>) {
        let selected = self
            .choices
            .iter()
            .find(|choice| choice.selection == self.selection);
        let size = |group: &str| {
            self.choices
                .iter()
                .filter(|choice| choice.group.as_deref() == Some(group))
                .count()
        };
        let mut picker: Vec<Choice> = Vec::new();
        for choice in &self.choices {
            let Some(group) = choice.group.as_deref().filter(|group| size(group) > 1) else {
                picker.push(choice.clone());
                continue;
            };
            if picker
                .iter()
                .any(|entry| entry.group.as_deref() == Some(group))
            {
                continue;
            }
            picker.push(
                match selected.filter(|selected| selected.group.as_deref() == Some(group)) {
                    Some(selected) => selected.clone(),
                    None => Choice {
                        label: format!("{group} ({})", size(group)),
                        ..choice.clone()
                    },
                },
            );
        }
        (picker, selected.cloned())
    }

    fn status_label(&self) -> String {
        let selected = self
            .choices
            .iter()
            .find(|choice| choice.selection == self.selection)
            .map_or("Unavailable", |choice| choice.label.as_str());
        if self.failure.is_some() {
            format!("Rawr · {selected} · Error")
        } else if self.visible_theme().is_none() {
            format!("Rawr · {selected} · Closed")
        } else {
            format!("Rawr · {selected} · Ready")
        }
    }

    fn effective_theme_label(&self, catalog: &catalog::Authority) -> String {
        let native = self.visible_theme().map_or_else(
            || crate::theme::Value::default().native().to_string(),
            |theme| theme.native().to_string(),
        );
        let Some(session) = self.active_preview() else {
            return format!("Theme → {native}");
        };
        let selected = session
            .selected_theme
            .and_then(|index| definition(catalog, &session.key)?.themes().get(index))
            .map_or("Inherited", |theme| theme.name());
        format!("{selected} → {native}")
    }

    fn lower(
        &mut self,
        catalog: &catalog::Authority,
        pending: Option<Pending>,
    ) -> iced::Task<Message> {
        let Some(pending) = pending else {
            return iced::Task::none();
        };
        let Some(owner) = self.owner(pending.instance) else {
            return iced::Task::none();
        };
        let lowered = match &owner {
            Owner::Application => match (self.application.as_mut(), catalog.application()) {
                (Some(application), Some(authority)) => {
                    application.lower_task_authority(pending.plan, &authority)
                }
                (Some(_), None) | (None, Some(_)) | (None, None) => Ok(iced::Task::none()),
            },
            Owner::Preview(key) => {
                match self.preview.as_mut().filter(|session| &session.key == key) {
                    Some(session) => session.lower_task(catalog, pending.plan),
                    None => Ok(iced::Task::none()),
                }
            }
        };
        match lowered {
            Ok(task) => task.map(Message::Script),
            Err(error) => {
                match owner {
                    Owner::Application => {
                        if let Some(application) = &mut self.application {
                            match catalog.application() {
                                Some(authority) => application.record_task_error_authority(
                                    &authority,
                                    program::Error::Task(Arc::new(error)),
                                ),
                                None => application
                                    .record_task_error(program::Error::Task(Arc::new(error))),
                            }
                        }
                    }
                    Owner::Preview(key) => {
                        if let Some(session) = &mut self.preview {
                            session.record(
                                diagnostic::Stage::Task,
                                positioned_report(
                                    catalog,
                                    &key,
                                    Stage::Update,
                                    rawr::diagnostic::code::FRAMEWORK_TASK_OUTPUT,
                                    error.to_string(),
                                ),
                            );
                        }
                    }
                }
                iced::Task::none()
            }
        }
    }

    fn complete_task(&mut self, completion: &effect::Completion) {
        if self
            .application
            .as_mut()
            .is_some_and(|application| application.complete_task(completion))
        {
            return;
        }
        if let Some(preview) = &mut self.preview {
            let _ = preview.tasks.complete(completion);
        }
    }
}

impl Preview {
    fn lower_task(
        &mut self,
        catalog: &catalog::Authority,
        plan: task::Plan,
    ) -> Result<iced::Task<runtime::Message>, task::Error> {
        if plan.is_none() {
            return Ok(iced::Task::none());
        }
        let owner = effect::Owner::new(self.instance, catalog.generation());
        let authority = task::Authority::preview(catalog.clone(), self.key.clone());
        let task = plan.lower(owner.instance(), owner.generation(), &authority)?;
        Ok(self.tasks.track(owner, task))
    }

    fn update_state(
        &mut self,
        catalog: &catalog::Authority,
        message: runtime::Message,
    ) -> (task::Plan, bool) {
        if message.instance() != self.instance || message.generation() != catalog.generation() {
            return (task::Plan::none(), false);
        }
        // Script work a native task left for the UI thread runs here, after
        // the freshness check. A stale request was dropped above.
        if let Some(resume) = message.resumption() {
            return match resume.run() {
                Some(message) => self.update_state(catalog, message),
                None => (task::Plan::none(), false),
            };
        }
        if let Some(error) = message.mapping_error() {
            self.record(
                diagnostic::Stage::Update,
                program_report(catalog, &self.key, Stage::Update, error),
            );
            return (task::Plan::none(), false);
        }
        if let Some(error) = message.task_failure() {
            self.record(
                diagnostic::Stage::Task,
                positioned_report(
                    catalog,
                    &self.key,
                    Stage::Update,
                    rawr::diagnostic::code::FRAMEWORK_TASK_OUTPUT,
                    error.to_string(),
                ),
            );
            return (task::Plan::none(), false);
        }
        // A `bind` preview without an update only shows its arguments.
        if definition(catalog, &self.key).is_some_and(|definition| definition.update().is_none()) {
            return (task::Plan::none(), false);
        }
        let mut plan = task::Plan::none();
        if let Some(state) = &mut self.state {
            let transition = match catalog.update(&self.key, state, message.into_payload()) {
                Ok(transition) => transition,
                Err(error) => {
                    self.record(
                        diagnostic::Stage::Update,
                        preview_report(catalog, &self.key, Stage::Update, error),
                    );
                    return (task::Plan::none(), false);
                }
            };
            let (candidate, output) = match transition.try_into_parts(task::Plan::from_dynamic) {
                Ok(parts) => parts,
                Err(error) => {
                    self.record(
                        diagnostic::Stage::Update,
                        positioned_report(
                            catalog,
                            &self.key,
                            Stage::Update,
                            rawr::diagnostic::code::FRAMEWORK_TASK_OUTPUT,
                            error.to_string(),
                        ),
                    );
                    return (task::Plan::none(), false);
                }
            };
            plan = output.unwrap_or_default();
            *state = candidate;
        }
        self.diagnostics
            .retain(|entry| entry.stage() != diagnostic::Stage::Update);
        (plan, true)
    }

    fn settle(&mut self, catalog: &catalog::Authority, timing: Option<&timing::Session>) {
        self.refresh_theme(catalog);
        self.refresh_view(catalog, timing);
    }

    fn refresh_theme(&mut self, catalog: &catalog::Authority) {
        let Some(index) = self.selected_theme else {
            return;
        };
        let result = catalog
            .theme(&self.key, index, self.state.as_ref())
            .map_err(|error| preview_report(catalog, &self.key, Stage::Theme(index), error))
            .and_then(|value| {
                crate::theme::Value::from_dynamic(value).map_err(|error| {
                    positioned_report(
                        catalog,
                        &self.key,
                        Stage::Theme(index),
                        rawr::diagnostic::code::FRAMEWORK_THEME_OUTPUT,
                        error.to_string(),
                    )
                })
            });
        match result {
            Ok(theme) => {
                self.theme = theme;
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::Theme);
            }
            Err(report) => self.record(diagnostic::Stage::Theme, report),
        }
    }

    fn refresh_view(&mut self, catalog: &catalog::Authority, timing: Option<&timing::Session>) {
        let evaluate = || {
            catalog
                .subject(&self.key, self.state.as_ref())
                .map_err(|error| preview_report(catalog, &self.key, Stage::Subject, error))
                .and_then(|value| {
                    widget::Value::from_dynamic(value).map_err(|error| {
                        positioned_report(
                            catalog,
                            &self.key,
                            Stage::Subject,
                            rawr::diagnostic::code::FRAMEWORK_VIEW_OUTPUT,
                            error.to_string(),
                        )
                    })
                })
        };
        let result = match timing {
            Some(timing) => timing.observe_result(timing::Phase::ViewEvaluation, evaluate),
            None => evaluate(),
        };
        match result {
            Ok(display) => {
                self.display = display;
                self.diagnostics
                    .retain(|entry| entry.stage() != diagnostic::Stage::View);
            }
            Err(report) => self.record(diagnostic::Stage::View, report),
        }
    }

    fn view(
        &self,
        catalog: &catalog::Authority,
        timing: Option<&timing::Session>,
    ) -> iced::Element<'static, runtime::Message> {
        let context = widget::Context::preview(
            catalog,
            &self.key,
            self.instance,
            &self.lifecycle,
            &self.theme,
        );
        let context = match timing {
            Some(session) => context.observed(session),
            None => context,
        };
        let content = match timing {
            Some(session) => session.observe(timing::Phase::DescriptorLowering, || {
                self.display.lower(&context)
            }),
            None => self.display.lower(&context),
        };
        let content = diagnostic::overlay(content, &self.diagnostics);
        widget::diagnostic::overlay(content, self.lifecycle.clone())
    }

    fn record(&mut self, stage: diagnostic::Stage, report: rawr::diagnostic::Report) {
        self.diagnostics.retain(|entry| entry.stage() != stage);
        self.diagnostics.push(diagnostic::Entry::new(stage, report));
    }
}

/// Shift+Esc closes the visible preview and reveals the application, as in
/// granita. Bare Escape stays with the hosted program. ←/→ that the program
/// leaves alone step through the selected preview's group.
fn shortcut(event: iced::Event, status: iced::event::Status) -> Option<Message> {
    use iced::keyboard::key::Named;

    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
            modifiers,
            repeat: false,
            ..
        }) if status == iced::event::Status::Ignored
            && modifiers == iced::keyboard::Modifiers::SHIFT =>
        {
            Some(Message::Control(Control::Close))
        }
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Named(named @ (Named::ArrowLeft | Named::ArrowRight)),
            modifiers,
            ..
        }) if status == iced::event::Status::Ignored && modifiers.is_empty() => {
            Some(Message::Control(Control::Step(named == Named::ArrowRight)))
        }
        _ => None,
    }
}

fn step(label: &'static str, forward: bool) -> iced::Element<'static, Message> {
    iced::widget::container(
        iced::widget::button(iced::widget::text(label).size(12))
            .on_press(Message::Control(Control::Step(forward)))
            .padding([2, 6])
            .style(chrome::button),
    )
    .id(if forward {
        chrome::NEXT_ID
    } else {
        chrome::PREVIOUS_ID
    })
    .boxed()
}

fn choices(catalog: &dyn catalog::Evaluation) -> Vec<Choice> {
    let mut choices =
        Vec::with_capacity(catalog.previews().len() + usize::from(catalog.has_application()));
    if catalog.has_application() {
        choices.push(Choice {
            selection: Selection::Application,
            label: "Application".to_owned(),
            group: None,
        });
    }
    choices.extend(catalog.previews().iter().map(|definition| Choice {
        selection: Selection::Preview(definition.key().clone()),
        label: if definition.group().is_empty() {
            definition.name().to_owned()
        } else {
            format!("{} / {}", definition.group(), definition.name())
        },
        group: Some(definition.group().to_owned()).filter(|group| !group.is_empty()),
    }));
    choices
}

fn application_candidate(
    catalog: &catalog::Authority,
    source: &rawr::diagnostic::Source,
    timing: Option<&timing::Session>,
) -> Result<(program::Adapter, task::Plan), rawr::diagnostic::Report> {
    let launch = catalog
        .application()
        .ok_or_else(|| plain_report("Catalog has no application selection", source))?;
    let mut adapter =
        program::Adapter::boot_authority(launch, Some(source.clone()), timing.cloned())
            .map_err(|error| error.report(Some(source.clone())))?;
    if adapter.last_good_output().is_none() || adapter.last_good_theme().is_none() {
        return Err(adapter.diagnostics().last().map_or_else(
            || plain_report("application preview candidate failed", source),
            |entry| entry.report().clone(),
        ));
    }
    let startup = adapter.startup_plan().clone();
    adapter.clear_timing();
    Ok((adapter, startup))
}

fn preview_candidate(
    catalog: &dyn catalog::Evaluation,
    key: &preview::Key,
    selected_theme: Option<usize>,
    inherited: &crate::theme::Value,
    source: &rawr::diagnostic::Source,
    timing: Option<&timing::Session>,
) -> Result<Preview, rawr::diagnostic::Report> {
    if definition(catalog, key).is_none() {
        return Err(plain_report(
            "preview selection is not in this Catalog",
            source,
        ));
    }
    let state = catalog
        .seed(key)
        .map_err(|error| preview_report(catalog, key, Stage::Seed, error))?;
    let theme = match selected_theme {
        Some(index) => catalog
            .theme(key, index, state.as_ref())
            .map_err(|error| preview_report(catalog, key, Stage::Theme(index), error))
            .and_then(|value| {
                crate::theme::Value::from_dynamic(value).map_err(|error| {
                    positioned_report(
                        catalog,
                        key,
                        Stage::Theme(index),
                        rawr::diagnostic::code::FRAMEWORK_THEME_OUTPUT,
                        error.to_string(),
                    )
                })
            })?,
        None => inherited.clone(),
    };
    let evaluate = || {
        catalog
            .subject(key, state.as_ref())
            .map_err(|error| preview_report(catalog, key, Stage::Subject, error))
            .and_then(|value| {
                widget::Value::from_dynamic(value).map_err(|error| {
                    positioned_report(
                        catalog,
                        key,
                        Stage::Subject,
                        rawr::diagnostic::code::FRAMEWORK_VIEW_OUTPUT,
                        error.to_string(),
                    )
                })
            })
    };
    let display = match timing {
        Some(timing) => timing.observe_result(timing::Phase::ViewEvaluation, evaluate),
        None => evaluate(),
    }?;
    let instance = application::Instance::new();
    let lifecycle = Arc::new(widget::diagnostic::Sink::new(
        instance,
        catalog.generation(),
    ));
    Ok(Preview {
        key: key.clone(),
        instance,
        lifecycle,
        state,
        display,
        theme,
        selected_theme,
        diagnostics: Vec::new(),
        tasks: effect::Tasks::default(),
    })
}

fn definition<'a>(
    catalog: &'a dyn catalog::Evaluation,
    key: &preview::Key,
) -> Option<&'a preview::Definition> {
    catalog
        .previews()
        .iter()
        .find(|definition| definition.key() == key)
}

#[derive(Clone, Copy)]
enum Stage {
    Seed,
    Update,
    Subject,
    Theme(usize),
}

fn preview_report(
    catalog: &dyn catalog::Evaluation,
    key: &preview::Key,
    stage: Stage,
    error: preview::RuntimeError,
) -> rawr::diagnostic::Report {
    match error {
        preview::RuntimeError::Invocation(error) => program_report(catalog, key, stage, &error),
        preview::RuntimeError::SeedConstruction(error) => positioned_report(
            catalog,
            key,
            stage,
            rawr::diagnostic::code::FRAMEWORK_CONSTRUCTION,
            error.to_string(),
        ),
        preview::RuntimeError::Value(error) => positioned_report(
            catalog,
            key,
            stage,
            rawr::diagnostic::code::RUNTIME_VALUE,
            error.to_string(),
        ),
        error @ (preview::RuntimeError::UnknownPreview
        | preview::RuntimeError::ForeignSource
        | preview::RuntimeError::MissingSource) => positioned_report(
            catalog,
            key,
            stage,
            rawr::diagnostic::code::FRAMEWORK_AUTHORITY,
            error.to_string(),
        ),
    }
}

fn program_report(
    catalog: &dyn catalog::Evaluation,
    key: &preview::Key,
    stage: Stage,
    error: &rawr::program::Error,
) -> rawr::diagnostic::Report {
    let report = evidence(catalog, key, stage).map_or_else(
        || rawr::diagnostic::Report::from_program(None, error),
        |(source, location)| {
            rawr::diagnostic::Report::from_program_at(source, location.span(), error)
        },
    );
    report.unwrap_or_else(|_| {
        positioned_report(
            catalog,
            key,
            stage,
            rawr::diagnostic::code::FRAMEWORK_AUTHORITY,
            error.to_string(),
        )
    })
}

fn evidence(
    catalog: &dyn catalog::Evaluation,
    key: &preview::Key,
    stage: Stage,
) -> Option<(rawr::diagnostic::Source, preview::Location)> {
    let definition = definition(catalog, key)?;
    let location = match stage {
        Stage::Seed => definition
            .locations()
            .seed()
            .map_or(definition.locations().attribute(), |location| {
                location.declaration()
            }),
        Stage::Update => definition
            .locations()
            .update()
            .map_or(definition.locations().subject(), |location| {
                location.declaration()
            }),
        Stage::Subject => definition.locations().subject(),
        Stage::Theme(index) => definition
            .locations()
            .themes()
            .get(index)
            .map_or(definition.locations().attribute(), |location| {
                location.declaration()
            }),
    };
    catalog
        .source(location)
        .ok()
        .map(|source| (source, location))
}

fn positioned_report(
    catalog: &dyn catalog::Evaluation,
    key: &preview::Key,
    stage: Stage,
    code: rawr::diagnostic::code::Code,
    message: String,
) -> rawr::diagnostic::Report {
    let Some((source, location)) = evidence(catalog, key, stage) else {
        return rawr::diagnostic::Report::error(code, message).finish();
    };
    let builder = rawr::diagnostic::Report::error(code, message);
    builder
        .primary(source.clone(), location.span(), None)
        .map_or_else(
            |_| {
                rawr::diagnostic::Report::error(code, "preview callback failed")
                    .source(source)
                    .finish()
            },
            |builder| builder.finish(),
        )
}

fn plain_report(message: &str, source: &rawr::diagnostic::Source) -> rawr::diagnostic::Report {
    rawr::diagnostic::Report::error(rawr::diagnostic::code::FRAMEWORK_NATIVE, message)
        .source(source.clone())
        .finish()
}

impl fmt::Display for Choice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.label)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::hash::Hasher as _;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use iced_test::futures::futures::StreamExt;
    use rawr::{host as rawr_host, timing, Dynamic, Engine, Position, Shared, Span};

    use super::{chrome, install, theme, Application, Control, Host, Message, Selection, TITLE};
    use crate::preview::Compiler;

    static NEXT_PROJECT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn preview_shell_title_does_not_inherit_the_selected_application_title() {
        let host = host(titled_application_source());
        let application = host
            .application
            .as_ref()
            .expect("the titled application must be selected");
        assert_eq!(application.title(), "Source-owned title");
        assert!(application.centered());
        assert_eq!(TITLE, "Rawr preview");
    }

    #[test]
    fn arrows_step_through_the_group_wrapping_past_a_failing_preview() {
        let mut host = host(full_source());
        let compact = selection(&host, "counter / Compact");
        let default = selection(&host, "counter / Default");
        let broken = selection(&host, "counter / Broken");
        assert!(
            host.neighbor(true).is_none(),
            "the application has no group"
        );
        let _task = Host::update_native(
            &mut host,
            Message::Control(Control::Select(compact.clone())),
        );

        use iced::keyboard::{key, Event, Key, Location, Modifiers};
        let arrow = |named, modifiers| {
            iced::Event::Keyboard(Event::KeyPressed {
                key: Key::Named(named),
                modified_key: Key::Named(named),
                physical_key: key::Physical::Code(key::Code::ArrowRight),
                location: Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        let ignored = iced::event::Status::Ignored;
        assert!(
            super::shortcut(arrow(key::Named::ArrowRight, Modifiers::SHIFT), ignored).is_none()
        );
        assert!(super::shortcut(
            arrow(key::Named::ArrowRight, Modifiers::NONE),
            iced::event::Status::Captured
        )
        .is_none());
        let right =
            || super::shortcut(arrow(key::Named::ArrowRight, Modifiers::NONE), ignored).unwrap();
        let left = super::shortcut(arrow(key::Named::ArrowLeft, Modifiers::NONE), ignored).unwrap();

        let members = |host: &Host| {
            host.group()
                .map(|(members, index)| (members.len(), index))
                .unwrap()
        };
        let index = members(&host).1;
        let _task = Host::update_native(&mut host, right());
        assert_eq!(host.selection(), &default);
        assert_eq!(members(&host), (3, (index + 1) % 3));
        // Broken fails, but is selected beside its report, so the next step
        // wraps around to Compact.
        let _task = Host::update_native(&mut host, right());
        assert_eq!(host.selection(), &broken);
        assert!(host.failure.is_some());
        let _task = Host::update_native(&mut host, right());
        assert_eq!(host.selection(), &compact);
        assert!(host.failure.is_none());
        let _task = Host::update_native(&mut host, left);
        assert_eq!(host.selection(), &broken);
        // The footer's › arrow steps too, and shows the position.
        let position = format!("{}/3", members(&host).1 + 1);
        let mut simulator = iced_test::simulator(host.view());
        assert!(simulator.find(position.as_str()).is_ok());
        simulator
            .click(iced_test::selector::id(chrome::NEXT_ID))
            .unwrap();
        let step = simulator
            .into_messages()
            .find(|message| matches!(message, Message::Control(Control::Step(true))))
            .expect("the › arrow steps forward");
        let _task = Host::update_native(&mut host, step);
        assert_eq!(host.selection(), &compact);

        // The picker shows one entry per group: the selected preview for its
        // own, else the group and its size.
        let counter = |host: &Host| {
            let (picker, selected) = host.picker();
            let entries: Vec<String> = picker
                .iter()
                .filter(|choice| choice.group.as_deref() == Some("counter"))
                .map(|choice| choice.label.clone())
                .collect();
            (entries, selected.map(|choice| choice.label))
        };
        assert_eq!(
            counter(&host),
            (
                vec!["counter / Compact".to_owned()],
                Some("counter / Compact".to_owned())
            )
        );
        let _task = Host::update_native(
            &mut host,
            Message::Control(Control::Select(super::Selection::Application)),
        );
        assert_eq!(
            counter(&host),
            (
                vec!["counter (3)".to_owned()],
                Some("Application".to_owned())
            )
        );
    }

    #[test]
    fn bind_previews_call_a_function_with_each_argument_set() {
        let source = r#"
use iced::Element;
use iced::widget::{button, text};

#[derive(Debug, Clone, Copy, PartialEq)] enum Filter { All, Active }
#[derive(Debug, Clone)] enum Message { Set(Filter) }

#[preview(name = "editing", bind = (true))]
#[preview(name = "viewing", bind = (false))]
fn toolbar(editing: bool) -> Element<'static, Message> {
    text(if editing { "Editing" } else { "Viewing" }).boxed()
}

#[preview(name = "active", bind = ("All", Filter::All, Filter::All))]
#[preview(name = "inactive", bind = ("All", Filter::All, Filter::Active))]
fn filter(label: &'static str, kind: Filter, current: Filter) -> Element<'static, Message> {
    let state = if kind == current { "on" } else { "off" };
    button(text(format!("{label} {state}"))).on_press(Message::Set(kind)).boxed()
}
"#;
        let mut host = host(source);
        for (label, expected) in [
            ("toolbar / editing", "Editing"),
            ("toolbar / viewing", "Viewing"),
            ("filter / inactive", "All off"),
            ("filter / active", "All on"),
        ] {
            let choice = selection(&host, label);
            let _task = Host::update_native(&mut host, Message::Control(Control::Select(choice)));
            assert!(host.failure.is_none(), "{label}: {:?}", host.failure);
            assert_text(&host, expected);
        }
        // Its group is the function's; a click goes nowhere.
        assert_eq!(host.group().unwrap().0.len(), 2);
        let mut simulator = iced_test::simulator(host.view());
        simulator.click("All on").unwrap();
        for message in simulator.into_messages() {
            let _task = Host::update_native(&mut host, message);
        }
        assert!(host.reports().is_empty());
        assert_text(&host, "All on");

        for (attribute, expected) in [
            ("name = \"x\", bind = (true, false)", "TypeMismatch"),
            ("name = \"x\", bind = (true), seed = on", "not both"),
            ("group = \"g\", bind = (true)", "needs a `name`"),
        ] {
            let source = format!(
                "use iced::Element;\nuse iced::widget::text;\nfn on() -> bool {{ true }}\n#[preview({attribute})]\nfn toolbar(editing: bool) -> Element<'static, ()> {{ text(editing).boxed() }}\n"
            );
            let error = Compiler::new()
                .unwrap()
                .check_named_source("bind.rs", source)
                .err()
                .unwrap_or_else(|| panic!("`{attribute}` must be refused"));
            assert!(
                format!("{error:?}").contains(expected),
                "`{attribute}`: {error:?}"
            );
        }
    }

    #[test]
    fn run_presentation_keeps_the_application_title_and_shift_escape_closes() {
        let preview = host(titled_application_source());
        assert_eq!(preview.title(), TITLE);
        assert!(!preview.centered());

        let mut run = host(titled_application_source()).into_run();
        assert_eq!(run.selection(), &Selection::Application);
        assert_eq!(run.title(), "Source-owned title");
        assert!(run.centered());

        let fixture = first_preview_selection(&run);
        let _task = Host::update_native(&mut run, Message::Control(Control::Select(fixture)));
        assert!(run.active_preview().is_some());
        assert_eq!(run.title(), "Source-owned title");

        use iced::keyboard::{key, Event, Key, Location, Modifiers};
        let escape = |modifiers| {
            iced::Event::Keyboard(Event::KeyPressed {
                key: Key::Named(key::Named::Escape),
                modified_key: Key::Named(key::Named::Escape),
                physical_key: key::Physical::Code(key::Code::Escape),
                location: Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            })
        };
        assert!(super::shortcut(escape(Modifiers::NONE), iced::event::Status::Ignored).is_none());
        assert!(super::shortcut(escape(Modifiers::SHIFT), iced::event::Status::Captured).is_none());
        let close = super::shortcut(escape(Modifiers::SHIFT), iced::event::Status::Ignored)
            .expect("Shift+Esc closes the preview");
        let _task = Host::update_native(&mut run, close);
        assert_eq!(run.selection(), &Selection::Application);
        assert!(run.active_preview().is_none());
    }

    #[test]
    fn observed_preview_separates_descriptor_lowering_from_scripted_dispatch() {
        let mut host = host(scripted_preview_source());
        let session = timing::Session::start();
        host.timing = Some(session.clone());

        drop(iced_test::simulator(host.view()));

        let snapshot = session.snapshot();
        assert_eq!(
            snapshot
                .aggregate(timing::Phase::DescriptorLowering)
                .count(),
            1
        );
        assert_eq!(
            snapshot
                .aggregate(timing::Phase::ScriptedHookDispatch)
                .count(),
            1
        );
        assert!(snapshot
            .attempts()
            .iter()
            .filter(|attempt| attempt.phase() == timing::Phase::ScriptedHookDispatch)
            .all(|attempt| attempt.outcome() == timing::Outcome::Success));
    }

    #[test]
    fn observed_preview_switch_reset_and_native_view_keep_calculation_boundaries() {
        let mut host = host(full_source());
        let session = timing::Session::start();
        host.timing = Some(session.clone());

        let compact = selection(&host, "counter / Compact");
        assert!(host.select(compact, None).is_some());
        let selected = session.snapshot();
        assert_eq!(selected.aggregate(timing::Phase::ViewEvaluation).count(), 1);

        drop(Host::view_native(&host));
        drop(Host::view_native(&host));
        let displayed = session.snapshot();
        assert_eq!(
            displayed.aggregate(timing::Phase::ViewEvaluation).count(),
            1
        );
        assert_eq!(
            displayed
                .aggregate(timing::Phase::DescriptorLowering)
                .count(),
            2
        );

        let default = selection(&host, "counter / Default");
        assert!(host.select(default, None).is_some());
        let generation = host.catalog().generation();
        let mut instance = host.instance().unwrap();
        for _ in 0..3 {
            assert!(host.reset().is_some());
            assert_eq!(host.catalog().generation(), generation);
            assert_ne!(host.instance().unwrap(), instance);
            instance = host.instance().unwrap();
        }
        let reset = session.snapshot();
        assert_eq!(reset.aggregate(timing::Phase::ViewEvaluation).count(), 5);
        assert_eq!(
            reset.aggregate(timing::Phase::DescriptorLowering).count(),
            2
        );
    }

    #[test]
    fn preview_form_controls_apply_nested_text_and_toggle_messages() {
        let mut host = host(preview_form_controls_source());
        assert!(matches!(host.selection, Selection::Preview(_)));

        let messages = typewrite(&host, "Preview input", "rawr");
        assert_eq!(messages.len(), 4);
        for message in messages {
            assert!(one_message(Host::update_native(&mut host, message)).is_none());
        }
        assert_text(&host, "rawr");

        let toggled = click(&host, "Preview toggle");
        assert!(one_message(Host::update_native(&mut host, toggled)).is_none());
        assert_text(&host, "true");
        assert!(host.reports().is_empty());
    }

    #[test]
    fn preview_slider_applies_one_nested_owner_stamped_message() {
        let mut host = host(preview_slider_source());
        let (content, _) = chrome_bounds(&host);
        let changed = click_at(
            &host,
            iced::Point::new(content.x + content.width / 2.0, content.y + 8.0),
        );
        let Message::Script(message) = &changed else {
            panic!("preview slider must emit through the private Script branch");
        };
        assert_eq!(message.instance(), host.preview.as_ref().unwrap().instance);
        assert_eq!(message.generation(), host.catalog().generation());

        assert!(one_message(Host::update_native(&mut host, changed)).is_none());
        assert_text(&host, "50");
        assert!(host.reports().is_empty());
    }

    #[test]
    fn named_app_and_preview_choices_share_one_generation() {
        let host = host(full_source());

        assert_eq!(
            host.choices
                .iter()
                .map(|choice| choice.label.as_str())
                .collect::<Vec<_>>(),
            [
                "Application",
                "counter / Broken",
                "counter / Compact",
                "counter / Default",
            ]
        );
        assert!(matches!(host.selection, Selection::Application));
        assert_eq!(host.visible_generation(), Some(host.catalog().generation()));
    }

    #[test]
    fn select_reset_and_theme_transactions_obey_instance_law() {
        let mut host = host(full_source());
        let application = host.instance().unwrap();
        let application_theme = host.active_theme().clone();
        let compact = selection(&host, "counter / Compact");

        assert!(host.select(compact.clone(), None).is_some());
        let selected = host.instance().unwrap();
        assert_ne!(selected, application);
        assert!(host.reset().is_some());
        let reset = host.instance().unwrap();
        assert_ne!(reset, selected);

        assert!(host.select_theme(theme::Choice::Explicit {
            index: 0,
            name: "State::dark".to_owned(),
        }));
        assert_eq!(host.instance(), Some(reset));
        assert_eq!(host.active_theme().native(), &iced::Theme::Dark);

        let theme = host.active_theme().clone();
        assert!(!host.select_theme(theme::Choice::Explicit {
            index: 1,
            name: "State::broken_theme".to_owned(),
        }));
        assert_eq!(host.instance(), Some(reset));
        assert_eq!(host.active_theme(), &theme);
        assert!(host.failure.is_some());

        let alternate = selection(&host, "counter / Default");
        assert!(host.select(alternate.clone(), Some(1)).is_none());
        assert_eq!(host.selection, compact);
        assert_eq!(host.instance(), Some(reset));
        assert_eq!(host.active_theme(), &theme);

        assert!(host.select(alternate, None).is_some());
        let inherited = host.instance().unwrap();
        assert_ne!(inherited, reset);
        assert_eq!(host.active_theme(), &application_theme);

        let broken = selection(&host, "counter / Broken");
        assert!(host.select(broken, None).is_none());
        assert_eq!(host.selection, selection(&host, "counter / Default"));
        assert_eq!(host.instance(), Some(inherited));
        assert_eq!(host.active_theme(), &application_theme);
        assert_eq!(host.application.as_ref().unwrap().instance(), application);
    }

    #[test]
    fn preview_lifecycle_preserves_the_materialized_application() {
        let mut host = host(full_source());
        let increment = click(&host, "Increment");
        let _ = Host::update_native(&mut host, increment);
        assert_text(&host, "1");

        let application = host.application.as_ref().unwrap();
        let instance = application.instance();
        let state = format!("{:?}", application.state());
        let output = application.display().identity();
        let theme = application.theme().clone();
        let diagnostics = application.diagnostics().to_vec();

        let compact = selection(&host, "counter / Compact");
        assert!(host.select(compact, None).is_some());
        let first_preview = host.preview.as_ref().unwrap().instance;
        assert!(host.reset().is_some());
        assert_ne!(host.preview.as_ref().unwrap().instance, first_preview);
        let default = selection(&host, "counter / Default");
        assert!(host.select(default, None).is_some());

        let application = host.application.as_ref().unwrap();
        assert_eq!(application.instance(), instance);
        assert_eq!(format!("{:?}", application.state()), state);
        assert_eq!(application.display().identity(), output);
        assert_eq!(application.theme(), &theme);
        assert_eq!(application.diagnostics(), diagnostics);

        host.close_preview();
        assert!(matches!(host.selection, Selection::Application));
        assert!(host.preview.is_none());
        assert_eq!(host.instance(), Some(instance));
        assert_text(&host, "1");

        assert!(host.reset().is_some());
        assert_ne!(host.instance(), Some(instance));
        assert_text(&host, "0");
    }

    #[test]
    fn delayed_application_work_updates_the_hidden_application() {
        let (mut host, startup) = booted_host(application_task_source());
        let application = host.application.as_ref().unwrap().instance();
        let delayed = one_message(startup).expect("application startup must emit Done");
        assert_script_owner(&delayed, application, host.catalog().generation());

        let preview = first_preview_selection(&host);
        assert!(host.select(preview.clone(), None).is_some());
        let preview_instance = host.preview.as_ref().unwrap().instance;
        assert_text(&host, "0");

        let follow_up = Host::update_native(&mut host, delayed);
        assert_eq!(host.selection, preview);
        assert_eq!(host.preview.as_ref().unwrap().instance, preview_instance);
        assert_text(&host, "0");
        let follow_up = one_message(follow_up).expect("hidden app update must emit Done");
        assert_script_owner(&follow_up, application, host.catalog().generation());

        let _ = Host::update_native(&mut host, follow_up);
        assert_eq!(host.preview.as_ref().unwrap().instance, preview_instance);
        assert_text(&host, "0");

        host.close_preview();
        assert_eq!(host.instance(), Some(application));
        assert_text(&host, "9");
    }

    #[test]
    fn hidden_application_follow_up_operation_keeps_application_authority() {
        let starts = Arc::new(AtomicUsize::new(0));
        let (mut host, startup) =
            booted_host_with_operation(application_operation_source(), starts.clone());
        let application = host.application.as_ref().unwrap().instance();
        let preview = first_preview_selection(&host);
        assert!(host.select(preview.clone(), None).is_some());
        assert_eq!(host.selection, preview);
        assert_text(&host, "preview");

        let first = one_message(startup).expect("application startup operation must complete");
        assert_script_owner(&first, application, host.catalog().generation());
        let follow_up = Host::update_native(&mut host, first);

        assert_eq!(host.selection, preview);
        assert_text(&host, "preview");
        let second = one_message(follow_up).expect("hidden application must issue its follow-up");
        assert_script_owner(&second, application, host.catalog().generation());
        let Message::Script(message) = &second else {
            panic!("application follow-up must retain the private Script branch");
        };
        assert!(message.task_failure().is_none());
        assert!(one_message(Host::update_native(&mut host, second)).is_none());
        assert_eq!(starts.load(Ordering::SeqCst), 2);

        host.close_preview();
        assert_eq!(host.instance(), Some(application));
        assert_text(&host, "3");
    }

    #[test]
    fn initial_candidate_failures_keep_native_controls_and_explicit_recovery() {
        for (source, expected) in [
            (failing_application_boot_source(), "broken application boot"),
            (failing_application_view_source(), "broken application view"),
            (
                failing_application_theme_source(),
                "broken application theme",
            ),
        ] {
            let (mut host, startup) = booted_host(source);

            assert!(host.application.is_none());
            assert!(host.preview.is_none());
            assert!(host.instance().is_none());
            assert!(one_message(startup).is_none());
            assert!(matches!(host.selection, Selection::Application));
            let report = host.failure.as_ref().unwrap();
            assert_eq!(report.code(), rawr::diagnostic::code::RUNTIME_THROW);
            assert!(report.message().contains(expected), "{report}");
            assert_text(&host, "Reset");
            assert_text(&host, report.message());

            assert!(host.reset().is_none());
            assert!(host.application.is_none());
            assert!(host.preview.is_none());
            assert!(host.instance().is_none());
            let healthy = host
                .choices
                .iter()
                .find_map(|choice| match &choice.selection {
                    Selection::Preview(_) => Some(choice.selection.clone()),
                    Selection::Application => None,
                })
                .unwrap();
            assert!(host.select(healthy.clone(), None).is_some());
            assert_eq!(host.selection, healthy);
            assert!(host.instance().is_some());
            assert!(host.failure.is_none());
            assert_text(&host, "healthy preview");
        }
    }

    #[test]
    fn fallback_renders_every_canonical_report_field() {
        let mut host = host(failing_application_boot_source());
        let primary = rawr::diagnostic::Source::new("child.rs", "broken\n").unwrap();
        let related = rawr::diagnostic::Source::new("related.rs", "first\n").unwrap();
        let primary_span = Span::new(Position::new(1, 1), Position::new(1, 7));
        let related_span = Span::new(Position::new(1, 1), Position::new(1, 6));
        host.apply(|state, _catalog, _context| {
            state.failure = Some(
                rawr::diagnostic::Report::error(
                    rawr::diagnostic::code::RESOLVE_DUPLICATE_ITEM,
                    "complete canonical report",
                )
                .primary(primary, primary_span, Some("primary callback".into()))
                .unwrap()
                .secondary(related, related_span, Some("related callback".into()))
                .unwrap()
                .help("repair the callback")
                .note("retained source evidence")
                .finish(),
            );
        });

        assert_text(
            &host,
            "preview host · resolve · error[RAWR-RESOLVE-DUPLICATE-ITEM]",
        );
        assert_text(&host, "complete canonical report");
        assert_text(
            &host,
            &format!("primary: child.rs:{primary_span} — primary callback"),
        );
        assert_text(
            &host,
            &format!("secondary: related.rs:{related_span} — related callback"),
        );
        assert_text(&host, "help: repair the callback");
        assert_text(&host, "note: retained source evidence");
    }

    #[test]
    fn footer_reserves_space_and_ignores_content_theme_changes() {
        let mut host = host(full_source());
        let compact = selection(&host, "counter / Compact");
        assert!(host.select(compact, None).is_some());
        let before = chrome_bounds(&host);
        let before_theme = Host::theme_native(&host);
        let rawr_theme = chrome::theme();

        assert!(host.select_theme(theme::Choice::Explicit {
            index: 0,
            name: "State::dark".to_owned(),
        }));

        let after = chrome_bounds(&host);
        assert_ne!(Host::theme_native(&host), before_theme);
        assert_eq!(Host::theme_native(&host), iced::Theme::Dark);
        assert_eq!(chrome::theme(), rawr_theme);
        assert_eq!(before, after);
        assert!(before.0.y + before.0.height <= before.1.y);
        assert!(before.0.height > 0.0);
        assert!(before.1.height <= chrome::FOOTER_MAX_HEIGHT);
        assert_eq!(
            chrome::button(&iced::Theme::Light, iced::widget::button::Status::Hovered),
            chrome::button(&iced::Theme::Dark, iced::widget::button::Status::Hovered),
        );
        assert_eq!(
            chrome::pick_list(
                &iced::Theme::Light,
                iced::widget::pick_list::Status::Opened { is_hovered: true },
            ),
            chrome::pick_list(
                &iced::Theme::Dark,
                iced::widget::pick_list::Status::Opened { is_hovered: true },
            ),
        );
    }

    #[test]
    fn fallback_report_uses_a_separate_panel_above_the_compact_footer() {
        let host = host(failing_application_boot_source());
        let report = host.failure.as_ref().unwrap();
        let mut simulator = iced_test::simulator(host.view());
        let content = simulator
            .find(iced_test::selector::id(chrome::CONTENT_ID))
            .unwrap()
            .bounds();
        let footer = simulator
            .find(iced_test::selector::id(chrome::FOOTER_ID))
            .unwrap()
            .bounds();
        let panel = simulator
            .find(iced_test::selector::id(chrome::DIAGNOSTIC_ID))
            .unwrap()
            .bounds();
        let diagnostic = simulator.find(report.message()).unwrap().bounds();

        assert!(content.y + content.height <= panel.y);
        assert!(diagnostic.y >= panel.y);
        assert!(diagnostic.y + diagnostic.height <= panel.y + panel.height);
        assert!(panel.y + panel.height <= footer.y);
        assert!(footer.height <= chrome::FOOTER_MAX_HEIGHT);
        assert_text(&host, "Rawr · Application · Error");
        assert_centered_zones(&host);
    }

    #[test]
    fn footer_controls_keep_stable_ids_and_compact_height_in_every_state() {
        let mut active = host(full_source());
        assert_compact_controls(&active, false, false, false);

        let compact = selection(&active, "counter / Compact");
        assert!(active.select(compact, None).is_some());
        assert_text(&active, "Inherited → Light");
        assert_compact_controls(&active, true, true, false);

        assert!(active.select_theme(theme::Choice::Explicit {
            index: 0,
            name: "State::dark".to_owned(),
        }));
        assert_text(&active, "dark → Dark");
        assert_compact_controls(&active, true, true, false);

        let failed = host(failing_application_boot_source());
        assert_compact_controls(&failed, false, false, true);
    }

    #[test]
    fn footer_uses_equal_zones_with_a_centered_selector_and_close() {
        let mut host = host(full_source());

        assert_centered_zones(&host);
        assert_text(&host, "Rawr · Application · Ready");
        assert_text(&host, "Theme → Light");

        let compact = selection(&host, "counter / Compact");
        assert!(host.select(compact, None).is_some());
        assert_centered_zones(&host);
        assert_text(&host, "Rawr · counter / Compact · Ready");
        assert_text(&host, "Inherited → Light");
        assert_text(&host, "Close");

        let mut simulator = iced_test::simulator(host.view());
        simulator.click("Close").unwrap();
        let close = simulator
            .into_messages()
            .next()
            .expect("Close must emit one host-owned control message");
        let _ = Host::update_native(&mut host, close);

        assert!(matches!(host.selection, Selection::Application));
        assert!(host.preview.is_none());
        assert_text(&host, "Rawr · Application · Ready");
        assert_centered_zones(&host);
    }

    #[test]
    fn standalone_close_has_no_hidden_app_and_reset_reopens_the_preview() {
        let mut host = host(standalone_source());
        let preview = host.preview.as_ref().unwrap().instance;

        let _ = Host::update_native(&mut host, Message::Control(Control::Close));

        assert!(host.application.is_none());
        assert!(host.preview.is_none());
        assert_eq!(host.status_label(), "Rawr · main / Standalone · Closed");
        assert_text(&host, "Rawr · main / Standalone · Closed");
        assert_text(&host, "No preview session is active");
        assert!(host.reset().is_some());
        assert!(host.application.is_none());
        assert_ne!(host.preview.as_ref().unwrap().instance, preview);
        assert_text(&host, "standalone");
    }

    #[test]
    fn footer_names_the_effective_inherited_and_explicit_themes() {
        let mut host = host(full_source());
        let compact = selection(&host, "counter / Compact");
        assert!(host.select(compact, None).is_some());

        assert_text(&host, "Inherited → Light");
        assert!(host.select_theme(theme::Choice::Explicit {
            index: 0,
            name: "State::dark".to_owned(),
        }));
        assert_text(&host, "dark → Dark");

        assert!(host.select_theme(theme::Choice::Inherited));
        assert_text(&host, "Inherited → Light");
    }

    #[test]
    fn public_and_native_controls_share_the_same_transactions() {
        let mut control = Compiler::new()
            .unwrap()
            .check_named_source("preview-host.rs", full_source())
            .unwrap()
            .prepare()
            .unwrap()
            .into_control();
        let mut native = host(full_source());
        let compact = control
            .previews()
            .iter()
            .find(|preview| preview.name() == "Compact")
            .unwrap()
            .key()
            .clone();
        let broken = control
            .previews()
            .iter()
            .find(|preview| preview.name() == "Broken")
            .unwrap()
            .key()
            .clone();

        control.select(&compact).unwrap();
        let _ = Host::update_native(
            &mut native,
            Message::Control(Control::Select(Selection::Preview(compact.clone()))),
        );
        assert_eq!(control.status(), crate::control::Status::from_host(&native));

        let public = control.select(&broken).unwrap_err();
        let _ = Host::update_native(
            &mut native,
            Message::Control(Control::Select(Selection::Preview(broken))),
        );
        assert_eq!(
            public.report().unwrap().code(),
            native.failure.as_ref().unwrap().code()
        );
        assert_eq!(control.status(), crate::control::Status::from_host(&native));

        control.select(&compact).unwrap();
        let _ = Host::update_native(
            &mut native,
            Message::Control(Control::Select(Selection::Preview(compact))),
        );
        control.reset().unwrap();
        let _ = Host::update_native(&mut native, Message::Control(Control::Reset));
        assert_eq!(control.status(), crate::control::Status::from_host(&native));

        let _ = control.close();
        let _ = Host::update_native(&mut native, Message::Control(Control::Close));
        assert_eq!(control.status(), crate::control::Status::from_host(&native));
    }

    #[test]
    fn standalone_subject_failure_keeps_its_exact_fallback_evidence() {
        let host = host(failing_standalone_subject_source());
        let report = host.failure.as_ref().unwrap();

        assert!(host.application.is_none());
        assert!(host.preview.is_none());
        assert_eq!(report.code(), rawr::diagnostic::code::RUNTIME_THROW);
        assert_eq!(report.labels()[0].source().identity(), "preview-host.rs");
        assert_text(&host, report.message());
        assert_text(&host, "preview host · runtime · error[RAWR-RUNTIME-THROW]");
    }

    #[test]
    fn child_module_app_failures_keep_each_callback_source() {
        for (failure, expected, line) in [
            ("boot", "broken child boot", 11),
            ("view", "broken child view", 13),
            ("theme", "broken child theme", 14),
        ] {
            let project = Project::new(failure);
            project.write("Cargo.toml", "[package]\nname = \"preview-child\"\n");
            project.write("src/main.rs", child_application_root());
            project.write("src/app.rs", &child_application(failure));
            let target = rawr::project::Target::discover(project.path()).unwrap();
            let prepared = Compiler::new()
                .unwrap()
                .check(&target)
                .unwrap()
                .prepare()
                .unwrap();
            let host = prepared.into_control().into_state();
            let report = host.failure.as_ref().unwrap();

            assert!(host.application.is_none());
            assert!(host.preview.is_none());
            assert!(report.message().contains(expected), "{report}");
            assert!(report.labels()[0]
                .source()
                .identity()
                .ends_with("src/app.rs"));
            assert_eq!(report.labels()[0].span().start().line(), Some(line));
            assert_text(&host, report.message());
        }
    }

    #[test]
    fn child_module_update_failure_keeps_original_source_position() {
        let project = Project::new("update");
        project.write("Cargo.toml", "[package]\nname = \"preview-child\"\n");
        project.write("src/main.rs", child_application_root());
        project.write("src/app.rs", &child_application("update"));
        let target = rawr::project::Target::discover(project.path()).unwrap();
        let prepared = Compiler::new()
            .unwrap()
            .check(&target)
            .unwrap()
            .prepare()
            .unwrap();
        let mut host = prepared.into_control().into_state();
        let message = click(&host, "Trigger");

        let _ = Host::update_native(&mut host, message);

        let Some(adapter) = &host.application else {
            panic!("healthy child application must retain its active session");
        };
        let report = adapter.diagnostics()[0].report();
        assert!(report.message().contains("broken child update"));
        assert!(report.labels()[0]
            .source()
            .identity()
            .ends_with("src/app.rs"));
        assert_eq!(report.labels()[0].span().start().line(), Some(12));
    }

    #[test]
    fn failed_candidate_suppresses_its_startup_work() {
        let (host, startup) = booted_host(failing_candidate_with_work_source());

        assert!(host.application.is_none());
        assert!(host.preview.is_none());
        assert!(host.instance().is_none());
        assert!(one_message(startup).is_none());
        assert!(host
            .failure
            .as_ref()
            .unwrap()
            .message()
            .contains("broken view"));
    }

    #[test]
    fn application_startup_and_update_done_route_after_commit() {
        let (mut host, startup) = booted_host(application_task_source());
        let instance = host.instance().unwrap();
        let startup = one_message(startup).expect("committed app startup Done must emit");
        assert_script_owner(&startup, instance, host.catalog().generation());

        let follow_up = Host::update_native(&mut host, startup);
        assert_text(&host, "8");
        let follow_up = one_message(follow_up).expect("app update Done must emit");
        assert_script_owner(&follow_up, instance, host.catalog().generation());
    }

    #[test]
    fn preview_update_done_and_none_route_only_after_state_commit() {
        for (source, emits) in [
            (preview_task_source(true), true),
            (preview_task_source(false), false),
        ] {
            let (mut host, startup) = booted_host(&source);
            assert!(one_message(startup).is_none());
            let increment = click(&host, "Increment");

            let follow_up = Host::update_native(&mut host, increment);

            assert_text(&host, "1");
            assert_eq!(one_message(follow_up).is_some(), emits);
        }
    }

    #[test]
    fn operation_mapper_cannot_cross_preview_message_keys() {
        let starts = Arc::new(AtomicUsize::new(0));
        let mut host = host_with_operation(preview_operation_source(), starts.clone());
        let active = host.preview.as_ref().unwrap().key.clone();
        let wrong = host
            .catalog()
            .previews()
            .iter()
            .find(|definition| definition.key() != &active)
            .unwrap()
            .key()
            .clone();
        let Message::Script(message) = click(&host, "Load") else {
            panic!("preview button must emit a script message");
        };
        let catalog = host.catalog();
        let (instance, plan) = host.apply(|state, catalog, _context| {
            let preview = state.preview.as_mut().unwrap();
            (preview.instance, preview.update_state(catalog, message).0)
        });
        let authority = crate::task::Authority::preview(catalog.clone(), wrong);

        let task = plan
            .lower(instance, catalog.generation(), &authority)
            .unwrap()
            .map(Message::Script);
        let failure = one_message(task).expect("wrong preview key must emit one task failure");
        let Message::Script(failure) = failure else {
            panic!("task failure must retain the private Script branch");
        };

        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert!(failure
            .task_failure()
            .is_some_and(|error| error.to_string().contains("invalid Message")));
    }

    #[test]
    fn failed_preview_update_rolls_back_and_emits_no_work() {
        let (mut host, startup) = booted_host(failing_preview_update_source());
        assert!(one_message(startup).is_none());
        let instance = host.instance();
        let increment = click(&host, "Increment");

        let follow_up = Host::update_native(&mut host, increment);

        assert!(one_message(follow_up).is_none());
        assert_eq!(host.instance(), instance);
        assert_text(&host, "0");
        let Some(session) = &host.preview else {
            panic!("preview session must remain active");
        };
        assert_eq!(session.diagnostics.len(), 1);
        assert!(session.diagnostics[0].message().contains("invalid Task"));
    }

    #[test]
    fn delayed_preview_done_drops_after_reset() {
        let (mut host, startup) = booted_host(&preview_task_source(true));
        assert!(one_message(startup).is_none());
        let increment = click(&host, "Increment");
        let delayed = Host::update_native(&mut host, increment);
        assert_text(&host, "1");
        let handles = host.preview.as_ref().unwrap().tasks.handles();
        assert_eq!(handles.len(), 1);
        assert!(!handles[0].is_aborted());
        let delayed = one_message(delayed).expect("preview update Done must emit");

        assert!(host.reset().is_some());
        assert!(handles[0].is_aborted());
        assert_text(&host, "0");
        let follow_up = Host::update_native(&mut host, delayed);

        assert!(one_message(follow_up).is_none());
        assert_text(&host, "0");
    }

    #[test]
    fn delayed_application_done_aborts_after_application_reset_and_race_drops() {
        let (mut host, startup) = booted_host(application_task_source());
        let instance = host.application.as_ref().unwrap().instance();
        let handles = host.application.as_ref().unwrap().task_handles();
        assert_eq!(handles.len(), 1);
        assert!(!handles[0].is_aborted());
        let delayed = one_message(startup).expect("application startup Done must emit");
        assert_script_owner(&delayed, instance, host.catalog().generation());

        assert!(host.reset().is_some());
        assert!(handles[0].is_aborted());
        assert_ne!(host.application.as_ref().unwrap().instance(), instance);
        assert_text(&host, "7");
        let follow_up = Host::update_native(&mut host, delayed);

        assert!(one_message(follow_up).is_none());
        assert_text(&host, "7");
    }

    #[test]
    fn preview_script_messages_update_and_stale_instances_fall_through() {
        let mut host = host(full_source());
        let application = host.application.as_ref().unwrap().instance();
        let application_output = host.application.as_ref().unwrap().display().identity();
        let compact = selection(&host, "counter / Compact");
        assert!(host.select(compact, None).is_some());
        let stale = click(&host, "Increment");
        let alternate = selection(&host, "counter / Default");
        assert!(host.select(alternate, None).is_some());

        let _ = Host::update_native(&mut host, stale);
        assert_text(&host, "0");
        assert_eq!(host.application.as_ref().unwrap().instance(), application);
        assert_eq!(
            host.application.as_ref().unwrap().display().identity(),
            application_output
        );

        let increment = click(&host, "Increment");
        let _ = Host::update_native(&mut host, increment);
        assert_text(&host, "1");
    }

    #[test]
    fn standalone_catalog_boots_directly_without_hidden_application_work() {
        let host = host(standalone_source());

        assert_eq!(host.choices.len(), 1);
        assert!(matches!(host.selection, Selection::Preview(_)));
        assert!(host.application.is_none());
        assert!(host.preview.is_some());
        assert_text(&host, "standalone");
    }

    #[test]
    fn application_subscription_retires_for_preview_and_late_output_is_dropped() {
        let starts = Arc::new(AtomicUsize::new(0));
        let mut host = host_with_stream(subscription_source(), starts.clone());
        let application = host.application.as_ref().unwrap().instance();
        let initial = Host::subscription_native(&host);
        let initial_hash = subscription_hash(initial);
        let late = one_subscription_message(Host::subscription_native(&host));
        assert_eq!(starts.load(Ordering::SeqCst), 1);

        let preview = first_preview_selection(&host);
        assert!(host.select(preview, None).is_some());
        assert!(
            iced::advanced::subscription::into_recipes(Host::subscription_native(&host)).is_empty()
        );
        let _ = Host::update_native(&mut host, late);
        assert_eq!(host.application.as_ref().unwrap().instance(), application);

        assert!(host.select(Selection::Application, None).is_some());
        let restarted = Host::subscription_native(&host);
        assert_eq!(subscription_hash(restarted), initial_hash);
    }

    #[test]
    fn reload_retains_selected_preview_and_rebinds_hidden_application() {
        let project = Project::new("reload-preview");
        project.write("src/main.rs", full_source());
        let engine = reload_engine();
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));
        let hidden = host.application.as_ref().unwrap().instance();
        let delayed_application = click(&host, "Increment");
        let selected = selection(&host, "counter / Default");
        assert!(host.select(selected.clone(), None).is_some());
        let preview = host.preview.as_ref().unwrap().instance;
        let delayed_preview = click(&host, "Increment");

        let changed = full_source()
            .replace("self.value += 1", "self.value += 2")
            .replace("button(\"Increment\")", "button(\"Reloaded\")");
        project.write("src/main.rs", &changed);
        let plan = reload_plan(&host, reload_candidate(&engine, &project));
        let replacement = plan.bind(&engine, reload_registry()).unwrap();
        let retained = host.generation().unwrap();
        let prepared = host.prepare_for_test(replacement).unwrap();
        assert!(Arc::ptr_eq(&host.generation().unwrap(), &retained));
        assert_eq!(host.preview.as_ref().unwrap().instance, preview);
        let outcome = host.commit_for_test(prepared).unwrap();

        assert_eq!(
            outcome
                .evidence()
                .iter()
                .map(rawr::reload::compatibility::Evidence::action)
                .collect::<Vec<_>>(),
            vec![
                rawr::reload::compatibility::Action::Retain,
                rawr::reload::compatibility::Action::Retain,
            ]
        );
        assert_eq!(host.selection, selected);
        assert_eq!(host.preview.as_ref().unwrap().instance, preview);
        assert_eq!(host.application.as_ref().unwrap().instance(), hidden);
        assert!(one_message(outcome.into_task()).is_none());
        let _ = Host::update_native(&mut host, delayed_preview);
        let _ = Host::update_native(&mut host, delayed_application);
        assert_text(&host, "0");

        let current = click(&host, "Reloaded");
        let _ = Host::update_native(&mut host, current);
        assert_text(&host, "2");
        host.close_preview();
        assert!(matches!(host.selection, Selection::Application));
        assert_text(&host, "0");
    }

    #[test]
    fn retained_application_drops_old_generation_mapper_failure_before_diagnostic() {
        let project = Project::new("reload-stale-form-mapper");
        project.write(
            "src/main.rs",
            &reload_throwing_control_source("Old generation"),
        );
        let engine = reload_engine();
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));
        let instance = host.application.as_ref().unwrap().instance();
        let stale = typewrite(&host, "Reload input", "x").pop().unwrap();
        let Message::Script(stale_message) = &stale else {
            panic!("text input must emit through the private Script branch");
        };
        let old_generation = stale_message.generation();
        assert_eq!(stale_message.instance(), instance);
        assert!(stale_message.mapping_error().is_some());

        project.write(
            "src/main.rs",
            &reload_throwing_control_source("New generation"),
        );
        let replacement = reload_plan(&host, reload_candidate(&engine, &project))
            .bind(&engine, reload_registry())
            .unwrap();
        let prepared = host.prepare_for_test(replacement).unwrap();
        let outcome = host.commit_for_test(prepared).unwrap();
        assert_eq!(outcome.evidence().len(), 1);
        assert_eq!(
            outcome.evidence()[0].action(),
            rawr::reload::compatibility::Action::Retain
        );
        assert!(one_message(outcome.into_task()).is_none());
        assert_eq!(host.application.as_ref().unwrap().instance(), instance);
        assert_ne!(host.catalog().generation(), old_generation);
        let reports = host.reports().len();

        assert!(one_message(Host::update_native(&mut host, stale)).is_none());

        assert_eq!(host.application.as_ref().unwrap().instance(), instance);
        assert_eq!(host.reports().len(), reports);
        assert_text(&host, "New generation");
    }

    #[test]
    fn retained_application_drops_old_generation_slider_mapper_failure_before_diagnostic() {
        let project = Project::new("reload-stale-slider-mapper");
        project.write(
            "src/main.rs",
            &reload_throwing_slider_source("Old generation"),
        );
        let engine = reload_engine();
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));
        let instance = host.application.as_ref().unwrap().instance();
        let (content, _) = chrome_bounds(&host);
        let stale = click_at(
            &host,
            iced::Point::new(content.x + content.width / 2.0, content.y + 8.0),
        );
        let Message::Script(stale_message) = &stale else {
            panic!("slider must emit through the private Script branch");
        };
        let old_generation = stale_message.generation();
        assert_eq!(stale_message.instance(), instance);
        assert!(stale_message.mapping_error().is_some());

        project.write(
            "src/main.rs",
            &reload_throwing_slider_source("New generation"),
        );
        let replacement = reload_plan(&host, reload_candidate(&engine, &project))
            .bind(&engine, reload_registry())
            .unwrap();
        let prepared = host.prepare_for_test(replacement).unwrap();
        let outcome = host.commit_for_test(prepared).unwrap();
        assert_eq!(outcome.evidence().len(), 1);
        assert_eq!(
            outcome.evidence()[0].action(),
            rawr::reload::compatibility::Action::Retain
        );
        assert!(one_message(outcome.into_task()).is_none());
        assert_eq!(host.application.as_ref().unwrap().instance(), instance);
        assert_ne!(host.catalog().generation(), old_generation);
        let reports = host.reports().len();

        assert!(one_message(Host::update_native(&mut host, stale)).is_none());

        assert_eq!(host.application.as_ref().unwrap().instance(), instance);
        assert_eq!(host.reports().len(), reports);
        assert_text(&host, "New generation");
    }

    #[test]
    fn reload_candidate_added_application_boots_only_when_selected() {
        let project = Project::new("reload-added-application");
        project.write("src/main.rs", standalone_source());
        let engine = reload_engine();
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));
        let preview = host.preview.as_ref().unwrap().instance;

        project.write("src/main.rs", &added_application_source("App {}"));
        let plan = host
            .plan_for_test(reload_candidate(&engine, &project))
            .unwrap();
        assert_eq!(plan.evidence_set().len(), 1);
        assert!(matches!(
            plan.evidence_set()[0].owner(),
            rawr::reload::compatibility::Owner::Preview(_)
        ));
        let replacement = plan.bind(&engine, reload_registry()).unwrap();
        host.install_replacement_for_test(replacement).unwrap();

        assert!(host.application.is_none());
        assert_eq!(host.preview.as_ref().unwrap().instance, preview);
        host.close_preview();
        assert!(matches!(host.selection, Selection::Application));
        assert!(host.application.is_some());
        assert!(host.preview.is_none());
        assert_text(&host, "added application");

        project.write("src/main.rs", standalone_source());
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));
        let selected = host.selection.clone();
        let preview = host.preview.as_ref().unwrap().instance;
        project.write(
            "src/main.rs",
            &added_application_source("throw \"added application boot failed\""),
        );
        let replacement = host
            .plan_for_test(reload_candidate(&engine, &project))
            .unwrap()
            .bind(&engine, reload_registry())
            .unwrap();
        host.install_replacement_for_test(replacement).unwrap();

        host.close_preview();
        assert_eq!(host.selection, selected);
        assert!(host.application.is_none());
        assert_eq!(host.preview.as_ref().unwrap().instance, preview);
        assert!(host
            .failure
            .as_ref()
            .is_some_and(|report| report.message().contains("added application boot failed")));
        assert_text(&host, "standalone");
    }

    #[test]
    fn reload_proves_each_hidden_and_selected_owner_before_one_commit() {
        let project = Project::new("reload-owner-set");
        project.write("src/main.rs", &owners_source("i64", "i64", "", ""));
        let engine = reload_engine();
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));
        let selected = first_preview_selection(&host);
        assert!(host.select(selected.clone(), None).is_some());
        let mut application_type = "i64";
        let mut preview_type = "i64";

        for _ in 0..4 {
            let previous_application = host.application.as_ref().unwrap().instance();
            let previous_preview = host.preview.as_ref().unwrap().instance;
            application_type = if application_type == "i64" {
                "bool"
            } else {
                "i64"
            };
            project.write(
                "src/main.rs",
                &owners_source(application_type, preview_type, "", ""),
            );
            let replacement = reload_plan(&host, reload_candidate(&engine, &project))
                .bind(&engine, reload_registry())
                .unwrap();
            let outcome = host.install_replacement_for_test(replacement).unwrap();
            assert_eq!(
                outcome
                    .evidence()
                    .iter()
                    .map(rawr::reload::compatibility::Evidence::action)
                    .collect::<Vec<_>>(),
                vec![
                    rawr::reload::compatibility::Action::Reset,
                    rawr::reload::compatibility::Action::Retain,
                ]
            );
            assert_ne!(
                host.application.as_ref().unwrap().instance(),
                previous_application
            );
            assert_eq!(host.preview.as_ref().unwrap().instance, previous_preview);
            assert_eq!(host.selection, selected);

            let previous_application = host.application.as_ref().unwrap().instance();
            let previous_preview = host.preview.as_ref().unwrap().instance;
            preview_type = if preview_type == "i64" { "bool" } else { "i64" };
            project.write(
                "src/main.rs",
                &owners_source(application_type, preview_type, "", ""),
            );
            let replacement = reload_plan(&host, reload_candidate(&engine, &project))
                .bind(&engine, reload_registry())
                .unwrap();
            let outcome = host.install_replacement_for_test(replacement).unwrap();
            assert_eq!(
                outcome
                    .evidence()
                    .iter()
                    .map(rawr::reload::compatibility::Evidence::action)
                    .collect::<Vec<_>>(),
                vec![
                    rawr::reload::compatibility::Action::Retain,
                    rawr::reload::compatibility::Action::Reset,
                ]
            );
            assert_eq!(
                host.application.as_ref().unwrap().instance(),
                previous_application
            );
            assert_ne!(host.preview.as_ref().unwrap().instance, previous_preview);
            assert_eq!(host.selection, selected);
        }
    }

    #[test]
    fn reload_owner_failure_preserves_the_whole_preview_host() {
        let project = Project::new("reload-owner-failure");
        project.write("src/main.rs", &owners_source("i64", "i64", "", ""));
        let engine = reload_engine();
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));
        let selected = first_preview_selection(&host);
        assert!(host.select(selected, None).is_some());
        let retained = host.generation().unwrap();
        let application = host.application.as_ref().unwrap().instance();
        let preview = host.preview.as_ref().unwrap().instance;

        project.write(
            "src/main.rs",
            &owners_source("i64", "i64", "throw \"app failed\"", ""),
        );
        let replacement = reload_plan(&host, reload_candidate(&engine, &project))
            .bind(&engine, reload_registry())
            .unwrap();
        assert!(host.install_replacement_for_test(replacement).is_err());
        assert!(Arc::ptr_eq(&host.generation().unwrap(), &retained));
        assert_eq!(host.application.as_ref().unwrap().instance(), application);
        assert_eq!(host.preview.as_ref().unwrap().instance, preview);

        project.write(
            "src/main.rs",
            &owners_source("i64", "i64", "", "throw \"preview failed\""),
        );
        let replacement = reload_plan(&host, reload_candidate(&engine, &project))
            .bind(&engine, reload_registry())
            .unwrap();
        assert!(host.install_replacement_for_test(replacement).is_err());
        assert!(Arc::ptr_eq(&host.generation().unwrap(), &retained));
        assert_eq!(host.application.as_ref().unwrap().instance(), application);
        assert_eq!(host.preview.as_ref().unwrap().instance, preview);
    }

    #[test]
    fn reload_missing_any_retained_owner_preserves_the_whole_preview_host() {
        let project = Project::new("reload-missing-owner");
        project.write("src/main.rs", &owners_source("i64", "i64", "", ""));
        let engine = reload_engine();
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));
        let selected = first_preview_selection(&host);
        assert!(host.select(selected, None).is_some());
        let retained = host.generation().unwrap();
        let application = host.application.as_ref().unwrap().instance();
        let preview = host.preview.as_ref().unwrap().instance;

        project.write("src/main.rs", preview_only_owner_source());
        let replacement = reload_plan(&host, reload_candidate(&engine, &project))
            .bind(&engine, reload_registry())
            .unwrap();
        assert!(matches!(
            host.install_replacement_for_test(replacement),
            Err(install::Error::Missing)
        ));
        assert!(Arc::ptr_eq(&host.generation().unwrap(), &retained));
        assert_eq!(host.application.as_ref().unwrap().instance(), application);
        assert_eq!(host.preview.as_ref().unwrap().instance, preview);

        project.write("src/main.rs", application_only_owner_source());
        let replacement = reload_plan(&host, reload_candidate(&engine, &project))
            .bind(&engine, reload_registry())
            .unwrap();
        assert!(matches!(
            host.install_replacement_for_test(replacement),
            Err(install::Error::Missing)
        ));
        assert!(Arc::ptr_eq(&host.generation().unwrap(), &retained));
        assert_eq!(host.application.as_ref().unwrap().instance(), application);
        assert_eq!(host.preview.as_ref().unwrap().instance, preview);
    }

    #[test]
    fn reload_aborts_old_tasks_and_drops_delayed_envelopes() {
        let project = Project::new("reload-task-retirement");
        project.write("src/main.rs", application_task_source());
        let engine = reload_engine();
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));
        let (pending, ready) = host.take_boot();
        let startup = iced::Task::batch([host.lower(pending), ready]);
        let handles = host.application.as_ref().unwrap().task_handles();
        assert_eq!(handles.len(), 1);
        assert!(!handles[0].is_aborted());
        let delayed = one_message(startup).expect("old startup must emit one delayed message");
        let instance = host.application.as_ref().unwrap().instance();

        let changed =
            application_task_source().replace("text(self.value)", "text(\"new generation\")");
        project.write("src/main.rs", &changed);
        let plan = reload_plan(&host, reload_candidate(&engine, &project));
        let replacement = plan.bind(&engine, reload_registry()).unwrap();
        let outcome = host.install_replacement_for_test(replacement).unwrap();

        assert_eq!(
            outcome.evidence()[0].action(),
            rawr::reload::compatibility::Action::Retain
        );
        assert!(handles[0].is_aborted());
        assert_eq!(host.application.as_ref().unwrap().instance(), instance);
        assert!(one_message(outcome.into_task()).is_none());
        let follow_up = Host::update_native(&mut host, delayed);
        assert!(one_message(follow_up).is_none());
        assert_text(&host, "new generation");
    }

    #[test]
    fn reload_retires_subscription_identity_and_drops_old_stream_output() {
        let project = Project::new("reload-subscription-retirement");
        project.write("src/main.rs", subscription_source());
        let starts = Arc::new(AtomicUsize::new(0));
        let events = stream_facade();
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        engine.register_host_facade(events.clone()).unwrap();
        let engine = Shared::new(engine);
        let mut host = reload_host_with_registry(
            &engine,
            reload_candidate(&engine, &project),
            stream_registry(events.clone(), starts.clone()),
        );
        let old = Host::subscription_native(&host);
        let old_hash = subscription_hash(old);
        let delayed = one_subscription_message(Host::subscription_native(&host));

        let changed = subscription_source().replace("value + 1", "value + 2");
        project.write("src/main.rs", &changed);
        let plan = reload_plan(&host, reload_candidate(&engine, &project));
        let replacement = plan
            .bind(&engine, stream_registry(events, starts.clone()))
            .unwrap();
        let outcome = host.install_replacement_for_test(replacement).unwrap();
        assert_eq!(
            outcome.evidence()[0].action(),
            rawr::reload::compatibility::Action::Retain
        );
        assert!(one_message(outcome.into_task()).is_none());
        let current_subscription = Host::subscription_native(&host);
        assert_ne!(subscription_hash(current_subscription), old_hash);

        let _ = Host::update_native(&mut host, delayed);
        assert_text(&host, "7");
        let current = one_subscription_message(Host::subscription_native(&host));
        let _ = Host::update_native(&mut host, current);
        assert_text(&host, "9");
        assert_eq!(starts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn application_update_between_prepare_and_admission_is_stale() {
        let project = Project::new("reload-stale-application-update");
        project.write("src/main.rs", full_source());
        let engine = reload_engine();
        let mut host = reload_host(&engine, reload_candidate(&engine, &project));

        let changed = full_source().replace("button(\"Increment\")", "button(\"Candidate\")");
        project.write("src/main.rs", &changed);
        let replacement = reload_plan(&host, reload_candidate(&engine, &project))
            .bind(&engine, reload_registry())
            .unwrap();
        let prepared = host.prepare_for_test(replacement).unwrap();

        let update = click(&host, "Increment");
        let _ = Host::update_native(&mut host, update);
        assert!(matches!(
            host.commit_for_test(prepared),
            Err(install::Error::Stale)
        ));
        assert_text(&host, "1");
    }

    #[test]
    fn preview_selection_close_and_reset_each_revoke_preparation() {
        #[derive(Clone, Copy)]
        enum Mutation {
            Select,
            Close,
            Reset,
        }

        for mutation in [Mutation::Select, Mutation::Close, Mutation::Reset] {
            let name = match mutation {
                Mutation::Select => "select",
                Mutation::Close => "close",
                Mutation::Reset => "reset",
            };
            let project = Project::new(name);
            project.write("src/main.rs", full_source());
            let engine = reload_engine();
            let mut host = reload_host(&engine, reload_candidate(&engine, &project));
            let Selection::Preview(selected) = selection(&host, "counter / Default") else {
                panic!("test requires a preview selection");
            };
            if !matches!(mutation, Mutation::Select) {
                host.select_control(&selected).unwrap();
            }

            let changed =
                full_source().replace("button(\"Increment\")", "button(\"Prepared candidate\")");
            project.write("src/main.rs", &changed);
            let replacement = reload_plan(&host, reload_candidate(&engine, &project))
                .bind(&engine, reload_registry())
                .unwrap();
            let prepared = host.prepare_for_test(replacement).unwrap();

            match mutation {
                Mutation::Select => host.select_control(&selected).unwrap(),
                Mutation::Close => host.close_preview(),
                Mutation::Reset => host.reset_control().unwrap(),
            }
            assert!(matches!(
                host.commit_for_test(prepared),
                Err(install::Error::Stale)
            ));
        }
    }

    #[test]
    fn cross_host_admission_rejects_a_distinct_slot_identity() {
        let project = Project::new("reload-cross-host");
        project.write("src/main.rs", full_source());
        let engine = reload_engine();
        let first = reload_host(&engine, reload_candidate(&engine, &project));
        let mut second = reload_host(&engine, reload_candidate(&engine, &project));
        project.write(
            "src/main.rs",
            &full_source().replace("button(\"Increment\")", "button(\"Candidate\")"),
        );
        let replacement = reload_plan(&first, reload_candidate(&engine, &project))
            .bind(&engine, reload_registry())
            .unwrap();
        let prepared = first.prepare_for_test(replacement).unwrap();
        let retained = first.generation().unwrap();

        assert!(matches!(
            second.commit_for_test(prepared),
            Err(install::Error::Stale)
        ));
        assert!(Arc::ptr_eq(&first.generation().unwrap(), &retained));
        assert_text(&second, "0");
    }

    #[test]
    fn stale_prepared_operation_never_starts_its_factory() {
        let project = Project::new("reload-stale-prepared-operation");
        project.write("src/main.rs", full_source());
        let effects = effects_facade();
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        engine.register_host_facade(effects.clone()).unwrap();
        let engine = Shared::new(engine);
        let starts = Arc::new(AtomicUsize::new(0));
        let mut host = reload_host_with_registry(
            &engine,
            reload_candidate(&engine, &project),
            operation_registry(effects.clone(), starts.clone()),
        );

        project.write("src/main.rs", application_operation_source());
        let replacement = reload_plan(&host, reload_candidate(&engine, &project))
            .bind(&engine, operation_registry(effects, starts.clone()))
            .unwrap();
        let prepared = host.prepare_for_test(replacement).unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 0);

        host.apply(|_state, _catalog, _context| {});
        assert!(matches!(
            host.commit_for_test(prepared),
            Err(install::Error::Stale)
        ));
        assert_eq!(starts.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn multi_owner_operation_and_subscription_activate_only_after_admission() {
        let project = Project::new("reload-multi-owner-activation");
        project.write(
            "src/main.rs",
            &multi_owner_activation_source("i64", "0", "Task::none()"),
        );
        let effects = effects_facade();
        let events = stream_facade();
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        engine.register_host_facade(effects.clone()).unwrap();
        engine.register_host_facade(events.clone()).unwrap();
        let engine = Shared::new(engine);
        let operations = Arc::new(AtomicUsize::new(0));
        let subscriptions = Arc::new(AtomicUsize::new(0));
        let mut host = reload_host_with_registry(
            &engine,
            reload_candidate(&engine, &project),
            operation_stream_registry(
                effects.clone(),
                operations.clone(),
                events.clone(),
                subscriptions.clone(),
            ),
        );
        let selected = first_preview_selection(&host);
        assert!(host.select(selected, None).is_some());

        project.write(
            "src/main.rs",
            &multi_owner_activation_source("bool", "false", "Task::perform(load(1), loaded)"),
        );
        let replacement = reload_plan(&host, reload_candidate(&engine, &project))
            .bind(
                &engine,
                operation_stream_registry(
                    effects,
                    operations.clone(),
                    events,
                    subscriptions.clone(),
                ),
            )
            .unwrap();
        let prepared = host.prepare_for_test(replacement).unwrap();
        assert_eq!(operations.load(Ordering::SeqCst), 0);
        assert_eq!(subscriptions.load(Ordering::SeqCst), 0);

        let outcome = host.commit_for_test(prepared).unwrap();
        assert_eq!(
            outcome
                .evidence()
                .iter()
                .map(rawr::reload::compatibility::Evidence::action)
                .collect::<Vec<_>>(),
            vec![
                rawr::reload::compatibility::Action::Reset,
                rawr::reload::compatibility::Action::Retain,
            ]
        );
        assert_eq!(operations.load(Ordering::SeqCst), 0);
        assert_eq!(subscriptions.load(Ordering::SeqCst), 0);

        assert!(one_message(outcome.into_task()).is_some());
        assert_eq!(operations.load(Ordering::SeqCst), 1);
        assert_eq!(subscriptions.load(Ordering::SeqCst), 0);
        host.close_preview();
        let _message = one_subscription_message(Host::subscription_native(&host));
        assert_eq!(subscriptions.load(Ordering::SeqCst), 1);
    }

    impl Host {
        fn instance(&self) -> Option<rawr::application::Instance> {
            match self.selection {
                Selection::Application => self
                    .application
                    .as_ref()
                    .map(crate::program::Adapter::instance),
                Selection::Preview(_) => self.preview.as_ref().map(|preview| preview.instance),
            }
        }

        fn visible_generation(&self) -> Option<rawr::program::GenerationId> {
            match self.selection {
                Selection::Application => self
                    .application
                    .as_ref()
                    .map(crate::program::Adapter::generation),
                Selection::Preview(_) => self.preview.as_ref().map(|_| self.catalog().generation()),
            }
        }

        fn active_theme(&self) -> &crate::theme::Value {
            self.visible_theme()
                .expect("test expected an active session theme")
        }
    }

    impl Host {
        pub(crate) fn pick(&mut self, key: rawr::preview::Key) {
            self.select(Selection::Preview(key), None);
        }
    }

    fn host(source: &str) -> Host {
        let prepared = Compiler::new()
            .unwrap()
            .check_named_source("preview-host.rs", source)
            .unwrap()
            .prepare()
            .unwrap();
        prepared.into_control().into_state()
    }

    fn reload_engine() -> Shared<Engine> {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        Shared::new(engine)
    }

    fn reload_registry() -> rawr_host::runtime::Registry {
        let mut registry = rawr_host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        registry
    }

    fn reload_candidate(engine: &Shared<Engine>, project: &Project) -> rawr::reload::Candidate {
        let target = rawr::project::Target::discover(project.path()).unwrap();
        engine.compile_package_generation(&target).unwrap()
    }

    fn reload_host(engine: &Shared<Engine>, candidate: rawr::reload::Candidate) -> Host {
        reload_host_with_registry(engine, candidate, reload_registry())
    }

    fn reload_host_with_registry(
        engine: &Shared<Engine>,
        candidate: rawr::reload::Candidate,
        registry: rawr_host::runtime::Registry,
    ) -> Host {
        let source = candidate.source().entry_source().diagnostic().clone();
        let bootstrap = candidate.bind(engine, registry).unwrap();
        Host::bootstrap(bootstrap, source, None).unwrap()
    }

    fn reload_plan(
        host: &Host,
        candidate: rawr::reload::Candidate,
    ) -> rawr::reload::compatibility::Plan {
        host.plan_for_test(candidate).unwrap()
    }

    fn booted_host(source: &str) -> (Host, iced::Task<Message>) {
        let prepared = Compiler::new()
            .unwrap()
            .check_named_source("preview-host.rs", source)
            .unwrap()
            .prepare()
            .unwrap();
        Application::new(prepared.into_control().into_state()).boot_native()
    }

    fn host_with_operation(source: &str, starts: Arc<AtomicUsize>) -> Host {
        let effects = effects_facade();
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        engine.register_host_facade(effects.clone()).unwrap();
        let source = rawr::diagnostic::Source::new("preview-operation.rs", source).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_preview_catalog_source(source.clone())
            .unwrap();
        let launch = engine.launch_previews(descriptor).unwrap();
        let registry = operation_registry(effects, starts);
        let catalog = Arc::new(launch.bind(registry).unwrap());
        Host::new(catalog, source, None).unwrap()
    }

    fn operation_registry(
        effects: rawr_host::facade::Definition,
        starts: Arc<AtomicUsize>,
    ) -> rawr_host::runtime::Registry {
        let operation = effects
            .items()
            .find(|item| item.path() == "effects::load")
            .unwrap()
            .clone();
        let mut binding = rawr_host::runtime::Builder::new(effects);
        binding
            .bind_operation(
                &operation,
                move |arguments| {
                    starts.fetch_add(1, Ordering::SeqCst);
                    async move {
                        let mut values = arguments.into_values().into_vec();
                        Ok::<_, ()>(Dynamic::from(values.remove(0).as_int().map_err(|_| ())?))
                    }
                },
                |_| Dynamic::from(-1_i64),
            )
            .unwrap();
        let mut registry = rawr_host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        registry.install(binding.finish()).unwrap();
        registry
    }

    fn host_with_stream(source: &str, starts: Arc<AtomicUsize>) -> Host {
        let events = stream_facade();
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        engine.register_host_facade(events.clone()).unwrap();
        let source = rawr::diagnostic::Source::new("preview-stream.rs", source).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_preview_catalog_source(source.clone())
            .unwrap();
        let launch = engine.launch_previews(descriptor).unwrap();
        let registry = stream_registry(events, starts);
        let catalog = Arc::new(launch.bind(registry).unwrap());
        Host::new(catalog, source, None).unwrap()
    }

    fn stream_registry(
        events: rawr_host::facade::Definition,
        starts: Arc<AtomicUsize>,
    ) -> rawr_host::runtime::Registry {
        let operation = events
            .items()
            .find(|item| item.path() == "events::ticks")
            .unwrap()
            .clone();
        let mut binding = rawr_host::runtime::Builder::new(events);
        binding
            .bind_stream_operation(
                &operation,
                |index, value, writer| {
                    if index != 0 {
                        return Err(rawr_host::stream::identity::Error::new(
                            "unexpected stream argument index",
                        ));
                    }
                    let value = value.as_int().map_err(|_| {
                        rawr_host::stream::identity::Error::new("expected canonical i64 argument")
                    })?;
                    writer.i64(value);
                    Ok(())
                },
                move |arguments| {
                    starts.fetch_add(1, Ordering::SeqCst);
                    let mut values = arguments.into_values().into_vec();
                    iced::futures::stream::iter([Ok::<_, ()>(values.remove(0))])
                },
                |_| Dynamic::from(-1_i64),
            )
            .unwrap();
        let mut registry = rawr_host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        registry.install(binding.finish()).unwrap();
        registry
    }

    fn operation_stream_registry(
        effects: rawr_host::facade::Definition,
        operations: Arc<AtomicUsize>,
        events: rawr_host::facade::Definition,
        subscriptions: Arc<AtomicUsize>,
    ) -> rawr_host::runtime::Registry {
        let operation = effects
            .items()
            .find(|item| item.path() == "effects::load")
            .unwrap()
            .clone();
        let mut operation_binding = rawr_host::runtime::Builder::new(effects);
        operation_binding
            .bind_operation(
                &operation,
                move |arguments| {
                    operations.fetch_add(1, Ordering::SeqCst);
                    async move {
                        let mut values = arguments.into_values().into_vec();
                        Ok::<_, ()>(Dynamic::from(values.remove(0).as_int().map_err(|_| ())?))
                    }
                },
                |_| Dynamic::from(-1_i64),
            )
            .unwrap();

        let stream = events
            .items()
            .find(|item| item.path() == "events::ticks")
            .unwrap()
            .clone();
        let mut stream_binding = rawr_host::runtime::Builder::new(events);
        stream_binding
            .bind_stream_operation(
                &stream,
                |index, value, writer| {
                    if index != 0 {
                        return Err(rawr_host::stream::identity::Error::new(
                            "unexpected stream argument index",
                        ));
                    }
                    let value = value.as_int().map_err(|_| {
                        rawr_host::stream::identity::Error::new("expected canonical i64 argument")
                    })?;
                    writer.i64(value);
                    Ok(())
                },
                move |arguments| {
                    subscriptions.fetch_add(1, Ordering::SeqCst);
                    let mut values = arguments.into_values().into_vec();
                    iced::futures::stream::iter([Ok::<_, ()>(values.remove(0))])
                },
                |_| Dynamic::from(-1_i64),
            )
            .unwrap();

        let mut registry = rawr_host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        registry.install(operation_binding.finish()).unwrap();
        registry.install(stream_binding.finish()).unwrap();
        registry
    }

    fn booted_host_with_operation(
        source: &str,
        starts: Arc<AtomicUsize>,
    ) -> (Host, iced::Task<Message>) {
        Application::new(host_with_operation(source, starts)).boot_native()
    }

    fn effects_facade() -> rawr_host::facade::Definition {
        let mut builder =
            rawr_host::facade::Builder::new("effects", rawr_host::facade::Abi::new(1).unwrap())
                .unwrap();
        let mut load = rawr_host::signature::Builder::new();
        load.parameter(rawr_host::type_ref::Type::builtin(
            rawr_host::type_ref::Builtin::I64,
        ));
        load.output(rawr_host::type_ref::Type::builtin(
            rawr_host::type_ref::Builtin::I64,
        ));
        builder
            .item(rawr_host::item::Definition::operation("load", load.finish()).unwrap())
            .unwrap();
        builder.finish().unwrap()
    }

    fn stream_facade() -> rawr_host::facade::Definition {
        let mut builder =
            rawr_host::facade::Builder::new("events", rawr_host::facade::Abi::new(1).unwrap())
                .unwrap();
        let mut ticks = rawr_host::signature::Builder::new();
        ticks.parameter(rawr_host::type_ref::Type::builtin(
            rawr_host::type_ref::Builtin::I64,
        ));
        ticks.output(rawr_host::type_ref::Type::builtin(
            rawr_host::type_ref::Builtin::I64,
        ));
        builder
            .item(
                rawr_host::item::Definition::stream_operation(
                    "ticks",
                    ticks.finish(),
                    rawr_host::stream::Version::new(1),
                )
                .unwrap(),
            )
            .unwrap();
        builder.finish().unwrap()
    }

    fn subscription_hash(subscription: iced::Subscription<Message>) -> u64 {
        let mut recipes = iced::advanced::subscription::into_recipes(subscription);
        assert_eq!(recipes.len(), 1);
        let mut hasher = iced::advanced::subscription::Hasher::default();
        recipes.remove(0).hash(&mut hasher);
        hasher.finish()
    }

    fn one_subscription_message(subscription: iced::Subscription<Message>) -> Message {
        let mut recipes = iced::advanced::subscription::into_recipes(subscription);
        assert_eq!(recipes.len(), 1);
        let mut stream = recipes
            .remove(0)
            .stream(Box::pin(iced::futures::stream::empty()));
        iced::futures::executor::block_on(async move {
            let message = stream.next().await.expect("stream must emit one item");
            assert!(stream.next().await.is_none());
            message
        })
    }

    fn selection(host: &Host, label: &str) -> Selection {
        host.choices
            .iter()
            .find(|choice| choice.label == label)
            .unwrap()
            .selection
            .clone()
    }

    fn first_preview_selection(host: &Host) -> Selection {
        host.choices
            .iter()
            .find_map(|choice| match &choice.selection {
                Selection::Application => None,
                Selection::Preview(_) => Some(choice.selection.clone()),
            })
            .expect("test Catalog must expose one preview")
    }

    fn click(host: &Host, label: &str) -> Message {
        let mut simulator = iced_test::simulator(host.view());
        simulator.click(label).unwrap();
        simulator
            .into_messages()
            .find(|message| matches!(message, Message::Script(_)))
            .expect("script widget must emit one host-routed message")
    }

    fn click_at(host: &Host, point: iced::Point) -> Message {
        let mut simulator = iced_test::simulator(host.view());
        simulator.point_at(point);
        let _ = simulator.simulate(iced_test::simulator::click());
        simulator
            .into_messages()
            .find(|message| matches!(message, Message::Script(_)))
            .expect("script widget must emit one host-routed message")
    }

    fn typewrite(host: &Host, placeholder: &str, text: &str) -> Vec<Message> {
        let mut simulator = iced_test::simulator(host.view());
        simulator.click(placeholder).unwrap();
        let _ = simulator.typewrite(text);
        simulator
            .into_messages()
            .filter(|message| matches!(message, Message::Script(_)))
            .collect()
    }

    fn assert_text(host: &Host, expected: &str) {
        let mut simulator = iced_test::simulator(host.view());
        simulator.find(expected).unwrap();
    }

    fn chrome_bounds(host: &Host) -> (iced::Rectangle, iced::Rectangle) {
        let mut simulator = iced_test::simulator(host.view());
        let content = simulator
            .find(iced_test::selector::id(chrome::CONTENT_ID))
            .unwrap()
            .bounds();
        let footer = simulator
            .find(iced_test::selector::id(chrome::FOOTER_ID))
            .unwrap()
            .bounds();
        (content, footer)
    }

    fn assert_centered_zones(host: &Host) {
        let mut simulator = iced_test::simulator(host.view());
        let footer = simulator
            .find(iced_test::selector::id(chrome::FOOTER_ID))
            .unwrap()
            .bounds();
        let status = simulator
            .find(iced_test::selector::id(chrome::STATUS_ID))
            .unwrap()
            .bounds();
        let selection = simulator
            .find(iced_test::selector::id(chrome::SELECTION_ID))
            .unwrap()
            .bounds();
        let actions = simulator
            .find(iced_test::selector::id(chrome::ACTIONS_ID))
            .unwrap()
            .bounds();

        assert!(status.x + status.width <= selection.x + 0.5);
        assert!(selection.x + selection.width <= actions.x + 0.5);
        assert!((status.width - selection.width).abs() <= 1.0);
        assert!((selection.width - actions.width).abs() <= 1.0);
        let footer_center = footer.x + footer.width / 2.0;
        let selection_center = selection.x + selection.width / 2.0;
        assert!((footer_center - selection_center).abs() <= 1.0);
    }

    fn assert_compact_controls(
        host: &Host,
        has_close: bool,
        has_theme: bool,
        has_diagnostic: bool,
    ) {
        const HEIGHT: f32 = 480.0;
        let mut simulator = iced_test::Simulator::with_size(
            iced_test::core::Settings::default(),
            iced::Size::new(1024.0, HEIGHT),
            host.view(),
        );
        let footer = simulator
            .find(iced_test::selector::id(chrome::FOOTER_ID))
            .unwrap()
            .bounds();

        assert!(footer.height <= chrome::FOOTER_MAX_HEIGHT);
        assert!((footer.y + footer.height - HEIGHT).abs() <= 1.0);
        for id in [
            chrome::STATUS_ID,
            chrome::SELECTION_ID,
            chrome::ACTIONS_ID,
            chrome::SELECTOR_ID,
            chrome::RESET_ID,
        ] {
            simulator.find(iced_test::selector::id(id)).unwrap();
        }
        assert_eq!(
            simulator
                .find(iced_test::selector::id(chrome::CLOSE_ID))
                .is_ok(),
            has_close
        );
        assert_eq!(
            simulator
                .find(iced_test::selector::id(chrome::THEME_ID))
                .is_ok(),
            has_theme
        );
        assert_eq!(
            simulator
                .find(iced_test::selector::id(chrome::DIAGNOSTIC_ID))
                .is_ok(),
            has_diagnostic
        );
    }

    fn assert_script_owner(
        message: &Message,
        instance: rawr::application::Instance,
        generation: rawr::program::GenerationId,
    ) {
        let Message::Script(message) = message else {
            panic!("Task must route into the private Script branch");
        };
        assert_eq!(message.instance(), instance);
        assert_eq!(message.generation(), generation);
    }

    fn one_message(task: iced::Task<Message>) -> Option<Message> {
        let mut stream = iced_test::runtime::task::into_stream(task)?;
        iced_test::futures::futures::executor::block_on(async move {
            let mut messages = Vec::new();
            while let Some(action) = stream.next().await {
                match action {
                    iced_test::runtime::Action::Output(Message::Script(message)) => {
                        messages.extend(crate::dispatch::inline(message).map(Message::Script));
                    }
                    iced_test::runtime::Action::Output(message) => messages.push(message),
                    action => panic!("expected message output, found {action:?}"),
                }
            }
            if messages.last().is_some_and(|message| {
                matches!(message, Message::Script(message) if message.task_completion().is_some())
            }) {
                let _completion = messages.pop();
            }
            assert!(messages.iter().all(|message| {
                !matches!(message, Message::Script(message) if message.task_completion().is_some())
            }));
            assert!(messages.len() <= 1, "task emitted extra script outputs");
            messages.pop()
        })
    }

    fn full_source() -> &'static str {
        r#"
use iced::Theme;
use iced::Element;
use iced::widget::{button, column, text};

#[derive(Default)] struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Increment }

impl State {
    fn update(&mut self, message: Message) {
        match message { Message::Increment => self.value += 1 }
    }

    fn dark(&self) -> Theme { Theme::Dark }
    fn broken_theme(&self) -> Theme { throw "broken theme" }

    #[preview(name = "Default", group = "counter", themes = [State::dark, State::broken_theme])]
    #[preview(name = "Compact", group = "counter", themes = [State::dark, State::broken_theme])]
    fn view(&self) -> Element<'_, Message> {
        let increment = button("Increment").on_press(Message::Increment);
        let value = text(self.value);
        column![increment, value]
    }

    #[preview(name = "Broken", group = "counter")]
    fn broken(&self) -> Element<'_, Message> { throw "broken subject" }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
    }

    fn preview_form_controls_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::{checkbox, column, text, text_input};

#[derive(Default)] struct State { value: String, toggled: bool }
#[derive(Debug, Clone)] enum Leaf { Input(String) }
#[derive(Debug, Clone)] enum Child { Input(Leaf) }
#[derive(Debug, Clone)] enum Message { Form(Child), Toggle(bool) }

pub fn input() -> Element<'_, Leaf> {
    text_input("Preview input", "").on_input(Leaf::Input)
}

pub fn form() -> Element<'_, Child> {
    input().map(Child::Input)
}

impl State {
    fn update(&mut self, message: Message) {
        match message {
            Message::Form(Child::Input(Leaf::Input(value))) => self.value = value,
            Message::Toggle(toggled) => self.toggled = toggled,
        }
    }

    #[preview(name = "Form controls")]
    fn view(&self) -> Element<'_, Message> {
        column![
            form().map(Message::Form),
            checkbox(self.toggled)
                .label("Preview toggle")
                .on_toggle(Message::Toggle),
            text(self.value),
            text(self.toggled),
        ]
    }
}
"#
    }

    fn preview_slider_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::{column, slider, text};

#[derive(Default)] struct State { value: i64 }
#[derive(Debug, Clone)] enum Leaf { Changed(i64) }
#[derive(Debug, Clone)] enum Message { Slider(Leaf) }

pub fn control(value: i64) -> Element<'_, Leaf> {
    slider(0..=100, value, Leaf::Changed).step(5)
}

impl State {
    fn update(&mut self, message: Message) {
        match message { Message::Slider(Leaf::Changed(value)) => self.value = value }
    }

    #[preview(name = "Slider")]
    fn view(&self) -> Element<'_, Message> {
        column![control(self.value).map(Message::Slider), text(self.value)]
    }
}
"#
    }

    fn reload_throwing_control_source(label: &str) -> String {
        format!(
            r#"
use iced::Element;
use iced::widget::{{column, text, text_input}};

#[derive(Default)] struct State;
#[derive(Debug, Clone)] enum Message {{ Input(String) }}

impl Message {{
    fn map_input(value: String) -> Message {{ throw "retained mapper failed" }}
}}

impl State {{
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{
        column![
            text_input("Reload input", "").on_input(Message::map_input),
            text("{label}"),
        ]
    }}
}}

pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
        )
    }

    fn reload_throwing_slider_source(label: &str) -> String {
        format!(
            r#"
use iced::Element;
use iced::widget::{{column, slider, text}};

#[derive(Default)] struct State;
#[derive(Debug, Clone)] enum Message {{ Changed(i64) }}

impl Message {{
    fn map_slider(value: i64) -> Message {{ throw "retained slider mapper failed" }}
}}

impl State {{
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{
        column![
            slider(0..=100, 0, Message::map_slider).step(5),
            text("{label}"),
        ]
    }}
}}

pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
        )
    }

    fn owners_source(
        application: &str,
        preview: &str,
        application_view: &str,
        preview_view: &str,
    ) -> String {
        let application_view = if application_view.is_empty() {
            "text(\"application\")"
        } else {
            application_view
        };
        let preview_view = if preview_view.is_empty() {
            "button(\"Preview\")"
        } else {
            preview_view
        };
        format!(
            r#"
use iced::Element;
use iced::widget::{{button, text}};

#[derive(Default)] struct App {{ value: {application} }}
#[derive(Debug, Clone, Copy)] enum AppMessage {{ Tick }}
impl App {{
    fn update(&mut self, message: AppMessage) {{}}
    fn view(&self) -> Element<AppMessage> {{ {application_view} }}
}}

#[derive(Default)] struct Fixture {{ value: {preview} }}
#[derive(Debug, Clone, Copy)] enum PreviewMessage {{ Tick }}
impl Fixture {{
    fn update(&mut self, message: PreviewMessage) {{}}
    #[preview(name = "Owner")]
    fn view(&self) -> Element<PreviewMessage> {{ {preview_view} }}
}}

pub fn main() -> iced::Result {{ iced::run(App::update, App::view) }}
"#,
        )
    }

    fn standalone_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

#[preview(name = "Standalone")]
fn sample() -> Element<'_, ()> { text("standalone") }
"#
    }

    fn preview_only_owner_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::button;

#[derive(Default)] struct Fixture { value: i64 }
#[derive(Debug, Clone, Copy)] enum PreviewMessage { Tick }
impl Fixture {
    fn update(&mut self, message: PreviewMessage) {}
    #[preview(name = "Owner")]
    fn view(&self) -> Element<PreviewMessage> { button("Preview") }
}
"#
    }

    fn application_only_owner_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct App { value: i64 }
#[derive(Debug, Clone, Copy)] enum AppMessage { Tick }
impl App {
    fn update(&mut self, message: AppMessage) {}
    fn view(&self) -> Element<AppMessage> { text("application") }
}

pub fn main() -> iced::Result { iced::run(App::update, App::view) }
"#
    }

    fn titled_application_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

struct App;
#[derive(Debug, Clone, Copy)] enum Message { Tick }
impl App {
    fn new() -> App { App {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<Message> { text("application") }
    fn title(&self) -> String { "Source-owned title" }
}

#[preview(name = "Shell fixture")]
fn fixture() -> Element<'_, ()> { text("preview") }

pub fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .centered()
        .run()
}
"#
    }

    fn added_application_source(boot: &str) -> String {
        format!(
            r#"
use iced::Element;
use iced::widget::text;

struct App;
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}
impl App {{
    fn new() -> App {{ {boot} }}
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<'_, Message> {{ text("added application") }}
}}

#[preview(name = "Standalone")]
fn sample() -> Element<'_, ()> {{ text("standalone") }}

pub fn main() -> iced::Result {{
    iced::application(App::new, App::update, App::view).run()
}}
"#,
        )
    }

    fn subscription_source() -> &'static str {
        r#"
use events::ticks;
use iced::Subscription;
use iced::Element;
use iced::widget::text;
struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Tick(i64) }
pub fn tick(value: i64) -> Message { Message::Tick(value) }
impl State {
    fn new() -> State { State { value: 7 } }
    fn update(&mut self, message: Message) {
        match message { Message::Tick(value) => self.value = value + 1 }
    }
    #[preview(name = "Passive preview", seed = State::new)]
    fn view(&self) -> Element<'_, Message> { text(self.value) }
    fn subscription(&self) -> Subscription<Message> { ticks(self.value).map(tick) }
}
pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .subscription(State::subscription)
        .run()
}
"#
    }

    fn multi_owner_activation_source(field: &str, value: &str, task: &str) -> String {
        format!(
            r#"
use effects::load;
use events::ticks;
use iced::{{Subscription, Task}};
use iced::Element;
use iced::widget::text;

struct App {{ value: {field} }}
#[derive(Debug, Clone, Copy)] enum Message {{ Loaded(i64), Tick(i64) }}
pub fn loaded(value: i64) -> Message {{ Message::Loaded(value) }}
pub fn tick(value: i64) -> Message {{ Message::Tick(value) }}
impl App {{
    fn boot() -> (App, Task<Message>) {{ (App {{ value: {value} }}, {task}) }}
    fn update(&mut self, message: Message) {{
        match message {{ Message::Loaded(_) => (), Message::Tick(_) => () }}
    }}
    fn view(&self) -> Element<'_, Message> {{ text(self.value) }}
    fn subscription(&self) -> Subscription<Message> {{ ticks(4).map(tick) }}
}}

#[preview(name = "Owner")]
fn owner() -> Element<'_, ()> {{ text("preview owner") }}

pub fn main() -> iced::Result {{
    iced::application(App::boot, App::update, App::view)
        .subscription(App::subscription)
        .run()
}}
"#,
        )
    }

    fn scripted_preview_source() -> &'static str {
        r#"
use rawr_iced::widget::layout::Size;
use iced::Element;

struct Dial;

#[widget]
impl Dial {
    fn layout(
        &mut self,
        input: rawr_iced::widget::layout::Input,
    ) -> rawr_iced::widget::layout::Node {
        input.resolve(Size::fixed(20, 20), 20, 20)
    }
}

#[preview(name = "Scripted")]
fn sample() -> Element<()> { Dial {} }
"#
    }

    fn failing_application_boot_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn new() -> State { throw "broken application boot" }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { text("application") }
}

#[preview(name = "Healthy")]
fn healthy() -> Element<'_, ()> { text("healthy preview") }

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view).run()
}
"#
    }

    fn failing_application_view_source() -> &'static str {
        r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { throw "broken application view" }
}

#[preview(name = "Healthy")]
fn healthy() -> Element<'_, ()> { text("healthy preview") }

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#
    }

    fn failing_application_theme_source() -> &'static str {
        r#"
use iced::Theme;
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn new() -> State { State {} }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { text("application") }
    fn theme(&self) -> Theme { throw "broken application theme" }

}

#[preview(name = "Healthy")]
fn healthy() -> Element<'_, ()> { text("healthy preview") }

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .theme(State::theme)
        .run()
}
"#
    }

    fn failing_candidate_with_work_source() -> &'static str {
        r#"
use iced::Task;
use iced::Element;
use iced::widget::text;

struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn boot() -> (State, Task<Message>) {
        (State {}, Task::done(Message::Tick))
    }
    fn update(&mut self, message: Message) {}
    fn view(&self) -> Element<'_, Message> { throw "broken view with work" }
}

#[preview(name = "Healthy")]
fn healthy() -> Element<'_, ()> { text("healthy preview") }

pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
    }

    fn application_task_source() -> &'static str {
        r#"
use iced::Task;
use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Increment }

impl State {
    fn boot() -> (State, Task<Message>) {
        (State { value: 7 }, Task::done(Message::Increment))
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        self.value += 1;
        Task::done(Message::Increment)
    }
    #[preview]
    fn view(&self) -> Element<'_, Message> { text(self.value) }
}

pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
    }

    fn preview_task_source(done: bool) -> String {
        let task = if done {
            "Task::done(Message::Increment)"
        } else {
            "Task::none()"
        };
        format!(
            r#"
use iced::Task;
use iced::Element;
use iced::widget::{{button, column, text}};

#[derive(Default)] struct State {{ value: i64 }}
#[derive(Debug, Clone, Copy)] enum Message {{ Increment }}

impl State {{
    fn update(&mut self, message: Message) -> Task<Message> {{
        self.value += 1;
        {task}
    }}
    #[preview]
    fn view(&self) -> Element<'_, Message> {{
        let increment = button("Increment").on_press(Message::Increment);
        let value = text(self.value);
        column![increment, value]
    }}
}}
"#
        )
    }

    fn preview_operation_source() -> &'static str {
        r#"
use effects::load;
use iced::Task;
use iced::Element;
use iced::widget::button;

#[derive(Default)] struct First { value: i64 }
#[derive(Debug, Clone, Copy)] enum FirstMessage { Load, Loaded(i64) }
pub fn first_loaded(value: i64) -> FirstMessage { FirstMessage::Loaded(value) }
impl First {
    fn update(&mut self, message: FirstMessage) -> Task<FirstMessage> {
        match message {
            FirstMessage::Load => Task::perform(load(self.value), first_loaded),
            FirstMessage::Loaded(value) => { self.value = value; Task::none() }
        }
    }

    #[preview(name = "First")]
    fn view(&self) -> Element<'_, FirstMessage> {
        button("Load").on_press(FirstMessage::Load)
    }
}

#[derive(Default)] struct Second;
#[derive(Debug, Clone, Copy)] enum SecondMessage { Tick }
impl Second {
    fn update(&mut self, message: SecondMessage) {}
    #[preview(name = "Second")]
    fn view(&self) -> Element<'_, SecondMessage> {
        button("Tick").on_press(SecondMessage::Tick)
    }
}
"#
    }

    fn application_operation_source() -> &'static str {
        r#"
use effects::load;
use iced::Task;
use iced::Element;
use iced::widget::text;

struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { First(i64), Second(i64) }
pub fn first(value: i64) -> Message { Message::First(value) }
pub fn second(value: i64) -> Message { Message::Second(value) }
impl State {
    fn boot() -> (State, Task<Message>) {
        (State { value: 0 }, Task::perform(load(1), first))
    }
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::First(value) => {
                self.value += value;
                Task::perform(load(2), second)
            },
            Message::Second(value) => {
                self.value += value;
                Task::none()
            }
        }
    }
    fn view(&self) -> Element<'_, Message> { text(self.value) }
}

#[preview(name = "Preview")]
fn sample() -> Element<'_, ()> { text("preview") }

pub fn main() -> iced::Result {
    iced::application(State::boot, State::update, State::view).run()
}
"#
    }

    fn failing_preview_update_source() -> &'static str {
        r#"
use iced::Task;
use iced::Element;
use iced::widget::{button, column, text};

#[derive(Default)] struct State { value: i64 }
#[derive(Debug, Clone, Copy)] enum Message { Increment }

impl State {
    fn update(&mut self, message: Message) -> Task<Message> {
        self.value += 1;
        throw "invalid Task"
    }
    #[preview]
    fn view(&self) -> Element<'_, Message> {
        let increment = button("Increment").on_press(Message::Increment);
        let value = text(self.value);
        column![increment, value]
    }
}
"#
    }

    fn failing_standalone_subject_source() -> &'static str {
        r#"
use iced::Element;

#[preview(name = "Broken standalone")]
fn broken() -> Element<'_, ()> { throw "broken standalone subject" }
"#
    }

    fn child_application_root() -> &'static str {
        r#"
pub mod app;
use crate::app::State;
use iced::Element;
use iced::widget::text;

#[preview(name = "Healthy")]
fn healthy() -> Element<'_, ()> { text("healthy preview") }

pub fn main() -> iced::Result {
    iced::application(State::new, State::update, State::view)
        .theme(State::theme)
        .run()
}
"#
    }

    fn child_application(failure: &str) -> String {
        let boot = if failure == "boot" {
            "throw \"broken child boot\""
        } else {
            "State {}"
        };
        let update = if failure == "update" {
            "throw \"broken child update\""
        } else {
            ""
        };
        let view = if failure == "view" {
            "throw \"broken child view\""
        } else {
            "button(\"Trigger\").on_press(Message::Tick)"
        };
        let theme = if failure == "theme" {
            "throw \"broken child theme\""
        } else {
            "Theme::Light"
        };
        format!(
            r#"
use iced::Theme;
use iced::Element;
use iced::widget::button;

pub struct State;
#[derive(Debug, Clone, Copy)] pub enum Message {{ Tick }}

impl State {{
    pub fn new() -> State {{ {boot} }}
    pub fn update(&mut self, message: Message) {{ {update} }}
    pub fn view(&self) -> Element<'_, Message> {{ {view} }}
    pub fn theme(&self) -> Theme {{ {theme} }}
}}
"#
        )
    }

    struct Project {
        root: PathBuf,
    }

    impl Project {
        fn new(name: &str) -> Self {
            let id = NEXT_PROJECT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "rawr-preview-host-{}-{id}-{name}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(
                root.join("Cargo.toml"),
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            )
            .unwrap();
            Self { root }
        }

        fn path(&self) -> &Path {
            &self.root
        }

        fn write(&self, path: &str, source: &str) {
            let path = self.root.join(path);
            // A `.rs` file is a Rawr file: it starts with `// rawr`.
            let marked;
            let source = if path.extension().is_some_and(|extension| extension == "rs") {
                marked = format!("// rawr\n{}", source);
                marked.as_str()
            } else {
                source
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, source).unwrap();
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).ok();
        }
    }
}
