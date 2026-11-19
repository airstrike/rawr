//! The browser universe's debugger (D1057, D1071): the inspector a runner
//! built with the `browser-debugger` feature attaches to its universe
//! before the program loads, so every run has it from its start.
//!
//! The page asks it with [`debug`], one request of the inspector's socket
//! protocol as JSON text, and gets a promise of the answer's JSON text. A
//! read is answered at the program's next step that matches it: `main`'s,
//! once the sleep it waits in ends, or a call the page makes into the
//! program, as iced's `view`. A breakpoint or a step pauses only on
//! `main`'s own stack, which [`browser::suspend`] suspends while the pause
//! waits for the page's next request, so the page runs its events and asks
//! meanwhile. A step that lands in a call the page made goes on until
//! `main`'s stack runs again. Each pause, resume and change to the
//! breakpoints is told to the listener [`debug_events`] names.
//!
//! Two requests are the page's own, beside the socket's protocol. `hello`
//! says what this instance's debugger can do before the program runs:
//! whether a pause can wait, which needs the browser's JSPI. `hold`, asked
//! before `run`, holds the program at the first statement it wrote, as
//! `RAWR_DEBUG_HOLD` holds a native one: the page sets its breakpoints
//! meanwhile, and the protocol's `release` lets it run.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use serde_json::{json, Value};
use wasm_bindgen::prelude::{wasm_bindgen, JsValue};
use wasm_bindgen::JsCast as _;

use super::{browser, Universe};
use crate::inspect::{Host, Inspector};

#[wasm_bindgen]
extern "C" {
    /// A page's function that takes each event of the debugger as its
    /// JSON text.
    pub type Listener;

    /// The listener with `event` as its argument, called later.
    #[wasm_bindgen(method, js_name = bind)]
    fn with_event(this: &Listener, context: &JsValue, event: &str) -> js_sys::Function;

    #[wasm_bindgen(js_name = queueMicrotask)]
    fn queue_microtask(callback: &js_sys::Function);
}

thread_local! {
    /// The inspector attached to this instance's universe.
    static INSPECTOR: RefCell<Option<Inspector>> = const { RefCell::new(None) };
    /// Each request's promise's resolver, by the request's number, until
    /// its answer comes.
    static ANSWERS: RefCell<HashMap<u64, js_sys::Function>> = RefCell::new(HashMap::new());
    /// The number the next request gets.
    static NEXT: Cell<u64> = const { Cell::new(0) };
    /// The resolver of the promise a paused `main` waits on.
    static PARKED: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    /// The page's listener [`debug_events`] named.
    static LISTENER: RefCell<Option<Listener>> = const { RefCell::new(None) };
}

/// The page's side of a pause. Its state lives in this thread's locals:
/// a page's functions can't go between threads, and a host must.
struct Page;

impl Host for Page {
    fn can_wait(&self) -> bool {
        browser::on_main_stack()
    }

    fn wait(&self) -> Result<(), String> {
        let parked = js_sys::Promise::new(&mut |resolve, _| PARKED.set(Some(resolve)));
        browser::suspend(&parked).map_err(|reason| {
            reason
                .as_string()
                .unwrap_or_else(|| "rawr: this page can't suspend the program".to_owned())
        })
    }

    fn wake(&self) {
        if let Some(resolve) = PARKED.take() {
            let _ = resolve.call0(&JsValue::NULL);
        }
    }
}

/// Attach a debugger to `universe`, before its program loads.
pub(super) fn attach(universe: &mut Universe) {
    let inspector = Inspector::hosted(Page, tell);
    if universe.attach(inspector.clone()) {
        INSPECTOR.set(Some(inspector));
    }
}

/// Tell `event` to the page's listener once the step that made it is
/// done: the inspector holds its state while it tells, and a listener that
/// asks it something at once finds it let go.
fn tell(event: &Value) {
    LISTENER.with_borrow(|listener| {
        if let Some(listener) = listener {
            queue_microtask(&listener.with_event(&JsValue::NULL, &event.to_string()));
        }
    });
}

/// Ask the program's debugger `request`, one JSON object of the
/// inspector's socket protocol: a read (`{"name": .., "frame": ..}`), an
/// `eval`, a `break`, a `clear`, a `step`, a `continue`, a `pause` or a
/// `release`. The promise settles with the answer's JSON text, as the
/// socket would write it: a read at the program's next step that matches
/// it, a step where it pauses again. The page's own requests are answered
/// at once: `{"op": "hello"}` with `{"debugger": true, "pause": ..}`,
/// `pause` false where the browser can't suspend the program, and `{"op":
/// "hold"}` with `{"held": ..}`, false once the program started.
#[wasm_bindgen]
pub fn debug(request: &str) -> js_sys::Promise {
    let id = NEXT.get();
    NEXT.set(id + 1);
    let answer = js_sys::Promise::new(&mut |resolve, _| {
        ANSWERS.with_borrow_mut(|answers| answers.insert(id, resolve));
    });
    let request = match serde_json::from_str::<Value>(request) {
        Ok(request) => request,
        Err(error) => {
            settle(
                id,
                &json!({ "error": format!("the request isn't JSON: {error}") }),
            );
            return answer;
        }
    };
    match INSPECTOR.with_borrow(Option::clone) {
        Some(_) if request["op"] == "hello" => {
            settle(id, &json!({ "debugger": true, "pause": can_suspend() }))
        }
        Some(inspector) if request["op"] == "hold" => {
            let held = browser::unstarted();
            if held {
                inspector.hold();
            }
            settle(id, &json!({ "held": held }));
        }
        Some(inspector) => inspector.ask(&request, move |answer| settle(id, &answer)),
        None => settle(
            id,
            &json!({ "error": "this instance runs no program under a debugger" }),
        ),
    }
    answer
}

/// Tell `listener` each event of the program's debugger as JSON text:
/// `{"event": "paused", "file", "line", "function", "reason"}`, `{"event":
/// "resumed"}` and `{"event": "breakpoints", "revision", "breakpoints"}`.
#[wasm_bindgen]
pub fn debug_events(listener: Listener) {
    LISTENER.set(Some(listener));
}

/// Whether this browser suspends a stack through JSPI: its
/// `WebAssembly.Suspending` is the browser's own. A page without one
/// instantiates the module with a stand-in of its own code, which refuses
/// every suspension.
fn can_suspend() -> bool {
    let suspending = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("WebAssembly"))
        .and_then(|assembly| js_sys::Reflect::get(&assembly, &JsValue::from_str("Suspending")));
    suspending.ok().is_some_and(|suspending| {
        suspending
            .dyn_ref::<js_sys::Function>()
            .is_some_and(|suspending| {
                String::from(suspending.to_string()).contains("[native code]")
            })
    })
}

/// Settle request `id`'s promise with `answer`.
fn settle(id: u64, answer: &Value) {
    let resolve = ANSWERS.with_borrow_mut(|answers| answers.remove(&id));
    if let Some(resolve) = resolve {
        let _ = resolve.call1(&JsValue::NULL, &JsValue::from_str(&answer.to_string()));
    }
}
