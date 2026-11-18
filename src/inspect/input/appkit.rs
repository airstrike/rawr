//! AppKit's side of an input: on the main thread, each event is built as
//! an `NSEvent` and handed to the window's view through the responder
//! method the window would call for it (`mouseDown:`, `keyDown:`,
//! `scrollWheel:`, …). Calling the view's method, not the window's
//! `sendEvent:`, keeps the window from making itself key or bringing its
//! app forward. Nothing is posted to the window server.
//!
//! A capture of a window (#1989) is AppKit's too: the window draws a frame
//! through its own renderer, read back from the Metal layer it drew into
//! (see [`capture`]).
//!
//! AppKit, QuartzCore, Metal and CoreGraphics are found in the process at run
//! time: a program that shows a window has loaded them, and one that
//! hasn't shows no window.

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Once, OnceLock};
use std::time::Duration;

use super::travel::Travel;
use super::{Button, Error, Gesture, Key, Modifiers, Point, Sent, Unit};
use crate::inspect::screenshot;

pub(in crate::inspect) type Id = *mut c_void;
pub(in crate::inspect) type Sel = *const c_void;

/// The name the agent cursor's layer goes by among the view's layers.
const CURSOR: &CStr = c"rawr-agent-cursor";

/// About one frame at 60 Hz: the compositor shows a drawn frame by then.
const FRAME: Duration = Duration::from_millis(17);

/// How often a wait for the main thread looks whether it is given up.
const POLL: Duration = Duration::from_millis(50);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Vector {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Rect {
    origin: Vector,
    size: Size,
}

/// One event of a gesture, in the order the window takes them.
#[derive(Clone, Debug)]
enum Event {
    Pointer {
        motion: Motion,
        at: Point,
        button: Button,
        clicks: isize,
    },
    Scroll {
        at: Point,
        x: f64,
        y: f64,
        unit: Unit,
    },
    Key {
        down: bool,
        key: Key,
        flags: usize,
    },
    /// A modifier key goes down or up: `flags` are those held after it.
    Flags {
        code: u16,
        flags: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Motion {
    Moved,
    Down,
    Up,
    Dragged,
}

/// The functions an input calls, found in the process. rawr's menu bar
/// items call them too (#1990).
pub(in crate::inspect) struct Runtime {
    pub(in crate::inspect) send: unsafe extern "C" fn(),
    #[cfg(target_arch = "x86_64")]
    send_rect: unsafe extern "C" fn(),
    class: unsafe extern "C" fn(*const c_char) -> Id,
    selector: unsafe extern "C" fn(*const c_char) -> Sel,
    pub(in crate::inspect) push: unsafe extern "C" fn() -> *mut c_void,
    pub(in crate::inspect) pop: unsafe extern "C" fn(*mut c_void),
    release: unsafe extern "C" fn(*const c_void),
    scroll: unsafe extern "C" fn(*const c_void, u32, u32, i32, i32, i32) -> *mut c_void,
    mouse: unsafe extern "C" fn(*const c_void, u32, Vector, u32) -> *mut c_void,
    locate: unsafe extern "C" fn(*mut c_void, Vector),
    field: unsafe extern "C" fn(*mut c_void, u32, i64),
    display: unsafe extern "C" fn() -> u32,
    bounds: unsafe extern "C" fn(u32) -> Rect,
    path: unsafe extern "C" fn() -> *mut c_void,
    move_to: unsafe extern "C" fn(*mut c_void, *const c_void, f64, f64),
    arc_to: unsafe extern "C" fn(*mut c_void, *const c_void, f64, f64, f64, f64, f64),
    close: unsafe extern "C" fn(*mut c_void),
    color: unsafe extern "C" fn(f64, f64, f64, f64) -> *mut c_void,
    method: unsafe extern "C" fn(Id, Sel) -> *mut c_void,
    implementation: unsafe extern "C" fn(*mut c_void) -> *const c_void,
    implement: unsafe extern "C" fn(*mut c_void, *const c_void) -> *const c_void,
    retain: unsafe extern "C" fn(Id) -> Id,
    unretain: unsafe extern "C" fn(Id),
    bitmap:
        unsafe extern "C" fn(*mut u8, usize, usize, usize, usize, *mut c_void, u32) -> *mut c_void,
    space: unsafe extern "C" fn(*const c_void) -> *mut c_void,
    /// The address of `kCGColorSpaceSRGB`, the sRGB colour space's name.
    srgb: usize,
    translate: unsafe extern "C" fn(*mut c_void, f64, f64),
    scale: unsafe extern "C" fn(*mut c_void, f64, f64),
}

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dispatch_async_f(
        queue: *const c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
    static _dispatch_main_q: u8;
}

/// `dlsym`'s handle for every image the process has loaded.
const LOADED: *mut c_void = -2_isize as *mut c_void;

/// Send `$selector` to `$receiver` with arguments of the given C types,
/// for a result of type `$result`.
macro_rules! msg {
    ($runtime:expr, $result:ty; $receiver:expr, $selector:literal $(, $argument:expr => $kind:ty)* $(,)?) => {{
        let runtime: &$crate::inspect::input::appkit::Runtime = $runtime;
        let receiver: $crate::inspect::input::appkit::Id = $receiver;
        let selector = runtime.selector(concat!($selector, "\0"));
        // SAFETY: `objc_msgSend` calls the receiver's method for the
        // selector with the arguments as they are passed, so called through
        // the method's own C signature it is that call. Every selector here
        // is called with the types AppKit declares for it, and a nil
        // receiver answers zero.
        // An argument may be a `msg!` of its own.
        #[allow(unused_unsafe)]
        let result = unsafe {
            let function: unsafe extern "C" fn(
                $crate::inspect::input::appkit::Id,
                $crate::inspect::input::appkit::Sel
                $(, $kind)*
            ) -> $result =
                std::mem::transmute(runtime.send);
            function(receiver, selector $(, $argument)*)
        };
        result
    }};
}
pub(in crate::inspect) use msg;

impl Runtime {
    /// The functions, once AppKit is loaded: `None` before.
    pub(in crate::inspect) fn get() -> Option<&'static Self> {
        static RUNTIME: OnceLock<Runtime> = OnceLock::new();
        if let Some(runtime) = RUNTIME.get() {
            return Some(runtime);
        }
        let runtime = Self::find()?;
        Some(RUNTIME.get_or_init(|| runtime))
    }

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
                send_rect: function(c"objc_msgSend_stret")?,
                class: function(c"objc_getClass")?,
                selector: function(c"sel_registerName")?,
                push: function(c"objc_autoreleasePoolPush")?,
                pop: function(c"objc_autoreleasePoolPop")?,
                release: function(c"CFRelease")?,
                scroll: function(c"CGEventCreateScrollWheelEvent2")?,
                mouse: function(c"CGEventCreateMouseEvent")?,
                locate: function(c"CGEventSetLocation")?,
                field: function(c"CGEventSetIntegerValueField")?,
                display: function(c"CGMainDisplayID")?,
                bounds: function(c"CGDisplayBounds")?,
                path: function(c"CGPathCreateMutable")?,
                move_to: function(c"CGPathMoveToPoint")?,
                arc_to: function(c"CGPathAddArcToPoint")?,
                close: function(c"CGPathCloseSubpath")?,
                color: function(c"CGColorCreateSRGB")?,
                method: function(c"class_getInstanceMethod")?,
                implementation: function(c"method_getImplementation")?,
                implement: function(c"method_setImplementation")?,
                retain: function(c"objc_retain")?,
                unretain: function(c"objc_release")?,
                bitmap: function(c"CGBitmapContextCreate")?,
                space: function(c"CGColorSpaceCreateWithName")?,
                srgb: {
                    let name = symbol(c"kCGColorSpaceSRGB");
                    if name.is_null() {
                        return None;
                    }
                    name as usize
                },
                translate: function(c"CGContextTranslateCTM")?,
                scale: function(c"CGContextScaleCTM")?,
            })
        }
    }

    pub(in crate::inspect) fn selector(&self, name: &'static str) -> Sel {
        // SAFETY: `name` ends in NUL, as `msg!` writes it.
        unsafe { (self.selector)(name.as_ptr().cast()) }
    }

    pub(in crate::inspect) fn class(&self, name: &CStr) -> Id {
        // SAFETY: `objc_getClass` reads a NUL-terminated name.
        unsafe { (self.class)(name.as_ptr()) }
    }

    /// The application, or nil before the program made one.
    pub(in crate::inspect) fn application(&self) -> Id {
        let variable = symbol(c"NSApp").cast::<Id>();
        if variable.is_null() {
            return std::ptr::null_mut();
        }
        // SAFETY: `NSApp` is AppKit's global holding the shared
        // application, read on the main thread that sets it.
        unsafe { variable.read() }
    }

    /// A method's `NSRect` result.
    fn rect(&self, receiver: Id, selector: &'static str) -> Rect {
        let selector = self.selector(selector);
        #[cfg(target_arch = "x86_64")]
        {
            let mut rect = Rect::default();
            // SAFETY: on x86_64 a 32-byte struct comes back through
            // `objc_msgSend_stret`, into the pointer it is given first.
            unsafe {
                let function: unsafe extern "C" fn(*mut Rect, Id, Sel) =
                    std::mem::transmute(self.send_rect);
                function(&mut rect, receiver, selector);
            }
            rect
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            // SAFETY: elsewhere `objc_msgSend` returns the struct as the
            // method does.
            unsafe {
                let function: unsafe extern "C" fn(Id, Sel) -> Rect =
                    std::mem::transmute(self.send);
                function(receiver, selector)
            }
        }
    }

    pub(in crate::inspect) fn string(&self, text: &str) -> Id {
        let text = CString::new(text.replace('\0', "")).unwrap_or_default();
        msg!(self, Id; self.class(c"NSString"), "stringWithUTF8String:", text.as_ptr() => *const c_char)
    }

    pub(in crate::inspect) fn text(&self, string: Id) -> String {
        let characters = msg!(self, *const c_char; string, "UTF8String");
        if characters.is_null() {
            return String::new();
        }
        // SAFETY: `UTF8String` is NUL-terminated and lives as long as the
        // autorelease pool this runs in.
        unsafe { CStr::from_ptr(characters) }
            .to_string_lossy()
            .into_owned()
    }

    fn release(&self, object: *const c_void) {
        if !object.is_null() {
            // SAFETY: `object` is a Core Foundation object this module made
            // and holds once.
            unsafe { (self.release)(object) }
        }
    }
}

