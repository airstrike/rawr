//! Watcher implementation for Darwin's FSEvents API
//!
//! The FSEvents API provides a mechanism to notify clients about directories they ought to re-scan
//! in order to keep their internal data structures up-to-date with respect to the true state of
//! the file system. (For example, when files or directories are created, modified, or removed.) It
//! sends these notifications "in bulk", possibly notifying the client of changes to several
//! directories in a single callback.
//!
//! For more information see the [FSEvents API reference][ref].
//!
//! TODO: document event translation
//!
//! [ref]: https://developer.apple.com/library/mac/documentation/Darwin/Reference/FSEvents_Ref/

#![allow(non_upper_case_globals, dead_code)]

use crate::event::*;
use crate::{
    unbounded, Config, Error, EventHandler, PathsMut, RecursiveMode, Result, Sender, Watcher,
};
use fsevent_sys as fs;
use fsevent_sys::core_foundation as cf;
use std::collections::HashMap;
use std::ffi::CStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread;

mod drain;
mod lifecycle;

bitflags::bitflags! {
  #[repr(C)]
  #[derive(Debug)]
  struct StreamFlags: u32 {
    const NONE = fs::kFSEventStreamEventFlagNone;
    const MUST_SCAN_SUBDIRS = fs::kFSEventStreamEventFlagMustScanSubDirs;
    const USER_DROPPED = fs::kFSEventStreamEventFlagUserDropped;
    const KERNEL_DROPPED = fs::kFSEventStreamEventFlagKernelDropped;
    const IDS_WRAPPED = fs::kFSEventStreamEventFlagEventIdsWrapped;
    const HISTORY_DONE = fs::kFSEventStreamEventFlagHistoryDone;
    const ROOT_CHANGED = fs::kFSEventStreamEventFlagRootChanged;
    const MOUNT = fs::kFSEventStreamEventFlagMount;
    const UNMOUNT = fs::kFSEventStreamEventFlagUnmount;
    const ITEM_CREATED = fs::kFSEventStreamEventFlagItemCreated;
    const ITEM_REMOVED = fs::kFSEventStreamEventFlagItemRemoved;
    const INODE_META_MOD = fs::kFSEventStreamEventFlagItemInodeMetaMod;
    const ITEM_RENAMED = fs::kFSEventStreamEventFlagItemRenamed;
    const ITEM_MODIFIED = fs::kFSEventStreamEventFlagItemModified;
    const FINDER_INFO_MOD = fs::kFSEventStreamEventFlagItemFinderInfoMod;
    const ITEM_CHANGE_OWNER = fs::kFSEventStreamEventFlagItemChangeOwner;
    const ITEM_XATTR_MOD = fs::kFSEventStreamEventFlagItemXattrMod;
    const IS_FILE = fs::kFSEventStreamEventFlagItemIsFile;
    const IS_DIR = fs::kFSEventStreamEventFlagItemIsDir;
    const IS_SYMLINK = fs::kFSEventStreamEventFlagItemIsSymlink;
    const OWN_EVENT = fs::kFSEventStreamEventFlagOwnEvent;
    const IS_HARDLINK = fs::kFSEventStreamEventFlagItemIsHardlink;
    const IS_LAST_HARDLINK = fs::kFSEventStreamEventFlagItemIsLastHardlink;
    const ITEM_CLONED = fs::kFSEventStreamEventFlagItemCloned;
  }
}

/// FSEvents-based `Watcher` implementation
pub struct FsEventWatcher {
    paths: cf::CFMutableArrayRef,
    since_when: fs::FSEventStreamEventId,
    latency: cf::CFTimeInterval,
    flags: fs::FSEventStreamCreateFlags,
    event_handler: Arc<Mutex<dyn EventHandler>>,
    runloop: Option<(
        Arc<lifecycle::Stop>,
        Arc<drain::Delivery>,
        thread::JoinHandle<()>,
    )>,
    recursive_info: HashMap<PathBuf, bool>,
}

