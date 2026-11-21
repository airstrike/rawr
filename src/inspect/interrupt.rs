//! What gives up a request the program answers later: its tool went away,
//! or, for one its event loop answers, the program paused, which holds that
//! loop. A wait for the answer blocks on a channel, which the answer ends,
//! or a wake registered here.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// One request's interruption: fired once, it runs each wake registered
/// on it.
#[derive(Default)]
pub struct Interrupt {
    state: Mutex<Fired>,
}

#[derive(Default)]
struct Fired {
    fired: bool,
    wakes: Vec<Box<dyn FnOnce() + Send>>,
}

impl Interrupt {
    fn state(&self) -> MutexGuard<'_, Fired> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Give the request up, waking each wait on it.
    pub fn fire(&self) {
        let wakes = {
            let mut state = self.state();
            if state.fired {
                return;
            }
            state.fired = true;
            std::mem::take(&mut state.wakes)
        };
        for wake in wakes {
            wake();
        }
    }

    /// Whether the request was given up.
    #[must_use]
    pub fn is_fired(&self) -> bool {
        self.state().fired
    }

    /// Run `wake` when the request is given up, at once if it was.
    pub fn on_fire(&self, wake: impl FnOnce() + Send + 'static) {
        let mut state = self.state();
        if state.fired {
            drop(state);
            wake();
        } else {
            state.wakes.push(Box::new(wake));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::Interrupt;

    #[test]
    fn a_fire_wakes_each_wait_once_and_a_late_wait_at_once() {
        let interrupt = Interrupt::default();
        let (send, woken) = mpsc::channel();
        let early = send.clone();
        interrupt.on_fire(move || early.send("early").unwrap());
        interrupt.fire();
        interrupt.fire();
        interrupt.on_fire(move || send.send("late").unwrap());
        assert_eq!(woken.iter().collect::<Vec<_>>(), ["early", "late"]);
        assert!(interrupt.is_fired());
    }
}