/// Send `gesture` to the window titled `title`, or the program's front
/// window, waiting for the program's first window, each event and the
/// frame drawn after them, until `interrupted` says to give up.
pub(super) fn send(
    gesture: &Gesture,
    title: Option<&str>,
    interrupted: &dyn Fn() -> bool,
) -> Result<Sent, Error> {
    let missing = || Error::Window {
        title: title.map(str::to_owned),
    };
    let Some(runtime) = arrived(interrupted) else {
        return Err(missing());
    };
    let runtime = runtime?;
    let owned = title.map(str::to_owned);
    let (rest, size) =
        main(interrupted, move || resting(runtime, owned.as_deref()))?.ok_or_else(missing)?;
    let mut sent = None;
    for Turn { events, pause } in turns(events(gesture, rest, size.width, size.height)) {
        let title = title.map(str::to_owned);
        sent = Some(
            main(interrupted, move || {
                let mut sent = None;
                for event in &events {
                    sent = Some(deliver(runtime, event, title.as_deref())?);
                }
                sent
            })?
            .ok_or_else(missing)?,
        );
        if !pause.is_zero() {
            std::thread::sleep(pause);
        }
    }
    // A redraw the events asked for runs before the loop waits again: two
    // more turns of it see one drawn, and the compositor shows it a frame
    // later.
    main(interrupted, || ())?;
    main(interrupted, || ())?;
    std::thread::sleep(FRAME);
    sent.ok_or_else(missing)
}

/// Whether the program has shown a window since it started.
static SHOWN: AtomicBool = AtomicBool::new(false);

/// AppKit's functions once the program's first window has come, `None` in a
/// program without AppKit, which shows no window, or why the wait was given
/// up (#1947). A program that shows a window opens it some turns of its
/// event loop after it starts, its executor's threads taking part, so input
/// sent in between waits for that window, looking each [`POLL`]. Once one
/// was shown, a program with none takes no input, and the input says so at
/// once.
fn arrived(interrupted: &dyn Fn() -> bool) -> Option<Result<&'static Runtime, Error>> {
    let runtime = Runtime::get()?;
    while !SHOWN.load(Ordering::Acquire) {
        match main(interrupted, move || window(runtime, None).is_some()) {
            Ok(true) => SHOWN.store(true, Ordering::Release),
            Ok(false) if interrupted() => return Some(Err(Error::Interrupted)),
            Ok(false) => std::thread::sleep(POLL),
            Err(error) => return Some(Err(error)),
        }
    }
    Some(Ok(runtime))
}

/// An event and how long the cursor rests after it, before the next.
#[derive(Clone, Debug)]
struct Timed {
    event: Event,
    pause: Duration,
}

impl From<Event> for Timed {
    fn from(event: Event) -> Self {
        Self {
            event,
            pause: Duration::ZERO,
        }
    }
}

/// Events the window takes in one turn of its event loop, and how long the
/// cursor rests after them.
#[derive(Clone, Debug)]
struct Turn {
    events: Vec<Event>,
    pause: Duration,
}

/// `events` in the turns the window takes them in (#1415). Key events that
/// follow one another with no rest go in one turn, as keys typed faster
/// than the window draws reach it: a text of any length takes one turn,
/// not two for each character, each waiting on the window's next frame.
/// Each pointer event has a turn of its own, so the window draws the
/// cursor and what follows it at each point.
fn turns(events: Vec<Timed>) -> Vec<Turn> {
    let keyed = |event: &Event| matches!(event, Event::Key { .. } | Event::Flags { .. });
    let mut turns: Vec<Turn> = Vec::new();
    for Timed { event, pause } in events {
        match turns.last_mut() {
            Some(last)
                if last.pause.is_zero()
                    && keyed(&event)
                    && last.events.last().is_some_and(keyed) =>
            {
                last.events.push(event);
                last.pause = pause;
            }
            _ => turns.push(Turn {
                events: vec![event],
                pause,
            }),
        }
    }
    turns
}

/// The events `gesture` is, in order, in a window `width` by `height`
/// points whose agent cursor rests at `rest`, if it shows one. A pointer
/// gesture first moves the pointer along the cursor's travel to its point
/// (see [`Travel`]), a pointer move at each point of it, so what follows
/// the pointer follows it there; a drag moves with its button held along
/// a straight travel to its end.
fn events(gesture: &Gesture, rest: Option<Point>, width: f64, height: f64) -> Vec<Timed> {
    let pointer = |motion, at, button, clicks| Event::Pointer {
        motion,
        at,
        button,
        clicks,
    };
    let travel = |travel: Travel, motion, button, clicks| {
        let pause = travel.pause();
        travel.points().map(move |at| Timed {
            event: pointer(motion, at, button, clicks),
            pause,
        })
    };
    let approach = |to: Point, button| -> Vec<Timed> {
        match rest {
            Some(from) => travel(
                Travel::curved(from, to, width, height),
                Motion::Moved,
                button,
                0,
            )
            .collect(),
            None => vec![pointer(Motion::Moved, to, button, 0).into()],
        }
    };
    match gesture {
        Gesture::Move(at) => approach(*at, Button::Left),
        Gesture::Click { at, button, count } => {
            let mut events = approach(*at, *button);
            for click in 1..=*count as isize {
                events.push(pointer(Motion::Down, *at, *button, click).into());
                events.push(pointer(Motion::Up, *at, *button, click).into());
            }
            events
        }
        Gesture::Drag { from, to, button } => {
            let mut events = approach(*from, *button);
            events.push(pointer(Motion::Down, *from, *button, 1).into());
            events.extend(travel(
                Travel::straight(*from, *to),
                Motion::Dragged,
                *button,
                1,
            ));
            events.push(pointer(Motion::Up, *to, *button, 1).into());
            events
        }
        Gesture::Scroll { at, x, y, unit } => {
            let mut events = approach(*at, Button::Left);
            events.push(
                Event::Scroll {
                    at: *at,
                    x: *x,
                    y: *y,
                    unit: *unit,
                }
                .into(),
            );
            events
        }
        Gesture::Press { key, modifiers } => {
            let held = modifiers.held();
            let mut events = Vec::new();
            let mut flags = 0;
            for (code, flag) in &held {
                flags |= flag;
                events.push(Event::Flags { code: *code, flags });
            }
            events.push(Event::Key {
                down: true,
                key: key.clone(),
                flags,
            });
            events.push(Event::Key {
                down: false,
                key: key.clone(),
                flags,
            });
            for (code, flag) in held.iter().rev() {
                flags &= !flag;
                events.push(Event::Flags { code: *code, flags });
            }
            events.into_iter().map(Timed::from).collect()
        }
        Gesture::Type(text) => text
            .chars()
            .flat_map(|character| {
                let key = Key::typing(character);
                [
                    Event::Key {
                        down: true,
                        key: key.clone(),
                        flags: 0,
                    },
                    Event::Key {
                        down: false,
                        key,
                        flags: 0,
                    },
                ]
            })
            .map(Timed::from)
            .collect(),
    }
}