impl fmt::Debug for FsEventWatcher {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("FsEventWatcher")
            .field("paths", &self.paths)
            .field("since_when", &self.since_when)
            .field("latency", &self.latency)
            .field("flags", &self.flags)
            .field("event_handler", &Arc::as_ptr(&self.event_handler))
            .field("runloop", &self.runloop)
            .field("recursive_info", &self.recursive_info)
            .finish()
    }
}

// CFMutableArrayRef is a type alias to *mut libc::c_void, so FsEventWatcher is not Send/Sync
// automatically. It's Send because the pointer is not used in other threads.
unsafe impl Send for FsEventWatcher {}

// It's Sync because all methods that change the mutable state use `&mut self`.
unsafe impl Sync for FsEventWatcher {}

fn translate_flags(flags: StreamFlags, precise: bool) -> Vec<Event> {
    let mut evs = Vec::new();

    // «Denotes a sentinel event sent to mark the end of the "historical" events
    // sent as a result of specifying a `sinceWhen` value in the FSEvents.Create
    // call that created this event stream. After invoking the client's callback
    // with all the "historical" events that occurred before now, the client's
    // callback will be invoked with an event where the HistoryDone flag is set.
    // The client should ignore the path supplied in this callback.»
    // — https://www.mbsplugins.eu/FSEventsNextEvent.shtml
    //
    // As a result, we just stop processing here and return an empty vec, which
    // will ignore this completely and not emit any Events whatsoever.
    if flags.contains(StreamFlags::HISTORY_DONE) {
        return evs;
    }

    // FSEvents provides two possible hints as to why events were dropped,
    // however documentation on what those mean is scant, so we just pass them
    // through in the info attr field. The intent is clear enough, and the
    // additional information is provided if the user wants it.
    if flags.contains(StreamFlags::MUST_SCAN_SUBDIRS) {
        let e = Event::new(EventKind::Other).set_flag(Flag::Rescan);
        evs.push(if flags.contains(StreamFlags::USER_DROPPED) {
            e.set_info("rescan: user dropped")
        } else if flags.contains(StreamFlags::KERNEL_DROPPED) {
            e.set_info("rescan: kernel dropped")
        } else {
            e
        });
    }

    // In imprecise mode, let's not even bother parsing the kind of the event
    // except for the above very special events.
    if !precise {
        evs.push(Event::new(EventKind::Any));
        return evs;
    }

    // This is most likely a rename or a removal. We assume rename but may want
    // to figure out if it was a removal some way later (TODO). To denote the
    // special nature of the event, we add an info string.
    if flags.contains(StreamFlags::ROOT_CHANGED) {
        evs.push(
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
                .set_info("root changed"),
        );
    }

    // A path was mounted at the event path; we treat that as a create.
    if flags.contains(StreamFlags::MOUNT) {
        evs.push(Event::new(EventKind::Create(CreateKind::Other)).set_info("mount"));
    }

    // A path was unmounted at the event path; we treat that as a remove.
    if flags.contains(StreamFlags::UNMOUNT) {
        evs.push(Event::new(EventKind::Remove(RemoveKind::Other)).set_info("mount"));
    }

    if flags.contains(StreamFlags::ITEM_CREATED) {
        evs.push(if flags.contains(StreamFlags::IS_DIR) {
            Event::new(EventKind::Create(CreateKind::Folder))
        } else if flags.contains(StreamFlags::IS_FILE) {
            Event::new(EventKind::Create(CreateKind::File))
        } else {
            let e = Event::new(EventKind::Create(CreateKind::Other));
            if flags.contains(StreamFlags::IS_SYMLINK) {
                e.set_info("is: symlink")
            } else if flags.contains(StreamFlags::IS_HARDLINK) {
                e.set_info("is: hardlink")
            } else if flags.contains(StreamFlags::ITEM_CLONED) {
                e.set_info("is: clone")
            } else {
                Event::new(EventKind::Create(CreateKind::Any))
            }
        });
    }

    if flags.contains(StreamFlags::ITEM_REMOVED) {
        evs.push(if flags.contains(StreamFlags::IS_DIR) {
            Event::new(EventKind::Remove(RemoveKind::Folder))
        } else if flags.contains(StreamFlags::IS_FILE) {
            Event::new(EventKind::Remove(RemoveKind::File))
        } else {
            let e = Event::new(EventKind::Remove(RemoveKind::Other));
            if flags.contains(StreamFlags::IS_SYMLINK) {
                e.set_info("is: symlink")
            } else if flags.contains(StreamFlags::IS_HARDLINK) {
                e.set_info("is: hardlink")
            } else if flags.contains(StreamFlags::ITEM_CLONED) {
                e.set_info("is: clone")
            } else {
                Event::new(EventKind::Remove(RemoveKind::Any))
            }
        });
    }

    // FSEvents provides no mechanism to associate the old and new sides of a
    // rename event.
    if flags.contains(StreamFlags::ITEM_RENAMED) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Name(
            RenameMode::Any,
        ))));
    }

    // This is only described as "metadata changed", but it may be that it's
    // only emitted for some more precise subset of events... if so, will need
    // amending, but for now we have an Any-shaped bucket to put it in.
    if flags.contains(StreamFlags::INODE_META_MOD) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Any,
        ))));
    }

    if flags.contains(StreamFlags::FINDER_INFO_MOD) {
        evs.push(
            Event::new(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Other)))
                .set_info("meta: finder info"),
        );
    }

    if flags.contains(StreamFlags::ITEM_CHANGE_OWNER) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Ownership,
        ))));
    }

    if flags.contains(StreamFlags::ITEM_XATTR_MOD) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Extended,
        ))));
    }

    // This is specifically described as a data change, which we take to mean
    // is a content change.
    if flags.contains(StreamFlags::ITEM_MODIFIED) {
        evs.push(Event::new(EventKind::Modify(ModifyKind::Data(
            DataChange::Content,
        ))));
    }

    if flags.contains(StreamFlags::OWN_EVENT) {
        for ev in &mut evs {
            *ev = std::mem::take(ev).set_process_id(std::process::id());
        }
    }

    evs
}

