//! The shim is separate from the unchanged parser.rs:195–336 extraction.
#![allow(
    clippy::question_mark,
    clippy::unnecessary_map_or,
    reason = "the included extraction is unchanged upstream code"
)]

use std::str;

enum SyntaxViolation {
    C0SpaceIgnored,
    TabOrNewlineIgnored,
}

fn ascii_tab_or_new_line(character: char) -> bool {
    matches!(character, '\t' | '\n' | '\r')
}

fn c0_control_or_space(character: char) -> bool {
    character <= ' '
}

include!("upstream.rs");

pub(super) fn input(source: &str) -> Input<'_> {
    Input::new_trim_c0_control_and_space(source, None)
}

pub(super) fn remaining<'i>(input: &Input<'i>) -> &'i str {
    input.chars.as_str()
}

pub(super) fn raw(input: &mut Input<'_>) -> Option<char> {
    input.chars.next()
}

pub(super) fn utf8<'i>(input: &mut Input<'i>) -> Option<(char, &'i str)> {
    input.next_utf8()
}

pub(super) fn first<'i>(input: &Input<'i>) -> (Option<char>, Input<'i>) {
    input.split_first()
}

pub(super) fn count<'i>(input: &Input<'i>) -> (u32, Input<'i>) {
    input.count_matching(|c| matches!(c, '/' | '\\'))
}