impl Modifiers {
    /// Each modifier held: its key code and its `NSEventModifierFlags` bit.
    fn held(self) -> Vec<(u16, usize)> {
        [
            (self.shift, 56, 1 << 17),
            (self.control, 59, 1 << 18),
            (self.alt, 58, 1 << 19),
            (self.command, 55, 1 << 20),
        ]
        .into_iter()
        .filter(|(held, _, _)| *held)
        .map(|(_, code, flag)| (code, flag))
        .collect()
    }
}

/// Run `work` on the main thread, where AppKit's views take events, and
/// wait for it, looking at each [`POLL`] whether `interrupted` gives it up.
pub(in crate::inspect) fn main<T: Send + 'static>(
    interrupted: &dyn Fn() -> bool,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Error> {
    unsafe extern "C" fn run(context: *mut c_void) {
        // SAFETY: `context` is the box `main` leaked for this one call.
        let work = unsafe { Box::from_raw(context.cast::<Box<dyn FnOnce() + Send>>()) };
        work();
    }
    let (done, finished) = mpsc::channel();
    let work: Box<dyn FnOnce() + Send> = Box::new(move || {
        let _ = done.send(work());
    });
    let context = Box::into_raw(Box::new(work)).cast::<c_void>();
    // SAFETY: the main queue runs `run` once with `context`, which owns
    // the work; a reply after the wait was given up goes to a dropped
    // receiver.
    unsafe { dispatch_async_f((&raw const _dispatch_main_q).cast(), context, run) };
    loop {
        match finished.recv_timeout(POLL) {
            Ok(value) => return Ok(value),
            Err(mpsc::RecvTimeoutError::Timeout) if interrupted() => {
                return Err(Error::Interrupted);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Error::Ended),
        }
    }
}

/// Hand `event` to the window titled `title`, or the front one: what it
/// reached, or `None` when there is no such window.
fn deliver(runtime: &Runtime, event: &Event, title: Option<&str>) -> Option<Sent> {
    // SAFETY: the pool is pushed and popped on this thread around the
    // objects this call makes.
    let pool = unsafe { (runtime.push)() };
    let sent = window(runtime, title).map(|window| {
        let frame = runtime.rect(window, "frame\0");
        // The window's own coordinates start at its bottom-left corner.
        let base = |at: Point| Vector {
            x: at.x,
            y: frame.size.height - at.y,
        };
        let content = msg!(runtime, Id; window, "contentView");
        let (target, selector, made, at) = match event {
            Event::Scroll { at, x, y, unit } => (
                hit(runtime, content, base(*at)),
                "scrollWheel:\0",
                scroll(runtime, base(*at), *x, *y, *unit),
                Some(*at),
            ),
            Event::Pointer {
                motion,
                at,
                button,
                clicks,
            } => {
                let (kind, selector) = pointer(*motion, *button);
                let made = mouse(runtime, kind, *button, base(*at), *clicks);
                (hit(runtime, content, base(*at)), selector, made, Some(*at))
            }
            Event::Key { down, key, flags } => {
                let characters = key.characters.as_deref().unwrap_or_default();
                let (kind, selector) = if *down {
                    (10, "keyDown:\0")
                } else {
                    (11, "keyUp:\0")
                };
                let made = keyboard(runtime, window, kind, *flags, characters, key.code);
                (responder(runtime, window, content), selector, made, None)
            }
            Event::Flags { code, flags } => {
                let made = keyboard(runtime, window, 12, *flags, "", *code);
                (
                    responder(runtime, window, content),
                    "flagsChanged:\0",
                    made,
                    None,
                )
            }
        };
        if !made.is_null() {
            let selector = runtime.selector(selector);
            // SAFETY: each selector is an `NSResponder` event method,
            // which takes the event and returns nothing.
            unsafe {
                let function: unsafe extern "C" fn(Id, Sel, Id) = std::mem::transmute(runtime.send);
                function(target, selector, made);
            }
        }
        if let Some(at) = at {
            point(runtime, window, content, base(at));
        }
        Sent {
            title: runtime.text(msg!(runtime, Id; window, "title")),
            width: frame.size.width,
            height: frame.size.height,
            cursor: at,
        }
    });
    // SAFETY: `pool` was pushed above on this thread.
    unsafe { (runtime.pop)(pool) };
    sent
}

/// Where the agent cursor rests in the window titled `title`, or the
/// front one, if it shows one, and the window's size; `None` when there is
/// no such window.
fn resting(runtime: &Runtime, title: Option<&str>) -> Option<(Option<Point>, Size)> {
    // SAFETY: the pool is pushed and popped on this thread around the
    // objects this call makes.
    let pool = unsafe { (runtime.push)() };
    let found = window(runtime, title).map(|window| {
        let frame = runtime.rect(window, "frame\0");
        let content = msg!(runtime, Id; window, "contentView");
        let layer = msg!(runtime, Id; content, "layer");
        let rest = named(runtime, layer, CURSOR).map(|cursor| {
            let position = msg!(runtime, Vector; cursor, "position");
            let at = placed(runtime, content, layer, position);
            let base = msg!(runtime, Vector; content, "convertPoint:toView:", at => Vector, std::ptr::null_mut() => Id);
            Point {
                x: base.x,
                y: frame.size.height - base.y,
            }
        });
        (rest, frame.size)
    });
    // SAFETY: `pool` was pushed above on this thread.
    unsafe { (runtime.pop)(pool) };
    found
}

/// The window titled `title`, or the front one: a visible window with a
/// view.
pub(in crate::inspect) fn window(runtime: &Runtime, title: Option<&str>) -> Option<Id> {
    windows(runtime).into_iter().find(|&window| {
        title.is_none_or(|title| runtime.text(msg!(runtime, Id; window, "title")) == title)
    })
}

/// The program's visible windows with a view, front first.
fn windows(runtime: &Runtime) -> Vec<Id> {
    let application = runtime.application();
    if application.is_null() {
        return Vec::new();
    }
    let windows = msg!(runtime, Id; application, "orderedWindows");
    let count = msg!(runtime, usize; windows, "count");
    (0..count)
        .map(|index| msg!(runtime, Id; windows, "objectAtIndex:", index => usize))
        .filter(|&window| msg!(runtime, u8; window, "isVisible") != 0)
        .filter(|&window| !msg!(runtime, Id; window, "contentView").is_null())
        .collect()
}

