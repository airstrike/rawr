//! Rawr's snippets in a browser: the interpreter and rawr-iced on wasm.
//!
//! [`kind`] says how a snippet runs, as the `?play` server decides it from
//! rawr-iced's claims. [`run_text`] runs a plain program and returns its
//! output as the server's text result. [`run_async`] runs a program whose
//! `main` is `async` on the page's event loop and real clock, and streams
//! its output as it is written. [`run`] runs an iced snippet as one
//! iced application on the page's `#iced` element; a later call swaps in the
//! edited program, and its State carries over when its script types are
//! unchanged. Both wrap a snippet with `rawr-snippet`, as the server does.
#![cfg(target_arch = "wasm32")]

mod fonts;
mod tasks;
mod text;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use iced::futures::channel::mpsc;
use iced::widget::{text as label, Widget as _};
use iced::{Element, Subscription, Task};
use rawr_iced::program::{Adapter, Application};
use rawr_iced::runtime;
use wasm_bindgen::prelude::*;

/// The Counter, for a page that has no snippet of its own.
const SAMPLE: &str = include_str!("../../play/tests/fixtures/counter.rs");

/// A compiled program waiting for the app to swap it in.
struct Generation {
    adapter: Adapter,
    /// The page's theme, unless the app sets its own.
    theme: Option<iced::Theme>,
    /// How many messages to send a freshly booted State.
    messages: usize,
    /// The fonts its source needs.
    fonts: Vec<usize>,
}

