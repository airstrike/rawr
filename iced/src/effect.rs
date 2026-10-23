//! Native lifecycle ownership for lowered cancellable tasks.

use std::fmt;
use std::sync::Arc;

use rawr::{application, program};

use crate::runtime;

/// Exact host owner of one lowered task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Owner {
    instance: application::Instance,
    generation: program::GenerationId,
}

/// Unforgeable native acknowledgement that one lowered task finished.
#[derive(Clone)]
pub(crate) struct Completion {
    owner: Owner,
    stamp: Arc<()>,
}

/// Handles retained by one live application or preview owner.
#[derive(Default)]
pub(crate) struct Tasks {
    active: Vec<Active>,
}

struct Active {
    owner: Owner,
    stamp: Arc<()>,
    handle: iced::task::Handle,
}

impl Owner {
    pub(crate) const fn new(
        instance: application::Instance,
        generation: program::GenerationId,
    ) -> Self {
        Self {
            instance,
            generation,
        }
    }

    pub(crate) const fn instance(self) -> application::Instance {
        self.instance
    }

    pub(crate) const fn generation(self) -> program::GenerationId {
        self.generation
    }
}

impl Completion {
    pub(crate) const fn owner(&self) -> Owner {
        self.owner
    }
}

impl fmt::Debug for Completion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Completion(<private>)")
    }
}

impl Tasks {
    /// Retain one complete lowered task as a single cancellable unit.
    pub(crate) fn track(
        &mut self,
        owner: Owner,
        task: iced::Task<runtime::Message>,
    ) -> iced::Task<runtime::Message> {
        let stamp = Arc::new(());
        let (task, handle) = task.abortable();
        self.active.push(Active {
            owner,
            stamp: stamp.clone(),
            handle,
        });
        task.chain(iced::Task::done(runtime::Message::task_completed(
            Completion { owner, stamp },
        )))
    }

    /// Remove a finished handle only when its exact owner still matches.
    pub(crate) fn complete(&mut self, completion: &Completion) -> bool {
        let Some(index) = self.active.iter().position(|active| {
            active.owner == completion.owner && Arc::ptr_eq(&active.stamp, &completion.stamp)
        }) else {
            return false;
        };
        self.active.swap_remove(index);
        true
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.active.len()
    }

    #[cfg(test)]
    pub(crate) fn handles(&self) -> Vec<iced::task::Handle> {
        self.active
            .iter()
            .map(|active| active.handle.clone())
            .collect()
    }
}

impl Drop for Tasks {
    fn drop(&mut self) {
        for active in &self.active {
            active.handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use iced_test::futures::futures::StreamExt;
    use rawr::{application, host, Dynamic, Engine, Shared};

    use super::{Owner, Tasks};
    use crate::runtime;

    #[test]
    fn completion_follows_every_composite_output_and_removes_its_handle() {
        let owner = owner();
        let first =
            runtime::Message::new(owner.instance(), owner.generation(), Dynamic::from(1_i64));
        let second =
            runtime::Message::new(owner.instance(), owner.generation(), Dynamic::from(2_i64));
        let mut tasks = Tasks::default();
        let task = tasks.track(
            owner,
            iced::Task::batch([iced::Task::done(first), iced::Task::done(second)]),
        );

        assert_eq!(tasks.len(), 1);
        let messages = messages(task);
        assert_eq!(messages.len(), 3);
        assert!(messages[..2]
            .iter()
            .all(|message| message.task_completion().is_none()));
        let completion = messages[2]
            .task_completion()
            .expect("the chained final message must acknowledge completion");
        assert!(tasks.complete(completion));
        assert_eq!(tasks.len(), 0);
    }

    #[test]
    fn owner_retirement_aborts_every_retained_composite_handle() {
        let owner = owner();
        let mut tasks = Tasks::default();
        let _task = tasks.track(
            owner,
            iced::Task::perform(std::future::pending::<Dynamic>(), move |payload| {
                runtime::Message::new(owner.instance(), owner.generation(), payload)
            }),
        );
        let handles = tasks.handles();

        assert!(handles.iter().all(|handle| !handle.is_aborted()));
        drop(tasks);
        assert!(handles.iter().all(iced::task::Handle::is_aborted));
    }

    #[test]
    fn stale_and_cross_owner_completions_cannot_remove_fresh_tasks() {
        let first = owner();
        let second = owner();
        let mut first_tasks = Tasks::default();
        let mut second_tasks = Tasks::default();
        let first_task = first_tasks.track(
            first,
            iced::Task::done(runtime::Message::new(
                first.instance(),
                first.generation(),
                Dynamic::from(1_i64),
            )),
        );
        let _second_task = second_tasks.track(
            second,
            iced::Task::done(runtime::Message::new(
                second.instance(),
                second.generation(),
                Dynamic::from(2_i64),
            )),
        );
        let completion = messages(first_task)
            .pop()
            .and_then(|message| message.task_completion().cloned())
            .expect("the first task must finish with its exact completion");

        assert!(!second_tasks.complete(&completion));
        assert_eq!(second_tasks.len(), 1);
        assert!(first_tasks.complete(&completion));
        assert_eq!(first_tasks.len(), 0);

        let replacement = first_tasks.track(
            first,
            iced::Task::done(runtime::Message::new(
                first.instance(),
                first.generation(),
                Dynamic::from(3_i64),
            )),
        );
        assert!(!first_tasks.complete(&completion));
        assert_eq!(first_tasks.len(), 1);
        let replacement = messages(replacement)
            .pop()
            .and_then(|message| message.task_completion().cloned())
            .expect("the replacement task must retain a fresh completion stamp");
        assert!(first_tasks.complete(&replacement));
        assert_eq!(first_tasks.len(), 0);
        assert_eq!(format!("{replacement:?}"), "Completion(<private>)");
    }

    fn owner() -> Owner {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(include_str!("../examples/scripts/application_counter.rs"))
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let launch = launch.bind(registry).unwrap();
        Owner::new(application::Instance::new(), launch.generation())
    }

    fn messages(task: iced::Task<runtime::Message>) -> Vec<runtime::Message> {
        let mut stream = iced_test::runtime::task::into_stream(task)
            .expect("a tracked nonempty task must expose one native stream");
        iced_test::futures::futures::executor::block_on(async move {
            let mut messages = Vec::new();
            while let Some(action) = stream.next().await {
                match action {
                    iced_test::runtime::Action::Output(message) => {
                        messages.extend(crate::dispatch::inline(message));
                    }
                    action => panic!("expected a task output, found {action:?}"),
                }
            }
            messages
        })
    }
}
