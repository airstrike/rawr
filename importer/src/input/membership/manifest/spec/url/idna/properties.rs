// Copyright The rust-url developers. MIT OR Apache-2.0.
//! Read-only pinned property queries at fail-fast label/context/bidi checks.

use rawr_toml::document;

use crate::input::membership::manifest::session;

use super::{invalid, Work};

#[derive(Clone, Copy)]
pub(super) struct Checks {
    pub(super) combining: bool,
    pub(super) context: bool,
}

pub(super) fn label<A: document::Admission>(
    data: &idna_adapter::Adapter,
    label: &[char],
    checks: Checks,
    session: &mut session::Session<'_, A>,
) -> Work<()> {
    session.check()?;
    if checks.combining && label.first().is_some_and(|first| data.is_mark(*first)) {
        return invalid(session);
    }
    if checks.context {
        for (index, &c) in label.iter().enumerate() {
            if c != '\u{200c}' && c != '\u{200d}' {
                continue;
            }
            let (head, joiner_and_tail) = label.split_at(index);
            let Some(previous) = head.last() else {
                return invalid(session);
            };
            if data.is_virama(*previous) {
                continue;
            }
            if c == '\u{200d}' {
                return invalid(session);
            }
            if !joining(
                data,
                head.iter().rev().copied(),
                idna_adapter::LEFT_OR_DUAL_JOINING_MASK,
            ) || !joining(
                data,
                joiner_and_tail[1..].iter().copied(),
                idna_adapter::RIGHT_OR_DUAL_JOINING_MASK,
            ) {
                return invalid(session);
            }
        }
    }
    // The reached source's scalar work guard, not the DNS label length limit.
    if !label.iter().all(char::is_ascii) && label.len() > 1000 {
        return invalid(session);
    }
    Ok(())
}

fn joining(
    data: &idna_adapter::Adapter,
    input: impl Iterator<Item = char>,
    mask: idna_adapter::JoiningTypeMask,
) -> bool {
    for c in input {
        let joining = data.joining_type(c);
        if joining.to_mask().intersects(mask) {
            return true;
        }
        if joining.is_transparent() {
            continue;
        }
        return false;
    }
    false
}

fn is_bidi(data: &idna_adapter::Adapter, input: &[char]) -> bool {
    for &c in input {
        if c < '\u{0590}'
            || ('\u{0900}'..='\u{fb1c}').contains(&c)
            || ('\u{1f000}'..='\u{3ffff}').contains(&c)
            || ('\u{ff00}'..='\u{107ff}').contains(&c)
            || ('\u{11000}'..='\u{1e7ff}').contains(&c)
        {
            continue;
        }
        if idna_adapter::RTL_MASK.intersects(data.bidi_class(c).to_mask()) {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Numeral {
    Undecided,
    European,
    Arabic,
}

pub(super) fn bidi<A: document::Admission>(
    data: &idna_adapter::Adapter,
    input: &[char],
    session: &mut session::Session<'_, A>,
) -> Work<()> {
    session.check()?;
    if !is_bidi(data, input) {
        return Ok(());
    }
    for label in input.split(|c| *c == '.') {
        let Some((&first, tail)) = label.split_first() else {
            continue;
        };
        let first = data.bidi_class(first);
        if !idna_adapter::FIRST_BC_MASK.intersects(first.to_mask()) {
            return invalid(session);
        }
        let is_ltr = first.is_ltr();
        let mut middle = tail;
        while let Some((&last, prior)) = middle.split_last() {
            let last = data.bidi_class(last);
            if last.is_nonspacing_mark() {
                middle = prior;
                continue;
            }
            let mask = if is_ltr {
                idna_adapter::LAST_LTR_MASK
            } else {
                idna_adapter::LAST_RTL_MASK
            };
            if !mask.intersects(last.to_mask()) {
                return invalid(session);
            }
            if is_ltr {
                for &c in prior {
                    if !idna_adapter::MIDDLE_LTR_MASK.intersects(data.bidi_class(c).to_mask()) {
                        return invalid(session);
                    }
                }
            } else {
                let mut numerals = Numeral::Undecided;
                for &c in prior {
                    let class = data.bidi_class(c);
                    if !idna_adapter::MIDDLE_RTL_MASK.intersects(class.to_mask()) {
                        return invalid(session);
                    }
                    match numerals {
                        Numeral::Undecided => {
                            if class.is_european_number() {
                                numerals = Numeral::European;
                            } else if class.is_arabic_number() {
                                numerals = Numeral::Arabic;
                            }
                        }
                        Numeral::European if class.is_arabic_number() => return invalid(session),
                        Numeral::Arabic if class.is_european_number() => return invalid(session),
                        _ => {}
                    }
                }
                if (numerals == Numeral::European && last.is_arabic_number())
                    || (numerals == Numeral::Arabic && last.is_european_number())
                {
                    return invalid(session);
                }
            }
            break;
        }
    }
    Ok(())
}
