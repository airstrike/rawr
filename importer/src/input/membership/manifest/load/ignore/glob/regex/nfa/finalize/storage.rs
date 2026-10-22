//! Fallible exact vectors and prospectively admitted shared final ownership.
use super::{Admission, Error, Session, Step};
use rawr_toml::document::Request;
use std::{alloc::Layout, sync::Arc};

pub(in super::super) fn vector<T, A: Admission>(
    length: usize,
    session: &mut Session<'_, A>,
) -> Step<Vec<T>> {
    session.check()?;
    if length == 0 {
        return Ok(Vec::new());
    }
    let Ok(layout) = Layout::array::<T>(length) else {
        return session.fail(Error::Overflow);
    };
    session.admit(Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut values = Vec::new();
    if let Err(error) = values.try_reserve_exact(length) {
        return session.fail(Error::Allocation(error));
    }
    Ok(values)
}

pub(in super::super) fn repeat<T: Copy, A: Admission>(
    value: T,
    length: usize,
    session: &mut Session<'_, A>,
) -> Step<Vec<T>> {
    let mut values = vector(length, session)?;
    session.items(length)?;
    values.resize(length, value);
    Ok(values)
}

pub(in super::super) fn copy<T: Copy, A: Admission>(
    source: &[T],
    session: &mut Session<'_, A>,
) -> Step<Vec<T>> {
    let mut values = vector(source.len(), session)?;
    session.items(source.len())?;
    values.extend_from_slice(source);
    Ok(values)
}

/// Replace shrinking conversions with an admitted exact destination. The old
/// capacity remains owned until every element has moved; no charge is refunded.
pub(super) fn compact<T, A: Admission>(
    source: crate::input::membership::manifest::storage::Buffer<T>,
    session: &mut Session<'_, A>,
) -> Step<Vec<T>> {
    let mut values = vector(source.len(), session)?;
    session.items(source.len())?;
    values.extend(source.into_iter());
    Ok(values)
}

pub(super) fn shared<T, A: Admission>(value: T, session: &mut Session<'_, A>) -> Step<Arc<T>> {
    let Ok((layout, _)) = Layout::new::<[usize; 2]>().extend(Layout::new::<T>()) else {
        return session.fail(Error::Overflow);
    };
    let layout = layout.pad_to_align();
    session.admit(Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    Ok(Arc::new(value))
}
