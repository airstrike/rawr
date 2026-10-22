// Copyright the faster-hex contributors. MIT.
//! faster-hex0.10.0's reached ARM scalar decoder, fixed to gix's default SHA-1.

use rawr_toml::document;

use crate::input::membership::manifest::{session::Session, Error, Step};

const NIL: u8 = u8::MAX;

const fn unhex() -> [u8; 256] {
    let mut values = [0; 256];
    let mut index = 0;
    while index < 256 {
        values[index] = match index as u8 {
            b'0'..=b'9' => index as u8 - b'0',
            b'a'..=b'f' => index as u8 - b'a' + 10,
            b'A'..=b'F' => index as u8 - b'A' + 10,
            _ => NIL,
        };
        index += 1;
    }
    values
}

const fn unhex4() -> [u8; 256] {
    let unhex = unhex();
    let mut values = [NIL; 256];
    let mut index = 0;
    while index < 256 {
        if unhex[index] != NIL {
            values[index] = unhex[index] << 4;
        }
        index += 1;
    }
    values
}

static UNHEX: [u8; 256] = unhex();
static UNHEX4: [u8; 256] = unhex4();

/// Prior gix validation guarantees 40 hex bytes. Preserve the decoder's second
/// validation pass; a violated private precondition is terminal, not a new
/// source semantic parse error or a selectable repository fallback.
pub(super) fn decode<A: document::Admission>(
    source: &[u8],
    session: &mut Session<'_, A>,
) -> Step<[u8; 20]> {
    session.items(1)?;
    let mut output = [0; 20];
    if source.len() != output.len() * 2 || !source.iter().all(|&byte| UNHEX[byte as usize] != NIL) {
        return session.fail(Error::State);
    }
    for (slot, bytes) in output.iter_mut().zip(source.as_chunks::<2>().0.iter()) {
        *slot = UNHEX4[bytes[0] as usize] | UNHEX[bytes[1] as usize];
    }
    Ok(output)
}