/// The view a pointer event at `base` goes to: the deepest under it, as the
/// window finds it, else the content view.
fn hit(runtime: &Runtime, content: Id, base: Vector) -> Id {
    let parent = msg!(runtime, Id; content, "superview");
    let probe = if parent.is_null() {
        base
    } else {
        msg!(runtime, Vector; parent, "convertPoint:fromView:", base => Vector, std::ptr::null_mut() => Id)
    };
    let found = msg!(runtime, Id; content, "hitTest:", probe => Vector);
    if found.is_null() {
        content
    } else {
        found
    }
}

/// The responder a key event goes to: the window's first responder when a
/// view is, else the content view.
fn responder(runtime: &Runtime, window: Id, content: Id) -> Id {
    let first = msg!(runtime, Id; window, "firstResponder");
    if first.is_null() || first == window {
        content
    } else {
        first
    }
}

/// The `NSEventType` and the responder method of a pointer event.
fn pointer(motion: Motion, button: Button) -> (usize, &'static str) {
    match (motion, button) {
        (Motion::Moved, _) => (5, "mouseMoved:\0"),
        (Motion::Down, Button::Left) => (1, "mouseDown:\0"),
        (Motion::Up, Button::Left) => (2, "mouseUp:\0"),
        (Motion::Dragged, Button::Left) => (6, "mouseDragged:\0"),
        (Motion::Down, Button::Right) => (3, "rightMouseDown:\0"),
        (Motion::Up, Button::Right) => (4, "rightMouseUp:\0"),
        (Motion::Dragged, Button::Right) => (7, "rightMouseDragged:\0"),
        (Motion::Down, Button::Middle) => (25, "otherMouseDown:\0"),
        (Motion::Up, Button::Middle) => (26, "otherMouseUp:\0"),
        (Motion::Dragged, Button::Middle) => (27, "otherMouseDragged:\0"),
    }
}

/// A key event of `kind` (`NSEventType`) for `window`.
fn keyboard(
    runtime: &Runtime,
    window: Id,
    kind: usize,
    flags: usize,
    characters: &str,
    code: u16,
) -> Id {
    let characters = runtime.string(characters);
    msg!(runtime, Id;
        runtime.class(c"NSEvent"),
        "keyEventWithType:location:modifierFlags:timestamp:windowNumber:context:characters:charactersIgnoringModifiers:isARepeat:keyCode:",
        kind => usize,
        Vector::default() => Vector,
        flags => usize,
        uptime(runtime) => f64,
        if window.is_null() { 0 } else { msg!(runtime, isize; window, "windowNumber") } => isize,
        std::ptr::null_mut() => Id,
        characters => Id,
        characters => Id,
        false => bool,
        code => u16,
    )
}

/// A scroll event at `base` in its window. AppKit makes no scroll event
/// of its own: this one starts as a Core Graphics event (see [`cocoa`]).
fn scroll(runtime: &Runtime, base: Vector, x: f64, y: f64, unit: Unit) -> Id {
    let units = match unit {
        Unit::Pixel => 0,
        Unit::Line => 1,
    };
    let wheel = |amount: f64| {
        -(amount
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)
    };
    // SAFETY: Core Graphics makes a fresh event, which `cocoa` releases.
    let event = unsafe { (runtime.scroll)(std::ptr::null(), units, 2, wheel(y), wheel(x), 0) };
    if !event.is_null() && unit == Unit::Pixel {
        // SAFETY: kCGScrollWheelEventIsContinuous on the fresh event: a
        // trackpad's precise deltas.
        unsafe { (runtime.field)(event, 88, 1) };
    }
    cocoa(runtime, event, base)
}

/// A mouse event of `kind` (`NSEventType`, which Core Graphics numbers
/// alike) for `button` at `base` in its window. AppKit's own mouse events
/// give every button number 0, the left button's: this one starts as a
/// Core Graphics event (see [`cocoa`]), which carries its button.
fn mouse(runtime: &Runtime, kind: usize, button: Button, base: Vector, clicks: isize) -> Id {
    let number = match button {
        Button::Left => 0,
        Button::Right => 1,
        Button::Middle => 2,
    };
    // SAFETY: Core Graphics makes a fresh event, which `cocoa` releases; it
    // is placed there.
    let event =
        unsafe { (runtime.mouse)(std::ptr::null(), kind as u32, Vector::default(), number) };
    if !event.is_null() {
        // SAFETY: kCGMouseEventClickState on the fresh event.
        unsafe { (runtime.field)(event, 1, clicks as i64) };
    }
    cocoa(runtime, event, base)
}

/// AppKit's event for the Core Graphics `event`, which is never posted:
/// a window-less event, whose location in its window is where it is on
/// the screen, placed so that location is `base`. Releases `event`.
fn cocoa(runtime: &Runtime, event: *mut c_void, base: Vector) -> Id {
    if event.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: Core Graphics' screen bounds, and the location of the fresh
    // event, in its global coordinates, which run down from the top of
    // the main display.
    unsafe {
        let screen = (runtime.bounds)((runtime.display)());
        (runtime.locate)(
            event,
            Vector {
                x: base.x,
                y: screen.size.height - base.y,
            },
        );
    }
    let made =
        msg!(runtime, Id; runtime.class(c"NSEvent"), "eventWithCGEvent:", event => *mut c_void);
    runtime.release(event);
    made
}

/// Seconds since the system started, as events are stamped.
fn uptime(runtime: &Runtime) -> f64 {
    let info = msg!(runtime, Id; runtime.class(c"NSProcessInfo"), "processInfo");
    msg!(runtime, f64; info, "systemUptime")
}

/// Show the agent cursor at `base` in `window`, above everything `content`
/// draws: a layer of its own, which takes no events and changes no layout.
fn point(runtime: &Runtime, window: Id, content: Id, base: Vector) {
    let layer = msg!(runtime, Id; content, "layer");
    if layer.is_null() {
        return;
    }
    let at = msg!(runtime, Vector; content, "convertPoint:fromView:", base => Vector, std::ptr::null_mut() => Id);
    let position = placed(runtime, content, layer, at);
    let transaction = runtime.class(c"CATransaction");
    msg!(runtime, (); transaction, "begin");
    msg!(runtime, (); transaction, "setDisableActions:", true => bool);
    let cursor = named(runtime, layer, CURSOR).unwrap_or_else(|| cursor(runtime, window, layer));
    msg!(runtime, (); cursor, "setPosition:", position => Vector);
    msg!(runtime, (); transaction, "commit");
}

/// `at` in `content`'s coordinates as a position in its `layer`, or a
/// position in the layer back in the view's coordinates. The layer's
/// coordinates run down from the top when it is flipped, the view's when
/// it is, and when the two differ each is the other mirrored.
fn placed(runtime: &Runtime, content: Id, layer: Id, at: Vector) -> Vector {
    let down = msg!(runtime, u8; layer, "contentsAreFlipped") != 0;
    let flipped = msg!(runtime, u8; content, "isFlipped") != 0;
    if down == flipped {
        at
    } else {
        Vector {
            x: at.x,
            y: runtime.rect(content, "bounds\0").size.height - at.y,
        }
    }
}

/// The agent cursor's outline, its coordinates running down: a dart whose
/// left edge leans a little and whose base is notched, each corner with
/// the radius it is rounded by. [`tip`] says where its rounded tip is.
const ARROW: [(f64, f64, f64); 4] = [
    (0.0, 0.0, 1.6),
    (0.94, 17.98, 2.4),
    (5.49, 11.78, 2.0),
    (13.16, 12.28, 2.4),
];

/// How far the white outline reaches beyond the fill, in points.
const OUTLINE: f64 = 1.75;

/// Where the rounded tip of [`ARROW`] reaches. A rounded corner's arc meets
/// the corner's bisector short of the corner by the radius over the sine
/// of half the corner's angle, less the radius.
fn tip() -> Vector {
    let [(x, y, radius), (ax, ay, _), _, (bx, by, _)] = ARROW;
    let unit = |dx: f64, dy: f64| {
        let length = dx.hypot(dy);
        (dx / length, dy / length)
    };
    let (ux, uy) = unit(ax - x, ay - y);
    let (vx, vy) = unit(bx - x, by - y);
    let half = ((1.0 - (ux * vx + uy * vy)) / 2.0).sqrt();
    let (across, along) = unit(ux + vx, uy + vy);
    let short = radius / half - radius;
    Vector {
        x: x + across * short,
        y: y + along * short,
    }
}

