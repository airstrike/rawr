// Copyright the gitoxide contributors. MIT OR Apache-2.0.
// Pinned gix-validate tag validation; keep this file out of rustfmt.
use std::vec::Vec;
type BStr = [u8];
type BString = Vec<u8>;
mod name { pub(super) use super::super::name::Failure as Error; }
trait ByteSlice {
    fn as_bstr(&self) -> &[u8];
    fn ends_with_str(&self, suffix: &str) -> bool;
}
impl ByteSlice for [u8] {
    fn as_bstr(&self) -> &[u8] { self }
    fn ends_with_str(&self, suffix: &str) -> bool { self.ends_with(suffix.as_bytes()) }
}
#[derive(Eq, PartialEq)]
pub(crate) enum Mode {
    Sanitize,
    Validate,
}

pub(crate) fn name_inner(input: &BStr, mode: Mode) -> Result<Option<BString>, name::Error> {
    let mut out: Option<BString> =
        matches!(mode, Mode::Sanitize).then(|| BString::from(Vec::with_capacity(input.len())));
    if input.is_empty() {
        return if let Some(mut out) = out {
            out.push(b'-');
            Ok(Some(out))
        } else {
            Err(name::Error::Empty)
        };
    }
    if *input.last().expect("non-empty") == b'/' && out.is_none() {
        return Err(name::Error::EndsWithSlash);
    }
    if input.first() == Some(&b'/') && out.is_none() {
        return Err(name::Error::StartsWithSlash);
    }

    let mut previous = 0;
    let mut component_start;
    let mut component_end = 0;
    let last = input.len() - 1;
    for (byte_pos, byte) in input.iter().enumerate() {
        match byte {
            b'\\' | b'^' | b':' | b'[' | b'?' | b' ' | b'~' | b'\0'..=b'\x1F' | b'\x7F' => {
                if let Some(out) = out.as_mut() {
                    out.push(b'-');
                } else {
                    return Err(name::Error::InvalidByte {
                        byte: (&[*byte][..]).into(),
                    });
                }
            }
            b'*' => {
                if let Some(out) = out.as_mut() {
                    out.push(b'-');
                } else {
                    return Err(name::Error::Asterisk);
                }
            }

            b'.' if previous == b'.' => {
                if out.is_none() {
                    return Err(name::Error::RepeatedDot);
                }
            }
            b'.' if previous == b'/' => {
                if let Some(out) = out.as_mut() {
                    out.push(b'-');
                } else {
                    return Err(name::Error::StartsWithDot);
                }
            }
            b'{' if previous == b'@' => {
                if let Some(out) = out.as_mut() {
                    out.push(b'-');
                } else {
                    return Err(name::Error::ReflogPortion);
                }
            }
            b'/' if previous == b'/' => {
                if out.is_none() {
                    return Err(name::Error::RepeatedSlash);
                }
            }
            c => {
                if *c == b'/' {
                    component_start = component_end;
                    component_end = byte_pos;

                    if input[component_start..component_end].ends_with_str(".lock") {
                        if let Some(out) = out.as_mut() {
                            while out.ends_with(b".lock") {
                                let len_without_suffix = out.len() - b".lock".len();
                                out.truncate(len_without_suffix);
                            }
                        } else {
                            return Err(name::Error::LockFileSuffix);
                        }
                    }
                }

                if let Some(out) = out.as_mut() {
                    out.push(*c);
                }

                if byte_pos == last && input[component_end + 1..].ends_with_str(".lock") {
                    if let Some(out) = out.as_mut() {
                        while out.ends_with(b".lock") {
                            let len_without_suffix = out.len() - b".lock".len();
                            out.truncate(len_without_suffix);
                        }
                    } else {
                        return Err(name::Error::LockFileSuffix);
                    }
                }
            }
        }
        previous = *byte;
    }

    if let Some(out) = out.as_mut() {
        while out.last() == Some(&b'/') {
            out.pop();
        }
        while out.first() == Some(&b'/') {
            out.remove(0);
        }
    }
    if out.as_ref().map_or(input, |b| b.as_bstr())[0] == b'.' {
        if let Some(out) = out.as_mut() {
            out[0] = b'-';
        } else {
            return Err(name::Error::StartsWithDot);
        }
    }
    let last = out.as_ref().map_or(input, |b| b.as_bstr()).len() - 1;
    if out.as_ref().map_or(input, |b| b.as_bstr())[last] == b'.' {
        if let Some(out) = out.as_mut() {
            let last = out.len() - 1;
            out[last] = b'-';
        } else {
            return Err(name::Error::EndsWithDot);
        }
    }
    Ok(out)
}
