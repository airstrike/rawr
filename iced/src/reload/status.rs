//! Native reload phase and bounded diagnostic presentation state.

use std::collections::VecDeque;

use iced::widget::Widget as _;

const DIAGNOSTIC_LIMIT: usize = 3;

/// One observable live-reload phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// The current installed generation is valid and no reload is active.
    Ready,
    /// A relevant source change is waiting for or entering validation.
    Checking,
    /// The exact newest revision is compiling and preparing off-live.
    Reloading,
    /// The newest revision failed and the last-good generation remains live.
    Error,
}

/// One native reload action outside scripted Message authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Control {
    Refresh,
    Retry,
    Copy,
    Reset,
    /// Save a PNG of the window (Cmd+Shift+S).
    Screenshot,
    /// Hide the current host note.
    Dismiss,
}

pub(crate) fn subscription() -> iced::Subscription<Control> {
    // Native refresh remains available while a script widget has focus.
    iced::event::listen_with(|event, _status, _window| shortcut(event))
}

fn shortcut(event: iced::Event) -> Option<Control> {
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character(key),
            modifiers,
            repeat: false,
            ..
        }) if modifiers == iced::keyboard::Modifiers::COMMAND && key.eq_ignore_ascii_case("r") => {
            Some(Control::Refresh)
        }
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: iced::keyboard::Key::Character(key),
            modifiers,
            repeat: false,
            ..
        }) if modifiers
            == iced::keyboard::Modifiers::COMMAND | iced::keyboard::Modifiers::SHIFT
            && key.eq_ignore_ascii_case("s") =>
        {
            Some(Control::Screenshot)
        }
        _ => None,
    }
}

/// One live host's bounded reload status and canonical diagnostic history.
pub(crate) struct State {
    phase: Phase,
    terminal: bool,
    reports: VecDeque<rawr::diagnostic::Report>,
    note: Option<String>,
    /// The newest `cargo check` errors, shown whatever the phase.
    cargo: Option<super::cargo::Report>,
    /// Agents waiting for the next reload to commit or fail.
    waiting: Vec<super::agent::Call>,
}

impl State {
    pub(crate) const fn new() -> Self {
        Self {
            phase: Phase::Ready,
            terminal: false,
            reports: VecDeque::new(),
            note: None,
            cargo: None,
            waiting: Vec::new(),
        }
    }

    /// Answer `call` when the next reload commits or fails.
    pub(crate) fn wait(&mut self, call: super::agent::Call) {
        self.waiting.push(call);
    }

    /// Show one short host note (a saved screenshot, a reseed) until dismissed
    /// or replaced.
    pub(crate) fn note(&mut self, note: String) {
        self.note = Some(note);
    }

    pub(crate) fn dismiss(&mut self) {
        self.note = None;
    }

    /// Show the newest `cargo check` result: its errors, or nothing.
    pub(crate) fn cargo(&mut self, checked: super::cargo::Checked) {
        self.cargo = checked.0;
    }

    #[cfg(test)]
    pub(crate) fn current_note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// Record a saved or failed screenshot as the current note.
    pub(crate) fn saved(&mut self, saved: crate::screenshot::Saved) {
        let note = match saved {
            Ok(path) => format!("Screenshot saved · {}", path.display()),
            Err(error) => format!("Screenshot failed · {error}"),
        };
        eprintln!("rawr: {note}");
        self.note(note);
    }

    #[cfg(test)]
    pub(crate) const fn phase(&self) -> Phase {
        self.phase
    }

    pub(crate) fn progress(&mut self, phase: Phase) {
        debug_assert!(matches!(phase, Phase::Checking | Phase::Reloading));
        self.phase = phase;
        self.terminal = false;
    }

    pub(crate) fn failed(&mut self, report: rawr::diagnostic::Report) {
        self.phase = Phase::Error;
        self.push(report);
    }