struct StreamContextInfo {
    event_handler: Arc<Mutex<dyn EventHandler>>,
    recursive_info: HashMap<PathBuf, bool>,
    delivery: Arc<drain::Delivery>,
}

// Free the context when the stream created by `FSEventStreamCreate` is released.
extern "C" fn release_context(info: *const libc::c_void) {
    // Safety:
    // - The [documentation] for `FSEventStreamContext` states that `release` is only
    //   called when the stream is deallocated, so it is safe to convert `info` back into a
    //   box and drop it.
    //
    // [docs]: https://developer.apple.com/documentation/coreservices/fseventstreamcontext?language=objc
    unsafe {
        drop(Box::from_raw(
            info as *const StreamContextInfo as *mut StreamContextInfo,
        ));
    }
}

struct FsEventPathsMut<'a>(&'a mut FsEventWatcher);
impl<'a> FsEventPathsMut<'a> {
    fn new(watcher: &'a mut FsEventWatcher) -> Self {
        watcher.stop();
        Self(watcher)
    }
}
impl PathsMut for FsEventPathsMut<'_> {
    fn add(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        self.0.append_path(path, recursive_mode)
    }

    fn remove(&mut self, path: &Path) -> Result<()> {
        self.0.remove_path(path)
    }

    fn commit(self: Box<Self>) -> Result<()> {
        self.0.run()
    }
}

