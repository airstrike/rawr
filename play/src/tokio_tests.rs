//! tokio in `?play` snippets: rawr-iced's own tokio, on the recording's
//! simulated clock. Each test records an app whose boot runs `run()` and
//! shows its output, and compares the last frame with an app that shows
//! the expected text at once.

use std::time::{Duration, Instant};

use super::{application, canvas, motion, wrap, Deadline, Options, MAX_SIDE};

/// An app that shows the `String` its boot task gives.
fn app(items: &str, start: &str) -> String {
    format!(
        "use iced::widget::text;
use iced::{{Task, Widget}};
use std::time::Duration;

fn main() -> iced::Result {{
    iced::application(App::start, App::update, App::view).run()
}}

#[derive(Default)]
struct App {{
    shown: String,
}}

#[derive(Debug, Clone)]
enum Message {{
    Shown(String),
}}

{items}

impl App {{
    fn start() -> (Self, Task<Message>) {{
        (Self::default(), {start})
    }}

    fn update(&mut self, message: Message) {{
        match message {{
            Message::Shown(shown) => self.shown = shown,
        }}
    }}

    fn view(&self) -> impl Widget<Message> {{
        text(self.shown.clone())
    }}
}}
"
    )
}

/// The frames `source` records on the simulated clock, at one scale.
fn record(source: &str) -> Vec<rawr_iced::snapshot::Frame> {
    let options = Options::default();
    let program = wrap(source, &options).unwrap();
    let deadline = Deadline::new(Instant::now() + Duration::from_secs(120));
    let _clock = rawr_iced::snapshot::clock::freeze();
    let (mut adapter, messages) = application(&program, source, &options, deadline.clone())
        .unwrap_or_else(|error| panic!("{error}"));
    let motion = rawr_iced::snapshot::Motion {
        step: motion::STEP,
        frames: motion::FRAMES,
        phase: motion::PHASE,
        span: motion::SPAN,
        deadline,
        extended: Instant::now() + Duration::from_secs(120),
        cut: None,
        animation_scale: None,
        still: false,
    };
    let recording = rawr_iced::snapshot::record_application(
        &mut adapter,
        messages,
        &[],
        canvas(&options),
        (MAX_SIDE, MAX_SIDE),
        Some(options.theme.variant()),
        motion,
        &mut |_| false,
        &mut |_| {},
    )
    .unwrap();
    if let Some(entry) = adapter.diagnostics().iter().next() {
        panic!("{entry:?}");
    }
    recording.frames
}

/// `run()` (an `async fn` among `items` that gives a `String`) shows
/// `expected` in the end, first at `at` on the simulated clock.
fn shows_at(items: &str, expected: &str, at: Duration) {
    let frames = record(&app(items, "Task::perform(run(), Message::Shown)"));
    let shown = record(&app(
        items,
        &format!("Task::done(Message::Shown({expected:?}.to_string()))"),
    ));
    let last = frames.last().unwrap();
    assert!(
        last.same_image(shown.last().unwrap()),
        "the app does not show {expected:?}"
    );
    let first = frames.iter().find(|frame| frame.same_image(last)).unwrap();
    assert_eq!(first.time, at, "{expected:?} showed at {:?}", first.time);
}

#[test]
fn tokio_sleep_waits_on_the_recording_clock() {
    shows_at(
        "async fn run() -> String {
    let start = tokio::time::Instant::now();
    tokio::time::sleep(Duration::from_millis(300)).await;
    format!(\"{} ms\", start.elapsed().as_millis())
}",
        "300 ms",
        Duration::from_millis(300),
    );
}

