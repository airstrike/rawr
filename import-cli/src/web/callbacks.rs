//! The glue of a runner with rawr's browser debugger, rewritten so a pause
//! can wait inside a page callback (#2269, `rawr::universe::debugger`).
//!
//! A page calls a wasm-bindgen closure through the closure's invoke shim,
//! a plain export, and JSPI can't suspend a stack a plain export runs on.
//! Each cast of a closure in the glue names its descriptor, with the
//! shim's function-table index (`shim_idx`), and the shim's own function.
//! A closure that takes no argument or one JS value and returns nothing,
//! as an event listener or an animation frame is, has that function call
//! the runner's `__rawr_enter0` or `__rawr_enter1` with the index instead:
//! a `jspi` export, entered through `WebAssembly.promising`, which calls
//! the shim from wasm on a stack a pause suspends.
//!
//! A suspended call holds what its closure borrows, so `makeMutClosure`
//! holds every closure call that comes while one is suspended and makes
//! them in order once it ends; the suspended closure stays taken until
//! then. An animation frame held so is drawn once, after, as it asks for
//! the next one only when it runs. A held event has been dispatched by
//! then: `preventDefault` no longer stops what the browser did with it.
//! A closure the glue makes with `makeClosure` waits the same way.

/// What a cast names its closure's descriptor with.
const CAST: &str = "Cast intrinsic for `Closure(Closure { ";

/// The start of `makeMutClosure`'s calling function.
const CALL: &str = "    const real = (...args) => {\n";

/// What follows the calling function.
const UNREF: &str = "\n    real._wbg_cb_unref = ";

/// The calling function of `makeMutClosure` that holds calls while one is
/// suspended, and the queue it holds them in.
const HOLDING: &str = "    const real = (...args) => {
        if (rawrHolding) {
            RAWR_HELD.push(() => real(...args));
            return;
        }
        state.cnt++;
        const a = state.a;
        state.a = 0;
        const entered = __rawr_entered();
        let ret;
        try {
            ret = f(a, state.b, ...args);
        } catch (error) {
            state.a = a;
            real._wbg_cb_unref();
            throw error;
        }
        // Suspended in a pause: its stack still runs the closure, so the
        // closure stays taken, and every closure call waits until it ends.
        if (__rawr_entered() > entered) {
            rawrHolding = true;
            ret.finally(() => {
                state.a = a;
                real._wbg_cb_unref();
                rawrRelease();
            });
            return;
        }
        state.a = a;
        real._wbg_cb_unref();
        return ret;
    };";

/// The queue of held closure calls, put before `makeMutClosure`.
const QUEUE: &str = "// Closure calls that came while a callback was suspended in a pause,
// made in order once it ends (rawr's browser debugger).
const RAWR_HELD = [];
let rawrHolding = false;

function rawrRelease() {
    rawrHolding = false;
    while (!rawrHolding && RAWR_HELD.length) RAWR_HELD.shift()();
}

";

/// The check a `makeClosure` call starts with, so it waits too.
const HELD: &str = "        if (rawrHolding) {
            RAWR_HELD.push(() => real(...args));
            return;
        }
";

/// `glue` with its page callbacks entered so a pause can wait in them, or
/// `None` when it has no `makeMutClosure` of the shape this rewrites, as
/// another wasm-bindgen's glue may not.
pub(super) fn enter(glue: &str) -> Option<String> {
    let start = glue.find("function makeMutClosure(arg0, arg1, f) {\n")?;
    let call = start + glue[start..].find(CALL)?;
    let unref = call + glue[call..].find(UNREF)?;
    let mut entered = String::with_capacity(glue.len() + QUEUE.len() + HOLDING.len());
    entered.push_str(&glue[..start]);
    entered.push_str(QUEUE);
    entered.push_str(&glue[start..call]);
    entered.push_str(HOLDING);
    entered.push_str(&glue[unref..]);
    if let Some(start) = entered.find("function makeClosure(")
        && let Some(call) = entered[start..].find(CALL)
    {
        entered.insert_str(start + call + CALL.len(), HELD);
    }
    for closure in closures(glue) {
        let arguments = closure.takes.parameters();
        let body = format!("\n    wasm.{}({arguments});\n}}", closure.shim);
        let entry = format!(
            "\n    return {}({}, {arguments});\n}}",
            closure.takes.entry(),
            closure.index
        );
        let head = format!("function {}({arguments}) {{", closure.shim);
        if let Some(at) = entered.find(&head).map(|at| at + head.len())
            && entered[at..].starts_with(&body)
        {
            entered.replace_range(at..at + body.len(), &entry);
        }
    }
    Some(entered)
}

/// A closure the glue casts that a pause can wait in.
#[derive(Debug, PartialEq)]
struct Closure<'glue> {
    /// The function-table index of its invoke shim.
    index: u32,
    /// What it takes.
    takes: Takes,
    /// The glue's function that calls its shim, named as the shim is.
    shim: &'glue str,
}

