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

use iced::futures::channel::oneshot;

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