impl FsEventWatcher {
    fn from_event_handler(event_handler: Arc<Mutex<dyn EventHandler>>) -> Result<Self> {
        let paths = unsafe {
            cf::CFArrayCreateMutable(cf::kCFAllocatorDefault, 0, &cf::kCFTypeArrayCallBacks)
        };
        if paths.is_null() {
            return Err(Error::generic("unable to create FSEvent path array"));
        }
        Ok(FsEventWatcher {
            paths,
            since_when: fs::kFSEventStreamEventIdSinceNow,
            latency: 0.0,
            flags: fs::kFSEventStreamCreateFlagFileEvents | fs::kFSEventStreamCreateFlagNoDefer,
            event_handler,
            runloop: None,
            recursive_info: HashMap::new(),
        })
    }

    fn watch_inner(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        self.stop();
        let result = self.append_path(path, recursive_mode);
        self.run()?;
        result
    }

    fn unwatch_inner(&mut self, path: &Path) -> Result<()> {
        self.stop();
        let result = self.remove_path(path);
        self.run()?;
        result
    }

    #[inline]
    fn is_running(&self) -> bool {
        self.runloop.is_some()
    }

    fn stop(&mut self) {
        if !self.is_running() {
            return;
        }

        if let Some((stop, _, thread_handle)) = self.runloop.take() {
            stop.request();
            if thread_handle.join().is_err() {
                if let Ok(mut handler) = self.event_handler.lock() {
                    handler.handle_event(Err(Error::generic("watcher thread panicked")));
                }
            }
        }
    }

    fn remove_path(&mut self, path: &Path) -> Result<()> {
        let str_path = path.to_str().unwrap();
        unsafe {
            let mut err: cf::CFErrorRef = ptr::null_mut();
            let cf_path = cf::str_path_to_cfstring_ref(str_path, &mut err);
            if cf_path.is_null() {
                cf::CFRelease(err as cf::CFRef);
                return Err(Error::watch_not_found().add_path(path.into()));
            }

            let mut to_remove = Vec::new();
            for idx in 0..cf::CFArrayGetCount(self.paths) {
                let item = cf::CFArrayGetValueAtIndex(self.paths, idx);
                if cf::CFStringCompare(item, cf_path, cf::kCFCompareCaseInsensitive)
                    == cf::kCFCompareEqualTo
                {
                    to_remove.push(idx);
                }
            }

            cf::CFRelease(cf_path);

            for idx in to_remove.iter().rev() {
                cf::CFArrayRemoveValueAtIndex(self.paths, *idx);
            }
        }
        let p = if let Ok(canonicalized_path) = path.canonicalize() {
            canonicalized_path
        } else {
            path.to_owned()
        };
        match self.recursive_info.remove(&p) {
            Some(_) => Ok(()),
            None => Err(Error::watch_not_found()),
        }
    }

    // https://github.com/thibaudgg/rb-fsevent/blob/master/ext/fsevent_watch/main.c
    fn append_path(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        if !path.exists() {
            return Err(Error::path_not_found().add_path(path.into()));
        }
        let canonical_path = path.to_path_buf().canonicalize()?;
        let str_path = path.to_str().unwrap();
        unsafe {
            let mut err: cf::CFErrorRef = ptr::null_mut();
            let cf_path = cf::str_path_to_cfstring_ref(str_path, &mut err);
            if cf_path.is_null() {
                // Most likely the directory was deleted, or permissions changed,
                // while the above code was running.
                cf::CFRelease(err as cf::CFRef);
                return Err(Error::path_not_found().add_path(path.into()));
            }
            cf::CFArrayAppendValue(self.paths, cf_path);
            cf::CFRelease(cf_path);
        }
        self.recursive_info
            .insert(canonical_path, recursive_mode.is_recursive());
        Ok(())
    }

    /// Deliver all notifications queued before this handoff. Call only from
    /// the owner thread, never from an event handler. Failure is not readiness.
    pub fn drain(&mut self) -> Result<()> {
        match &self.runloop {
            Some((_, delivery, _)) => delivery.flush(),
            None if self.recursive_info.is_empty() => Ok(()),
            None => Err(Error::generic("watcher is not running during drain")),
        }
    }

    fn run(&mut self) -> Result<()> {
        self.run_with_start(|stream| unsafe { fs::FSEventStreamStart(stream) != 0 })
    }

