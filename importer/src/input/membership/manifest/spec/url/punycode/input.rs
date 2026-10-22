// Copyright 2013 The rust-url developers. MIT OR Apache-2.0.
//! Preserve the internal decoder's distinct byte and scalar operations.

#[derive(Clone, Copy, Debug)]
pub(super) enum Input<'input> {
    Bytes(&'input [u8]),
    Chars(&'input [char]),
}

impl<'input> Input<'input> {
    pub(super) fn len(self) -> usize {
        match self {
            Self::Bytes(input) => input.len(),
            Self::Chars(input) => input.len(),
        }
    }

    pub(super) fn delimiter(self) -> Option<usize> {
        match self {
            Self::Bytes(input) => input.iter().rposition(|c| *c == b'-'),
            Self::Chars(input) => input.iter().rposition(|c| *c == '-'),
        }
    }

    pub(super) fn split(self, index: usize) -> (Self, Self) {
        match self {
            Self::Bytes(input) => {
                let (head, tail) = input.split_at(index);
                (Self::Bytes(head), Self::Bytes(tail))
            }
            Self::Chars(input) => {
                let (head, tail) = input.split_at(index);
                (Self::Chars(head), Self::Chars(tail))
            }
        }
    }

    pub(super) fn digit(&mut self) -> Option<Option<u32>> {
        Some(match self {
            Self::Bytes(input) => {
                let (&byte, tail) = input.split_first()?;
                *input = tail;
                match byte {
                    b'0'..=b'9' => Some(u32::from(byte - b'0' + 26)),
                    b'A'..=b'Z' => Some(u32::from(byte - b'A')),
                    b'a'..=b'z' => Some(u32::from(byte - b'a')),
                    _ => None,
                }
            }
            Self::Chars(input) => {
                let (&c, tail) = input.split_first()?;
                *input = tail;
                match c {
                    '0'..='9' => Some(u32::from(c) - u32::from('0') + 26),
                    'a'..='z' => Some(u32::from(c) - u32::from('a')),
                    _ => None,
                }
            }
        })
    }

    pub(super) fn next(&mut self) -> Option<char> {
        match self {
            Self::Bytes(input) => {
                let (&byte, tail) = input.split_first()?;
                *input = tail;
                Some(char::from(byte.to_ascii_lowercase()))
            }
            Self::Chars(input) => {
                let (&c, tail) = input.split_first()?;
                *input = tail;
                Some(c)
            }
        }
    }
}