/// The agent cursor's path: [`ARROW`] with its corners rounded, moved so
/// its rounded tip is at the origin, its coordinates running up unless
/// `down`. The caller releases it.
fn arrow(runtime: &Runtime, down: bool) -> *mut c_void {
    let sign = if down { 1.0 } else { -1.0 };
    let tip = tip();
    let corner = |index: usize| {
        let (x, y, radius) = ARROW[index % ARROW.len()];
        (x - tip.x, (y - tip.y) * sign, radius)
    };
    let (last_x, last_y, _) = corner(ARROW.len() - 1);
    let (first_x, first_y, _) = corner(0);
    // SAFETY: Core Graphics' path functions on the fresh path. It starts
    // in the middle of the last edge, and each corner is an arc tangent to
    // the edges into and out of it.
    unsafe {
        let path = (runtime.path)();
        (runtime.move_to)(
            path,
            std::ptr::null(),
            (last_x + first_x) / 2.0,
            (last_y + first_y) / 2.0,
        );
        for index in 0..ARROW.len() {
            let (x, y, radius) = corner(index);
            let (next_x, next_y, _) = corner(index + 1);
            (runtime.arc_to)(path, std::ptr::null(), x, y, next_x, next_y, radius);
        }
        (runtime.close)(path);
        path
    }
}

/// The agent cursor's layer in `layer`, made: an orange dart, its tip at
/// the layer's position, outlined in white and casting a soft shadow. The
/// outline is the same path filled and stroked white under the dart's own
/// fill, so all of the stroke's width shows outside the fill.
fn cursor(runtime: &Runtime, window: Id, layer: Id) -> Id {
    let make =
        || msg!(runtime, Id; msg!(runtime, Id; runtime.class(c"CAShapeLayer"), "alloc"), "init");
    let (cursor, fill) = (make(), make());
    let name = runtime.string(&CURSOR.to_string_lossy());
    msg!(runtime, (); cursor, "setName:", name => Id);
    // The layer's coordinates run down from the top when it is flipped.
    let down = msg!(runtime, u8; layer, "contentsAreFlipped") != 0;
    let path = arrow(runtime, down);
    // SAFETY: Core Graphics' colour function, making colours the layers
    // keep their own references to; this function releases its own.
    let colors = unsafe {
        [
            (runtime.color)(1.0, 1.0, 1.0, 1.0),
            (runtime.color)(0.97, 0.39, 0.04, 1.0),
            (runtime.color)(0.0, 0.0, 0.0, 1.0),
        ]
    };
    let [white, orange, shadow] = colors;
    let scale = msg!(runtime, f64; window, "backingScaleFactor");
    for shape in [cursor, fill] {
        msg!(runtime, (); shape, "setPath:", path => *mut c_void);
        msg!(runtime, (); shape, "setContentsScale:", scale => f64);
    }
    msg!(runtime, (); cursor, "setFillColor:", white => *mut c_void);
    msg!(runtime, (); cursor, "setStrokeColor:", white => *mut c_void);
    msg!(runtime, (); cursor, "setLineWidth:", 2.0 * OUTLINE => f64);
    msg!(runtime, (); cursor, "setLineJoin:", runtime.string("round") => Id);
    msg!(runtime, (); cursor, "setShadowColor:", shadow => *mut c_void);
    msg!(runtime, (); cursor, "setShadowOpacity:", 0.35_f32 => f32);
    msg!(runtime, (); cursor, "setShadowRadius:", 2.5_f64 => f64);
    let below = if down { 1.0 } else { -1.0 };
    msg!(runtime, (); cursor, "setShadowOffset:", Size { width: 0.0, height: below } => Size);
    msg!(runtime, (); fill, "setFillColor:", orange => *mut c_void);
    runtime.release(path);
    for color in colors {
        runtime.release(color);
    }
    msg!(runtime, (); cursor, "setZPosition:", 1.0e6_f64 => f64);
    msg!(runtime, (); cursor, "addSublayer:", fill => Id);
    msg!(runtime, (); fill, "release");
    msg!(runtime, (); layer, "addSublayer:", cursor => Id);
    msg!(runtime, (); cursor, "release");
    cursor
}

/// `NSWindowOcclusionStateVisible`.
const VISIBLE: usize = 1 << 1;

/// The Metal layer a capture is drawing, while it draws.
static ARMED: AtomicUsize = AtomicUsize::new(0);
/// The window a capture shows as visible, while it draws.
static FORCED: AtomicUsize = AtomicUsize::new(0);
/// The drawable the armed layer last handed out, retained.
static DRAWN: AtomicUsize = AtomicUsize::new(0);
/// `-[CAMetalLayer nextDrawable]`'s own implementation.
static NEXT_DRAWABLE: AtomicUsize = AtomicUsize::new(0);
/// `-[NSWindow occlusionState]`'s own implementation.
static OCCLUSION: AtomicUsize = AtomicUsize::new(0);

/// A frame the window drew for a capture: its window, its content view,
/// the Metal layer the content is drawn into and the drawable it was drawn
/// into, each retained but the view, and the layer's settings the capture
/// changed. Addresses, so the frame crosses to the main thread again.
struct Drawn {
    window: usize,
    content: usize,
    layer: usize,
    drawable: usize,
    framebuffer_only: bool,
    display_sync: bool,
}

/// An origin in a Metal texture.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Origin {
    x: usize,
    y: usize,
    z: usize,
}

/// A size in a Metal texture.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Extent {
    width: usize,
    height: usize,
    depth: usize,
}

/// A frame's pixels, read back: four bytes each, rows top first.
struct Pixels {
    bytes: Vec<u8>,
    width: usize,
    height: usize,
    /// Whether the bytes are blue, green, red, alpha, not red first.
    bgra: bool,
}

/// Capture the window titled `title`, or the program's front window, into
/// `file` (see [`screenshot`]): it draws a frame as it would for a redraw,
/// shown as visible while it does, and the drawable its renderer drew
/// into is read back, below the title bar its frame view draws.
pub(in crate::inspect) fn capture(
    title: Option<&str>,
    file: &Path,
    interrupted: &dyn Fn() -> bool,
) -> Result<screenshot::Shot, screenshot::Error> {
    let lost = |error: Error| match error {
        Error::Ended => screenshot::Error::Ended,
        _ => screenshot::Error::Interrupted,
    };
    let Some(runtime) = arrived(interrupted) else {
        return Err(screenshot::Error::Window {
            title: title.map(str::to_owned),
            shown: Vec::new(),
        });
    };
    let runtime = runtime.map_err(lost)?;
    let wanted = title.map(str::to_owned);
    let drawn = main(interrupted, move || draw(runtime, wanted.as_deref())).map_err(lost)??;
    // The renderer committed its frame as it drew; the GPU finishes it
    // within a frame.
    std::thread::sleep(FRAME);
    let file = file.to_owned();
    main(interrupted, move || compose(runtime, &drawn, &file)).map_err(lost)?
}

