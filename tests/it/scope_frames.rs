//! D558 tier 1: a call whose value borrows what it takes gives a scope
//! frame (`rawr::scope::Opening`). A `let` that binds it opens it, and the
//! rest of the block runs inside the frame's native call, which owns the
//! borrowed text and lends its region. `break`, `continue`, `return`, `?`
//! and errors leave the rest as `Err` values and cross the frame, which
//! ends however they leave. A statement that drops the frame opens and
//! closes it at once, and a handle kept past its frame is expired.
//! (Check (d), moved onto the frame values the runner gives.)

use rawr::scope::{self, Family, Opening, Region, RegionF, Regional, Scoped};
use rawr::{Dynamic, Engine, EvalAltResult, ImmutableString};
use std::cell::Cell;

/// `SplitWhitespace<'x>` for every `'x`.
struct WordsF;

impl Family for WordsF {
    type Of<'x> = std::str::SplitWhitespace<'x>;
}

impl Regional for WordsF {
    fn name() -> &'static str {
        "SplitWhitespace"
    }
}

thread_local! {
    static OPENED: Cell<usize> = const { Cell::new(0) };
    static CLOSED: Cell<usize> = const { Cell::new(0) };
}

struct Closing;

impl Drop for Closing {
    fn drop(&mut self) {
        CLOSED.with(|closed| closed.set(closed.get() + 1));
    }
}

fn reset() {
    OPENED.with(|opened| opened.set(0));
    CLOSED.with(|closed| closed.set(0));
}

fn frames() -> (usize, usize) {
    (OPENED.with(Cell::get), CLOSED.with(Cell::get))
}

fn runtime(message: String) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), rawr::Position::NONE).into()
}

fn engine() -> Engine {
    let mut engine = Engine::new();
    // `words(text)`: a frame that owns a copy of `text` and the iterator
    // that borrows it, held in the frame's region.
    engine.register_fn("words", |text: ImmutableString| -> Dynamic {
        let text = text.to_string();
        Dynamic::from(Opening::new("words", move |rest| {
            OPENED.with(|opened| opened.set(opened.get() + 1));
            let _closing = Closing;
            let mut region = Region::new();
            let words = region.insert::<WordsF>(text.split_whitespace());
            scope::open(&mut region, || rest(Dynamic::from(words)))
        }))
    });
    // The next word of a frame's iterator, or `""` at its end.
    engine.register_fn("next_word", |words: Scoped| -> Result<ImmutableString, Box<EvalAltResult>> {
        scope::with::<RegionF, _>(words.nonce(), |region| region.get_mut::<WordsF>(words).map(|words| words.next().unwrap_or_default().to_string()))
            .and_then(|word| word)
            .map(Into::into)
            .map_err(|error| runtime(error.to_string()))
    });
    engine
}

fn eval(source: &str) -> Result<String, Box<EvalAltResult>> {
    engine().eval::<String>(source)
}

#[test]
fn the_rest_of_the_block_runs_in_the_frame() {
    reset();
    let value = eval(
        r#"
        let text = "alpha beta gamma";
        let p = words(text);
        let a = next_word(p);
        let b = next_word(p);
        a + "," + b
    "#,
    )
    .unwrap();
    assert_eq!(value, "alpha,beta");
    assert_eq!(frames(), (1, 1));
}

#[test]
fn break_and_continue_cross_the_frame() {
    reset();
    let value = eval(
        r#"
        let mut firsts = String::new();
        for line in ["a b", "skip me", "c d", "stop here", "e f"] {
            let p = words(line);
            let first = next_word(p);
            if first == "skip" { continue; }
            if first == "stop" { break; }
            firsts += &first;
        }
        firsts
    "#,
    )
    .unwrap();
    assert_eq!(value, "ac");
    assert_eq!(frames(), (4, 4));
}

#[test]
fn return_crosses_the_frame() {
    reset();
    let value = eval(
        r#"
        fn second(line: &str) -> String {
            let p = words(line);
            next_word(p);
            return next_word(p);
            panic!("unreachable");
        }
        second("one two three")
    "#,
    )
    .unwrap();
    assert_eq!(value, "two");
    assert_eq!(frames(), (1, 1));
}

#[test]
fn nested_frames_both_close() {
    reset();
    let value = eval(
        r#"
        let p = words("x y");
        let q = words("1 2");
        let x = next_word(p);
        let one = next_word(q);
        x + one + next_word(p)
    "#,
    )
    .unwrap();
    assert_eq!(value, "x1y");
    assert_eq!(frames(), (2, 2));
}

#[test]
fn an_error_crosses_the_frame() {
    reset();
    let error = eval(
        r#"
        let p = words("a b");
        let zero = 0;
        let n = 1 / zero;
        n.to_string()
    "#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("zero"), "{error}");
    assert_eq!(frames(), (1, 1));
}

/// A statement that drops the frame opens and closes it at once, and a
/// handle kept past its frame is expired (E0597).
#[test]
fn a_dropped_frame_closes_at_once_and_a_kept_handle_expires() {
    reset();
    let error = eval(
        r#"
        words("a b");
        let mut kept = None;
        {
            let p = words("c d");
            kept = Some(p);
        }
        next_word(kept.unwrap())
    "#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("E0597"), "{error}");
    assert_eq!(frames(), (2, 2));
}
