//! One retained, signaled stop source; no polling or borrowed loop handles.
use core_foundation_sys::{base, runloop as cf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct Stop {
    loop_ref: cf::CFRunLoopRef,
    source: cf::CFRunLoopSourceRef,
    requested: AtomicBool,
}

// CoreFoundation permits signaling/waking a run loop from another thread.
// Both native objects are retained for the full Arc lifetime. Only the owner
// thread adds/removes the source; other threads only signal it.
unsafe impl Send for Stop {}
unsafe impl Sync for Stop {}

extern "C" fn perform(_: *const std::ffi::c_void) {
    // This callback is invoked by CoreFoundation on the owning run-loop thread.
    unsafe { cf::CFRunLoopStop(cf::CFRunLoopGetCurrent()) }
}

impl Stop {
    pub(super) fn attach() -> crate::Result<Arc<Self>> {
        unsafe {
            let loop_ref = cf::CFRunLoopGetCurrent();
            let mut context = cf::CFRunLoopSourceContext {
                version: 0,
                info: std::ptr::null_mut(),
                retain: None,
                release: None,
                copyDescription: None,
                equal: None,
                hash: None,
                schedule: None,
                cancel: None,
                perform,
            };
            let source = cf::CFRunLoopSourceCreate(base::kCFAllocatorDefault, 0, &mut context);
            if source.is_null() {
                return Err(crate::Error::generic(
                    "unable to create watcher stop source",
                ));
            }
            base::CFRetain(loop_ref.cast());
            cf::CFRunLoopAddSource(loop_ref, source, cf::kCFRunLoopDefaultMode);
            Ok(Arc::new(Self {
                loop_ref,
                source,
                requested: AtomicBool::new(false),
            }))
        }
    }

    pub(super) fn request(&self) {
        self.requested.store(true, Ordering::Release);
        self.signal();
    }

    pub(super) fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    pub(super) fn signal(&self) {
        // A signaled source remains pending until the run loop handles it,
        // including when shutdown arrives before CFRunLoopRun is entered.
        unsafe {
            cf::CFRunLoopSourceSignal(self.source);
            cf::CFRunLoopWakeUp(self.loop_ref);
        }
    }

    pub(super) fn detach(&self) {
        // Called only on the owner thread after its run has ended, or before
        // entry when stream startup fails. Parent ownership retains both refs.
        unsafe { cf::CFRunLoopRemoveSource(self.loop_ref, self.source, cf::kCFRunLoopDefaultMode) }
    }
}

impl Drop for Stop {
    fn drop(&mut self) {
        // No callback references Rust context. The owner has removed the source
        // before its Arc is released; these are precisely our two owned refs.
        unsafe {
            base::CFRelease(self.source.cast());
            base::CFRelease(self.loop_ref.cast());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_signaled_before_loop_entry_is_not_lost() {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let stop = Stop::attach().unwrap();
            stop.request();
            unsafe { cf::CFRunLoopRun() };
            assert!(stop.requested());
            stop.detach();
            tx.send(()).unwrap();
        });
        rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }
}