    pub(crate) fn terminal(&mut self, report: rawr::diagnostic::Report) {
        self.phase = Phase::Error;
        self.terminal = true;
        self.push(report);
    }

    fn push(&mut self, report: rawr::diagnostic::Report) {
        for call in self.waiting.drain(..) {
            call.failed(&report);
        }
        self.record(report);
    }

    fn record(&mut self, report: rawr::diagnostic::Report) {
        if self.reports.contains(&report) {
            return;
        }
        if self.reports.len() == DIAGNOSTIC_LIMIT {
            self.reports.pop_front();
        }
        self.reports.push_back(report);
    }

    pub(crate) fn ready(&mut self) {
        self.phase = Phase::Ready;
        self.terminal = false;
        self.reports.clear();
    }

    /// Mark a committed reload ready and note any owner whose state was
    /// reseeded from boot instead of carried across.
    pub(crate) fn installed(&mut self, evidence: &[rawr::reload::compatibility::Evidence]) {
        self.ready();
        let note = reseeded(evidence);
        for call in self.waiting.drain(..) {
            call.answer(format!(
                "ok\nreloaded{}",
                note.as_deref()
                    .map(|note| format!("\n{note}"))
                    .unwrap_or_default()
            ));
        }
        if let Some(note) = note {
            eprintln!("rawr: {note}");
            self.note(note);
        }
    }

    pub(crate) fn can_retry(&self) -> bool {
        self.phase == Phase::Error && !self.terminal
    }

    pub(crate) fn can_refresh(&self) -> bool {
        !self.terminal
    }

    pub(crate) fn copy(&self) -> Option<String> {
        self.reports.back().map(rawr::diagnostic::render)
    }

    #[cfg(test)]
    pub(crate) fn reports(&self) -> impl ExactSizeIterator<Item = &rawr::diagnostic::Report> {
        self.reports.iter()
    }

    pub(crate) fn view<'a, Message: Clone + 'static>(
        &'a self,
        content: iced::Element<'a, Message>,
        map: impl Fn(Control) -> Message + Copy + 'a,
    ) -> iced::Element<'a, Message> {
        let content = self.note_view(content, map);
        let content = self.cargo_view(content);
        if self.phase == Phase::Ready {
            return content;
        }

