// Copyright 2013-2016 The rust-url developers. MIT OR Apache-2.0.

//! Complete pinned IPv6 grammar, including its decimal embedded IPv4 tail.

use rawr_toml::document;

use super::{Invalid, Text};
use crate::input::membership::manifest::{session, Step};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(in super::super) struct Address {
    segments: [u16; 8],
}

impl Address {
    /// Input is the interior of brackets; bracket recognition belongs to host.
    pub(in super::super) fn parse<A: document::Admission>(
        input: &str,
        session: &mut session::Session<'_, A>,
    ) -> Step<Result<Self, Invalid>> {
        session.string(input.len())?;
        super::finish(parse(input), session)
    }

    pub(in super::super) fn segments(&self) -> &[u16; 8] {
        &self.segments
    }

    /// URL's hex-only serializer; even an IPv4 tail is rendered as hex pieces.
    pub(in super::super) fn serialize<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Text> {
        session.check()?;
        let compression = longest(&self.segments);
        Text::render(
            |emit| {
                let mut index = 0;
                while index < 8 {
                    if let Some((start, end)) = compression
                        && index == start
                    {
                        emit(b':');
                        if index == 0 {
                            emit(b':');
                        }
                        if end == 8 {
                            break;
                        }
                        index = end;
                    }
                    let value = self.segments[index];
                    let mut started = false;
                    for shift in [12, 8, 4, 0] {
                        let digit = ((value >> shift) & 15) as u8;
                        if digit != 0 || started || shift == 0 {
                            emit(if digit < 10 {
                                b'0' + digit
                            } else {
                                b'a' + digit - 10
                            });
                            started = true;
                        }
                    }
                    if index < 7 {
                        emit(b':');
                    }
                    index += 1;
                }
            },
            session,
        )
    }
}

/// Keep the first longest run, and never compress a lone zero.
fn longest(pieces: &[u16; 8]) -> Option<(usize, usize)> {
    let mut longest = None;
    let mut start = None;
    for index in 0..=8 {
        if pieces.get(index) == Some(&0) {
            if start.is_none() {
                start = Some(index);
            }
        } else if let Some(start) = start.take() {
            let length = index - start;
            if length >= 2
                && longest.is_none_or(|(old_start, old_end)| length > old_end - old_start)
            {
                longest = Some((start, index));
            }
        }
    }
    longest
}

fn parse(input: &str) -> Result<Address, Invalid> {
    let input = input.as_bytes();
    let len = input.len();
    let mut is_ipv4 = false;
    let mut pieces = [0u16; 8];
    let mut piece = 0;
    let mut compression = None;
    let mut index = 0;
    if len < 2 {
        return Err(Invalid::V6);
    }
    if input[0] == b':' {
        if input[1] != b':' {
            return Err(Invalid::V6);
        }
        index = 2;
        piece = 1;
        compression = Some(1);
    }
    while index < len {
        if piece == 8 {
            return Err(Invalid::V6);
        }
        if input[index] == b':' {
            if compression.is_some() {
                return Err(Invalid::V6);
            }
            index += 1;
            piece += 1;
            compression = Some(piece);
            continue;
        }
        let start = index;
        let end = start + (len - start).min(4);
        let mut value = 0u16;
        while index < end {
            let digit = match input[index] {
                b'0'..=b'9' => input[index] - b'0',
                b'a'..=b'f' => input[index] - b'a' + 10,
                b'A'..=b'F' => input[index] - b'A' + 10,
                _ => break,
            };
            value = value * 16 + u16::from(digit);
            index += 1;
        }
        if index < len {
            match input[index] {
                b'.' => {
                    if index == start {
                        return Err(Invalid::V6);
                    }
                    index = start;
                    if piece > 6 {
                        return Err(Invalid::V6);
                    }
                    is_ipv4 = true;
                }
                b':' => {
                    index += 1;
                    if index == len {
                        return Err(Invalid::V6);
                    }
                }
                _ => return Err(Invalid::V6),
            }
        }
        if is_ipv4 {
            break;
        }
        pieces[piece] = value;
        piece += 1;
    }
    if is_ipv4 {
        if piece > 6 {
            return Err(Invalid::V6);
        }
        let mut numbers = 0;
        while index < len {
            if numbers > 0 {
                if numbers < 4 && input[index] == b'.' {
                    index += 1;
                } else {
                    return Err(Invalid::V6);
                }
            }
            let mut ipv4_piece = None;
            while index < len {
                let digit = match input[index] {
                    byte @ b'0'..=b'9' => byte - b'0',
                    _ => break,
                };
                match ipv4_piece {
                    None => ipv4_piece = Some(u16::from(digit)),
                    Some(0) => return Err(Invalid::V6),
                    Some(ref mut value) => {
                        *value = *value * 10 + u16::from(digit);
                        if *value > 255 {
                            return Err(Invalid::V6);
                        }
                    }
                }
                index += 1;
            }
            pieces[piece] = match ipv4_piece {
                Some(value) => pieces[piece] * 256 + value,
                None => return Err(Invalid::V6),
            };
            numbers += 1;
            if numbers == 2 || numbers == 4 {
                piece += 1;
            }
        }
        if numbers != 4 {
            return Err(Invalid::V6);
        }
    }
    if index < len {
        return Err(Invalid::V6);
    }
    match compression {
        Some(compression) => {
            let mut swaps = piece - compression;
            piece = 7;
            while swaps > 0 {
                pieces.swap(piece, compression + swaps - 1);
                swaps -= 1;
                piece -= 1;
            }
        }
        None if piece != 8 => return Err(Invalid::V6),
        None => {}
    }
    Ok(Address { segments: pieces })
}
