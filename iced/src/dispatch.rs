//! Script work that a native task hands back to the UI thread.
//!
//! iced polls a task's stream on its executor, which may be another thread.
//! Script code and the imported-value arena belong to the UI thread, so a
//! lowered task never runs script inside its stream. Each step that needs
//! script sends a [`Resume`] envelope through iced's runtime instead. The
//! host's update checks the envelope's instance and generation first. A
//! fresh envelope runs its job there. A stale one is dropped, and the task
//! that sent it ends, so an old continuation never runs against new code.

use std::fmt;
use std::sync::{Arc, Mutex};

use iced::futures::channel::{mpsc, oneshot};

use crate::{effect, runtime};

type Job = Box<dyn FnOnce() -> Option<runtime::Message> + Send + 'static>;

/// One step of script work waiting for the UI thread.
#[derive(Clone)]
pub(crate) struct Resume(Arc<Mutex<Option<Job>>>);

impl Resume {
    fn new(job: impl FnOnce() -> Option<runtime::Message> + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(Some(Box::new(job)))))
    }

    /// Run the job once, on the UI thread. A job that ends in a message for
    /// the application returns it.
    pub(crate) fn run(&self) -> Option<runtime::Message> {
        let job = self.0.lock().ok()?.take()?;
        job()
    }
}

impl fmt::Debug for Resume {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Resume(<private>)")
    }
}

/// A task that asks the UI thread to run `job` and then runs the task the
/// job returns. Dropping the request without running it ends this task.
pub(crate) fn on_ui(
    owner: effect::Owner,
    job: impl FnOnce() -> iced::Task<runtime::Message> + Send + 'static,
) -> iced::Task<runtime::Message> {
    let (sender, receiver) = oneshot::channel();
    let request = runtime::Message::resume(
        owner,
        Resume::new(move || {
            let _ = sender.send(job());
            None
        }),
        false,
    );
    iced::Task::done(request).chain(
        iced::Task::future(receiver).then(|reply| reply.unwrap_or_else(|_| iced::Task::none())),
    )
}

/// A task that hands each output of `task` to `step` on the UI thread. The
/// native stream does not wait for one step before it yields the next
/// output, as a Rust `Task::sip` maps its progress without waiting. The
/// tasks the steps return run in output order. The task ends when the
/// native task has ended and every step it asked for has run or was
/// dropped as stale.
pub(crate) fn each_on_ui<T: Send + 'static>(
    owner: effect::Owner,
    task: iced::Task<T>,
    step: impl Fn(T) -> iced::Task<runtime::Message> + Send + Sync + 'static,
) -> iced::Task<runtime::Message> {
    let (sender, receiver) = mpsc::unbounded::<iced::Task<runtime::Message>>();
    let step = Arc::new(step);
    let requests = task.map(move |output| {
        let sender = sender.clone();
        let step = step.clone();
        runtime::Message::resume(
            owner,
            Resume::new(move || {
                let _ = sender.unbounded_send(step(output));
                None
            }),
            false,
        )
    });
    iced::Task::batch([requests, iced::Task::stream(receiver).then(|task| task)])
}

/// A message that asks the UI thread to run `job` and deliver the message it
/// returns. Subscriptions use this, since their streams wait for no reply.
pub(crate) fn deliver_on_ui(
    owner: effect::Owner,
    job: impl FnOnce() -> Option<runtime::Message> + Send + 'static,
) -> runtime::Message {
    runtime::Message::resume(owner, Resume::new(job), true)
}

/// Run a resume request inline, as the host's update would on its own
/// thread, and keep any other message. Tests that drain a task's stream
/// on the update thread use this.
#[cfg(test)]
pub(crate) fn inline(message: runtime::Message) -> Option<runtime::Message> {
    match message.resumption() {
        Some(resume) => resume.run(),
        None => Some(message),
    }
}

#[cfg(test)]
mod tests {
    use iced_test::futures::futures::StreamExt;
    use rawr::Dynamic;

    use super::each_on_ui;
    use crate::{effect, runtime};

    #[test]
    fn each_output_asks_for_its_step_without_waiting_and_keeps_its_order() {
        let owner = owner();
        let task = each_on_ui(
            owner,
            iced::Task::stream(iced::futures::stream::iter([1_i64, 2, 3])),
            move |value| {
                iced::Task::done(runtime::Message::new(
                    owner.instance(),
                    owner.generation(),
                    Dynamic::from(value),
                ))
            },
        );
        let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
        let mut requests = Vec::new();
        let mut delivered = Vec::new();
        iced_test::futures::futures::executor::block_on(async {
            // All three requests come before any of them runs.
            while requests.len() < 3 {
                if let Some(iced_test::runtime::Action::Output(message)) = stream.next().await {
                    assert!(message.resumption().is_some());
                    requests.push(message);
                }
            }
            for request in &requests {
                assert!(request.resumption().unwrap().run().is_none());
            }
            drop(requests);
            while let Some(action) = stream.next().await {
                if let iced_test::runtime::Action::Output(message) = action {
                    delivered.push(message.payload().as_int().unwrap());
                }
            }
        });
        assert_eq!(delivered, [1, 2, 3]);
    }

    fn owner() -> effect::Owner {
        let mut engine = rawr::Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = rawr::Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(include_str!(
                "../examples/scripts/application_counter.rs"
            ))
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = rawr::host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let launch = launch.bind(registry).unwrap();
        effect::Owner::new(rawr::application::Instance::new(), launch.generation())
    }
}
