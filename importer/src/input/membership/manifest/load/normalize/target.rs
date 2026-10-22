// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Shared admitted target projection and replacement text helpers.

use super::super::super::{schema::Target, session::Session, storage, value, Error, Step};
use rawr_toml::document;
use std::alloc::Layout;

/// Two passes over pure display inputs; no unadmitted formatting allocation.
pub(in crate::input::membership::manifest) fn formatted<A: document::Admission>(
    arguments: std::fmt::Arguments<'_>,
    session: &mut Session<'_, A>,
) -> Step<String> {
    session.check()?;
    struct Count(usize);
    impl std::fmt::Write for Count {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            self.0 = self.0.checked_add(value.len()).ok_or(std::fmt::Error)?;
            Ok(())
        }
    }
    let mut count = Count(0);
    if std::fmt::write(&mut count, arguments).is_err() {
        return session.fail(Error::Overflow);
    }
    session.string(count.0)?;
    session.items(1)?;
    session.admit(document::Request::Allocation {
        bytes: count.0,
        alignment: 1,
    })?;
    let mut output = String::new();
    if let Err(error) = output.try_reserve_exact(count.0) {
        return session.fail(Error::Allocation(error));
    }
    #[cfg(test)]
    storage::ALLOCATIONS.with(|count| count.set(count.get() + 1));
    struct Write<'a> {
        output: &'a mut String,
        remaining: usize,
    }
    impl std::fmt::Write for Write<'_> {
        fn write_str(&mut self, value: &str) -> std::fmt::Result {
            self.remaining = self
                .remaining
                .checked_sub(value.len())
                .ok_or(std::fmt::Error)?;
            self.output.push_str(value);
            Ok(())
        }
    }
    let mut writer = Write {
        output: &mut output,
        remaining: count.0,
    };
    if std::fmt::write(&mut writer, arguments).is_err() || writer.remaining != 0 {
        return session.fail(Error::State);
    }
    Ok(output)
}

pub(in super::super) fn field<'a, 'd, A: document::Admission>(
    target: Option<&'a Target<'d>>,
    name: &str,
    session: &mut Session<'_, A>,
) -> Step<Option<value::Input<'a, 'd>>> {
    session.check()?;
    let Some(target) = target else {
        return Ok(None);
    };
    let value::View::Map(mut fields) = value::Input::replay(target.values(), 0).view(session)?
    else {
        return session.fail(Error::State);
    };
    while let Some((key, value)) = fields.next(session)? {
        if key.is_some_and(|key| key.text.as_str() == name) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
pub(in super::super) fn string<'d, A: document::Admission>(
    input: Option<value::Input<'_, 'd>>,
    session: &mut Session<'_, A>,
) -> Step<Option<value::Text<'d>>> {
    match input.map(|input| input.view(session)).transpose()? {
        Some(value::View::Scalar(value::Kind::String(text))) => Ok(Some(text)),
        None => Ok(None),
        _ => session.fail(Error::State),
    }
}

pub(super) fn owned<A: document::Admission>(
    parts: &[&str],
    replace_hyphen: bool,
    session: &mut Session<'_, A>,
) -> Step<String> {
    session.check()?;
    let Some(length) = parts
        .iter()
        .try_fold(0usize, |length, part| length.checked_add(part.len()))
    else {
        return session.fail(Error::Overflow);
    };
    session.string(length)?;
    session.items(1)?;
    let Ok(layout) = Layout::array::<u8>(length) else {
        return session.fail(Error::Overflow);
    };
    session.admit(document::Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut output = String::new();
    if let Err(error) = output.try_reserve_exact(length) {
        return session.fail(Error::Allocation(error));
    }
    #[cfg(test)]
    storage::ALLOCATIONS.with(|count| count.set(count.get() + 1));
    for part in parts {
        for ch in part.chars() {
            output.push(if replace_hyphen && ch == '-' { '_' } else { ch });
        }
    }
    Ok(output)
}