    fn run_with_start(&mut self, start: fn(fs::FSEventStreamRef) -> bool) -> Result<()> {
        if unsafe { cf::CFArrayGetCount(self.paths) } == 0 {
            return Ok(());
        }

        // We need to associate the stream context with our callback in order to propagate events
        // to the rest of the system. This will be owned by the stream, and will be freed when the
        // stream is closed. This means we will leak the context if we panic before reaching
        // `FSEventStreamRelease`.
        let delivery = Arc::new(drain::Delivery::default());
        let context = Box::into_raw(Box::new(StreamContextInfo {
            event_handler: self.event_handler.clone(),
            recursive_info: self.recursive_info.clone(),
            delivery: Arc::clone(&delivery),
        }));

        let stream_context = fs::FSEventStreamContext {
            version: 0,
            info: context as *mut libc::c_void,
            retain: None,
            release: Some(release_context),
            copy_description: None,
        };

        let stream = unsafe {
            fs::FSEventStreamCreate(
                cf::kCFAllocatorDefault,
                callback,
                &stream_context,
                self.paths,
                self.since_when,
                self.latency,
                self.flags,
            )
        };

        if stream.is_null() {
            // Creation failed, so no stream owns the context or its release callback.
            unsafe {
                drop(Box::from_raw(context));
            }
            return Err(Error::generic("unable to create FSEvent stream"));
        }

        // Own the create reference across thread-spawn failure as well as normal
        // thread exit. A scheduled stream must be invalidated before this drops.
        struct Stream(fs::FSEventStreamRef);
        unsafe impl Send for Stream {}
        impl Drop for Stream {
            fn drop(&mut self) {
                unsafe { fs::FSEventStreamRelease(self.0) }
            }
        }
        let stream = Stream(stream);
        let (rl_tx, rl_rx) = unbounded();
        let handler = Arc::clone(&self.event_handler);
        let thread_delivery = Arc::clone(&delivery);
        let thread_handle = thread::Builder::new()
            .name("notify-rs fsevents loop".to_string())
            .spawn(move || {
                let _ = &stream; // Move the owning wrapper, not its raw field.
                let stop = match lifecycle::Stop::attach() {
                    Ok(stop) => stop,
                    Err(error) => {
                        let _ = rl_tx.send(Err(error));
                        return;
                    }
                };
                unsafe {
                    fs::FSEventStreamScheduleWithRunLoop(
                        stream.0,
                        cf::CFRunLoopGetCurrent(),
                        cf::kCFRunLoopDefaultMode,
                    );
                    if !start(stream.0) {
                        fs::FSEventStreamInvalidate(stream.0);
                        stop.detach();
                        let _ = rl_tx.send(Err(Error::generic("unable to start FSEvent stream")));
                        return;
                    }
                    thread_delivery.started(stream.0);
                    if rl_tx.send(Ok(Arc::clone(&stop))).is_ok() {
                        cf::CFRunLoopRun();
                        thread_delivery.stopped();
                        if !stop.requested() {
                            if let Ok(mut handler) = handler.lock() {
                                handler.handle_event(Err(Error::generic(
                                    "FSEvent run loop exited unexpectedly",
                                )));
                            }
                        }
                    }
                    thread_delivery.stopped();
                    fs::FSEventStreamStop(stream.0);
                    // Never purge the device journal: a successor stream may
                    // still need those events. Stop/invalidate/join own teardown.
                    fs::FSEventStreamInvalidate(stream.0);
                }
                stop.detach();
            })?;
        match rl_rx.recv() {
            Ok(Ok(stop)) => {
                self.runloop = Some((stop, delivery, thread_handle));
                Ok(())
            }
            result => {
                let _ = thread_handle.join();
                match result {
                    Ok(Err(error)) => Err(error),
                    _ => Err(Error::generic("watcher startup channel closed")),
                }
            }
        }
    }