/// Draw a frame of the window titled `title`, or the front one, with its
/// layer armed.
fn draw(runtime: &Runtime, title: Option<&str>) -> Result<Drawn, screenshot::Error> {
    // SAFETY: the pool is pushed and popped on this thread around the
    // objects this call makes.
    let pool = unsafe { (runtime.push)() };
    let drawn = window(runtime, title)
        .ok_or_else(|| screenshot::Error::Window {
            title: title.map(str::to_owned),
            shown: titles(runtime),
        })
        .and_then(|window| {
            let content = msg!(runtime, Id; window, "contentView");
            let layer = metal(runtime, msg!(runtime, Id; content, "layer"))
                .ok_or(screenshot::Error::Layer)?;
            hook(runtime);
            let framebuffer_only = msg!(runtime, u8; layer, "framebufferOnly") != 0;
            let display_sync = msg!(runtime, u8; layer, "displaySyncEnabled") != 0;
            // A drawable the capture reads back is more than a target, and
            // one shown at once is free again while the window is covered.
            msg!(runtime, (); layer, "setFramebufferOnly:", false => bool);
            msg!(runtime, (); layer, "setDisplaySyncEnabled:", false => bool);
            ARMED.store(layer as usize, Ordering::Release);
            FORCED.store(window as usize, Ordering::Release);
            let bounds = runtime.rect(content, "bounds\0");
            // The view's own redraw: the window's renderer draws its frame.
            msg!(runtime, (); content, "drawRect:", bounds => Rect);
            ARMED.store(0, Ordering::Release);
            FORCED.store(0, Ordering::Release);
            let drawable = DRAWN.swap(0, Ordering::AcqRel);
            let drawn = Drawn {
                // SAFETY: retaining live objects, which `restore` releases.
                window: unsafe { (runtime.retain)(window) } as usize,
                content: content as usize,
                // SAFETY: as for the window.
                layer: unsafe { (runtime.retain)(layer) } as usize,
                drawable,
                framebuffer_only,
                display_sync,
            };
            if drawable == 0 {
                restore(runtime, &drawn);
                return Err(screenshot::Error::Undrawn);
            }
            Ok(drawn)
        });
    // SAFETY: `pool` was pushed above on this thread.
    unsafe { (runtime.pop)(pool) };
    drawn
}

/// Read `drawn` back below its window's title bar, write the PNG to
/// `file` and give the layer back its settings.
fn compose(
    runtime: &Runtime,
    drawn: &Drawn,
    file: &Path,
) -> Result<screenshot::Shot, screenshot::Error> {
    // SAFETY: the pool is pushed and popped on this thread around the
    // objects this call makes.
    let pool = unsafe { (runtime.push)() };
    let shot = read(runtime, drawn).and_then(|pixels| {
        let window = drawn.window as Id;
        let content = drawn.content as Id;
        let frame = msg!(runtime, Id; content, "superview");
        let frame = if frame.is_null() { content } else { frame };
        let bounds = runtime.rect(frame, "bounds\0");
        let rep =
            msg!(runtime, Id; frame, "bitmapImageRepForCachingDisplayInRect:", bounds => Rect);
        if rep.is_null() {
            return Err(screenshot::Error::Write(
                "the window's frame drew no bitmap".to_owned(),
            ));
        }
        msg!(runtime, (); frame, "cacheDisplayInRect:toBitmapImageRep:", bounds => Rect, rep => Id);
        place(runtime, rep, frame, content, &pixels)?;
        cursor_over(runtime, rep, frame, content);
        let png = png(runtime, rep)?;
        std::fs::write(file, png).map_err(|error| screenshot::Error::Write(error.to_string()))?;
        let size = runtime.rect(window, "frame\0").size;
        Ok(screenshot::Shot {
            title: runtime.text(msg!(runtime, Id; window, "title")),
            width: size.width,
            height: size.height,
            scale: msg!(runtime, f64; window, "backingScaleFactor"),
            windows: titles(runtime),
        })
    });
    restore(runtime, drawn);
    // SAFETY: `pool` was pushed above on this thread.
    unsafe { (runtime.pop)(pool) };
    shot
}

/// Copy the drawable's texture into memory the CPU reads, on a queue of
/// the layer's own device.
fn read(runtime: &Runtime, drawn: &Drawn) -> Result<Pixels, screenshot::Error> {
    let texture = msg!(runtime, Id; drawn.drawable as Id, "texture");
    let format = msg!(runtime, usize; texture, "pixelFormat");
    // MTLPixelFormatRGBA8Unorm(_sRGB) and BGRA8Unorm(_sRGB).
    let bgra = match format {
        70 | 71 => false,
        80 | 81 => true,
        other => return Err(screenshot::Error::Format(other as u64)),
    };
    let width = msg!(runtime, usize; texture, "width");
    let height = msg!(runtime, usize; texture, "height");
    let row = width * 4;
    let device = msg!(runtime, Id; drawn.layer as Id, "device");
    let queue = msg!(runtime, Id; device, "newCommandQueue");
    // MTLResourceStorageModeShared.
    let buffer = msg!(runtime, Id; device, "newBufferWithLength:options:", row * height => usize, 0_usize => usize);
    if queue.is_null() || buffer.is_null() {
        // SAFETY: releasing what `new…` made, nil being ignored.
        unsafe {
            (runtime.unretain)(queue);
            (runtime.unretain)(buffer);
        }
        return Err(screenshot::Error::Write(
            "the GPU gave no buffer for the frame".to_owned(),
        ));
    }
    let commands = msg!(runtime, Id; queue, "commandBuffer");
    let blit = msg!(runtime, Id; commands, "blitCommandEncoder");
    msg!(runtime, (); blit,
        "copyFromTexture:sourceSlice:sourceLevel:sourceOrigin:sourceSize:toBuffer:destinationOffset:destinationBytesPerRow:destinationBytesPerImage:",
        texture => Id,
        0_usize => usize,
        0_usize => usize,
        Origin::default() => Origin,
        Extent { width, height, depth: 1 } => Extent,
        buffer => Id,
        0_usize => usize,
        row => usize,
        row * height => usize,
    );
    msg!(runtime, (); blit, "endEncoding");
    msg!(runtime, (); commands, "commit");
    msg!(runtime, (); commands, "waitUntilCompleted");
    let contents = msg!(runtime, *const u8; buffer, "contents");
    // SAFETY: the shared buffer holds `row * height` bytes, written by the
    // completed copy.
    let bytes = unsafe { std::slice::from_raw_parts(contents, row * height) }.to_vec();
    // SAFETY: releasing what `new…` made.
    unsafe {
        (runtime.unretain)(buffer);
        (runtime.unretain)(queue);
    }
    Ok(Pixels {
        bytes,
        width,
        height,
        bgra,
    })
}

/// Put `pixels` into `rep`, the frame view's bitmap, where the content
/// view is in it.
fn place(
    runtime: &Runtime,
    rep: Id,
    frame: Id,
    content: Id,
    pixels: &Pixels,
) -> Result<(), screenshot::Error> {
    let samples = msg!(runtime, isize; rep, "samplesPerPixel");
    let bits = msg!(runtime, isize; rep, "bitsPerPixel");
    let planar = msg!(runtime, u8; rep, "isPlanar") != 0;
    // NSBitmapFormatAlphaFirst.
    let alpha_first = msg!(runtime, usize; rep, "bitmapFormat") & 1 != 0;
    if samples != 4 || bits != 32 || planar || alpha_first {
        return Err(screenshot::Error::Write(format!(
            "the window's frame drew a bitmap of {samples} samples in {bits} bits a pixel"
        )));
    }
    let wide = msg!(runtime, isize; rep, "pixelsWide").max(0) as usize;
    let high = msg!(runtime, isize; rep, "pixelsHigh").max(0) as usize;
    let stride = msg!(runtime, isize; rep, "bytesPerRow").max(0) as usize;
    let data = msg!(runtime, *mut u8; rep, "bitmapData");
    if data.is_null() {
        return Err(screenshot::Error::Write(
            "the window's frame drew no pixels".to_owned(),
        ));
    }
    let (left, top) = offset(runtime, rep, frame, content);
    // SAFETY: the bitmap holds `stride * high` bytes, which this function
    // alone writes while it runs.
    let target = unsafe { std::slice::from_raw_parts_mut(data, stride * high) };
    let width = pixels.width.min(wide.saturating_sub(left));
    let height = pixels.height.min(high.saturating_sub(top));
    for y in 0..height {
        let from = &pixels.bytes[y * pixels.width * 4..][..width * 4];
        let to = &mut target[(top + y) * stride + left * 4..][..width * 4];
        let (to, _) = to.as_chunks_mut::<4>();
        let (from, _) = from.as_chunks::<4>();
        for (to, &[first, green, third, _]) in to.iter_mut().zip(from) {
            let (red, blue) = if pixels.bgra {
                (third, first)
            } else {
                (first, third)
            };
            *to = [red, green, blue, 255];
        }
    }
    Ok(())
}

