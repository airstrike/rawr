//! Script `async fn`, `async` blocks and `.await`, run through `Task`.

use rawr::{Engine, Shared};
use rawr_iced::program;

/// An app whose boot performs `future` and adds each loaded value up.
fn app(items: &str, future: &str) -> String {
    format!(
        r#"
use iced::Task;
use iced::widget::{{column, text, Column}};
#[derive(Debug, Clone)] enum Message {{ Loaded(i64) }}
struct Counter {{ value: i64 }}
pub fn main() -> iced::Result {{
    iced::application(Counter::boot, Counter::update, Counter::view).run()
}}
{items}
impl Counter {{
    fn boot() -> (Counter, Task<Message>) {{
        (Counter {{ value: 0 }}, Task::perform({future}, Message::Loaded))
    }}
    fn update(&mut self, message: Message) {{
        match message {{ Message::Loaded(value) => self.value += value }}
    }}
    fn view(&self) -> Column<'_, Message> {{ column![text(self.value)] }}
}}
"#
    )
}

fn engine() -> Shared<Engine> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine).unwrap();
    Shared::new(engine)
}

fn boot(source: &str) -> program::Adapter {
    let engine = engine();
    let descriptor = engine
        .compile_application_descriptor(source)
        .unwrap_or_else(|error| panic!("{error}"));
    let launch = engine.launch_application(descriptor).unwrap();
    let mut registry = rawr::host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    program::Adapter::boot(launch.bind(registry).unwrap()).unwrap()
}

/// Boot `source`, run its startup task to its end, apply what it sends,
/// and read the number the view shows.
fn loaded(source: &str) -> i64 {
    let adapter = boot(source);
    let application = program::Application::from_adapter(adapter);
    let (mut state, startup) = <program::Application as iced::Program>::boot(&application);
    let outputs = state.drain(startup).unwrap_or_default();
    for message in outputs {
        let _ = <program::Application as iced::Program>::update(&application, &mut state, message);
    }
    let mut simulator = iced_test::simulator(state.view());
    (0..1000)
        .find(|value| simulator.find(value.to_string().as_str()).is_ok())
        .expect("the view shows a number")
}

fn rejected(source: &str) -> String {
    match engine().compile_application_descriptor(source) {
        Ok(_) => panic!("the program compiled"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn an_async_block_awaits_tasks_in_order() {
    let source = app(
        "",
        "async { let a = Task::done(2).await; let b = Task::done(a * 10).await; a + b }",
    );
    assert_eq!(loaded(&source), 22);
}

#[test]
fn an_async_fn_awaits_another_and_resumes_its_loop() {
    let source = app(
        r#"
async fn double(n: i64) -> i64 { Task::done(n).await * 2 }
async fn sum(count: i64) -> i64 {
    let mut total = 0;
    for i in 0..count { total += double(i).await; }
    total
}
"#,
        "sum(4)",
    );
    assert_eq!(loaded(&source), 12);
}

#[test]
fn question_mark_returns_from_the_async_body() {
    let source = app(
        r#"
async fn parse(text: String) -> Result<i64, String> {
    let n: i64 = Task::done(text).await.parse().map_err(|_| "bad".to_string())?;
    Ok(n + 1)
}
async fn total() -> i64 {
    let good = parse("41".to_string()).await.unwrap_or(0);
    let bad = parse("x".to_string()).await.unwrap_or(100);
    good + bad
}
"#,
        "total()",
    );
    assert_eq!(loaded(&source), 142);
}

#[test]
fn an_async_method_and_an_async_closure_capture_their_values() {
    let source = app(
        r#"
#[derive(Debug, Clone)] struct Step { by: i64 }
impl Step {
    async fn apply(self, n: i64) -> i64 { Task::done(n).await + self.by }
}
fn twice(n: i64) -> Task<i64> {
    let step = Step { by: n };
    let run = async move |x: i64| step.clone().apply(x).await;
    run(1)
}
"#,
        "async { let a = twice(3).await; let b = Step { by: 10 }.apply(a).await; if b > 10 { b } else { 0 } }",
    );
    assert_eq!(loaded(&source), 14);
}

#[test]
fn await_outside_an_async_body_is_rejected() {
    let error = rejected(&app("fn f() -> i64 { Task::done(1).await }", "async { 1 }"));
    assert!(error.contains("`.await` outside an async block"), "{error}");
}

/// The sipper snippet's app, with `sleep` as its delay.
fn sipper_app(sleep: &str) -> String {
    format!(
        r#"
use iced::task::sipper;
use iced::widget::{{column, text, Column}};
use iced::Task;
use std::time::Duration;
#[derive(Debug, Clone)] enum Message {{ Progress(f32), Done }}
#[derive(Default)] struct Download {{ percent: f32, done: bool }}
pub fn main() -> iced::Result {{
    iced::application(Download::start, Download::update, Download::view).run()
}}
impl Download {{
    fn start() -> (Self, Task<Message>) {{
        let download = sipper(async |mut progress| {{
            for step in 0..=3 {{
                progress.send(step as f32 * 10.0).await;
                {sleep}
            }}
        }});
        (Self::default(), Task::sip(download, Message::Progress, |_| Message::Done))
    }}
    fn update(&mut self, message: Message) {{
        match message {{
            Message::Progress(percent) => self.percent += percent,
            Message::Done => self.done = true,
        }}
    }}
    fn view(&self) -> Column<'_, Message> {{
        column![text(if self.done {{ format!("done {{}}", self.percent) }} else {{ "running".to_string() }})]
    }}
}}
"#
    )
}

#[test]
fn a_script_sipper_sends_its_progress_and_then_its_output() {
    let mut adapter = boot(&sipper_app(""));
    adapter.deliver(Vec::new());
    for entry in adapter.diagnostics() {
        eprintln!("{entry:?}");
    }
    let mut simulator = iced_test::simulator(adapter.view());
    assert!(simulator.find("done 60").is_ok(), "the sipper sent 0 + 10 + 20 + 30");
}

#[test]
fn tokio_sleep_without_tokio_waits_and_then_goes_on() {
    let mut adapter = boot(&sipper_app(
        "tokio::time::sleep(Duration::from_millis(1)).await;",
    ));
    let start = std::time::Instant::now();
    loop {
        adapter.deliver(Vec::new());
        let mut simulator = iced_test::simulator(adapter.view());
        if simulator.find("done 60").is_ok() {
            break;
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "the sleeps end");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
