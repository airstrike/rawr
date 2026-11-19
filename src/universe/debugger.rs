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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use serde_json::{json, Value};
use wasm_bindgen::prelude::{wasm_bindgen, JsValue};

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
/// `eval`, a `break`, a `clear`, a `step`, a `continue` or a `pause`. The
/// promise settles with the answer's JSON text, as the socket would write
/// it: a read at the program's next step that matches it, a step where it
/// pauses again.
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

/// Settle request `id`'s promise with `answer`.
fn settle(id: u64, answer: &Value) {
    let resolve = ANSWERS.with_borrow_mut(|answers| answers.remove(&id));
    if let Some(resolve) = resolve {
        let _ = resolve.call1(&JsValue::NULL, &JsValue::from_str(&answer.to_string()));
    }
}