        let status = iced::widget::text(self.label()).size(12);
        let mut body = iced::widget::column![status].spacing(8);
        if self.phase == Phase::Error {
            let mut details = iced::widget::column![].spacing(8);
            for report in self.reports.iter().rev() {
                details = details.push(crate::diagnostic::report_detail("reload", report));
            }
            let retry = chrome_button("Retry");
            let retry = if self.can_retry() {
                retry.on_press(map(Control::Retry))
            } else {
                retry
            };
            let controls = iced::widget::row![retry].spacing(4);
            body = body
                .push(
                    iced::widget::scrollable(details)
                        .height(iced::Length::Fixed(260.0))
                        .boxed(),
                )
                .push(
                    controls
                        .push(chrome_button("Copy").on_press(map(Control::Copy)).boxed())
                        .push(chrome_button("Reset").on_press(map(Control::Reset)).boxed())
                        .boxed(),
                );
        }
        let card = iced::widget::container(body)
            .padding(10)
            .style(crate::preview::chrome::card(self.phase == Phase::Error));
        let card = iced::widget::themer(Some(crate::preview::chrome::theme()), card);
        let overlay = iced::widget::container(card)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .padding(12)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom);
        iced::widget::stack![
            iced::widget::container(content)
                .width(iced::Length::Fill)
                .height(iced::Length::Fill),
            overlay
        ]
        .boxed()
    }

    /// The newest `cargo check` errors in a card across the top. The
    /// reload card keeps the bottom right. It stays until a check passes.
    fn cargo_view<'a, Message: Clone + 'static>(
        &'a self,
        content: iced::Element<'a, Message>,
    ) -> iced::Element<'a, Message> {
        let Some(cargo) = &self.cargo else {
            return content;
        };
        let details = iced::widget::text(&cargo.details)
            .size(11)
            .font(iced::Font::MONOSPACE);
        // Long output scrolls inside a bounded card.
        let height = if cargo.details.lines().count() > 10 {
            iced::Length::Fixed(160.0)
        } else {
            iced::Length::Shrink
        };
        let body = iced::widget::column![
            iced::widget::text(&cargo.heading).size(12),
            iced::widget::scrollable(details).height(height),
        ]
        .spacing(8);
        let card = iced::widget::container(body)
            .padding(10)
            .width(iced::Length::Fill)
            .style(crate::preview::chrome::card(true));
        let card = iced::widget::themer(Some(crate::preview::chrome::theme()), card);
        let overlay = iced::widget::container(card)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .padding(12)
            .align_y(iced::alignment::Vertical::Top);
        iced::widget::stack![
            iced::widget::container(content)
                .width(iced::Length::Fill)
                .height(iced::Length::Fill),
            overlay
        ]
        .boxed()
    }

    fn note_view<'a, Message: Clone + 'static>(
        &'a self,
        content: iced::Element<'a, Message>,
        map: impl Fn(Control) -> Message + Copy + 'a,
    ) -> iced::Element<'a, Message> {
        let Some(note) = &self.note else {
            return content;
        };
        let card = iced::widget::container(
            iced::widget::row![
                iced::widget::text(note).size(12),
                chrome_button("Dismiss").on_press(map(Control::Dismiss)),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        )
        .padding([6, 10])
        .style(crate::preview::chrome::card(true));
        let card = iced::widget::themer(Some(crate::preview::chrome::theme()), card);
        let overlay = iced::widget::container(card)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .padding(12)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Top);
        iced::widget::stack![
            iced::widget::container(content)
                .width(iced::Length::Fill)
                .height(iced::Length::Fill),
            overlay
        ]
        .boxed()
    }

    const fn label(&self) -> &'static str {
        match self.phase {
            Phase::Ready => "Rawr · Ready",
            Phase::Checking => "Rawr · Checking…",
            Phase::Reloading => "Rawr · Reloading…",
            Phase::Error if self.terminal => {
                "Rawr · Error · Reload stopped · Last good still running"
            }
            Phase::Error => "Rawr · Error · Last good still running",
        }
    }
}

/// One line naming each reseeded owner and why; `None` when every live owner
/// kept its state (or was only newly added).
fn reseeded(evidence: &[rawr::reload::compatibility::Evidence]) -> Option<String> {
    use rawr::reload::compatibility::{Action, Owner, Reason};

    let owners: Vec<String> = evidence
        .iter()
        .filter(|evidence| evidence.action() == Action::Reset)
        .filter_map(|evidence| {
            // A changed State type also changes the callbacks that take it;
            // the shape change alone explains the reseed.
            let schema = [Reason::Schema];
            let reasons = if evidence.reasons().contains(&Reason::Schema) {
                &schema[..]
            } else {
                evidence.reasons()
            };
            let reasons: Vec<&str> = reasons
                .iter()
                .filter_map(|reason| match reason {
                    Reason::Schema => Some("state shape changed"),
                    Reason::Callback => Some("callback signature changed"),
                    Reason::Symbolic => Some("retained callables changed"),
                    Reason::Widget => Some("widget state changed"),
                    Reason::MissingCandidateOwner => Some("removed"),
                    Reason::MissingCurrentOwner => None,
                })
                .collect();
            if reasons.is_empty() {
                return None;
            }
            let owner = match evidence.owner() {
                Owner::Application => "App".to_owned(),
                Owner::Preview(key) => format!("Preview {}", key.variant()),
            };
            Some(format!("{owner}: {}", reasons.join(", ")))
        })
        .collect();
    (!owners.is_empty()).then(|| format!("State reseeded · {}", owners.join(" · ")))
}

