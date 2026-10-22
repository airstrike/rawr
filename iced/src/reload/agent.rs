//! `rawr mcp`'s channel into a live `rawr run`/`rawr preview` window.
//!
//! The window listens on `<project>/target/rawr/agent.sock`. A client writes
//! one request, a line per command, and closes its writing half; the window
//! answers once and closes. Commands:
//!
//! - `click X Y`, `click-text TEXT`, `click-id ID`: press and release the
//!   left button at a logical point, or at the center of the first widget
//!   showing `TEXT` (exact, else any case contained) or with the id `ID`;
//! - `type TEXT`: one key tap per character, each its own step;
//! - `key NAME`: a tap of `Enter`, `Tab`, `Escape`, `Backspace`, `Delete`,
//!   `Space`, the arrows (`Left`, `ArrowLeft`, ...), `Home`, `End`, `PageUp`,
//!   `PageDown` or one character, after `cmd+`/`ctrl+`/`shift+`/`alt+`;
//! - `reset`: boot the shown application or preview afresh;
//! - `wait`: answer when the next reload commits or fails;
//! - `screenshot`: capture the window once the commands have run.
//!
//! Input is iced events sent into the window's own event loop, handled by
//! its live widget tree as a user's would be (focus, an open menu, hover
//! and the messages they publish). A click by text or id goes to the
//! center of the widget's bounds in the window's layout. The window's
//! cursor comes only from the system, so its view sees the agent's pointer
//! until the real mouse moves. The answer's first line is `ok` or `error`; then
//! free text, `screenshot\t<path>` and, for a failed reload,
//! `diagnostic\t<file>\t<line>\t<column>\t<code>\t<message>` lines.

use std::cell::Cell;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

use iced::advanced::widget::{tree, Meta, Tree};
use iced::advanced::{layout, overlay, renderer, Layout, Shell, Widget};
use iced::widget::Widget as _;
use iced::keyboard::{self, key};
use iced::{mouse, Element, Length, Point, Size, Task, Vector};
use iced_runtime::core::widget::{self, Operation};
use iced_runtime::core::{window, Event, Rectangle, SmolStr};

/// The socket a live window listens on, under the project root.
pub fn socket(root: &std::path::Path) -> PathBuf {
    root.join("target").join("rawr").join("agent.sock")
}

/// One input command.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    Click(Target),
    Key(Key),
}

/// Where a click lands.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    Point(Point),
    Text(String),
    Id(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Key {
    key: keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<SmolStr>,
}

/// One parsed request.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Request {
    pub(crate) steps: Vec<Step>,
    pub(crate) reset: bool,
    pub(crate) wait: bool,
    pub(crate) screenshot: bool,
}

/// A request and the connection waiting for its answer.
#[derive(Clone)]
pub(crate) struct Call {
    pub(crate) request: Arc<Request>,
    reply: Arc<Mutex<Option<mpsc::Sender<String>>>>,
    /// Actions done so far: the reset, if asked, then each step.
    done: usize,
    /// Where the window shows the next click's target, once looked up.
    at: Option<Option<Point>>,
    /// The capture's frame has been drawn.
    drawn: bool,
}

