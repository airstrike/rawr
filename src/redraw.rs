//! A redraw of the program's windows, asked for once a new generation is
//! offered (#1479): an idle event-driven program, as an iced app waiting
//! for its next event, makes no call into marked code until something wakes
//! its event loop, so the generation would wait for the person's next input.
//! Asking the platform to redraw every window the process shows wakes that
//! loop and runs the program's next frame, whose first call into marked
//! code admits the generation (D553.3), so the frame draws with it.
//!
//! Nothing here knows the program's framework. On macOS each window's
//! content view is marked as needing display, on the main thread, as AppKit
//! itself does when a window's contents change: winit hands that to the
//! program as `RedrawRequested`. A process that shows no window asks for
//! nothing, as a terminal program: its next call into marked code, at its
//! own next tick or input, admits the generation. Other platforms ask for
//! nothing yet, so there a generation waits for the program's next event.
//!
//! What a frame calls is the program's: iced draws its widgets again but
//! builds its view only after a message, so a redraw shows an edit of code
//! a draw calls (a style, a theme's colors, a canvas), and an edit of code
//! only `view` calls shows at the next message.

/// Ask the platform to redraw every window the process shows. Any thread
/// may call this; it returns before the redraw runs.
pub(crate) fn windows() {
    #[cfg(target_os = "macos")]
    appkit::windows();
}

#[cfg(target_os = "macos")]
mod appkit {
    //! AppKit's side: the functions are found in the process at run time, as
    //! a program that shows a window has loaded AppKit and one that shows
    //! none hasn't.

    use std::ffi::{c_char, c_void, CStr};
    use std::sync::OnceLock;

    type Id = *mut c_void;
    type Sel = *const c_void;

    /// `dlsym`'s handle for every image the process has loaded.
    const LOADED: *mut c_void = -2_isize as *mut c_void;

    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dispatch_async_f(
            queue: *const c_void,
            context: *mut c_void,
            work: unsafe extern "C" fn(*mut c_void),
        );
        static _dispatch_main_q: u8;
    }

    /// The Objective-C functions a redraw calls.
    struct Runtime {
        send: unsafe extern "C" fn(),
        selector: unsafe extern "C" fn(*const c_char) -> Sel,
        push: unsafe extern "C" fn() -> *mut c_void,
        pop: unsafe extern "C" fn(*mut c_void),
        /// AppKit's global holding the shared application.
        application: *const Id,
    }

    // SAFETY: the fields are functions and the address of a global, which
    // any thread may hold; the global is read only on the main thread.
    unsafe impl Send for Runtime {}
    // SAFETY: as above.
    unsafe impl Sync for Runtime {}

    impl Runtime {
        /// The functions, once AppKit is loaded: `None` before.
        fn get() -> Option<&'static Self> {
            static RUNTIME: OnceLock<Runtime> = OnceLock::new();
            if let Some(runtime) = RUNTIME.get() {
                return Some(runtime);
            }
            let runtime = Self::find()?;
            Some(RUNTIME.get_or_init(|| runtime))
        }

        fn find() -> Option<Self> {
            let application = symbol(c"NSApp").cast::<Id>().cast_const();
            if application.is_null() {
                return None;
            }
            // SAFETY: each field's type is the C signature of the function
            // its symbol names.
            unsafe {
                Some(Self {
                    send: function(c"objc_msgSend")?,
                    selector: function(c"sel_registerName")?,
                    push: function(c"objc_autoreleasePoolPush")?,
                    pop: function(c"objc_autoreleasePoolPop")?,
                    application,
                })
            }
        }

        /// `receiver`'s answer to `selector`, a method that takes no
        /// argument and answers an object or nil.
        fn object(&self, receiver: Id, selector: &CStr) -> Id {
            // SAFETY: `objc_msgSend` called through the method's own C
            // signature is that call, and a nil receiver answers nil.
            unsafe {
                let send: unsafe extern "C" fn(Id, Sel) -> Id = std::mem::transmute(self.send);
                send(receiver, (self.selector)(selector.as_ptr()))
            }
        }

        /// The number an `NSArray` holds.
        fn count(&self, array: Id) -> usize {
            // SAFETY: as in `object`, for `count`'s `NSUInteger` answer.
            unsafe {
                let send: unsafe extern "C" fn(Id, Sel) -> usize = std::mem::transmute(self.send);
                send(array, (self.selector)(c"count".as_ptr()))
            }
        }

        /// The object an `NSArray` holds at `index`, below its count.
        fn at(&self, array: Id, index: usize) -> Id {
            // SAFETY: as in `object`, for `objectAtIndex:`'s signature.
            unsafe {
                let send: unsafe extern "C" fn(Id, Sel, usize) -> Id =
                    std::mem::transmute(self.send);
                send(array, (self.selector)(c"objectAtIndex:".as_ptr()), index)
            }
        }

        /// Mark `view` as needing display.
        fn invalidate(&self, view: Id) {
            // SAFETY: as in `object`, for `setNeedsDisplay:`'s `BOOL`
            // argument; a nil view ignores it.
            unsafe {
                let send: unsafe extern "C" fn(Id, Sel, bool) = std::mem::transmute(self.send);
                send(view, (self.selector)(c"setNeedsDisplay:".as_ptr()), true);
            }
        }

        /// Mark the content view of each of the application's windows as
        /// needing display, on the main thread.
        fn redraw(&self) {
            // SAFETY: `NSApp` is read on the main thread, which sets it.
            let application = unsafe { self.application.read() };
            if application.is_null() {
                return;
            }
            // SAFETY: the pool is pushed and popped on this thread around
            // the objects this call reads.
            let pool = unsafe { (self.push)() };
            let windows = self.object(application, c"windows");
            for index in 0..self.count(windows) {
                let window = self.at(windows, index);
                self.invalidate(self.object(window, c"contentView"));
            }
            // SAFETY: `pool` is the one pushed above.
            unsafe { (self.pop)(pool) };
        }
    }

    /// Ask the main thread to redraw the application's windows, when
    /// AppKit is loaded.
    pub(super) fn windows() {
        unsafe extern "C" fn run(context: *mut c_void) {
            // SAFETY: `context` is the runtime `windows` passed, which
            // lives as long as the process.
            let runtime = unsafe { &*context.cast::<Runtime>() };
            runtime.redraw();
        }
        let Some(runtime) = Runtime::get() else {
            return;
        };
        let context = std::ptr::from_ref(runtime).cast_mut().cast::<c_void>();
        // SAFETY: the main queue runs `run` once with `context`, a static
        // runtime that it only reads.
        unsafe { dispatch_async_f((&raw const _dispatch_main_q).cast(), context, run) };
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
        // SAFETY: `dlsym` reads a NUL-terminated name and looks it up in
        // every image the process loaded.
        unsafe { dlsym(LOADED, name.as_ptr()) }
    }
}