fn chrome_button<'a, Message: Clone + 'a>(
    label: &'a str,
) -> iced::widget::Button<'a, Message, iced::widget::Text<'a>> {
    iced::widget::button(iced::widget::text(label).size(12))
        .padding([2, 6])
        .style(crate::preview::chrome::button)
}

#[cfg(test)]
mod tests {
    use rawr::diagnostic::{code, Report};

    use super::{shortcut, Control, Phase, State, DIAGNOSTIC_LIMIT};

    #[test]
    fn refresh_shortcut_requires_one_native_command_r_press() {
        use iced::keyboard::{key, Event, Key, Location, Modifiers};

        let press = |text: &str, modifiers, repeat| {
            iced::Event::Keyboard(Event::KeyPressed {
                key: Key::Character(text.into()),
                modified_key: Key::Character(text.into()),
                physical_key: key::Physical::Code(key::Code::KeyR),
                location: Location::Standard,
                modifiers,
                text: None,
                repeat,
            })
        };
        assert_eq!(
            shortcut(press("r", Modifiers::COMMAND, false)),
            Some(Control::Refresh)
        );
        assert_eq!(
            shortcut(press("R", Modifiers::COMMAND, false)),
            Some(Control::Refresh)
        );
        assert_eq!(
            shortcut(press("s", Modifiers::COMMAND | Modifiers::SHIFT, false)),
            Some(Control::Screenshot)
        );
        assert_eq!(
            shortcut(press("S", Modifiers::COMMAND | Modifiers::SHIFT, false)),
            Some(Control::Screenshot)
        );
        for event in [
            press("s", Modifiers::COMMAND, false),
            press("s", Modifiers::SHIFT, false),
            press("r", Modifiers::NONE, false),
            press("r", Modifiers::COMMAND | Modifiers::ALT, false),
            press("r", Modifiers::COMMAND, true),
            press("x", Modifiers::COMMAND, false),
            iced::Event::Keyboard(Event::ModifiersChanged(Modifiers::COMMAND)),
        ] {
            assert_eq!(shortcut(event), None);
        }
        let mut state = State::new();
        assert!(state.can_refresh());
        assert!(!state.can_retry());
        state.progress(Phase::Reloading);
        assert!(state.can_refresh());
        state.terminal(report("watcher stopped"));
        assert!(!state.can_refresh());
    }

    #[test]
    fn repeated_failures_deduplicate_and_valid_recovery_clears_the_layer() {
        let mut state = State::new();
        let repeated = report("same");
        state.failed(repeated.clone());
        state.failed(repeated);
        assert_eq!(state.reports().len(), 1);
        assert_eq!(state.phase(), Phase::Error);

        for index in 0..(DIAGNOSTIC_LIMIT + 2) {
            state.failed(report(&format!("failure {index}")));
        }
        assert_eq!(state.reports().len(), DIAGNOSTIC_LIMIT);

        state.ready();
        assert_eq!(state.phase(), Phase::Ready);
        assert_eq!(state.reports().len(), 0);
    }

    #[test]
    fn copy_uses_the_exact_core_report_rendering() {
        let mut state = State::new();
        let report = report("copy exactly");
        state.failed(report.clone());
        assert_eq!(state.copy(), Some(rawr::diagnostic::render(&report)));
    }

    #[test]
    fn terminal_failure_keeps_recovery_controls_but_disables_retry() {
        let mut state = State::new();
        let terminal = report("watcher stopped");
        state.terminal(terminal.clone());

        assert_eq!(state.phase(), Phase::Error);
        assert!(!state.can_retry());
        assert_eq!(state.copy(), Some(rawr::diagnostic::render(&terminal)));

        state.progress(Phase::Checking);
        state.failed(report("new session failure"));
        assert!(state.can_retry());
    }