impl Call {
    /// Answer once; later answers and a closed connection are ignored.
    pub(crate) fn answer(&self, answer: String) {
        let reply = match self.reply.lock() {
            Ok(mut reply) => reply.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(reply) = reply {
            let _ = reply.send(answer);
        }
    }

    /// Answer a reload's failure with its rendered report and a line per
    /// labelled position.
    pub(crate) fn failed(&self, report: &rawr::diagnostic::Report) {
        let view = report.view();
        let mut answer = String::from("error\n");
        for label in view.labels() {
            let (line, column) = (label.span().start().line(), label.span().start().column());
            answer.push_str(&format!(
                "diagnostic\t{}\t{}\t{}\t{}\t{}\n",
                label.source().identity(),
                line.unwrap_or(0),
                column.unwrap_or(0),
                view.code(),
                label
                    .message()
                    .unwrap_or(view.message())
                    .replace(['\t', '\n'], " "),
            ));
        }
        for label in view.external_labels() {
            let ((line, column), _) = label.positions();
            answer.push_str(&format!(
                "diagnostic\t{}\t{line}\t{column}\t{}\t{}\n",
                label.file(),
                view.code(),
                label
                    .message()
                    .unwrap_or(view.message())
                    .replace(['\t', '\n'], " "),
            ));
        }
        // A report carried across the runner protocol keeps its position
        // only in its message: `... (line 215, position 27)`.
        if view.labels().is_empty() && view.external_labels().is_empty() {
            let position = view.message().rsplit_once("(line ").and_then(|(_, rest)| {
                let (line, rest) = rest.split_once(", position ")?;
                let column = rest.split_once(')')?.0;
                Some((line.parse::<usize>().ok()?, column.parse::<usize>().ok()?))
            });
            // The file is the report's source, else its rendered `--> file`.
            let rendered = rawr::diagnostic::render(report);
            let file = view
                .sources()
                .first()
                .map(|source| source.identity().to_owned())
                .or_else(|| {
                    rendered
                        .lines()
                        .find_map(|line| line.trim_start().strip_prefix("--> "))
                        .map(|file| file.trim().to_owned())
                });
            if let (Some((line, column)), Some(file)) = (position, file) {
                answer.push_str(&format!(
                    "diagnostic\t{file}\t{line}\t{column}\t{}\t{}\n",
                    view.code(),
                    view.message().replace(['\t', '\n'], " "),
                ));
            }
        }
        answer.push_str(&rawr::diagnostic::render(report));
        self.answer(answer);
    }
}

pub(crate) fn parse(text: &str) -> Result<Request, String> {
    let mut request = Request::default();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
        match command {
            "click" => {
                let mut numbers = rest.split_whitespace().map(str::parse::<f32>);
                let (Some(Ok(x)), Some(Ok(y)), None) =
                    (numbers.next(), numbers.next(), numbers.next())
                else {
                    return Err(format!("`click` takes two numbers: `{line}`"));
                };
                request
                    .steps
                    .push(Step::Click(Target::Point(Point::new(x, y))));
            }
            "click-text" => request
                .steps
                .push(Step::Click(Target::Text(rest.to_owned()))),
            "click-id" => request
                .steps
                .push(Step::Click(Target::Id(rest.trim().to_owned()))),
            // A step per character, so each sees the state the last one left.
            "type" => request.steps.extend(rest.chars().map(|character| {
                let text = SmolStr::new(character.to_string());
                Step::Key(Key {
                    key: match character {
                        ' ' => keyboard::Key::Named(key::Named::Space),
                        _ => keyboard::Key::Character(text.clone()),
                    },
                    modifiers: keyboard::Modifiers::empty(),
                    text: Some(text),
                })
            })),
            "key" => request.steps.push(Step::Key(named(rest.trim())?)),
            "reset" => request.reset = true,
            "wait" => request.wait = true,
            "screenshot" => request.screenshot = true,
            _ => return Err(format!("unknown agent command `{command}`")),
        }
    }
    Ok(request)
}

fn named(name: &str) -> Result<Key, String> {
    let mut modifiers = keyboard::Modifiers::empty();
    let mut rest = name;
    while let Some((modifier, after)) = rest.split_once('+').filter(|(_, after)| !after.is_empty())
    {
        modifiers |= match modifier.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "super" | "logo" => keyboard::Modifiers::LOGO,
            "ctrl" | "control" => keyboard::Modifiers::CTRL,
            "shift" => keyboard::Modifiers::SHIFT,
            "alt" | "option" => keyboard::Modifiers::ALT,
            _ => return Err(format!("unknown modifier `{modifier}` in `{name}`")),
        };
        rest = after;
    }
    let named = match rest.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(key::Named::Enter),
        "tab" => Some(key::Named::Tab),
        "escape" | "esc" => Some(key::Named::Escape),
        "backspace" => Some(key::Named::Backspace),
        "delete" => Some(key::Named::Delete),
        "space" => Some(key::Named::Space),
        "left" | "arrowleft" => Some(key::Named::ArrowLeft),
        "right" | "arrowright" => Some(key::Named::ArrowRight),
        "up" | "arrowup" => Some(key::Named::ArrowUp),
        "down" | "arrowdown" => Some(key::Named::ArrowDown),
        "home" => Some(key::Named::Home),
        "end" => Some(key::Named::End),
        "pageup" => Some(key::Named::PageUp),
        "pagedown" => Some(key::Named::PageDown),
        _ => None,
    };
    match (named, rest.chars().count()) {
        (Some(key::Named::Space), _) => Ok(Key {
            key: keyboard::Key::Named(key::Named::Space),
            modifiers,
            text: modifiers.is_empty().then(|| SmolStr::new(" ")),
        }),
        (Some(named), _) => Ok(Key {
            key: keyboard::Key::Named(named),
            modifiers,
            text: None,
        }),
        (None, 1) => Ok(Key {
            key: keyboard::Key::Character(SmolStr::new(rest)),
            modifiers,
            text: (modifiers.is_empty() || modifiers == keyboard::Modifiers::SHIFT)
                .then(|| SmolStr::new(rest)),
        }),
        (None, _) => Err(format!("unknown key `{name}`")),
    }
}