    fn configure_raw_mode(&mut self, _config: Config, tx: Sender<Result<bool>>) {
        tx.send(Ok(false))
            .expect("configuration channel disconnect");
    }
}

extern "C" fn callback(
    stream_ref: fs::FSEventStreamRef,
    info: *mut libc::c_void,
    num_events: libc::size_t,                        // size_t numEvents
    event_paths: *mut libc::c_void,                  // void *eventPaths
    event_flags: *const fs::FSEventStreamEventFlags, // const FSEventStreamEventFlags eventFlags[]
    event_ids: *const fs::FSEventStreamEventId,      // const FSEventStreamEventId eventIds[]
) {
    unsafe {
        callback_impl(
            stream_ref,
            info,
            num_events,
            event_paths,
            event_flags,
            event_ids,
        )
    }
}

unsafe fn callback_impl(
    _stream_ref: fs::FSEventStreamRef,
    info: *mut libc::c_void,
    num_events: libc::size_t,                        // size_t numEvents
    event_paths: *mut libc::c_void,                  // void *eventPaths
    event_flags: *const fs::FSEventStreamEventFlags, // const FSEventStreamEventFlags eventFlags[]
    event_ids: *const fs::FSEventStreamEventId,      // const FSEventStreamEventId eventIds[]
) {
    let event_paths = event_paths as *const *const libc::c_char;
    let info = info as *const StreamContextInfo;
    let event_handler = &(*info).event_handler;

    let mut delivered = 0;
    for p in 0..num_events {
        delivered = delivered.max(*event_ids.add(p));
        let path = CStr::from_ptr(*event_paths.add(p))
            .to_str()
            .expect("Invalid UTF8 string.");
        let path = PathBuf::from(path);

        let flag = *event_flags.add(p);
        let flag = StreamFlags::from_bits(flag).unwrap_or_else(|| {
            panic!("Unable to decode StreamFlags: {}", flag);
        });

        let mut handle_event = false;
        for (p, r) in &(*info).recursive_info {
            if path.starts_with(p) {
                if *r || &path == p {
                    handle_event = true;
                    break;
                } else if let Some(parent_path) = path.parent() {
                    if parent_path == p {
                        handle_event = true;
                        break;
                    }
                }
            }
        }

        if !handle_event {
            continue;
        }

        log::trace!("FSEvent: path = `{}`, flag = {:?}", path.display(), flag);

        for ev in translate_flags(flag, true).into_iter() {
            // TODO: precise
            let ev = ev.add_path(path.clone());
            let mut event_handler = event_handler.lock().expect("lock not to be poisoned");
            event_handler.handle_event(Ok(ev));
        }
    }
    (*info).delivery.completed(delivered);
}

impl Watcher for FsEventWatcher {
    /// Create a new watcher.
    fn new<F: EventHandler>(event_handler: F, _config: Config) -> Result<Self> {
        Self::from_event_handler(Arc::new(Mutex::new(event_handler)))
    }

    fn watch(&mut self, path: &Path, recursive_mode: RecursiveMode) -> Result<()> {
        self.watch_inner(path, recursive_mode)
    }

    fn paths_mut<'me>(&'me mut self) -> Box<dyn PathsMut + 'me> {
        Box::new(FsEventPathsMut::new(self))
    }

    fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.unwatch_inner(path)
    }

    fn configure(&mut self, config: Config) -> Result<bool> {
        let (tx, rx) = unbounded();
        self.configure_raw_mode(config, tx);
        rx.recv()?
    }

    fn kind() -> crate::WatcherKind {
        crate::WatcherKind::Fsevent
    }
}

impl Drop for FsEventWatcher {
    fn drop(&mut self) {
        self.stop();
        unsafe {
            cf::CFRelease(self.paths);
        }
    }
}

