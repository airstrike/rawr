//! What a hover reads where a program is paused:
//! `{"op": "hover", "file", "line", "column", "text", "frame"}` reads the
//! smallest expression a hover may read at that character of the file as
//! it compiled, in the frame of the function it is written in, the asked
//! frame first; `{"op": "hover", "code", "frame"}` reads one expression
//! given whole. A line `text` that differs from the line compiled reads
//! nothing.
//!
//! The answer names the expression and where it is written, `{"expression",
//! "range": {"line", "start", "end"}, "frame"}`, with its `value` (and its
//! `children` with `"children": true`), why a hover `refused` it, or why
//! nothing is `unread` there. A place reads from its binding's `Debug`
//! parts, with nothing compiled; std's arithmetic, casts, indexing and
//! lengths evaluate as `eval` does; anything else is refused (see
//! [`read`]). It is asked only of a paused program.

mod answer;
mod expression;
mod read;
mod within;

pub(super) use answer::Ask;