    #[test]
    fn notes_overlay_ready_content_until_dismissed() {
        let mut state = State::new();
        state.saved(Ok("/project/screenshots/app-20261020-120000.png".into()));
        let note = "Screenshot saved · /project/screenshots/app-20261020-120000.png";
        assert_eq!(state.current_note(), Some(note));
        {
            let content = iced::widget::Widget::boxed(iced::widget::text("application content"));
            let view = state.view(content, |_| ());
            let mut simulator = iced_test::simulator(view);
            simulator.find("application content").unwrap();
            simulator.find(note).unwrap();
        }
        state.dismiss();
        assert_eq!(state.current_note(), None);
    }

    #[test]
    fn reload_notes_a_reseed_only_when_the_state_shape_changed() {
        use rawr::reload::compatibility::{Owner, Owners, Plan};

        let compile = |name: &str, state: &str| {
            let root = std::env::temp_dir().join(format!(
                "rawr-reseed-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(root.join("src")).unwrap();
            crate::write_marked(&root, "src/main.rs", format!(
                    r#"
use iced::Element;
use iced::widget::text;

#[derive(Default)] {state}
#[derive(Debug, Clone, Copy)] enum Message {{ Tick }}
impl State {{
    fn update(&mut self, message: Message) {{}}
    fn view(&self) -> Element<Message> {{ text("{name}") }}
}}
pub fn main() -> iced::Result {{ iced::run(State::update, State::view) }}
"#
                ));
            let mut engine = rawr::Engine::new();
            crate::install(&mut engine).unwrap();
            let engine = rawr::Shared::new(engine);
            let target = rawr::project::Target::discover(&root).unwrap();
            let candidate = engine.compile_package_generation(&target).unwrap();
            std::fs::remove_dir_all(&root).unwrap();
            (engine, candidate)
        };
        let (engine, first) = compile("first", "struct State;");
        let mut registry = rawr::host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let current = first.bind(&engine, registry).unwrap().generation().clone();

        let (_, same) = compile("same", "struct State;");
        let kept = Plan::compare(current.clone(), same, Owners::one(Owner::Application));
        let mut state = State::new();
        state.installed(kept.evidence_set());
        assert_eq!(state.current_note(), None);

        let (_, wider) = compile("wider", "struct State { count: i64 }");
        let reseeded = Plan::compare(current, wider, Owners::one(Owner::Application));
        state.installed(reseeded.evidence_set());
        assert_eq!(
            state.current_note(),
            Some("State reseeded · App: state shape changed")
        );
        assert_eq!(state.phase(), Phase::Ready);
    }

    #[test]
    fn cargo_errors_show_while_ready_and_clear_when_a_check_passes() {
        let mut state = State::new();
        state.cargo(crate::reload::cargo::Checked(Some(crate::reload::cargo::Report {
            heading: "cargo check found 1 error".into(),
            details: "error[E0308]: mismatched types".into(),
        })));
        {
            let content = iced::widget::Widget::boxed(iced::widget::text("application content"));
            let mut simulator = iced_test::simulator(state.view(content, |_| ()));
            simulator.find("application content").unwrap();
            simulator.find("cargo check found 1 error").unwrap();
            assert!(simulator.find("Rawr · Ready").is_err());
        }
        state.cargo(crate::reload::cargo::Checked(None));
        let content = iced::widget::Widget::boxed(iced::widget::text("application content"));
        let mut simulator = iced_test::simulator(state.view(content, |_| ()));
        assert!(simulator.find("cargo check found 1 error").is_err());
    }

    #[test]
    fn ready_is_not_rendered_as_an_application_overlay() {
        let state = State::new();
        let content = iced::widget::Widget::boxed(iced::widget::text("application content"));
        let view = state.view(content, |_| ());
        let mut simulator = iced_test::simulator(view);

        simulator.find("application content").unwrap();
        assert!(simulator.find("Rawr · Ready").is_err());
    }

    fn report(message: &str) -> Report {
        Report::error(code::PARSE_SYNTAX, message.to_owned()).finish()
    }
}