/// Listen on the project's agent socket for the window's lifetime.
pub(crate) fn subscription() -> iced::Subscription<Call> {
    iced::Subscription::run(listen)
}

fn listen() -> impl iced::futures::Stream<Item = Call> {
    iced::stream::channel(
        8,
        |output: iced::futures::channel::mpsc::Sender<Call>| async move {
            let path = socket(&crate::screenshot::root());
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
            let _ = std::fs::remove_file(&path);
            let Ok(listener) = std::os::unix::net::UnixListener::bind(&path) else {
                return;
            };
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let output = output.clone();
                    std::thread::spawn(move || serve(stream, output));
                }
            });
        },
    )
}

fn serve(
    stream: std::os::unix::net::UnixStream,
    mut output: iced::futures::channel::mpsc::Sender<Call>,
) {
    let mut text = String::new();
    let mut reader = std::io::BufReader::new(&stream);
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|read| read > 0) {
        text.push_str(&line);
        line.clear();
    }
    let answer = match parse(&text) {
        Ok(request) => {
            let (sender, receiver) = mpsc::channel();
            let call = Call {
                request: Arc::new(request),
                reply: Arc::new(Mutex::new(Some(sender))),
                done: 0,
                at: None,
                drawn: false,
            };
            if iced::futures::executor::block_on(iced::futures::SinkExt::send(&mut output, call))
                .is_err()
            {
                return;
            }
            match receiver.recv() {
                Ok(answer) => answer,
                Err(_) => return,
            }
        }
        Err(error) => format!("error\n{error}"),
    };
    let mut stream = &stream;
    let _ = stream.write_all(answer.as_bytes());
    let _ = stream.write_all(b"\n");
}

/// Send `events` to the newest window, as its own input, then `then`.
fn inject<Message: Clone + Send + 'static>(events: Vec<Event>, then: Message) -> Task<Message> {
    iced::window::latest().then(move |window| match window {
        Some(window) => events
            .iter()
            .fold(Task::none(), |task, event| {
                task.chain(iced_runtime::task::effect(iced_runtime::Action::Event {
                    window,
                    event: event.clone(),
                }))
            })
            .chain(Task::done(then.clone())),
        None => Task::none(),
    })
}

/// One live host as the agent drives it.
pub(crate) struct Live<S, Message> {
    pub(crate) session: fn(&mut S) -> &mut Session,
    pub(crate) reset: fn(&mut S) -> Task<Message>,
    pub(crate) wait: fn(&mut S, Call),
    pub(crate) next: fn(Call) -> Message,
    pub(crate) shot: fn(Call, crate::screenshot::Saved) -> Message,
}

