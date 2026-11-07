//! AppKit's side of an input: on the main thread, each event is built as
//! an `NSEvent` and handed to the window's view through the responder
//! method the window would call for it (`mouseDown:`, `keyDown:`,
//! `scrollWheel:`, …). Calling the view's method, not the window's
//! `sendEvent:`, keeps the window from making itself key or bringing its
//! app forward. Nothing is posted to the window server.
//!
//! AppKit, QuartzCore and CoreGraphics are found in the process at run
//! time: a program that shows a window has loaded them, and one that
//! hasn't shows no window.

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

use super::{Button, Error, Gesture, Key, Modifiers, Point, Sent, Unit};

type Id = *mut c_void;
type Sel = *const c_void;

/// The name the agent cursor's layer goes by among the view's layers.
const CURSOR: &CStr = c"rawr-agent-cursor";

/// About one frame at 60 Hz: the compositor shows a drawn frame by then.
const FRAME: Duration = Duration::from_millis(17);

/// How far a drag moves between two of its events, in points.
const STRIDE: f64 = 8.0;

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

/// The functions an input calls, found in the process.
struct Runtime {
    send: unsafe extern "C" fn(),
    #[cfg(target_arch = "x86_64")]
    send_rect: unsafe extern "C" fn(),
    class: unsafe extern "C" fn(*const c_char) -> Id,
    selector: unsafe extern "C" fn(*const c_char) -> Sel,
    push: unsafe extern "C" fn() -> *mut c_void,
    pop: unsafe extern "C" fn(*mut c_void),
    release: unsafe extern "C" fn(*const c_void),
    scroll: unsafe extern "C" fn(*const c_void, u32, u32, i32, i32, i32) -> *mut c_void,
    mouse: unsafe extern "C" fn(*const c_void, u32, Vector, u32) -> *mut c_void,
    locate: unsafe extern "C" fn(*mut c_void, Vector),
    field: unsafe extern "C" fn(*mut c_void, u32, i64),
    display: unsafe extern "C" fn() -> u32,
    bounds: unsafe extern "C" fn(u32) -> Rect,
    path: unsafe extern "C" fn() -> *mut c_void,
    move_to: unsafe extern "C" fn(*mut c_void, *const c_void, f64, f64),
    line_to: unsafe extern "C" fn(*mut c_void, *const c_void, f64, f64),
    close: unsafe extern "C" fn(*mut c_void),
    color: unsafe extern "C" fn(f64, f64, f64, f64) -> *mut c_void,
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
        let runtime: &Runtime = $runtime;
        let receiver: Id = $receiver;
        let selector = runtime.selector(concat!($selector, "\0"));
        // SAFETY: `objc_msgSend` calls the receiver's method for the
        // selector with the arguments as they are passed, so called through
        // the method's own C signature it is that call. Every selector here
        // is called with the types AppKit declares for it, and a nil
        // receiver answers zero.
        // An argument may be a `msg!` of its own.
        #[allow(unused_unsafe)]
        let result = unsafe {
            let function: unsafe extern "C" fn(Id, Sel $(, $kind)*) -> $result =
                std::mem::transmute(runtime.send);
            function(receiver, selector $(, $argument)*)
        };
        result
    }};
}

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
                line_to: function(c"CGPathAddLineToPoint")?,
                close: function(c"CGPathCloseSubpath")?,
                color: function(c"CGColorCreateSRGB")?,
            })
        }
    }

    fn selector(&self, name: &'static str) -> Sel {
        // SAFETY: `name` ends in NUL, as `msg!` writes it.
        unsafe { (self.selector)(name.as_ptr().cast()) }
    }

    fn class(&self, name: &CStr) -> Id {
        // SAFETY: `objc_getClass` reads a NUL-terminated name.
        unsafe { (self.class)(name.as_ptr()) }
    }

    /// The application, or nil before the program made one.
    fn application(&self) -> Id {
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

    fn string(&self, text: &str) -> Id {
        let text = CString::new(text.replace('\0', "")).unwrap_or_default();
        msg!(self, Id; self.class(c"NSString"), "stringWithUTF8String:", text.as_ptr() => *const c_char)
    }

    fn text(&self, string: Id) -> String {
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
/// window, waiting up to `timeout` for each event and the frame drawn after
/// them.
pub(super) fn send(
    gesture: &Gesture,
    title: Option<&str>,
    timeout: Duration,
) -> Result<Sent, Error> {
    let missing = || Error::Window {
        title: title.map(str::to_owned),
    };
    let runtime = Runtime::get().ok_or_else(missing)?;
    let deadline = Instant::now() + timeout;
    let mut sent = None;
    for event in events(gesture) {
        let title = title.map(str::to_owned);
        sent = Some(
            main(deadline, timeout, move || {
                deliver(runtime, &event, title.as_deref())
            })?
            .ok_or_else(missing)?,
        );
    }
    // A redraw the events asked for runs before the loop waits again: two
    // more turns of it see one drawn, and the compositor shows it a frame
    // later.
    main(deadline, timeout, || ())?;
    main(deadline, timeout, || ())?;
    std::thread::sleep(FRAME);
    sent.ok_or_else(missing)
}

/// The events `gesture` is, in order.
fn events(gesture: &Gesture) -> Vec<Event> {
    let pointer = |motion, at, button, clicks| Event::Pointer {
        motion,
        at,
        button,
        clicks,
    };
    match gesture {
        Gesture::Move(at) => vec![pointer(Motion::Moved, *at, Button::Left, 0)],
        Gesture::Click { at, button, count } => {
            let mut events = vec![pointer(Motion::Moved, *at, *button, 0)];
            for click in 1..=*count as isize {
                events.push(pointer(Motion::Down, *at, *button, click));
                events.push(pointer(Motion::Up, *at, *button, click));
            }
            events
        }
        Gesture::Drag { from, to, button } => {
            let distance = (to.x - from.x).hypot(to.y - from.y);
            let steps = (distance / STRIDE).ceil().clamp(1.0, 60.0) as u32;
            let mut events = vec![
                pointer(Motion::Moved, *from, *button, 0),
                pointer(Motion::Down, *from, *button, 1),
            ];
            for step in 1..=steps {
                let part = f64::from(step) / f64::from(steps);
                let at = Point {
                    x: from.x + (to.x - from.x) * part,
                    y: from.y + (to.y - from.y) * part,
                };
                events.push(pointer(Motion::Dragged, at, *button, 1));
            }
            events.push(pointer(Motion::Up, *to, *button, 1));
            events
        }
        Gesture::Scroll { at, x, y, unit } => vec![
            pointer(Motion::Moved, *at, Button::Left, 0),
            Event::Scroll {
                at: *at,
                x: *x,
                y: *y,
                unit: *unit,
            },
        ],
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
            events
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
/// wait for it until `deadline`.
fn main<T: Send + 'static>(
    deadline: Instant,
    timeout: Duration,
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
    // the work; a reply after the deadline goes to a dropped receiver.
    unsafe { dispatch_async_f((&raw const _dispatch_main_q).cast(), context, run) };
    finished
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| Error::Unreached(timeout))
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

/// The window titled `title`, or the front one: a visible window with a
/// view.
fn window(runtime: &Runtime, title: Option<&str>) -> Option<Id> {
    let application = runtime.application();
    if application.is_null() {
        return None;
    }
    let windows = msg!(runtime, Id; application, "orderedWindows");
    let count = msg!(runtime, usize; windows, "count");
    (0..count)
        .map(|index| msg!(runtime, Id; windows, "objectAtIndex:", index => usize))
        .filter(|&window| msg!(runtime, u8; window, "isVisible") != 0)
        .filter(|&window| !msg!(runtime, Id; window, "contentView").is_null())
        .find(|&window| {
            title.is_none_or(|title| runtime.text(msg!(runtime, Id; window, "title")) == title)
        })
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
    let bounds = runtime.rect(content, "bounds\0");
    // The layer's coordinates run down from the top when it is flipped.
    let down = msg!(runtime, u8; layer, "contentsAreFlipped") != 0;
    let flipped = msg!(runtime, u8; content, "isFlipped") != 0;
    let position = if down == flipped {
        at
    } else {
        Vector {
            x: at.x,
            y: bounds.size.height - at.y,
        }
    };
    let transaction = runtime.class(c"CATransaction");
    msg!(runtime, (); transaction, "begin");
    msg!(runtime, (); transaction, "setDisableActions:", true => bool);
    let cursor = cursor(runtime, window, layer, down);
    msg!(runtime, (); cursor, "setPosition:", position => Vector);
    msg!(runtime, (); transaction, "commit");
}

/// The agent cursor's layer in `layer`, made the first time: an arrow whose
/// tip is at the layer's position.
fn cursor(runtime: &Runtime, window: Id, layer: Id, down: bool) -> Id {
    let name = runtime.string(&CURSOR.to_string_lossy());
    let layers = msg!(runtime, Id; layer, "sublayers");
    let count = msg!(runtime, usize; layers, "count");
    for index in 0..count {
        let sublayer = msg!(runtime, Id; layers, "objectAtIndex:", index => usize);
        if msg!(runtime, u8; msg!(runtime, Id; sublayer, "name"), "isEqualToString:", name => Id)
            != 0
        {
            return sublayer;
        }
    }
    let cursor =
        msg!(runtime, Id; msg!(runtime, Id; runtime.class(c"CAShapeLayer"), "alloc"), "init");
    msg!(runtime, (); cursor, "setName:", name => Id);
    // A pointer arrow, 20 points tall, its tip at the origin.
    const ARROW: [(f64, f64); 7] = [
        (0.0, 0.0),
        (0.0, 17.0),
        (4.2, 13.2),
        (7.0, 19.6),
        (9.8, 18.4),
        (7.1, 12.2),
        (12.4, 12.2),
    ];
    let sign = if down { 1.0 } else { -1.0 };
    // SAFETY: Core Graphics' path and colour functions, making objects the
    // layer keeps its own references to; this function releases its own.
    let (path, colors) = unsafe {
        let path = (runtime.path)();
        for (index, (x, y)) in ARROW.iter().enumerate() {
            let line = if index == 0 {
                runtime.move_to
            } else {
                runtime.line_to
            };
            line(path, std::ptr::null(), *x, y * sign);
        }
        (runtime.close)(path);
        let colors = [
            (runtime.color)(1.0, 0.36, 0.0, 1.0),
            (runtime.color)(1.0, 1.0, 1.0, 1.0),
            (runtime.color)(0.0, 0.0, 0.0, 1.0),
        ];
        (path, colors)
    };
    let [fill, stroke, shadow] = colors;
    msg!(runtime, (); cursor, "setPath:", path => *mut c_void);
    msg!(runtime, (); cursor, "setFillColor:", fill => *mut c_void);
    msg!(runtime, (); cursor, "setStrokeColor:", stroke => *mut c_void);
    msg!(runtime, (); cursor, "setShadowColor:", shadow => *mut c_void);
    runtime.release(path);
    for color in colors {
        runtime.release(color);
    }
    msg!(runtime, (); cursor, "setLineWidth:", 1.5_f64 => f64);
    msg!(runtime, (); cursor, "setShadowOpacity:", 0.4_f32 => f32);
    msg!(runtime, (); cursor, "setShadowRadius:", 1.5_f64 => f64);
    msg!(runtime, (); cursor, "setShadowOffset:", Size { width: 0.0, height: -sign } => Size);
    msg!(runtime, (); cursor, "setZPosition:", 1.0e6_f64 => f64);
    let scale = msg!(runtime, f64; window, "backingScaleFactor");
    msg!(runtime, (); cursor, "setContentsScale:", scale => f64);
    msg!(runtime, (); layer, "addSublayer:", cursor => Id);
    msg!(runtime, (); cursor, "release");
    cursor
}

/// The C function `name` names, as a pointer of type `F`.
///
/// # Safety
///
/// `F` is the function's C signature.
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
        let flags: Vec<(String, usize)> = events(&gesture)
            .iter()
            .map(|event| match event {
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

    #[test]
    fn a_drag_moves_in_short_strides_with_its_button_held() {
        let gesture = Gesture::Drag {
            from: Point { x: 0.0, y: 0.0 },
            to: Point { x: 40.0, y: 0.0 },
            button: Button::Left,
        };
        let motions: Vec<Motion> = events(&gesture)
            .iter()
            .map(|event| match event {
                Event::Pointer { motion, .. } => *motion,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            motions,
            [
                Motion::Moved,
                Motion::Down,
                Motion::Dragged,
                Motion::Dragged,
                Motion::Dragged,
                Motion::Dragged,
                Motion::Dragged,
                Motion::Up,
            ]
        );
    }
}
