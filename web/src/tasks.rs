//! A plain program whose `main` is `async`, in the browser. It is wrapped
//! as the `?play` server wraps it, an iced application whose boot Task
//! awaits `main`, but no window is opened: its tasks run on the page's
//! event loop (`wasm-bindgen-futures`), each output goes through the
//! adapter's update, and the tasks that update starts run in turn. tokio's
//! timers are the page's (rawr-iced's tokio facade on wasm32), so a sleep
//! takes the time it says. Output reaches the page as it is written, and
//! the run ends with the server's text result.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use iced::futures::channel::oneshot;
use iced::futures::future::{self, AbortHandle, Abortable, Either};
use iced::futures::StreamExt as _;
use rawr_iced::program::{Adapter, Application};
use rawr_iced::runtime;
use rawr_snippet::{plain, SOURCE_NAME};

use crate::text::{self, Output};

thread_local! {
    /// The program running now. A new run stops it.
    static RUNNING: RefCell<Option<Rc<Run>>> = const { RefCell::new(None) };
}

/// A program running on the page's event loop.
struct Run {
    adapter: RefCell<Adapter>,
    /// Tasks still running.
    tasks: Cell<usize>,
    /// Each running task's abort.
    aborts: RefCell<Vec<AbortHandle>>,
    /// Told once the program ends or is stopped.
    done: RefCell<Option<oneshot::Sender<()>>>,
}

impl Run {
    /// Stop every task and tell the waiting run.
    fn end(&self) {
        for abort in self.aborts.borrow_mut().drain(..) {
            abort.abort();
        }
        if let Some(done) = self.done.borrow_mut().take() {
            let _ = done.send(());
        }
    }

    /// The status `main` ended with: the wrapper's title, `-1` until then.
    fn status(&self) -> Option<i32> {
        self.adapter
            .borrow_mut()
            .refresh_title()
            .ok()
            .and_then(|status| status.parse::<i32>().ok())
            .filter(|&status| status >= 0)
    }
}

/// Run `task` on the page's event loop, applying each output through the
/// adapter and running the task that update starts.
fn spawn(run: &Rc<Run>, task: iced::Task<runtime::Message>) {
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        return;
    };
    let (abort, registration) = AbortHandle::new_pair();
    run.aborts.borrow_mut().push(abort);
    run.tasks.set(run.tasks.get() + 1);
    let run = run.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let driven = {
            let run = run.clone();
            async move {
                while let Some(action) = stream.next().await {
                    let iced_runtime::Action::Output(message) = action else {
                        continue;
                    };
                    let next = Application::update_native(&mut run.adapter.borrow_mut(), message);
                    spawn(&run, next);
                    if run.status().is_some() {
                        // `main` returned: the program ends, as a runtime
                        // drops its tasks once `main` returns.
                        run.end();
                    }
                }
            }
        };
        let _ = Abortable::new(driven, registration).await;
        run.tasks.set(run.tasks.get() - 1);
        if run.tasks.get() == 0 {
            run.end();
        }
    });
}

/// Run an `async fn main` program; its output as the server's text result,
/// or a compile error as [`text::error`]. `output` is called with
/// `(stream, text)` as the program writes. A later run stops this one.
pub async fn run(source: String, output: js_sys::Function) -> String {
    if let Some(previous) = RUNNING.with(|running| running.borrow_mut().take()) {
        previous.end();
    }
    let source = rawr_snippet::without_tests(&source);
    let Some(main) = plain::async_main(&source) else {
        return text::error("the program has no `async fn main`");
    };
    let program = plain::async_program(&source, &main);
    let sink = Arc::new(Mutex::new(Output::default()));
    let compiler = match rawr_iced::application::Compiler::configured(|engine| {
        text::sandbox(engine, &sink);
    }) {
        Ok(compiler) => compiler,
        Err(error) => return text::error(&error.to_string()),
    };
    text::stream(Some(output));
    let started = crate::now();
    let checked = if rawr_snippet::has_top_level_mod(&program) {
        compiler.check_named_crate_source(SOURCE_NAME, &program)
    } else {
        compiler.check_named_source(SOURCE_NAME, &program)
    };
    let prepared = checked.and_then(rawr_iced::application::Checked::prepare);
    let mut adapter = match prepared {
        Ok(prepared) => prepared.into_adapter(),
        Err(error) => {
            text::stream(None);
            let evidence = rawr::diagnostic::Source::new(SOURCE_NAME, &program).ok();
            let report = error.report(evidence);
            // The deadline or the output cap ended the boot: that is the
            // limit, not an error in the program.
            if report.code() == rawr::diagnostic::code::RUNTIME_EVALUATION
                && report.message() == "Script terminated"
            {
                return limited(&sink);
            }
            let text = rawr::diagnostic::render(&report);
            return text::error(&rawr_snippet::report(&text, &program, &source));
        }
    };
    let task = Application::start(&mut adapter);
    let (done, ended) = oneshot::channel();
    let run = Rc::new(Run {
        adapter: RefCell::new(adapter),
        tasks: Cell::new(0),
        aborts: RefCell::default(),
        done: RefCell::new(Some(done)),
    });
    RUNNING.with(|running| *running.borrow_mut() = Some(run.clone()));
    spawn(&run, task);
    if run.tasks.get() == 0 {
        run.end();
    }
    let left = (text::TIME_MS - (crate::now() - started)).max(0.0);
    let deadline = wasmtimer::tokio::sleep(std::time::Duration::from_secs_f64(left / 1000.0));
    let finished = matches!(
        future::select(ended, Box::pin(deadline)).await,
        Either::Left(_)
    );
    run.end();
    text::stream(None);
    RUNNING.with(|running| {
        let mut running = running.borrow_mut();
        if running
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, &run))
        {
            *running = None;
        }
    });
    let status = run.status();
    let failure = run
        .adapter
        .borrow()
        .diagnostics()
        .first()
        .map(|entry| entry.report().clone());
    drop(compiler);
    let mut output = std::mem::take(&mut *sink.lock().unwrap());
    if let Some(report) = failure {
        output.status = 1;
        let text = rawr::diagnostic::render(&report);
        if output.truncated || text.contains(text::OUTPUT_LIMIT) {
            output.limit = true;
        } else if let Some(reason) = text::limit(&text) {
            output.limit = true;
            output.push(true, &format!("{reason}\n"));
        } else {
            output.status = text::PANIC_STATUS;
            let message = text::panic(&report, &program, &source);
            output.push(true, &format!("{}\n", message.trim_end()));
        }
    } else if !finished && !output.truncated {
        output.status = 1;
        output.limit = true;
        output.push(true, &format!("{}\n", text::TIME_LIMIT));
    } else if status == Some(1) {
        output.status = 1;
    }
    output.finish()
}

/// The result of a program the time limit or the output cap cut.
fn limited(sink: &Arc<Mutex<Output>>) -> String {
    let mut output = std::mem::take(&mut *sink.lock().unwrap());
    output.status = 1;
    if !output.truncated {
        output.limit = true;
        output.push(true, &format!("{}\n", text::TIME_LIMIT));
    }
    output.finish()
}
