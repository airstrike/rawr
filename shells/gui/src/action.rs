//! What a panel's update gives its parent: an instruction for the parent
//! to act on, and a task for the runtime.

use iced::Task;

/// A panel's update: `instruction` for the parent, `task` for the runtime.
#[must_use = "an Action's task must reach the runtime and its instruction the parent"]
pub struct Action<I, M> {
    /// What the parent does next, if anything.
    pub instruction: Option<I>,
    /// Work the runtime runs, whose messages return to the panel.
    pub task: Task<M>,
}

impl<I, M> Action<I, M> {
    /// Nothing to do.
    pub fn none() -> Self {
        Self {
            instruction: None,
            task: Task::none(),
        }
    }

    /// Only an instruction for the parent.
    pub fn instruction(instruction: I) -> Self {
        Self {
            instruction: Some(instruction),
            task: Task::none(),
        }
    }

    /// Only a task.
    pub fn task(task: Task<M>) -> Self {
        Self {
            instruction: None,
            task,
        }
    }
}
