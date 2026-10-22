//! Delivery acknowledgement for the incumbent stream, never replay authority.
use crate::{Error, Result};
use fsevent_sys as fs;
use std::sync::{Condvar, Mutex};

#[derive(Debug, Default)]
pub(super) struct Delivery {
    state: Mutex<State>,
    changed: Condvar,
}
#[derive(Debug, Default)]
struct State {
    active: Option<Active>,
    delivered: fs::FSEventStreamEventId,
}
#[derive(Debug)]
struct Active(fs::FSEventStreamRef);
// Only used under Delivery.state. The owner clears it under that lock before
// stop/invalidation/release. FlushAsync is the cross-thread, nonblocking flush
// request; no callback is awaited while holding this native-lifecycle lock.
unsafe impl Send for Active {}

impl Delivery {
    pub(super) fn started(&self, stream: fs::FSEventStreamRef) {
        self.state.lock().unwrap().active = Some(Active(stream));
    }
    pub(super) fn completed(&self, event: fs::FSEventStreamEventId) {
        let mut state = self.state.lock().unwrap();
        state.delivered = state.delivered.max(event);
        self.changed.notify_all();
    }
    pub(super) fn stopped(&self) {
        self.state.lock().unwrap().active = None;
        self.changed.notify_all();
    }
    pub(super) fn flush(&self) -> Result<()> {
        self.flush_with(|stream| unsafe { fs::FSEventStreamFlushAsync(stream) })
    }
    fn flush_with(
        &self,
        request: impl FnOnce(fs::FSEventStreamRef) -> fs::FSEventStreamEventId,
    ) -> Result<()> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::generic("watcher delivery lock poisoned"))?;
        let stream = state
            .active
            .as_ref()
            .ok_or_else(|| Error::generic("watcher exited before drain"))?;
        // Apple FSEvents.h: returns the greatest event ID ever queued for this
        // stream. The callback acknowledges it only after every handler in that
        // batch has returned. Cleanup cannot invalidate the stream during this
        // short request because it also acquires state before native teardown.
        let target = request(stream.0);
        if target < state.delivered {
            return Err(Error::generic(
                "watcher event identity regressed during drain",
            ));
        }
        let state = self
            .changed
            .wait_while(state, |state| {
                state.active.is_some() && state.delivered < target
            })
            .map_err(|_| Error::generic("watcher delivery wait poisoned"))?;
        if state.active.is_none() {
            return Err(Error::generic("watcher exited during drain"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn drain_waits_for_handler_completion_or_backend_loss() {
        for lost in [false, true] {
            let delivery = Arc::new(Delivery::default());
            // This deterministic request closure never calls the native API.
            delivery.started(std::ptr::null_mut());
            let (requested, request) = mpsc::channel();
            let (finished, finish) = mpsc::channel();
            let worker_delivery = Arc::clone(&delivery);
            let worker = std::thread::spawn(move || {
                let result = worker_delivery.flush_with(|_| {
                    requested.send(()).unwrap();
                    7
                });
                finished.send(result).unwrap();
            });
            request.recv_timeout(Duration::from_secs(2)).unwrap();
            delivery.completed(6);
            assert!(finish.recv_timeout(Duration::from_millis(20)).is_err());
            if lost {
                delivery.stopped();
            } else {
                delivery.completed(7);
            }
            assert_eq!(
                finish
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .is_err(),
                lost
            );
            worker.join().unwrap();
            delivery.stopped();
        }
    }
}