thread_local! {
    /// Wakes the running app; `None` until the first [`run`].
    static WAKE: RefCell<Option<mpsc::UnboundedSender<()>>> = const { RefCell::new(None) };
    /// The receiving end, which the app's subscription takes once.
    static INBOX: RefCell<Option<mpsc::UnboundedReceiver<()>>> = const { RefCell::new(None) };
    /// The latest program [`run`] compiled.
    static NEXT: RefCell<Option<Generation>> = const { RefCell::new(None) };
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

/// How `code` runs: `"iced"` for [`run`], `"async"` for [`run_async`] (an
/// `async fn main`), or `"text"` for [`run_text`].
#[wasm_bindgen]
pub fn kind(code: &str) -> String {
    let program = rawr_snippet::plain::program(code);
    let iced = rawr::Engine::new()
        .parse_unit(&program)
        .is_ok_and(|unit| claims().claim_snippet(&rawr::layer::Facts::of_unit(&unit)));
    if iced {
        "iced"
    } else if rawr_snippet::plain::async_main(&rawr_snippet::without_tests(code)).is_some() {
        "async"
    } else {
        "text"
    }
    .to_owned()
}

/// Run a plain program; its output as the server's JSON text result, or
/// `{"kind":"error","text":..}` when it does not compile.
#[wasm_bindgen]
pub fn run_text(code: &str) -> String {
    text::run(code)
}

/// Run a program whose `main` is `async`: its tasks run on the page's event
/// loop, and tokio's timers on the page's clock. `output` is called with
/// `(stream, text)` as the program writes. Resolves to the server's JSON
/// text result, or `{"kind":"error","text":..}` when it does not compile.
/// A later call stops a program still running.
#[wasm_bindgen]
pub async fn run_async(code: String, output: js_sys::Function) -> String {
    tasks::run(code, output).await
}

/// Run an iced snippet: the first call opens the app, and a later one swaps
/// in the edited program. `theme` (`"dark"` or `"light"`) is used unless
/// the app sets its own, and `messages` go to a full app's fresh State. A
/// program that does not compile is an error with its report, and the
/// running app is kept.
#[wasm_bindgen]
pub fn run(
    code: &str,
    theme: Option<String>,
    messages: Option<Vec<String>>,
) -> Result<(), JsValue> {
    let messages = messages.unwrap_or_default();
    let prelude = claims().prelude_imports();
    let options = rawr_snippet::Options {
        size: (0, 0),
        theme: "",
        messages: &messages,
        prelude: &prelude,
    };
    let program = rawr_snippet::live(code, &options).map_err(|error| JsValue::from_str(&error))?;
    let started = now();
    let adapter = compile(&program, code).map_err(|error| JsValue::from_str(&error))?;
    log(&format!("rawr-web: compiled in {:.1} ms", now() - started));
    let theme = rawr_snippet::app_theme(code).is_none().then(|| {
        if theme.as_deref() == Some("light") {
            iced::Theme::Light
        } else {
            iced::Theme::Dark
        }
    });
    let generation = Generation {
        adapter,
        theme,
        messages: messages.len(),
        fonts: fonts::needed(code).collect(),
    };
    NEXT.with(|next| *next.borrow_mut() = Some(generation));
    if let Some(wake) = WAKE.with(|wake| wake.borrow().clone()) {
        let _ = wake.unbounded_send(());
        return Ok(());
    }
    let (sender, receiver) = mpsc::unbounded();
    WAKE.with(|wake| *wake.borrow_mut() = Some(sender));
    INBOX.with(|inbox| *inbox.borrow_mut() = Some(receiver));
    iced::application(Web::boot, Web::update, Web::view)
        .theme(Web::theme)
        .subscription(Web::subscription)
        .run()
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// rawr-iced's claims, read from its manifest as `?play` reads them.
fn claims() -> &'static rawr::layer::Claims {
    static CLAIMS: OnceLock<rawr::layer::Claims> = OnceLock::new();
    CLAIMS.get_or_init(|| {
        rawr_cli::layer::Manifest::parse(rawr_iced::MANIFEST)
            .expect("rawr-iced's manifest is one rawr reads")
            .claims
    })
}

#[derive(Default)]
struct Web {
    adapter: Option<Adapter>,
    theme: Option<iced::Theme>,
    /// Fonts fetched or on their way.
    fonts: BTreeSet<usize>,
}

#[derive(Debug, Clone)]
enum Message {
    App(runtime::Message),
    Swap,
    /// A font came in: the view is laid out again, so its text uses it.
    Font,
}

impl Web {
    fn boot() -> (Self, Task<Message>) {
        let mut web = Self::default();
        let task = web.swap();
        (web, task)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::App(message) => match &mut self.adapter {
                Some(adapter) => Application::update_native(adapter, message).map(Message::App),
                None => Task::none(),
            },
            Message::Swap => self.swap(),
            Message::Font => Task::none(),
        }
    }

    /// Run the latest compiled program in place of the current one, from
    /// its State when that carries over.
    fn swap(&mut self) -> Task<Message> {
        let Some(Generation {
            mut adapter,
            theme,
            messages,
            fonts,
        }) = NEXT.with(|next| next.borrow_mut().take())
        else {
            return Task::none();
        };
        let started = now();
        let carried = self
            .adapter
            .as_ref()
            .and_then(Adapter::export_state)
            .is_some_and(|bytes| adapter.seed_state(&bytes));
        let mut tasks = vec![Application::start(&mut adapter).map(Message::App)];
        if !carried {
            for index in 0..messages {
                let name = format!("{}{index}", rawr_snippet::MESSAGE);
                match adapter.message_of(&name) {
                    Some(Ok(message)) => tasks
                        .push(Application::update_native(&mut adapter, message).map(Message::App)),
                    Some(Err(error)) => log(&format!("rawr-web: message {index}: {error}")),
                    None => {}
                }
            }
        }
        for index in fonts {
            if self.fonts.insert(index) {
                tasks.push(fonts::load(index).map(|()| Message::Font));
            }
        }
        self.adapter = Some(adapter);
        self.theme = theme;
        log(&format!(
            "rawr-web: swapped in {:.1} ms, state {}",
            now() - started,
            if carried { "carried" } else { "booted" },
        ));
        Task::batch(tasks)
    }

    fn view(&self) -> Element<'_, Message> {
        match &self.adapter {
            Some(adapter) => adapter.view().map(Message::App).boxed(),
            None => label("").boxed(),
        }
    }

    fn theme(&self) -> iced::Theme {
        match (&self.theme, &self.adapter) {
            (Some(theme), _) => theme.clone(),
            (None, Some(adapter)) => Application::theme_native(adapter),
            (None, None) => iced::Theme::Dark,
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let app = self
            .adapter
            .as_ref()
            .map_or_else(Subscription::none, |adapter| {
                Application::subscription_native(adapter).map(Message::App)
            });
        Subscription::batch([app, Subscription::run(wakes).map(|()| Message::Swap)])
    }
}

/// The wakes [`run`] sends, as a stream for the app's subscription.
fn wakes() -> impl iced::futures::Stream<Item = ()> {
    use iced::futures::StreamExt;
    INBOX
        .with(|inbox| inbox.borrow_mut().take())
        .map(StreamExt::boxed_local)
        .unwrap_or_else(|| iced::futures::stream::empty().boxed_local())
}

/// A program checked and booted, or its report against the snippet.
fn compile(program: &str, source: &str) -> Result<Adapter, String> {
    let compiler = rawr_iced::application::Compiler::new().map_err(|error| error.to_string())?;
    compiler
        .check_named_source(rawr_snippet::SOURCE_NAME, program)
        .and_then(rawr_iced::application::Checked::prepare)
        .map(rawr_iced::application::Prepared::into_adapter)
        .map_err(|error| {
            let evidence = rawr::diagnostic::Source::new(rawr_snippet::SOURCE_NAME, program).ok();
            let report = rawr::diagnostic::render(&error.report(evidence));
            rawr_snippet::report(&report, program, source)
        })
}

fn now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0.0, |performance| performance.now())
}

fn log(message: &str) {
    web_sys::console::log_1(&message.into());
}