impl<S, Message: Clone + Send + 'static> Live<S, Message> {
    /// Take `call`'s next action: the reset, then each step as events the
    /// window handles as its own input; once all are done, wait, capture
    /// after the next redraw or answer. Each action is its own turn of the
    /// window's loop, so the next one sees the view the last one left.
    pub(crate) fn run(&self, state: &mut S, call: Call) -> Task<Message> {
        let reset = usize::from(call.request.reset);
        let next = (self.next)(Call {
            done: call.done + 1,
            at: None,
            ..call.clone()
        });
        if call.done < reset {
            (self.session)(state).pointer = None;
            return Task::batch([(self.reset)(state), Task::done(next)]);
        }
        match call.request.steps.get(call.done - reset) {
            Some(Step::Key(key)) => return inject(tap(key.clone()).collect(), next),
            Some(Step::Click(target)) => {
                let point = match (target, &call.at) {
                    (Target::Point(point), _) | (_, Some(Some(point))) => *point,
                    (Target::Text(text), Some(None)) => {
                        call.answer(format!("error\nno widget shows `{text}`"));
                        return Task::none();
                    }
                    (Target::Id(id), Some(None)) => {
                        call.answer(format!("error\nno widget has the id `{id}`"));
                        return Task::none();
                    }
                    (_, None) => {
                        let (find, map) = (Find::new(target.clone()), self.next);
                        return iced_runtime::task::widget(find).map(move |at| {
                            map(Call {
                                at: Some(at),
                                ..call.clone()
                            })
                        });
                    }
                };
                let session = (self.session)(state);
                session.pointer = Some(point);
                session.released.set(false);
                return inject(
                    vec![
                        Event::Mouse(mouse::Event::CursorMoved { position: point }),
                        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                    ],
                    next,
                );
            }
            None => {}
        }
        if call.request.wait {
            (self.wait)(state, call);
            Task::none()
        } else if call.request.screenshot && !call.drawn {
            // This update redraws the window; the capture follows that frame.
            (self.session)(state).drawn = Some(Call {
                drawn: true,
                ..call
            });
            Task::none()
        } else if call.request.screenshot {
            (self.session)(state).drawn = None;
            let path = crate::screenshot::root()
                .join("target")
                .join("rawr")
                .join("mcp")
                .join("window.png");
            let shot = self.shot;
            crate::screenshot::capture_into(path, move |saved| shot(call.clone(), saved))
        } else {
            call.answer(format!(
                "ok\n{} input step(s) delivered",
                call.request.steps.len()
            ));
            Task::none()
        }
    }
}

/// Answer a capture: its path, or why it failed.
pub(crate) fn shot(call: &Call, saved: crate::screenshot::Saved) {
    call.answer(match saved {
        Ok(path) => format!("ok\nscreenshot\t{}", path.display()),
        Err(error) => format!("error\n{error}"),
    });
}

/// The window's agent state: the pointer its last click left, until the
/// real mouse moves, and a capture waiting for the next frame.
#[derive(Default)]
pub(crate) struct Session {
    pointer: Option<Point>,
    released: Cell<bool>,
    drawn: Option<Call>,
}

impl Session {
    fn cursor(&self, cursor: mouse::Cursor) -> mouse::Cursor {
        match self.pointer {
            Some(point) if !self.released.get() => mouse::Cursor::Available(point),
            _ => cursor,
        }
    }
}

/// The window's whole view, seeing the agent's pointer where the real
/// cursor would be (the window's own cursor comes only from the system).
pub(crate) fn view<'a, Message: Clone + 'a>(
    session: &'a Session,
    content: Element<'a, Message>,
    map: fn(Call) -> Message,
) -> Element<'a, Message> {
    Pointed {
        session,
        content,
        map,
    }
    .boxed()
}

struct Pointed<'a, Message> {
    session: &'a Session,
    content: Element<'a, Message>,
    map: fn(Call) -> Message,
}

impl<Message> Meta for Pointed<'_, Message> {}

impl<Message: Clone> Widget<Message, iced::Theme, iced::Renderer> for Pointed<'_, Message> {
    fn tag(&self) -> tree::Tag {
        self.content.tag()
    }

    fn state(&self) -> tree::State {
        self.content.state()
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.content.diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) {
        self.content.layout(tree, renderer, limits);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .operate(tree, layout, viewport, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position })
                if self.session.pointer != Some(*position) =>
            {
                self.session.released.set(true);
            }
            Event::Window(window::Event::RedrawRequested(_)) => {
                if let Some(call) = &self.session.drawn {
                    shell.publish((self.map)(call.clone()));
                }
            }
            _ => {}
        }
        let cursor = self.session.cursor(cursor);
        self.content
            .update(tree, event, layout, cursor, renderer, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.mouse_interaction(
            tree,
            layout,
            self.session.cursor(cursor),
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.draw(
            tree,
            renderer,
            theme,
            style,
            layout,
            self.session.cursor(cursor),
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let session = self.session;
        self.content
            .overlay(tree, layout, renderer, viewport, translation, window)
            .into_iter()
            .map(|content| overlay::Element::new(Box::new(Over { session, content })))
            .collect()
    }
}

/// An overlay (a `pick_list` menu, a tooltip) seeing the agent's pointer.
struct Over<'a, Message> {
    session: &'a Session,
    content: overlay::Element<'a, Message, iced::Theme, iced::Renderer>,
}

