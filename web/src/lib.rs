//! Rawr's iced snippets in a browser: the interpreter and rawr-iced on wasm,
//! one iced application on the page's `#iced` canvas, and [`run`], which the
//! page calls with a snippet's source. A later call swaps in the edited
//! program, and its State carries over when its script types are unchanged.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;

use iced::futures::channel::mpsc;
use iced::widget::{column, container, text, Widget as _};
use iced::{Element, Length, Subscription, Task};
use rawr_iced::program::{Adapter, Application};
use rawr_iced::runtime;
use wasm_bindgen::prelude::*;

/// The Counter, for a page that has no snippet of its own.
const SAMPLE: &str = include_str!("../../play/tests/fixtures/counter.rs");

thread_local! {
    /// Edits for the running app; `None` until the first [`run`].
    static EDITS: RefCell<Option<mpsc::UnboundedSender<String>>> = const { RefCell::new(None) };
    /// The receiving end, which the app's subscription takes once.
    static INBOX: RefCell<Option<mpsc::UnboundedReceiver<String>>> = const { RefCell::new(None) };
}

#[wasm_bindgen(start)]
fn main() {
    console_error_panic_hook::set_once();
}

/// The built-in sample snippet.
#[wasm_bindgen]
pub fn sample() -> String {
    SAMPLE.to_owned()
}

/// Run `code`. The first call opens the app; a later one swaps in the
/// edited program.
#[wasm_bindgen]
pub fn run(code: &str) -> Result<(), JsValue> {
    let code = code.to_owned();
    let running = EDITS.with(|edits| edits.borrow().clone());
    if let Some(sender) = running {
        let _ = sender.unbounded_send(code);
        return Ok(());
    }
    let (sender, receiver) = mpsc::unbounded();
    EDITS.with(|edits| *edits.borrow_mut() = Some(sender));
    INBOX.with(|inbox| *inbox.borrow_mut() = Some(receiver));
    iced::application(move || Web::boot(&code), Web::update, Web::view)
        .theme(Web::theme)
        .subscription(Web::subscription)
        .run()
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

#[derive(Default)]
struct Web {
    adapter: Option<Adapter>,
    /// The last edit's compile error; the app before it keeps running.
    error: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    App(runtime::Message),
    Edit(String),
}

impl Web {
    fn boot(code: &str) -> (Self, Task<Message>) {
        let mut web = Self::default();
        let task = web.load(code);
        (web, task)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::App(message) => match &mut self.adapter {
                Some(adapter) => Application::update_native(adapter, message).map(Message::App),
                None => Task::none(),
            },
            Message::Edit(code) => self.load(&code),
        }
    }

    /// Compile `code` and run it in place of the current program, from its
    /// State when that carries over.
    fn load(&mut self, code: &str) -> Task<Message> {
        let started = now();
        let mut adapter = match compile(code) {
            Ok(adapter) => adapter,
            Err(error) => {
                log(&error);
                self.error = Some(error);
                return Task::none();
            }
        };
        let compiled = now();
        let carried = self
            .adapter
            .as_ref()
            .and_then(Adapter::export_state)
            .is_some_and(|bytes| adapter.seed_state(&bytes));
        let task = Application::start(&mut adapter).map(Message::App);
        self.adapter = Some(adapter);
        self.error = None;
        log(&format!(
            "rawr-web: at {started:.0} ms, compiled in {:.1} ms, swapped in {:.1} ms, state {}",
            compiled - started,
            now() - compiled,
            if carried { "carried" } else { "booted" },
        ));
        task
    }

    fn view(&self) -> Element<'_, Message> {
        let app = match &self.adapter {
            Some(adapter) => adapter.view().map(Message::App).boxed(),
            None => text("").boxed(),
        };
        match &self.error {
            None => app,
            Some(error) => column![
                container(app).height(Length::Fill),
                text(error.as_str()).size(14).style(text::danger),
            ]
            .boxed(),
        }
    }

    fn theme(&self) -> iced::Theme {
        self.adapter
            .as_ref()
            .map_or(iced::Theme::Light, Application::theme_native)
    }

    fn subscription(&self) -> Subscription<Message> {
        let app = self
            .adapter
            .as_ref()
            .map_or_else(Subscription::none, |adapter| {
                Application::subscription_native(adapter).map(Message::App)
            });
        Subscription::batch([app, Subscription::run(edits).map(Message::Edit)])
    }
}

/// The edits [`run`] sends, as a stream for the app's subscription.
fn edits() -> impl iced::futures::Stream<Item = String> {
    use iced::futures::StreamExt;
    INBOX
        .with(|inbox| inbox.borrow_mut().take())
        .map(StreamExt::boxed_local)
        .unwrap_or_else(|| iced::futures::stream::empty().boxed_local())
}

/// A program checked and booted, or its errors.
fn compile(code: &str) -> Result<Adapter, String> {
    // A snippet's own tests are for `cargo test`.
    let code = code.split("\n#[cfg(test)]").next().unwrap_or(code);
    let compiler = rawr_iced::application::Compiler::new().map_err(|error| error.to_string())?;
    compiler
        .check_named_source("play.rs", code)
        .and_then(rawr_iced::application::Checked::prepare)
        .map(rawr_iced::application::Prepared::into_adapter)
        .map_err(|error| error.to_string())
}

fn now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0.0, |performance| performance.now())
}

fn log(message: &str) {
    web_sys::console::log_1(&message.into());
}
