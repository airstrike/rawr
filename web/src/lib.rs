//! Rawr's snippets in a browser: the interpreter and rawr-iced on wasm.
//!
//! [`kind`] says how a snippet runs, as the `?play` server decides it from
//! rawr-iced's claims. [`run_text`] runs a plain program and returns its
//! output as the server's text result. [`run_async`] runs a program whose
//! `main` is `async` on the page's event loop and real clock, and streams
//! its output as it is written. [`run`] runs an iced snippet as one
//! iced application on the page's `#iced` element; a later call swaps in the
//! edited program, and its State carries over, native values and all, when
//! its script types are unchanged. Both wrap a snippet with `rawr-snippet`,
//! as the server does.
#![cfg(target_arch = "wasm32")]

mod fonts;
mod tasks;
mod text;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use iced::futures::channel::mpsc;
use iced::widget::{column, container, text as label, Widget as _};
use iced::{Element, Fill, Subscription, Task};
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
    /// The name of a full app's State type, as its source names it.
    state: Option<String>,
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
        state: state_name(code),
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
    webgl_on_webkit();
    iced::application(Web::boot, Web::update, Web::view)
        .theme(Web::theme)
        .subscription(Web::subscription)
        .run()
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// On WebKit, iced draws with WebGL2. WebKit's WebGPU is the one in Safari
/// and in every browser on iOS. It fails a command buffer that copies twice
/// out of one buffer while that buffer is still mapped. wgpu's
/// `StagingBelt` uploads iced's quads and glyphs that way. So every frame
/// fails validation at submit, and the canvas stays black. wgpu takes WebGL2
/// when `navigator.gpu` is undefined. Every engine on iOS reports
/// `navigator.vendor` as Apple's, and desktop Safari does too.
fn webgl_on_webkit() {
    let navigator = js_sys::Reflect::get(&js_sys::global(), &"navigator".into())
        .ok()
        .filter(JsValue::is_object);
    let Some(navigator) = navigator else {
        return;
    };
    let vendor = js_sys::Reflect::get(&navigator, &"vendor".into()).ok();
    if vendor.and_then(|vendor| vendor.as_string()).as_deref() != Some("Apple Computer, Inc.") {
        return;
    }
    let hidden = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&hidden, &"value".into(), &JsValue::UNDEFINED);
    let _ = js_sys::Reflect::set(&hidden, &"configurable".into(), &JsValue::TRUE);
    let _ = js_sys::Reflect::define_property(
        navigator.unchecked_ref::<js_sys::Object>(),
        &"gpu".into(),
        &hidden,
    );
    log("rawr-web: WebKit's WebGPU fails iced's uploads, so iced draws with WebGL2");
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
    /// The running app's State type name, as its source names it.
    state: Option<String>,
    /// Why the last swap booted a fresh State, shown under the app.
    note: Option<String>,
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
            state,
            fonts,
        }) = NEXT.with(|next| next.borrow_mut().take())
        else {
            return Task::none();
        };
        let started = now();
        // The State itself carries, as rawr-iced's in-process reload keeps
        // it: an `Instant` or a `Duration` in it too, so a running stopwatch
        // keeps running. Its subscription is the new program's, from it.
        let carried = self
            .adapter
            .as_mut()
            .is_some_and(|previous| adapter.carry_state(previous));
        // Another app's State (another example) boots without a note.
        self.note = (!carried && self.adapter.is_some() && state.is_some() && state == self.state)
            .then(|| "State changed shape; started fresh".to_owned());
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
        self.state = state;
        log(&format!(
            "rawr-web: swapped in {:.1} ms, state {}{}",
            now() - started,
            if carried { "carried" } else { "booted" },
            self.note
                .as_deref()
                .map(|note| format!(" · {note}"))
                .unwrap_or_default(),
        ));
        Task::batch(tasks)
    }

    fn view(&self) -> Element<'_, Message> {
        let app = match &self.adapter {
            Some(adapter) => adapter.view().map(Message::App).boxed(),
            None => return label("").boxed(),
        };
        match &self.note {
            // One line under the app, as rawr-iced's window notes a reseed.
            Some(note) => column![
                container(app).height(Fill),
                container(label(note).size(13).style(label::secondary)).padding([4, 8]),
            ]
            .boxed(),
            None => app,
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

/// The State type a full app's `update` is a method of, as the source
/// names it (`Counter` for `Counter::update`), so a swap tells an edited
/// State from another app's.
fn state_name(code: &str) -> Option<String> {
    let at = code.find("::update")?;
    let before = code[..at].trim_end_matches(|c: char| c.is_alphanumeric() || c == '_');
    let name = &code[before.len()..at];
    (!name.is_empty()).then(|| name.to_owned())
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
    sandbox();
    // rand, tracing_subscriber and the examples' shipped files, as the
    // server gives a snippet.
    let compiler = rawr_iced::application::Compiler::configured(|engine| {
        rawr_play_sandbox::install(engine).expect("the sandbox's facades install");
    })
    .map_err(|error| error.to_string())?;
    // An inline `mod` needs the crate compiler, which reads the one file
    // as a crate's root.
    let checked = if rawr_snippet::has_top_level_mod(program) {
        compiler.check_named_crate_source(rawr_snippet::SOURCE_NAME, program)
    } else {
        compiler.check_named_source(rawr_snippet::SOURCE_NAME, program)
    };
    checked
        .and_then(|checked| {
            checked.prepare_with(
                rawr_play_sandbox::runtime_facades()
                    .expect("the sandbox's runtime facades bind every item"),
            )
        })
        .map(rawr_iced::application::Prepared::into_adapter)
        .map_err(|error| {
            let evidence = rawr::diagnostic::Source::new(rawr_snippet::SOURCE_NAME, program).ok();
            let report = rawr::diagnostic::render(&error.report(evidence));
            rawr_snippet::report(&report, program, source)
        })
}

/// The page's sandbox, once: rand seeded from the page, as `rand::rng()`
/// is seeded from the system, and tracing's lines in the console.
fn sandbox() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let seed = (js_sys::Math::random() * f64::from(u32::MAX)) as u64
            ^ ((js_sys::Math::random() * f64::from(u32::MAX)) as u64) << 32;
        rawr_play_sandbox::rand::seed(seed);
        rawr_play_sandbox::tracing::write_to(log);
    });
}

fn now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0.0, |performance| performance.now())
}

fn log(message: &str) {
    web_sys::console::log_1(&message.into());
}
