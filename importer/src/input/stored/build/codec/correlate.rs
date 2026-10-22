//! Total public-artifact coverage with bounded borrowed lookup work.

use std::cmp::Ordering;

use crate::cargo::unit;
use crate::input::build;
use crate::input::stored::{Budget, Error, Result};

#[cfg(test)]
std::thread_local! {
    static COMPARISONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn compared() {
    #[cfg(test)]
    COMPARISONS.with(|count| count.set(count.get() + 1));
}

fn outputs(left: &unit::Output, right: &unit::Output) -> Ordering {
    compared();
    crate::input::public_output_key(left).cmp(&crate::input::public_output_key(right))
}

pub(super) fn require(
    expected: &[unit::Output],
    artifacts: &[build::Artifact],
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.limited(
        "Build compiler artifact units",
        budget.limits.units,
        expected.len(),
    )?;
    budget.limited(
        "Cargo compiler artifacts",
        budget.limits.units,
        artifacts.len(),
    )?;
    let bytes = expected
        .len()
        .checked_mul(std::mem::size_of::<(&unit::Output, bool)>())
        .ok_or_else(|| budget.overflow())?;
    budget.storage(expected.len(), bytes)?;
    let mut index = Vec::new();
    index
        .try_reserve_exact(expected.len())
        .map_err(|error| budget.failure(Error::Allocation(error)))?;
    index.extend(expected.iter().map(|output| (output, false)));
    index.sort_unstable_by(|left, right| outputs(left.0, right.0));
    index.dedup_by(|left, right| outputs(left.0, right.0) == Ordering::Equal);
    let mut unexpected = false;
    for artifact in artifacts {
        match index.binary_search_by(|(output, _)| {
            compared();
            crate::input::compare_artifact(output, artifact)
        }) {
            Ok(found) => index[found].1 = true,
            Err(_) => unexpected = true,
        }
    }
    // Preserve the legacy cause precedence, including its original expected order.
    for output in expected {
        let found = index
            .binary_search_by(|(candidate, _)| outputs(candidate, output))
            .map_err(|_| Error::Encoding("artifact coverage index differs"))?;
        if !index[found].1 {
            return Err(Error::Encoding("missing-build-artifact"));
        }
    }
    if unexpected {
        return Err(Error::Encoding("unexpected-build-artifact"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