#[test]
fn test_fsevent_watcher_drop() {
    use super::*;
    use std::time::Duration;

    let dir = tempfile::tempdir().unwrap();

    let (tx, rx) = std::sync::mpsc::channel();

    {
        let mut watcher = FsEventWatcher::new(tx, Default::default()).unwrap();
        watcher.watch(dir.path(), RecursiveMode::Recursive).unwrap();
        thread::sleep(Duration::from_millis(2000));
        println!("is running -> {}", watcher.is_running());

        thread::sleep(Duration::from_millis(1000));
        watcher.unwatch(dir.path()).unwrap();
        println!("is running -> {}", watcher.is_running());
    }

    thread::sleep(Duration::from_millis(1000));

    for res in rx {
        let e = res.unwrap();
        println!("debug => {:?} {:?}", e.kind, e.paths);
    }

    println!("in test: {} works", file!());
}

#[test]
fn test_steam_context_info_send_and_sync() {
    fn check_send<T: Send + Sync>() {}
    check_send::<StreamContextInfo>();
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn start_failure_is_returned_and_leaves_no_running_thread() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = FsEventWatcher::new(|_| {}, Config::default()).unwrap();
        watcher
            .append_path(dir.path(), RecursiveMode::Recursive)
            .unwrap();
        assert!(watcher.run_with_start(|_| false).is_err());
        assert!(!watcher.is_running());
        // Failed startup cleanup leaves the same configured watcher reusable.
        watcher.run().unwrap();
        assert!(watcher.is_running());
        watcher.stop();
        assert!(!watcher.is_running());
    }

    #[test]
    fn unexpected_loop_exit_reports_failure_and_can_be_joined() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = FsEventWatcher::new(tx, Config::default()).unwrap();
        watcher.watch(dir.path(), RecursiveMode::Recursive).unwrap();
        // Signal the real stop source without declaring intentional retirement.
        watcher.runloop.as_ref().unwrap().0.signal();
        let result = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(result.is_err());
        watcher.stop();
        assert!(!watcher.is_running());
    }

    #[test]
    fn drain_delivers_a_write_before_retirement() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("input.rs");
        std::fs::write(&file, "before").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = FsEventWatcher::new(tx, Config::default()).unwrap();
        watcher.latency = 60.0;
        watcher.watch(dir.path(), RecursiveMode::Recursive).unwrap();
        std::fs::write(&file, "after").unwrap();
        watcher.drain().unwrap();
        assert!(rx.try_iter().any(|event| event
            .unwrap()
            .paths
            .iter()
            .any(|path| path.ends_with("input.rs"))));
        watcher.stop();
    }

    #[test]
    fn successor_handoff_waits_for_the_incumbent_callback() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("queued.rs");
        std::fs::write(&file, "before").unwrap();
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let mut first = true;
        let mut incumbent = FsEventWatcher::new(
            move |event: Result<Event>| {
                if first
                    && event.as_ref().is_ok_and(|event| {
                        event.paths.iter().any(|path| path.ends_with("queued.rs"))
                    })
                {
                    first = false;
                    entered_tx.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(2)).unwrap();
                }
            },
            Config::default(),
        )
        .unwrap();
        incumbent
            .watch(dir.path(), RecursiveMode::Recursive)
            .unwrap();
        std::fs::write(&file, "after").unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut successor = FsEventWatcher::new(|_| {}, Config::default()).unwrap();
        successor
            .watch(dir.path(), RecursiveMode::Recursive)
            .unwrap();
        let (done_tx, done) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            incumbent.drain().unwrap();
            incumbent.stop();
            done_tx.send(()).unwrap();
        });
        assert!(done.recv_timeout(Duration::from_millis(20)).is_err());
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        successor.stop();
    }

    #[test]
    fn immediate_retirement_and_empty_batch_are_successful() {
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..8 {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut watcher = FsEventWatcher::new(tx, Config::default()).unwrap();
            watcher.watch(dir.path(), RecursiveMode::Recursive).unwrap();
            watcher.unwatch(dir.path()).unwrap();
            assert!(!watcher.is_running());
            assert!(rx.try_iter().all(|event| event.is_ok()));
        }
    }
}
