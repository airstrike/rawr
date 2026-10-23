//! Bounded authenticated-runner transport for deterministic snapshot outcomes.

use std::path::{Path, PathBuf};

use super::{Class, Commit, Difference, Outcome};

const VERSION: u32 = 1;
const LIMIT: usize = 8 * 1024 * 1024;

/// Encode one complete snapshot outcome for the existing bounded runner frame.
pub fn encode(outcome: &Outcome) -> Result<Box<[u8]>, Error> {
    let mut output = Vec::new();
    put(&mut output, &VERSION.to_le_bytes())?;
    path(&mut output, outcome.baseline())?;
    path(&mut output, outcome.actual())?;
    number(&mut output, outcome.artifacts())?;
    byte(&mut output, u8::from(outcome.updated()))?;
    number(&mut output, outcome.differences().len())?;
    for difference in outcome.differences() {
        string(&mut output, difference.identity())?;
        string(&mut output, difference.theme())?;
        byte(
            &mut output,
            match difference.class() {
                Class::Missing => 0,
                Class::Pixel => 1,
                Class::Size => 2,
                Class::Stale => 3,
                Class::Input => 4,
            },
        )?;
        path(&mut output, difference.expected())?;
        path(&mut output, difference.actual())?;
        string(&mut output, difference.detail())?;
    }
    number(&mut output, outcome.notices().len())?;
    for notice in outcome.notices() {
        path(&mut output, notice.target())?;
        path(&mut output, notice.recoverable())?;
        string(&mut output, &notice.source().to_string())?;
    }
    Ok(output.into_boxed_slice())
}

/// Decode one exact bounded child outcome without reading snapshot artifacts.
pub fn decode(bytes: &[u8]) -> Result<Outcome, Error> {
    if bytes.len() > LIMIT {
        return Err(Error::Limit);
    }
    let mut input = bytes;
    if fixed::<4>(&mut input).map(u32::from_le_bytes)? != VERSION {
        return Err(Error::Version);
    }
    let baseline = take_path(&mut input)?;
    let actual = take_path(&mut input)?;
    let artifacts = take_number(&mut input)?;
    let updated = match take_byte(&mut input)? {
        0 => false,
        1 => true,
        _ => return Err(Error::Malformed),
    };
    let differences_len = take_number(&mut input)?;
    let mut differences = Vec::new();
    differences
        .try_reserve_exact(differences_len)
        .map_err(|_| Error::Allocation)?;
    for _ in 0..differences_len {
        let identity = take_string(&mut input)?;
        let theme = take_string(&mut input)?;
        let class = match take_byte(&mut input)? {
            0 => Class::Missing,
            1 => Class::Pixel,
            2 => Class::Size,
            3 => Class::Stale,
            4 => Class::Input,
            _ => return Err(Error::Malformed),
        };
        let expected = take_path(&mut input)?;
        let rendered = take_path(&mut input)?;
        let detail = take_string(&mut input)?;
        differences.push(Difference::new(
            identity, theme, class, expected, rendered, detail,
        ));
    }
    let notices_len = take_number(&mut input)?;
    let mut commit = Commit::new();
    for _ in 0..notices_len {
        let target = take_path(&mut input)?;
        let recoverable = take_path(&mut input)?;
        let message = take_string(&mut input)?;
        commit.warn(target, recoverable, std::io::Error::other(message));
    }
    if !input.is_empty() {
        return Err(Error::Malformed);
    }
    Ok(Outcome::new(
        baseline,
        actual,
        artifacts,
        differences,
        updated,
        commit,
    ))
}

fn byte(output: &mut Vec<u8>, value: u8) -> Result<(), Error> {
    put(output, &[value])
}

fn number(output: &mut Vec<u8>, value: usize) -> Result<(), Error> {
    let value = u64::try_from(value).map_err(|_| Error::Limit)?;
    put(output, &value.to_le_bytes())
}

fn string(output: &mut Vec<u8>, value: &str) -> Result<(), Error> {
    number(output, value.len())?;
    put(output, value.as_bytes())
}

fn path(output: &mut Vec<u8>, value: &Path) -> Result<(), Error> {
    string(output, value.to_str().ok_or(Error::Path)?)
}

fn put(output: &mut Vec<u8>, value: &[u8]) -> Result<(), Error> {
    let actual = output.len().checked_add(value.len()).ok_or(Error::Limit)?;
    if actual > LIMIT {
        return Err(Error::Limit);
    }
    output
        .try_reserve_exact(value.len())
        .map_err(|_| Error::Allocation)?;
    output.extend_from_slice(value);
    Ok(())
}

fn take_byte(input: &mut &[u8]) -> Result<u8, Error> {
    fixed::<1>(input).map(|bytes| bytes[0])
}

fn take_number(input: &mut &[u8]) -> Result<usize, Error> {
    usize::try_from(fixed::<8>(input).map(u64::from_le_bytes)?).map_err(|_| Error::Limit)
}

fn take_string(input: &mut &[u8]) -> Result<String, Error> {
    let length = take_number(input)?;
    let bytes = take(input, length)?;
    let value = std::str::from_utf8(bytes).map_err(|_| Error::Encoding)?;
    let mut retained = String::new();
    retained
        .try_reserve_exact(value.len())
        .map_err(|_| Error::Allocation)?;
    retained.push_str(value);
    Ok(retained)
}

fn take_path(input: &mut &[u8]) -> Result<PathBuf, Error> {
    Ok(PathBuf::from(take_string(input)?))
}

fn fixed<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], Error> {
    take(input, N)?.try_into().map_err(|_| Error::Malformed)
}

fn take<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], Error> {
    if length > input.len() {
        return Err(Error::Malformed);
    }
    let (value, remaining) = input.split_at(length);
    *input = remaining;
    Ok(value)
}

/// Snapshot-outcome wire rejection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("snapshot outcome wire version is unsupported")]
    Version,
    #[error("snapshot outcome wire is malformed")]
    Malformed,
    #[error("snapshot outcome wire exceeds its fixed bound")]
    Limit,
    #[error("snapshot outcome wire contains a non-UTF-8 path")]
    Path,
    #[error("snapshot outcome wire contains invalid UTF-8")]
    Encoding,
    #[error("snapshot outcome wire allocation failed")]
    Allocation,
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};
    use crate::snapshot::Mode;

    #[test]
    fn actual_snapshot_outcome_round_trips_every_public_scalar() {
        let root = std::env::temp_dir().join(format!(
            "rawr-snapshot-wire-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        crate::write_marked(&root, "src/main.rs", include_str!("../../examples/package_snapshots/src/main.rs"));
        let compiler = crate::preview::Compiler::new().unwrap();
        let target = rawr::project::Target::discover(&root).unwrap();
        let checked = compiler.check(&target).unwrap();
        let outcome = checked.snapshots().unwrap().execute(Mode::Update).unwrap();
        let decoded = decode(&encode(&outcome).unwrap()).unwrap();
        assert_eq!(decoded.artifacts(), outcome.artifacts());
        assert_eq!(decoded.updated(), outcome.updated());
        assert_eq!(decoded.baseline(), outcome.baseline());
        assert_eq!(decoded.actual(), outcome.actual());
        std::fs::remove_dir_all(root).ok();
    }
}
