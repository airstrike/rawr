//! AppKit's side: an observer of every window's notifications, on the
//! main thread, that hands each window's first show, moves, resizes and
//! close to the run's session. The functions are found in the process at
//! run time, as a program that shows a window has loaded AppKit and one
//! that shows none hasn't.
//!
//! AppKit's own frame autosave (`setFrameAutosaveName:`) isn't used: it
//! keeps frames in the user's defaults, under `~/Library/Preferences`, and
//! restores them whatever frame the program asks for.

use super::record::{Frame, Record};
use super::session::{Session, Window};
use std::ffi::{c_char, c_void, CStr};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

type Id = *mut c_void;
type Sel = *const c_void;
type Class = *mut c_void;

/// `dlsym`'s handle for every image the process has loaded.
const LOADED: *mut c_void = -2_isize as *mut c_void;

/// The notifications after which a window that is now visible has shown,
/// or a window already shown has moved or resized.
const CHANGES: [&CStr; 6] = [
    c"NSWindowDidBecomeKeyNotification",
    c"NSWindowDidBecomeMainNotification",
    c"NSWindowDidChangeOcclusionStateNotification",
    c"NSWindowDidChangeScreenNotification",
    c"NSWindowDidMoveNotification",
    c"NSWindowDidResizeNotification",
];

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// The run's session and the file it writes.
struct Kept {
    session: Session,
    file: PathBuf,
}

static KEPT: OnceLock<Mutex<Kept>> = OnceLock::new();

/// A `CGRect`, as AppKit passes an `NSRect`.
#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl From<Rect> for Frame {
    fn from(rect: Rect) -> Self {
        Self::new(rect.x, rect.y, rect.width, rect.height)
    }
}

impl From<Frame> for Rect {
    fn from(frame: Frame) -> Self {
        Self {
            x: frame.x(),
            y: frame.y(),
            width: frame.width(),
            height: frame.height(),
        }
    }
}

/// The Objective-C functions the observer calls.
struct Runtime {
    send: unsafe extern "C" fn(),
    #[cfg(target_arch = "x86_64")]
    send_returning_struct: unsafe extern "C" fn(),
    selector: unsafe extern "C" fn(*const c_char) -> Sel,
    class: unsafe extern "C" fn(*const c_char) -> Class,
    allocate_class: unsafe extern "C" fn(Class, *const c_char, usize) -> Class,
    add_method: unsafe extern "C" fn(Class, Sel, unsafe extern "C" fn(), *const c_char) -> bool,
    register_class: unsafe extern "C" fn(Class),
    push: unsafe extern "C" fn() -> *mut c_void,
    pop: unsafe extern "C" fn(*mut c_void),
}

// SAFETY: the fields are functions, which any thread may hold.
unsafe impl Send for Runtime {}
// SAFETY: as above.
unsafe impl Sync for Runtime {}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

impl Runtime {
    /// The functions, once AppKit is loaded: `None` before.
    fn find() -> Option<Self> {
        if symbol(c"NSApp").is_null() {
            return None;
        }
        // SAFETY: each field's type is the C signature of the function its
        // symbol names.
        unsafe {
            Some(Self {
                send: function(c"objc_msgSend")?,
                #[cfg(target_arch = "x86_64")]
                send_returning_struct: function(c"objc_msgSend_stret")?,
                selector: function(c"sel_registerName")?,
                class: function(c"objc_getClass")?,
                allocate_class: function(c"objc_allocateClassPair")?,
                add_method: function(c"class_addMethod")?,
                register_class: function(c"objc_registerClassPair")?,
                push: function(c"objc_autoreleasePoolPush")?,
                pop: function(c"objc_autoreleasePoolPop")?,
            })
        }
    }

    fn sel(&self, name: &CStr) -> Sel {
        // SAFETY: `sel_registerName` reads a NUL-terminated name.
        unsafe { (self.selector)(name.as_ptr()) }
    }

    /// `receiver`'s answer to `selector`, a method that takes no argument.
    ///
    /// # Safety
    ///
    /// `R` is the C type of the method's answer, an object, a `BOOL` or an
    /// integer: never a struct.
    unsafe fn answer<R>(&self, receiver: Id, selector: &CStr) -> R {
        // SAFETY: `objc_msgSend` called through the method's own C
        // signature is that call, as the caller promises.
        unsafe {
            let send: unsafe extern "C" fn(Id, Sel) -> R = std::mem::transmute(self.send);
            send(receiver, self.sel(selector))
        }
    }

    /// `receiver`'s answer to `selector`, a method that takes `argument`.
    ///
    /// # Safety
    ///
    /// As [`Runtime::answer`], and `A` is the C type of the argument.
    unsafe fn answer_to<A, R>(&self, receiver: Id, selector: &CStr, argument: A) -> R {
        // SAFETY: as in `answer`.
        unsafe {
            let send: unsafe extern "C" fn(Id, Sel, A) -> R = std::mem::transmute(self.send);
            send(receiver, self.sel(selector), argument)
        }
    }