impl<Message> overlay::Overlay<Message, iced::Theme, iced::Renderer> for Over<'_, Message> {
    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        cursor: mouse::Cursor,
    ) {
        self.content
            .as_overlay()
            .draw(renderer, theme, style, self.session.cursor(cursor));
    }

    fn operate(&mut self, renderer: &iced::Renderer, operation: &mut dyn Operation) {
        self.content.as_overlay_mut().operate(renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        let cursor = self.session.cursor(cursor);
        self.content
            .as_overlay_mut()
            .update(event, cursor, renderer, shell);
    }

    fn mouse_interaction(
        &self,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_overlay()
            .mouse_interaction(self.session.cursor(cursor), renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        renderer: &iced::Renderer,
    ) -> Vec<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let session = self.session;
        self.content
            .as_overlay_mut()
            .overlay(renderer)
            .into_iter()
            .map(|content| overlay::Element::new(Box::new(Over { session, content })))
            .collect()
    }

    fn index(&self) -> f32 {
        self.content.as_overlay().index()
    }
}

/// A key's press and release; a modified one between the modifiers going
/// down and up, as widgets track them (`cmd+a` selects all).
fn tap(key: Key) -> impl Iterator<Item = Event> {
    let physical = key::Physical::Unidentified(key::NativeCode::Unidentified);
    let modifiers = (!key.modifiers.is_empty()).then_some(key.modifiers);
    let changed = |modifiers| Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers));
    let (down, up) = (
        modifiers.map(changed),
        modifiers.map(|_| changed(keyboard::Modifiers::empty())),
    );
    down.into_iter().chain([
        Event::Keyboard(keyboard::Event::KeyPressed {
            key: key.key.clone(),
            modified_key: key.key.clone(),
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers: key.modifiers,
            text: key.text,
            repeat: false,
        }),
        Event::Keyboard(keyboard::Event::KeyReleased {
            key: key.key.clone(),
            modified_key: key.key,
            physical_key: physical,
            location: keyboard::Location::Standard,
            modifiers: key.modifiers,
        }),
    ])
    .chain(up)
}

/// The center of the first widget showing a text (a text input's value or
/// placeholder included), or with an id, where the window shows it: inside
/// a scrollable, shifted by its offset.
struct Find {
    target: Target,
    exact: Option<Rectangle>,
    loose: Option<Rectangle>,
    /// The scroll offset of the widgets being visited, and of the children
    /// of the last container visited.
    offset: Vector,
    inner: Vector,
}

impl Find {
    fn new(target: Target) -> Self {
        Self {
            target,
            exact: None,
            loose: None,
            offset: Vector::ZERO,
            inner: Vector::ZERO,
        }
    }

    fn id(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
        if let (Target::Id(wanted), Some(id), None) = (&self.target, id, self.exact) {
            if *id == widget::Id::from(wanted.clone()) {
                self.exact = Some(bounds - self.offset);
            }
        }
    }

    fn text(&mut self, bounds: Rectangle, text: &str) {
        if let Target::Text(wanted) = &self.target {
            if self.exact.is_none() && text.trim() == wanted.trim() {
                self.exact = Some(bounds - self.offset);
            } else if self.loose.is_none()
                && text.to_lowercase().contains(&wanted.trim().to_lowercase())
            {
                self.loose = Some(bounds - self.offset);
            }
        }
    }
}