#[test]
fn tokio_interval_ticks_on_the_recording_clock() {
    shows_at(
        "use tokio::time::interval;
async fn run() -> String {
    let mut ticker = interval(Duration::from_millis(100));
    let mut ticks = 0;
    for _ in 0..4 {
        ticker.tick().await;
        ticks += 1;
    }
    format!(\"{ticks} ticks\")
}",
        "4 ticks",
        // The first tick is at once.
        Duration::from_millis(300),
    );
}

#[test]
fn tokio_timeout_gives_the_output_or_elapsed() {
    shows_at(
        "use tokio::time::{sleep, timeout};
async fn run() -> String {
    let quick = timeout(Duration::from_millis(200), async { 7 }).await;
    let slow = timeout(Duration::from_millis(200), async {
        sleep(Duration::from_millis(500)).await;
        8
    })
    .await;
    format!(\"{quick:?} {}\", slow.is_err())
}",
        "Ok(7) true",
        Duration::from_millis(200),
    );
}

#[test]
fn tokio_mpsc_sends_until_the_sender_drops() {
    shows_at(
        "use tokio::sync::mpsc;
async fn run() -> String {
    let (sender, mut receiver) = mpsc::channel(2);
    let producer = async move {
        for n in 1..=4 {
            sender.send(n).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        drop(sender);
    };
    let consumer = async move {
        let mut sum = 0;
        while let Some(n) = receiver.recv().await {
            sum += n;
        }
        sum
    };
    let (_, sum) = tokio::join!(producer, consumer);
    format!(\"sum {sum}\")
}",
        "sum 10",
        Duration::from_millis(400),
    );
}

#[test]
fn tokio_mpsc_closes_when_the_moved_sender_ends() {
    shows_at(
        "use tokio::sync::mpsc;
async fn run() -> String {
    let (sender, mut receiver) = mpsc::channel(2);
    let producer = async move {
        for n in 1..=4 {
            sender.send(n).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    let consumer = async move {
        let mut sum = 0;
        while let Some(n) = receiver.recv().await {
            sum += n;
        }
        sum
    };
    let (_, sum) = tokio::join!(producer, consumer);
    format!(\"sum {sum}\")
}",
        "sum 10",
        Duration::from_millis(400),
    );
}

#[test]
fn tokio_mpsc_closes_when_the_last_sender_leaves_its_block() {
    shows_at(
        "use tokio::sync::mpsc;
async fn run() -> String {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    {
        let other = sender.clone();
        other.send(1).unwrap();
    }
    {
        let last = sender;
        last.send(2).unwrap();
    }
    let mut got = Vec::new();
    while let Some(n) = receiver.recv().await {
        got.push(n);
    }
    format!(\"{got:?}\")
}",
        "[1, 2]",
        Duration::ZERO,
    );
}

#[test]
fn tokio_mpsc_unbounded_and_cloned_senders() {
    shows_at(
        "use tokio::sync::mpsc;
async fn run() -> String {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let other = sender.clone();
    sender.send(1).unwrap();
    drop(sender);
    other.send(2).unwrap();
    drop(other);
    let mut got = Vec::new();
    while let Some(n) = receiver.recv().await {
        got.push(n);
    }
    format!(\"{got:?}\")
}",
        "[1, 2]",
        Duration::ZERO,
    );
}

#[test]
fn tokio_oneshot_sends_one_value() {
    shows_at(
        "use tokio::sync::oneshot;
async fn run() -> String {
    let (sender, receiver) = oneshot::channel();
    let send = async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        sender.send(42).unwrap();
    };
    let (_, received) = tokio::join!(send, receiver);
    format!(\"{}\", received.unwrap())
}",
        "42",
        Duration::from_millis(150),
    );
}

#[test]
fn tokio_watch_shows_the_latest_value() {
    shows_at(
        "use tokio::sync::watch;
async fn run() -> String {
    let (sender, mut receiver) = watch::channel(0);
    let send = async move {
        for n in 1..=3 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            sender.send(n).unwrap();
        }
    };
    let watch = async move {
        let mut seen = Vec::new();
        while seen.len() < 3 {
            receiver.changed().await.unwrap();
            seen.push(receiver.borrow_and_update());
        }
        seen
    };
    let (_, seen) = tokio::join!(send, watch);
    format!(\"{seen:?}\")
}",
        "[1, 2, 3]",
        Duration::from_millis(300),
    );
}

#[test]
fn tokio_yield_now_lets_the_others_run() {
    shows_at(
        "async fn run() -> String {
    tokio::task::yield_now().await;
    \"yielded\".to_string()
}",
        "yielded",
        Duration::ZERO,
    );
}

#[test]
fn tokio_join_and_try_join_run_futures_at_once() {
    shows_at(
        "use tokio::join;
use tokio::time::sleep;
async fn after(ms: u64, value: i64) -> Result<i64, String> {
    sleep(Duration::from_millis(ms)).await;
    if value < 0 { Err(format!(\"{value} failed\")) } else { Ok(value) }
}
async fn run() -> String {
    let (a, b, c) = join!(after(200, 1), after(100, 2), after(200, 3));
    let all = tokio::try_join!(after(100, 4), after(200, 5));
    let failed = tokio::try_join!(after(100, -1), after(200, 6));
    format!(\"{} {all:?} {failed:?}\", a.unwrap() + b.unwrap() + c.unwrap())
}",
        "6 Ok((4, 5)) Err(\"-1 failed\")",
        Duration::from_millis(500),
    );
}

#[test]
fn tokio_select_takes_the_first_branch_that_ends() {
    shows_at(
        "use tokio::sync::mpsc;
use tokio::time::sleep;
async fn run() -> String {
    let (sender, mut receiver) = mpsc::channel(1);
    let send = async move {
        sleep(Duration::from_millis(100)).await;
        sender.send(5).await.unwrap();
    };
    let wait = async move {
        tokio::select! {
            value = receiver.recv() => match value {
                Some(n) => format!(\"got {n}\"),
                None => \"closed\".to_string(),
            },
            _ = sleep(Duration::from_millis(300)) => \"timed out\".to_string(),
        }
    };
    let (_, first) = tokio::join!(send, wait);
    let second = tokio::select! {
        biased;
        _ = sleep(Duration::from_millis(200)) => \"slow\".to_string(),
        n = async { 7 } => {
            format!(\"ready {n}\")
        }
    };
    let (sender, mut receiver) = mpsc::channel::<i64>(1);
    drop(sender);
    let third = tokio::select! {
        Some(n) = receiver.recv() => format!(\"{n}\"),
        else => \"closed\".to_string(),
    };
    format!(\"{first}, {second}, {third}\")
}",
        "got 5, ready 7, closed",
        Duration::from_millis(100),
    );
}

/// The compile error `?play` gives for an app with `items`.
fn rejected(items: &str) -> String {
    let source = app(items, "Task::perform(run(), Message::Shown)");
    match super::render(&source, &Options::default()) {
        Err(super::PlayError::Compile(error)) => error,
        Err(error) => panic!("{error}"),
        Ok(_) => panic!("the snippet compiled"),
    }
}

/// tokio disables a branch whose value does not match its pattern and
/// keeps waiting on the others, and goes to `else` once every branch is
/// disabled.
#[test]
fn tokio_select_waits_on_the_others_when_a_pattern_does_not_match() {
    shows_at(
        "use tokio::sync::mpsc;
use tokio::time::sleep;
async fn run() -> String {
    let (sender, mut receiver) = mpsc::channel::<i64>(1);
    drop(sender);
    let first = tokio::select! {
        Some(n) = receiver.recv() => format!(\"got {n}\"),
        _ = sleep(Duration::from_millis(300)) => \"timed out\".to_string(),
    };
    let (sender, mut receiver) = mpsc::channel::<i64>(1);
    drop(sender);
    let (other, mut closed) = mpsc::channel::<i64>(1);
    drop(other);
    let second = tokio::select! {
        Some(n) = receiver.recv() => format!(\"got {n}\"),
        Some(n) = closed.recv() => format!(\"also {n}\"),
        else => \"both closed\".to_string(),
    };
    format!(\"{first}, {second}\")
}",
        "timed out, both closed",
        Duration::from_millis(300),
    );
}

#[test]
fn tokio_spawn_says_what_to_use_instead() {
    let error = rejected(
        "async fn run() -> String {
    tokio::spawn(async { 1 });
    String::new()
}",
    );
    assert!(
        error.contains("`tokio::spawn` in a program without a tokio dependency"),
        "{error}"
    );
    assert!(error.contains("tokio::join!"), "{error}");
}

#[test]
fn tokio_spawn_blocking_runs_at_the_instant_it_starts() {
    // The worker reads the recording's frozen time, and the recording waits
    // for it, so its output shows at the same simulated instant every run.
    shows_at(
        "use tokio::task::spawn_blocking;
fn count(limit: u64) -> u64 {
    let mut primes = 0;
    for n in 2..limit {
        if (2..n).take_while(|d| d * d <= n).all(|d| n % d != 0) {
            primes += 1;
        }
    }
    primes
}
async fn run() -> String {
    let start = tokio::time::Instant::now();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (primes, elapsed) = spawn_blocking(move || (count(1000), start.elapsed().as_millis()))
        .await
        .unwrap();
    format!(\"{primes} primes at {elapsed} ms\")
}",
        "168 primes at 200 ms",
        Duration::from_millis(200),
    );
}

#[test]
fn tokio_spawn_blocking_output_reaches_update_through_task_perform() {
    let items = "fn work(n: u64) -> String { format!(\"sum {}\", (1..=n).sum::<u64>()) }";
    let frames = record(&app(
        items,
        "Task::perform(tokio::task::spawn_blocking(move || work(100)), |result| \
         Message::Shown(result.unwrap()))",
    ));
    let shown = record(&app(
        items,
        "Task::done(Message::Shown(\"sum 5050\".to_string()))",
    ));
    let last = frames.last().unwrap();
    assert!(
        last.same_image(shown.last().unwrap()),
        "the app does not show the sum"
    );
    assert_eq!(
        frames
            .iter()
            .find(|frame| frame.same_image(last))
            .unwrap()
            .time,
        Duration::ZERO
    );
}

#[test]
fn std_thread_spawn_says_to_use_spawn_blocking() {
    let error = rejected(
        "async fn run() -> String {
    std::thread::spawn(move || 1).join().unwrap().to_string()
}",
    );
    assert!(error.contains("spawn_blocking"), "{error}");
}

#[test]
fn tokio_files_sockets_and_processes_are_unknown() {
    for path in [
        "tokio::fs::read_to_string(\"/etc/passwd\")",
        "tokio::net::TcpStream::connect(\"example.com:80\")",
        "tokio::process::Command::new(\"ls\")",
        "tokio::io::stdin()",
        "tokio::signal::ctrl_c()",
    ] {
        let error = rejected(&format!(
            "async fn run() -> String {{
    let _ = {path};
    String::new()
}}"
        ));
        assert!(
            error.contains("error[RAWR-RESOLVE-UNKNOWN-"),
            "{path}: {error}"
        );
    }
}

#[test]
fn tokio_mutex_lock_waits_for_the_holder() {
    shows_at(
        "use tokio::sync::Mutex;
use tokio::time::sleep;
async fn add(total: &Mutex<i64>, n: i64) {
    let mut held = total.lock().await;
    let before = *held;
    sleep(Duration::from_millis(100)).await;
    *held = before + n;
}
async fn run() -> String {
    let total = Mutex::new(0);
    tokio::join!(add(&total, 1), add(&total, 2));
    *total.lock().await += 10;
    let value = *total.lock().await;
    format!(\"total {value}\")
}",
        "total 13",
        Duration::from_millis(200),
    );
}