/// Where the content view's top-left corner is in `rep`, in pixels.
fn offset(runtime: &Runtime, rep: Id, frame: Id, content: Id) -> (usize, usize) {
    let bounds = runtime.rect(frame, "bounds\0");
    let placed = runtime.rect(content, "frame\0");
    let scale = scale(runtime, rep, bounds);
    let top = if msg!(runtime, u8; frame, "isFlipped") != 0 {
        placed.origin.y
    } else {
        bounds.size.height - placed.origin.y - placed.size.height
    };
    let pixels = |points: f64| (points * scale).round().max(0.0) as usize;
    (pixels(placed.origin.x), pixels(top))
}

/// Pixels per point in `rep`, a bitmap of `bounds`.
fn scale(runtime: &Runtime, rep: Id, bounds: Rect) -> f64 {
    let wide = msg!(runtime, isize; rep, "pixelsWide") as f64;
    if bounds.size.width > 0.0 {
        wide / bounds.size.width
    } else {
        1.0
    }
}

/// Draw the agent cursor, when the window shows one, into `rep` where it
/// is over the content view.
fn cursor_over(runtime: &Runtime, rep: Id, frame: Id, content: Id) {
    let layer = msg!(runtime, Id; content, "layer");
    let Some(cursor) = named(runtime, layer, CURSOR) else {
        return;
    };
    let wide = msg!(runtime, isize; rep, "pixelsWide").max(0) as usize;
    let high = msg!(runtime, isize; rep, "pixelsHigh").max(0) as usize;
    let stride = msg!(runtime, isize; rep, "bytesPerRow").max(0) as usize;
    let data = msg!(runtime, *mut u8; rep, "bitmapData");
    let bounds = runtime.rect(frame, "bounds\0");
    let scale = scale(runtime, rep, bounds);
    let (left, top) = offset(runtime, rep, frame, content);
    let height = runtime.rect(content, "bounds\0").size.height;
    let at = msg!(runtime, Vector; cursor, "position");
    // SAFETY: a bitmap context over the bitmap's own pixels, eight bits
    // to each of four premultiplied samples, alpha last, and the sRGB
    // space it is drawn in, both released here; `srgb` is the address of
    // Core Graphics' constant name.
    unsafe {
        let space = (runtime.space)(*(runtime.srgb as *const *const c_void));
        let context = (runtime.bitmap)(data, wide, high, 8, stride, space, 1);
        runtime.release(space);
        if context.is_null() {
            return;
        }
        // The context's origin is the bitmap's bottom-left corner.
        let bottom = high as f64 - top as f64 - height * scale;
        (runtime.translate)(context, left as f64, bottom);
        (runtime.scale)(context, scale, scale);
        if msg!(runtime, u8; layer, "contentsAreFlipped") != 0 {
            (runtime.translate)(context, 0.0, height);
            (runtime.scale)(context, 1.0, -1.0);
        }
        (runtime.translate)(context, at.x, at.y);
        msg!(runtime, (); cursor, "renderInContext:", context => *mut c_void);
        runtime.release(context);
    }
}

/// The PNG of `rep`.
fn png(runtime: &Runtime, rep: Id) -> Result<Vec<u8>, screenshot::Error> {
    let properties = msg!(runtime, Id; runtime.class(c"NSDictionary"), "dictionary");
    // NSBitmapImageFileTypePNG.
    let data = msg!(runtime, Id; rep, "representationUsingType:properties:", 4_usize => usize, properties => Id);
    if data.is_null() {
        return Err(screenshot::Error::Write(
            "AppKit made no PNG of the window".to_owned(),
        ));
    }
    let bytes = msg!(runtime, *const u8; data, "bytes");
    let length = msg!(runtime, usize; data, "length");
    // SAFETY: the data's own bytes, alive in this autorelease pool.
    Ok(unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec())
}

/// Give the layer back the settings `draw` changed and release what it
/// retained.
fn restore(runtime: &Runtime, drawn: &Drawn) {
    let layer = drawn.layer as Id;
    msg!(runtime, (); layer, "setFramebufferOnly:", drawn.framebuffer_only => bool);
    msg!(runtime, (); layer, "setDisplaySyncEnabled:", drawn.display_sync => bool);
    // SAFETY: releasing what `draw` and the hook retained, nil being
    // ignored.
    unsafe {
        (runtime.unretain)(drawn.drawable as Id);
        (runtime.unretain)(layer);
        (runtime.unretain)(drawn.window as Id);
    }
}

/// The titles of the program's windows, front first.
fn titles(runtime: &Runtime) -> Vec<String> {
    windows(runtime)
        .into_iter()
        .map(|window| runtime.text(msg!(runtime, Id; window, "title")))
        .collect()
}

/// The first Metal layer in `layer`'s tree, itself included.
fn metal(runtime: &Runtime, layer: Id) -> Option<Id> {
    let class = runtime.class(c"CAMetalLayer");
    if layer.is_null() || class.is_null() {
        return None;
    }
    let mut pending = vec![layer];
    while let Some(layer) = pending.pop() {
        if msg!(runtime, u8; layer, "isKindOfClass:", class => Id) != 0 {
            return Some(layer);
        }
        let sublayers = msg!(runtime, Id; layer, "sublayers");
        let count = msg!(runtime, usize; sublayers, "count");
        pending.extend(
            (0..count).map(|index| msg!(runtime, Id; sublayers, "objectAtIndex:", index => usize)),
        );
    }
    None
}

/// The sublayer of `layer` named `name`.
fn named(runtime: &Runtime, layer: Id, name: &CStr) -> Option<Id> {
    if layer.is_null() {
        return None;
    }
    let name = runtime.string(&name.to_string_lossy());
    let sublayers = msg!(runtime, Id; layer, "sublayers");
    let count = msg!(runtime, usize; sublayers, "count");
    (0..count)
        .map(|index| msg!(runtime, Id; sublayers, "objectAtIndex:", index => usize))
        .find(|&sublayer| {
            msg!(runtime, u8; msg!(runtime, Id; sublayer, "name"), "isEqualToString:", name => Id)
                != 0
        })
}

/// Put the capture's hooks in, once: `nextDrawable` keeps the armed
/// layer's drawable, and `occlusionState` says the forced window is
/// visible, so its renderer draws while it is covered. Neither changes
/// anything for a layer or a window no capture names.
fn hook(runtime: &Runtime) {
    static HOOKED: Once = Once::new();
    HOOKED.call_once(|| {
        replace(
            runtime,
            c"CAMetalLayer",
            "nextDrawable\0",
            &NEXT_DRAWABLE,
            next_drawable as *const c_void,
        );
        replace(
            runtime,
            c"NSWindow",
            "occlusionState\0",
            &OCCLUSION,
            occlusion_state as *const c_void,
        );
    });
}

/// Replace `class`'s method for `selector` with `with`, keeping its own
/// implementation in `original` first.
fn replace(
    runtime: &Runtime,
    class: &CStr,
    selector: &'static str,
    original: &AtomicUsize,
    with: *const c_void,
) {
    let class = runtime.class(class);
    if class.is_null() {
        return;
    }
    // SAFETY: the class's own method for the selector, whose
    // implementation is kept before `with`, of the same C signature,
    // takes its place.
    unsafe {
        let method = (runtime.method)(class, runtime.selector(selector));
        if method.is_null() {
            return;
        }
        original.store((runtime.implementation)(method) as usize, Ordering::Release);
        (runtime.implement)(method, with);
    }
}