    fn object(&self, receiver: Id, selector: &CStr) -> Id {
        // SAFETY: the methods this module names answer objects; nil answers
        // nil.
        unsafe { self.answer(receiver, selector) }
    }

    fn yes(&self, receiver: Id, selector: &CStr) -> bool {
        // SAFETY: the methods this module names answer `BOOL`, a byte.
        unsafe { self.answer::<i8>(receiver, selector) != 0 }
    }

    /// The `frame` of a window or a screen.
    fn frame(&self, receiver: Id) -> Frame {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: on x86-64 a struct the size of `NSRect` is answered
        // through the memory its first argument points at.
        let rect = unsafe {
            let send: unsafe extern "C" fn(*mut Rect, Id, Sel) =
                std::mem::transmute(self.send_returning_struct);
            let mut rect = Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            };
            send(&raw mut rect, receiver, self.sel(c"frame"));
            rect
        };
        #[cfg(not(target_arch = "x86_64"))]
        // SAFETY: on arm64 `objc_msgSend` answers `NSRect`, four doubles,
        // in registers, as `frame`'s signature does.
        let rect = unsafe {
            let send: unsafe extern "C" fn(Id, Sel) -> Rect = std::mem::transmute(self.send);
            send(receiver, self.sel(c"frame"))
        };
        rect.into()
    }

    fn set_frame(&self, window: Id, frame: Frame) {
        // SAFETY: `setFrame:display:` takes an `NSRect` and a `BOOL`.
        unsafe {
            let send: unsafe extern "C" fn(Id, Sel, Rect, i8) = std::mem::transmute(self.send);
            send(window, self.sel(c"setFrame:display:"), frame.into(), 1);
        }
    }

    /// The objects an `NSArray` holds.
    fn objects(&self, array: Id) -> impl Iterator<Item = Id> + '_ {
        // SAFETY: `count` answers an `NSUInteger`.
        let count: usize = unsafe { self.answer(array, c"count") };
        (0..count).map(move |index| {
            // SAFETY: `objectAtIndex:` takes an `NSUInteger` below the
            // count and answers an object.
            unsafe { self.answer_to(array, c"objectAtIndex:", index) }
        })
    }

    /// An `NSString`'s text, when it has any.
    fn text(&self, string: Id) -> Option<String> {
        // SAFETY: `UTF8String` answers a NUL-terminated string that lives
        // as long as the autorelease pool the caller's notification runs
        // in, or nil.
        let text: *const c_char = unsafe { self.answer(string, c"UTF8String") };
        (!text.is_null()).then(|| {
            // SAFETY: as above, a non-null answer is a NUL-terminated string.
            unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned()
        })
    }

    /// Whether `window` is one the person places: not a panel, a sheet, a
    /// child such as a popover's, or one that can't be the main window, as
    /// a tooltip or a menu; and not one the program keeps the frame of
    /// itself through an autosave name.
    fn placed(&self, window: Id) -> bool {
        // SAFETY: `objc_getClass` reads a NUL-terminated name.
        let panel = unsafe { (self.class)(c"NSPanel".as_ptr()) };
        // SAFETY: `isKindOfClass:` takes a class and answers a `BOOL`.
        let is_panel =
            unsafe { self.answer_to::<Class, i8>(window, c"isKindOfClass:", panel) } != 0;
        let autosaved = self
            .text(self.object(window, c"frameAutosaveName"))
            .is_some_and(|name| !name.is_empty());
        !is_panel
            && !self.yes(window, c"isSheet")
            && self.object(window, c"parentWindow").is_null()
            && self.yes(window, c"canBecomeMainWindow")
            && !autosaved
    }

    /// The frame of each screen.
    fn screens(&self) -> Vec<Frame> {
        // SAFETY: `objc_getClass` reads a NUL-terminated name.
        let screen = unsafe { (self.class)(c"NSScreen".as_ptr()) };
        self.objects(self.object(screen, c"screens"))
            .map(|screen| self.frame(screen))
            .collect()
    }
}

/// Follow this process's windows and keep their frames in `file`, when
/// AppKit is loaded.
pub(super) fn keep(file: PathBuf) {
    let Some(runtime) = Runtime::find() else {
        return;
    };
    let runtime = RUNTIME.get_or_init(|| runtime);
    let session = Session::new(Record::read(&file));
    if KEPT.set(Mutex::new(Kept { session, file })).is_err() {
        return;
    }
    // SAFETY: the pool is pushed and popped on this thread around the
    // objects this call reads.
    let pool = unsafe { (runtime.push)() };
    observe(runtime);
    // SAFETY: `pool` is the one pushed above.
    unsafe { (runtime.pop)(pool) };
}

