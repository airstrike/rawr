// Copyright The rust-url developers. MIT OR Apache-2.0.
//! Fixed URL deny list and the two pinned ASCII prefix tiers.

pub(super) fn prefix(input: &[u8]) -> bool {
    input.len() >= 4
        && input[0].eq_ignore_ascii_case(&b'x')
        && input[1].eq_ignore_ascii_case(&b'n')
        && input[2] == b'-'
        && input[3] == b'-'
}

pub(super) fn passthrough(label: &[u8]) -> bool {
    if label.len() >= 4 && label[2] == b'-' && label[3] == b'-' {
        return false;
    }
    if let Some((&first, tail)) = label.split_first() {
        if !first.is_ascii_lowercase() {
            return false;
        }
        for &byte in tail {
            if byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' {
                continue;
            }
            return false;
        }
        label.last() != Some(&b'-')
    } else {
        true
    }
}

pub(super) fn split(label: &[u8]) -> (&[u8], &[u8]) {
    match label.iter().position(|byte| !byte.is_ascii()) {
        Some(0) => (&[], label),
        // Retain one preceding ASCII scalar for possible canonical composition.
        Some(position) => label.split_at(position - 1),
        None => (label, &[]),
    }
}

fn denied(byte: u8) -> bool {
    byte <= b' '
        || byte == 0x7f
        || matches!(
            byte,
            b'%' | b'#'
                | b'/'
                | b':'
                | b'<'
                | b'>'
                | b'?'
                | b'@'
                | b'['
                | b'\\'
                | b']'
                | b'^'
                | b'|'
        )
}

pub(super) fn byte(byte: u8) -> char {
    if denied(byte) {
        '\u{fffd}'
    } else {
        char::from(byte.to_ascii_lowercase())
    }
}

pub(super) fn unicode(c: char, deny_dot: bool) -> char {
    if c.is_ascii() && (denied(c as u8) || c.is_ascii_uppercase() || (deny_dot && c == '.')) {
        '\u{fffd}'
    } else {
        c
    }
}