impl Operation<Option<Point>> for Find {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<Point>>)) {
        let outer = self.offset;
        self.offset = self.inner;
        operate(self);
        self.offset = outer;
        self.inner = outer;
    }

    fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle, _viewport: &Rectangle) {
        self.inner = self.offset;
        self.id(id, bounds);
    }

    fn scrollable(
        &mut self,
        id: Option<&widget::Id>,
        bounds: Rectangle,
        _content: Size,
        translation: Vector,
        _state: &mut dyn widget::operation::Scrollable,
    ) {
        self.inner = self.offset + translation;
        self.id(id, bounds);
    }

    fn focusable(
        &mut self,
        id: Option<&widget::Id>,
        bounds: Rectangle,
        _state: &mut dyn widget::operation::Focusable,
    ) {
        self.id(id, bounds);
    }

    fn text_input(
        &mut self,
        id: Option<&widget::Id>,
        bounds: Rectangle,
        state: &mut dyn widget::operation::TextInput,
    ) {
        self.id(id, bounds);
        self.text(bounds, &state.text());
    }

    fn text(
        &mut self,
        id: Option<&widget::Id>,
        bounds: Rectangle,
        text: &mut dyn widget::operation::Text,
    ) {
        self.id(id, bounds);
        Find::text(self, bounds, &text.text());
    }

    fn finish(&self) -> widget::operation::Outcome<Option<Point>> {
        widget::operation::Outcome::Some(self.exact.or(self.loose).map(|bounds| bounds.center()))
    }
}

#[cfg(test)]
mod tests {
    use iced::widget::{button, column, pick_list, text, text_input, Widget as _};
    use iced::{mouse, Event, Point, Size};
    use iced_runtime::core::widget::operation::{self, Operation, Outcome};
    use iced_runtime::core::{shell, window};
    use iced_runtime::user_interface::{self, UserInterface};

    use super::{Call, Find, Session, Step, Target};

    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Pressed(&'static str),
        Picked(&'static str),
        Edited(String),
        Submitted,
        Agent,
    }

    fn view(value: &str) -> iced::Element<'_, Message> {
        column![
            pick_list(None::<&str>, ["Alpha", "Beta"], |option| option.to_string())
                .on_select(Message::Picked)
                .placeholder("Pick one"),
            button(text("First")).on_press(Message::Pressed("first")),
            text_input("Your name", value)
                .on_input(Message::Edited)
                .on_submit(Message::Submitted),
        ]
        .boxed()
    }

    /// The window as the agent drives it: its view behind the agent's
    /// pointer, with the real cursor outside the window.
    struct Window {
        session: Session,
        cache: Option<user_interface::Cache>,
        renderer: iced::Renderer,
    }

    impl Window {
        fn new() -> Self {
            let renderer = iced::futures::executor::block_on(
                <iced::Renderer as iced::advanced::renderer::Headless>::new(
                    iced::advanced::renderer::Settings {
                        font: iced::Font::DEFAULT,
                        text_size: iced::Pixels(16.0),
                        ..iced::advanced::renderer::Settings::default()
                    },
                    Some("tiny-skia"),
                ),
            )
            .unwrap();
            Self {
                session: Session::default(),
                cache: None,
                renderer,
            }
        }

        fn find(&mut self, value: &str, target: &str) -> Option<Point> {
            let Window {
                session,
                cache,
                renderer,
            } = self;
            let content = super::view(session, view(value), |_| Message::Agent);
            let mut interface = UserInterface::build(
                content,
                Size::new(400.0, 300.0),
                cache.take().unwrap_or_default(),
                renderer,
            );
            let (sender, receiver) = std::sync::mpsc::channel();
            let find = Find::new(Target::Text(target.to_owned()));
            let mut find = operation::map(Box::new(find), move |at| sender.send(at).unwrap());
            interface.operate(renderer, &mut find);
            *cache = Some(interface.into_cache());
            let Outcome::Some(()) = find.finish() else {
                panic!("a search always answers");
            };
            receiver.recv().unwrap()
        }

        /// The window's own handling of `events`, as `inject` delivers them.
        fn send(&mut self, value: &str, events: &[Event]) -> Vec<Message> {
            let Window {
                session,
                cache,
                renderer,
            } = self;
            let content = super::view(session, view(value), |_| Message::Agent);
            let mut interface = UserInterface::build(
                content,
                Size::new(400.0, 300.0),
                cache.take().unwrap_or_default(),
                renderer,
            );
            let mut messages = shell::Bus::new();
            let _ = interface.update(
                &window::Headless,
                &shell::Waker::noop(),
                events,
                mouse::Cursor::Unavailable,
                renderer,
                &mut messages,
            );
            *cache = Some(interface.into_cache());
            messages.into_iter().collect()
        }