/// What a closure a pause can wait in takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Takes {
    Nothing,
    /// One JS value.
    Value,
}

impl Takes {
    /// The parameters the glue's function for its shim takes: the
    /// closure's two words, then its argument.
    fn parameters(self) -> &'static str {
        match self {
            Self::Nothing => "arg0, arg1",
            Self::Value => "arg0, arg1, arg2",
        }
    }

    /// The runner's export that enters it.
    fn entry(self) -> &'static str {
        match self {
            Self::Nothing => "__rawr_enter0",
            Self::Value => "__rawr_enter1",
        }
    }
}

/// The closures `glue` casts with `makeMutClosure` that take no argument
/// or one JS value and return nothing.
fn closures(glue: &str) -> impl Iterator<Item = Closure<'_>> {
    glue.match_indices(CAST).filter_map(|(at, _)| {
        let descriptor = &glue[at + CAST.len()..];
        let (descriptor, cast) = descriptor.split_once('\n')?;
        let arguments = descriptor
            .split_once("arguments: [")?
            .1
            .split_once("], shim_idx: ")?;
        let (index, rest) = arguments.1.split_once(", ret: ")?;
        if !rest.starts_with("Unit,") {
            return None;
        }
        let arguments: Vec<&str> = arguments
            .0
            .split(", ")
            .filter(|argument| !argument.is_empty())
            .collect();
        let takes = match arguments.as_slice() {
            [] => Takes::Nothing,
            [argument] if argument.starts_with("Externref") => Takes::Value,
            [argument] if argument.starts_with("NamedExternref(") => Takes::Value,
            _ => return None,
        };
        let shim = cast
            .trim_start()
            .strip_prefix("const ret = makeMutClosure(arg0, arg1, ")?
            .split_once(");")?
            .0;
        Some(Closure {
            index: index.parse().ok()?,
            takes,
            shim,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{Closure, Takes};

    /// Glue as wasm-bindgen 0.2.130 writes it, cut to two closures: a key
    /// listener and an animation frame, and one that returns a value.
    const GLUE: &str = "function __wbg_cast_1(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [NamedExternref(\"KeyboardEvent\")], shim_idx: 4, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, invoke_key);
            return ret;
        }
        function __wbg_cast_2(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [], shim_idx: 3, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, invoke_frame);
            return ret;
        }
        function __wbg_cast_3(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 9, ret: Result(Unit), inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, invoke_result);
            return ret;
        }
function invoke_frame(arg0, arg1) {
    wasm.invoke_frame(arg0, arg1);
}

function invoke_key(arg0, arg1, arg2) {
    wasm.invoke_key(arg0, arg1, arg2);
}

function invoke_result(arg0, arg1, arg2) {
    const ret = wasm.invoke_result(arg0, arg1, arg2);
    return ret;
}

function makeMutClosure(arg0, arg1, f) {
    const state = { a: arg0, b: arg1, cnt: 1 };
    const real = (...args) => {
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            wasm.__wbindgen_destroy_closure(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}
";

    /// The closures a pause can wait in are those that take none or one
    /// JS value and return nothing, each with its shim's table index.
    #[test]
    fn only_callbacks_of_no_result_are_entered() {
        let closures: Vec<Closure> = super::closures(GLUE).collect();
        assert_eq!(
            closures,
            [
                Closure {
                    index: 4,
                    takes: Takes::Value,
                    shim: "invoke_key"
                },
                Closure {
                    index: 3,
                    takes: Takes::Nothing,
                    shim: "invoke_frame"
                },
            ]
        );
    }

    /// Their shims call the runner's entries, `makeMutClosure` holds calls
    /// while one is suspended, and a closure that returns a value keeps
    /// its shim.
    #[test]
    fn the_glue_enters_callbacks_and_holds_calls() {
        let entered = super::enter(GLUE).expect("the glue has a makeMutClosure");
        assert!(entered.contains("function invoke_key(arg0, arg1, arg2) {\n    return __rawr_enter1(4, arg0, arg1, arg2);\n}"));
        assert!(entered.contains(
            "function invoke_frame(arg0, arg1) {\n    return __rawr_enter0(3, arg0, arg1);\n}"
        ));
        assert!(entered.contains("const ret = wasm.invoke_result(arg0, arg1, arg2);"));
        assert!(entered.contains("if (__rawr_entered() > entered) {"));
        assert!(entered.contains("function rawrRelease() {"));
        assert!(entered.contains("real._wbg_cb_unref = () => {"));
        assert!(!entered.contains("} finally {\n            state.a = a;"));
    }

    /// Glue with no `makeMutClosure` of this shape is left to its caller.
    #[test]
    fn other_glue_is_not_rewritten() {
        assert_eq!(super::enter("function other() {}\n"), None);
    }
}