/// `-[CAMetalLayer nextDrawable]` through the capture's hook.
unsafe extern "C" fn next_drawable(layer: Id, selector: Sel) -> Id {
    // SAFETY: `replace` kept the method's own implementation, of this
    // signature, before this one took its place.
    let drawable = unsafe {
        let own: unsafe extern "C" fn(Id, Sel) -> Id =
            std::mem::transmute(NEXT_DRAWABLE.load(Ordering::Acquire));
        own(layer, selector)
    };
    if !drawable.is_null()
        && ARMED.load(Ordering::Acquire) == layer as usize
        && let Some(runtime) = Runtime::get()
    {
        // SAFETY: the drawable is kept until the capture reads it, and the
        // one kept before is released.
        unsafe {
            let kept = (runtime.retain)(drawable);
            (runtime.unretain)(DRAWN.swap(kept as usize, Ordering::AcqRel) as Id);
        }
    }
    drawable
}

/// `-[NSWindow occlusionState]` through the capture's hook.
unsafe extern "C" fn occlusion_state(window: Id, selector: Sel) -> usize {
    // SAFETY: as in `next_drawable`.
    let state = unsafe {
        let own: unsafe extern "C" fn(Id, Sel) -> usize =
            std::mem::transmute(OCCLUSION.load(Ordering::Acquire));
        own(window, selector)
    };
    if FORCED.load(Ordering::Acquire) == window as usize {
        state | VISIBLE
    } else {
        state
    }
}

/// The C function `name` names, as a pointer of type `F`.
///
/// # Safety
///
/// `F` is the function's C signature.
pub(in crate::inspect) unsafe fn function<F: Copy>(name: &CStr) -> Option<F> {
    const {
        assert!(std::mem::size_of::<F>() == std::mem::size_of::<*mut c_void>());
    }
    let found = symbol(name);
    // SAFETY: a function pointer of the function's own signature, as the
    // caller promises, the size of the pointer `dlsym` found.
    (!found.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, F>(&found) })
}

pub(in crate::inspect) fn symbol(name: &CStr) -> *mut c_void {
    // SAFETY: `dlsym` reads a NUL-terminated name and looks it up in every
    // image the process loaded.
    unsafe { dlsym(LOADED, name.as_ptr()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
    }

    /// The runtime, with AppKit loaded as a program with a window has it.
    fn loaded() -> &'static Runtime {
        // SAFETY: loading a system framework by its path.
        let handle = unsafe {
            dlopen(
                c"/System/Library/Frameworks/AppKit.framework/AppKit".as_ptr(),
                1,
            )
        };
        assert!(!handle.is_null());
        Runtime::get().expect("AppKit is loaded")
    }

    #[test]
    fn each_button_keeps_its_number_and_every_pointer_event_lands_where_it_was_sent() {
        let runtime = loaded();
        // SAFETY: the pool is pushed and popped on this thread.
        let pool = unsafe { (runtime.push)() };
        let base = Vector { x: 40.0, y: 300.0 };
        for (button, number) in [(Button::Left, 0), (Button::Right, 1), (Button::Middle, 2)] {
            let (kind, _) = pointer(Motion::Down, button);
            let event = mouse(runtime, kind, button, base, 2);
            assert_eq!(msg!(runtime, usize; event, "type"), kind);
            assert_eq!(msg!(runtime, isize; event, "buttonNumber"), number);
            assert_eq!(msg!(runtime, isize; event, "clickCount"), 2);
            assert_eq!(msg!(runtime, Vector; event, "locationInWindow"), base);
            assert!(msg!(runtime, Id; event, "window").is_null());
        }
        let (kind, _) = pointer(Motion::Dragged, Button::Left);
        let event = mouse(runtime, kind, Button::Left, base, 1);
        assert_eq!(msg!(runtime, usize; event, "type"), kind);

        let event = scroll(runtime, base, 0.0, 3.0, Unit::Line);
        assert!(!event.is_null());
        assert_eq!(msg!(runtime, Vector; event, "locationInWindow"), base);
        assert!(msg!(runtime, Id; event, "window").is_null());
        // Toward the end is a negative delta, as a wheel turned toward the
        // person gives.
        assert!(msg!(runtime, f64; event, "scrollingDeltaY") < 0.0);
        assert_eq!(msg!(runtime, u8; event, "hasPreciseScrollingDeltas"), 0);
        let event = scroll(runtime, base, -12.0, 0.0, Unit::Pixel);
        assert!(msg!(runtime, f64; event, "scrollingDeltaX") > 0.0);
        assert_ne!(msg!(runtime, u8; event, "hasPreciseScrollingDeltas"), 0);

        let event = keyboard(runtime, std::ptr::null_mut(), 10, 1 << 20, "a", 0);
        assert_eq!(msg!(runtime, u16; event, "keyCode"), 0);
        assert_eq!(
            msg!(runtime, usize; event, "modifierFlags") & (1 << 20),
            1 << 20
        );
        assert_eq!(runtime.text(msg!(runtime, Id; event, "characters")), "a");
        // SAFETY: `pool` was pushed above on this thread.
        unsafe { (runtime.pop)(pool) };
    }

    #[test]
    fn a_held_modifier_goes_down_before_the_key_and_up_after_it() {
        let gesture = Gesture::Press {
            key: Key::named("a").unwrap(),
            modifiers: Modifiers {
                shift: true,
                command: true,
                ..Modifiers::default()
            },
        };
        let flags: Vec<(String, usize)> = events(&gesture, None, 100.0, 100.0)
            .iter()
            .map(|timed| match &timed.event {
                Event::Flags { code, flags } => (format!("flags {code}"), *flags),
                Event::Key { down, flags, .. } => (format!("key {down}"), *flags),
                other => panic!("{other:?}"),
            })
            .collect();
        let both = (1 << 17) | (1 << 20);
        assert_eq!(
            flags,
            [
                ("flags 56".to_owned(), 1 << 17),
                ("flags 55".to_owned(), both),
                ("key true".to_owned(), both),
                ("key false".to_owned(), both),
                ("flags 55".to_owned(), 1 << 17),
                ("flags 56".to_owned(), 0),
            ]
        );
    }

    /// Input sent before the program's first window waits for it instead
    /// of saying there is none (#1947): a test's main thread runs no event
    /// loop, so the wait goes on until the caller gives it up.
    #[test]
    fn input_before_the_first_window_waits_for_it() {
        let _ = loaded();
        let move_to = Gesture::Move(Point { x: 10.0, y: 10.0 });
        assert_eq!(send(&move_to, None, &|| true), Err(Error::Interrupted));
        assert!(!SHOWN.load(Ordering::Acquire));
    }

    /// Typed text reaches the window in one turn of its loop, whatever its
    /// length, and a pointer gesture in one turn for each event (#1415).
    #[test]
    fn typed_text_takes_one_turn_and_each_pointer_event_its_own() {
        let text: String = "abcdefghij".repeat(200);
        let typed = turns(events(&Gesture::Type(text), None, 100.0, 100.0));
        assert_eq!(typed.len(), 1);
        assert_eq!(typed[0].events.len(), 4000);
        let click = Gesture::Click {
            at: Point { x: 50.0, y: 50.0 },
            button: Button::Left,
            count: 1,
        };
        let pointed = events(&click, Some(Point { x: 0.0, y: 0.0 }), 100.0, 100.0);
        let count = pointed.len();
        let clicked = turns(pointed);
        assert_eq!(clicked.len(), count);
        assert!(clicked.iter().all(|turn| turn.events.len() == 1));
    }

    #[test]
    fn a_drag_moves_in_short_strides_with_its_button_held() {
        let gesture = Gesture::Drag {
            from: Point { x: 0.0, y: 0.0 },
            to: Point { x: 40.0, y: 0.0 },
            button: Button::Left,
        };
        let motions: Vec<Motion> = events(&gesture, None, 100.0, 100.0)
            .iter()
            .map(|timed| match &timed.event {
                Event::Pointer { motion, .. } => *motion,
                other => panic!("{other:?}"),
            })
            .collect();
        let (ends, held) = (motions.len() - 1, &motions[2..motions.len() - 1]);
        assert_eq!(motions[..2], [Motion::Moved, Motion::Down]);
        assert_eq!(motions[ends], Motion::Up);
        // 40 points in strides of at most 8.
        assert!(held.len() >= 5 && held.iter().all(|motion| *motion == Motion::Dragged));
    }
}