        fn click(&mut self, value: &str, point: Point) -> Vec<Message> {
            self.session.pointer = Some(point);
            self.session.released.set(false);
            self.send(
                value,
                &[
                    Event::Mouse(mouse::Event::CursorMoved { position: point }),
                    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                ],
            )
        }
    }

    #[test]
    fn a_failed_reload_answers_with_its_position() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let call = Call {
            request: std::sync::Arc::new(super::Request::default()),
            reply: std::sync::Arc::new(std::sync::Mutex::new(Some(sender))),
            done: 0,
            at: None,
            drawn: false,
        };
        let source = rawr::diagnostic::Source::new("src/main.rawr", "fn main() {}\n").unwrap();
        let report = rawr::diagnostic::Report::error(
            rawr::diagnostic::code::FRAMEWORK_NATIVE,
            "Type mismatch: expected 'i64', found 'String' (line 215, position 27)",
        )
        .source(source)
        .finish();
        call.failed(&report);
        let answer = receiver.recv().unwrap();
        assert!(answer.starts_with("error\n"), "{answer}");
        assert!(
            answer.contains("diagnostic\tsrc/main.rawr\t215\t27\t"),
            "{answer}"
        );
    }

    #[test]
    fn requests_parse() {
        let request = super::parse(
            "click 10 20\nclick-text Second\nclick-id name\ntype hi there\nkey cmd+a\nkey Enter\nreset\nwait\nscreenshot\n",
        )
        .unwrap();
        assert_eq!(request.steps.len(), 13);
        assert!(request.reset && request.wait && request.screenshot);
        assert_eq!(
            request.steps[0],
            Step::Click(Target::Point(Point::new(10.0, 20.0)))
        );
        assert_eq!(request.steps, super::parse("click 10 20\nclick-text Second\nclick-id name\ntype h\ntype i\nkey space\ntype there\nkey cmd+a\nkey enter").unwrap().steps);
        assert!(super::parse("click 1").is_err());
        assert!(super::parse("key hyper+a").is_err());
        assert!(super::parse("fly").is_err());
    }

    #[test]
    fn window_input_opens_a_pick_list_menu_and_types_into_a_field_found_by_placeholder() {
        let mut window = Window::new();
        assert_eq!(window.find("", "Third"), None);
        let first = window.find("", "first").unwrap();
        assert_eq!(window.click("", first), [Message::Pressed("first")]);

        // A click at the list (top left) opens the window's own menu; its
        // first option sits just below.
        assert!(window.click("", Point::new(10.0, 10.0)).is_empty());
        assert_eq!(
            window.click("", Point::new(10.0, 40.0)),
            [Message::Picked("Alpha")]
        );

        // The field is found by its placeholder; the click focuses it and the
        // keys edit it.
        let field = window.find("", "your name").unwrap();
        assert!(window.click("", field).is_empty());
        let mut value = String::new();
        for step in super::parse("type ab").unwrap().steps {
            let Step::Key(key) = step else {
                panic!("typing is keys");
            };
            let events: Vec<_> = super::tap(key).collect();
            for message in window.send(&value.clone(), &events) {
                let Message::Edited(edited) = message else {
                    panic!("typing edits: {message:?}");
                };
                value = edited;
            }
        }
        assert_eq!(value, "ab");
        let enter = super::parse("key Enter").unwrap().steps.remove(0);
        let Step::Key(enter) = enter else {
            panic!("a key");
        };
        assert_eq!(
            window.send("ab", &super::tap(enter).collect::<Vec<_>>()),
            [Message::Submitted]
        );
        // `cmd+a` selects all, so the next key replaces it.
        let mut keys = Vec::new();
        for step in super::parse("key cmd+a\ntype z").unwrap().steps {
            let Step::Key(key) = step else {
                panic!("a key");
            };
            keys.extend(super::tap(key));
        }
        assert_eq!(window.send("ab", &keys), [Message::Edited("z".to_owned())]);

        // The real mouse moving takes the pointer back.
        assert!(window
            .send(
                "ab",
                &[Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(390.0, 290.0)
                })]
            )
            .is_empty());
        assert!(window.session.released.get());
    }
}