/// Register an observer of `CHANGES` and of each window's close with the
/// default notification center.
fn observe(runtime: &Runtime) {
    let Some(observer) = observer(runtime) else {
        return;
    };
    // SAFETY: `objc_getClass` reads a NUL-terminated name.
    let center = unsafe { (runtime.class)(c"NSNotificationCenter".as_ptr()) };
    let center = runtime.object(center, c"defaultCenter");
    let names = CHANGES
        .iter()
        .map(|name| (*name, c"rawrWindowChanged:"))
        .chain([(c"NSWindowWillCloseNotification", c"rawrWindowClosed:")]);
    for (name, selector) in names {
        // AppKit's notification names are `NSString *const` globals.
        let name = symbol(name).cast::<Id>();
        if name.is_null() {
            continue;
        }
        // SAFETY: `addObserver:selector:name:object:` takes an object, a
        // selector, the name's string and nil for every sender.
        unsafe {
            let send: unsafe extern "C" fn(Id, Sel, Id, Sel, Id, Id) =
                std::mem::transmute(runtime.send);
            send(
                center,
                runtime.sel(c"addObserver:selector:name:object:"),
                observer,
                runtime.sel(selector),
                name.read(),
                std::ptr::null_mut(),
            );
        }
    }
}

/// An object of a class this module defines, whose two methods take a
/// window's notifications. The notification center keeps it, unretained,
/// for the life of the process.
fn observer(runtime: &Runtime) -> Option<Id> {
    const NAME: &CStr = c"RawrWindowFrames";
    // SAFETY: each call reads NUL-terminated names; the class is defined
    // once, with methods whose C signature `v@:@` is `changed`'s and
    // `closed`'s, and registered before its object is made.
    unsafe {
        let base = (runtime.class)(c"NSObject".as_ptr());
        let class = (runtime.allocate_class)(base, NAME.as_ptr(), 0);
        if class.is_null() {
            return None;
        }
        let methods: [(&CStr, unsafe extern "C" fn(Id, Sel, Id)); 2] = [
            (c"rawrWindowChanged:", changed),
            (c"rawrWindowClosed:", closed),
        ];
        for (selector, method) in methods {
            let method = std::mem::transmute::<
                unsafe extern "C" fn(Id, Sel, Id),
                unsafe extern "C" fn(),
            >(method);
            (runtime.add_method)(class, runtime.sel(selector), method, c"v@:@".as_ptr());
        }
        (runtime.register_class)(class);
        let object = runtime.object(runtime.object(class, c"alloc"), c"init");
        (!object.is_null()).then_some(object)
    }
}

/// A window showed, moved or resized. The first notification of a window
/// that is visible is its first show: it is given the frame the person
/// left it in, when the session has one, after the session's lock is let
/// go, since the move it makes notifies again.
unsafe extern "C" fn changed(_observer: Id, _selector: Sel, notification: Id) {
    let (Some(runtime), Some(kept)) = (RUNTIME.get(), KEPT.get()) else {
        return;
    };
    let window = runtime.object(notification, c"object");
    if window.is_null() {
        return;
    }
    let handle = Window::new(window as usize);
    let restored = {
        let Ok(mut kept) = kept.lock() else {
            return;
        };
        let Kept { session, file } = &mut *kept;
        let frame = runtime.frame(window);
        let restored = if session.knows(handle) {
            session.moved(handle, frame);
            None
        } else if runtime.yes(window, c"isVisible") && runtime.placed(window) {
            let name = runtime.text(runtime.object(window, c"identifier"));
            session.show(handle, name.as_deref(), frame, &runtime.screens())
        } else {
            return;
        };
        let _ = session.record().write(file);
        restored
    };
    if let Some(frame) = restored {
        runtime.set_frame(window, frame);
    }
}

/// A window is closing.
unsafe extern "C" fn closed(_observer: Id, _selector: Sel, notification: Id) {
    let (Some(runtime), Some(kept)) = (RUNTIME.get(), KEPT.get()) else {
        return;
    };
    let window = runtime.object(notification, c"object");
    if let Ok(mut kept) = kept.lock() {
        kept.session.closed(Window::new(window as usize));
    }
}

/// # Safety
///
/// `F` is the C signature of the function `name` names.
unsafe fn function<F: Copy>(name: &CStr) -> Option<F> {
    const {
        assert!(std::mem::size_of::<F>() == std::mem::size_of::<*mut c_void>());
    }
    let found = symbol(name);
    // SAFETY: a function pointer of the function's own signature, as the
    // caller promises, the size of the pointer `dlsym` found.
    (!found.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, F>(&found) })
}

fn symbol(name: &CStr) -> *mut c_void {
    // SAFETY: `dlsym` reads a NUL-terminated name and looks it up in every
    // image the process loaded.
    unsafe { dlsym(LOADED, name.as_ptr()) }
}
