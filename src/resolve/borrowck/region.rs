//! A value's borrows by the region positions of its type, as rustc's regions
//! are (T127). A position is a [`Path`] of [`Step`]s into the type: a
//! reference's own region is `[Own]` and what it refers to is below
//! `[Arg(0)]`, a type's arguments and a tuple's members are `[Arg(i)]`, and
//! a crate type's lifetime parameters are `[Lifetime(i)]`.
//!
//! A value's borrows are a [`Held`]: origins placed at positions. One placed
//! at a position is held at every position below it, so a value placed at
//! the root is held at all of them, as every borrow of a value was before
//! T127.
//!
//! A value's own region (`Own`) holds everything below the value as well,
//! as `&'a T<'b>` has `'b: 'a` and an iterator's items outlive it: reading
//! a position that ends with `Own` reads all of the value it is the own
//! region of.

use super::{Body, Flow, LoanId, Local, Origin};

/// One step from a value toward a region position in its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Step {
    /// The value's own region: a reference's, an iterator's or an opaque
    /// value's, which outlives nothing below it and is outlived by all of
    /// it.
    Own,
    /// A type argument, a tuple's member, an element, or what a reference
    /// refers to (`Arg(0)`).
    Arg(u8),
    /// A crate type's lifetime parameter, by its position.
    Lifetime(u8),
}

/// The most steps a path keeps: a longer one is cut to the value above,
/// which holds everything below it.
const DEPTH: usize = 8;
/// The most arguments or lifetimes a step can name.
const WIDTH: u8 = 63;

impl Step {
    fn code(self) -> u8 {
        match self {
            Self::Own => 1,
            Self::Arg(index) => 2 + index.min(WIDTH - 1),
            Self::Lifetime(index) => 2 + WIDTH + index.min(WIDTH - 1),
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Own,
            code if code < 2 + WIDTH => Self::Arg(code - 2),
            code => Self::Lifetime(code - 2 - WIDTH),
        }
    }
}

/// A region position in a value's type: up to [`DEPTH`] steps, packed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Path(u64);

impl Path {
    /// The value itself: every position of it.
    pub(crate) const ROOT: Self = Self(0);
    /// The value's own region, which holds all of it.
    pub(crate) const OWN: Self = Self(1);

    /// The path of `steps`.
    pub(crate) fn of(steps: &[Step]) -> Self {
        steps.iter().fold(Self::ROOT, |path, step| path.push(*step))
    }

    pub(crate) fn len(self) -> usize {
        (64 - self.0.leading_zeros() as usize).div_ceil(8)
    }

    pub(crate) fn is_root(self) -> bool {
        self.0 == 0
    }

    fn mask(steps: usize) -> u64 {
        if steps >= DEPTH {
            u64::MAX
        } else {
            (1 << (8 * steps)) - 1
        }
    }

    /// The steps, from the value down.
    pub(crate) fn steps(self) -> impl Iterator<Item = Step> {
        (0..self.len()).map(move |index| Step::from_code((self.0 >> (8 * index)) as u8))
    }

    pub(crate) fn last(self) -> Option<Step> {
        let len = self.len();
        (len > 0).then(|| Step::from_code((self.0 >> (8 * (len - 1))) as u8))
    }

    /// The path above the last step.
    pub(crate) fn parent(self) -> Self {
        Self(self.0 & Self::mask(self.len().saturating_sub(1)))
    }

    /// One step further down, or `None` past [`DEPTH`].
    fn try_push(self, step: Step) -> Option<Self> {
        let len = self.len();
        (len < DEPTH).then(|| Self(self.0 | u64::from(step.code()) << (8 * len)))
    }

    /// One step further down; past [`DEPTH`], the path as it is, which
    /// holds everything below it.
    pub(crate) fn push(self, step: Step) -> Self {
        self.try_push(step).unwrap_or(self)
    }

    /// `below` under this path, or `None` past [`DEPTH`].
    fn try_join(self, below: Self) -> Option<Self> {
        below
            .steps()
            .try_fold(self, |path, step| path.try_push(step))
    }

    /// `below` under this path; past [`DEPTH`], as far as it goes.
    pub(crate) fn join(self, below: Self) -> Self {
        below.steps().fold(self, Self::push)
    }

    pub(crate) fn starts_with(self, prefix: Self) -> bool {
        prefix.len() <= self.len() && self.0 & Self::mask(prefix.len()) == prefix.0
    }

    /// What is left below `prefix`, which this path starts with.
    pub(crate) fn strip(self, prefix: Self) -> Self {
        Self(self.0.checked_shr(8 * prefix.len() as u32).unwrap_or(0))
    }

    /// Whether this is an own region: reading it reads all of the value
    /// above it.
    pub(crate) fn is_own(self) -> bool {
        self.last() == Some(Step::Own)
    }
}

/// `origin` narrowed to the position `below` it: what a local holds there.
/// A loan is one region, held wherever it is placed.
pub(crate) fn narrow(origin: Origin, below: Path) -> Origin {
    match origin {
        Origin::Loan(_) => origin,
        // All of a value below its own region is that region already.
        Origin::Local(_, path) if path.is_own() => origin,
        Origin::Local(local, path) => match path.try_join(below) {
            Some(path) => Origin::Local(local, path),
            None => Origin::Local(local, cut(path.join(below))),
        },
    }
}

/// A path past [`DEPTH`] cut to an own region above, which holds all of
/// what it cut.
fn cut(path: Path) -> Path {
    Path(path.0 & Path::mask(DEPTH - 1)).push(Step::Own)
}

/// A value's borrows: each origin and the position it is placed at.
pub(crate) type Held = Vec<(Path, Origin)>;

/// `origins` held at every position of a value, whose structure these facts
/// don't know: each is all of what it names.
pub(crate) fn everywhere(origins: impl IntoIterator<Item = Origin>) -> Held {
    origins
        .into_iter()
        .map(|origin| (Path::ROOT, narrow(origin, Path::OWN)))
        .collect()
}

/// What a value holding `held` holds at the position `at`, as a value of
/// its own.
pub(crate) fn sub(held: &[(Path, Origin)], at: Path) -> Held {
    let mut out = Vec::new();
    for &(placed, origin) in held {
        let entry = if at.starts_with(placed) {
            // Placed above: held at every position below, `at`'s among them.
            (Path::ROOT, narrow(origin, at.strip(placed)))
        } else if placed.starts_with(at) {
            (placed.strip(at), origin)
        } else if at.is_own() && placed.starts_with(at.parent()) {
            // Below the value whose own region `at` is: all of it.
            (Path::ROOT, narrow(origin, Path::OWN))
        } else {
            continue;
        };
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

/// `held` placed at `at` of a bigger value.
pub(crate) fn place(held: &[(Path, Origin)], at: Path) -> Held {
    held.iter()
        .map(|&(placed, origin)| match at.try_join(placed) {
            Some(path) => (path, origin),
            // Too deep: held at all of what the path is cut to.
            None => (at.join(placed), narrow(origin, Path::OWN)),
        })
        .collect()
}

/// All of `held` at one position: the value's own region.
pub(crate) fn collapse(held: &[(Path, Origin)]) -> Held {
    sub(held, Path::OWN)
}

/// The origins of `held`, wherever they are placed.
pub(crate) fn origins(held: &[(Path, Origin)]) -> impl Iterator<Item = Origin> + '_ {
    held.iter().map(|(_, origin)| *origin)
}

/// For each loan, the locals its region reaches through the flows: where
/// one of them is live, the loan is live (rustc's outlives constraints at
/// every point). A loan placed at a position of a local reaches another
/// local only through a flow from that position, from a position above it,
/// or from the own region of a value it is below.
pub(crate) fn reach(body: &Body) -> Vec<Vec<bool>> {
    let mut by_source = vec![Vec::<&Flow>::new(); body.locals.len()];
    let mut from_loans = vec![Vec::<&Flow>::new(); body.loans.len()];
    for flow in &body.flows {
        match flow.from {
            Origin::Local(local, _) => by_source[local.0 as usize].push(flow),
            Origin::Loan(loan) => from_loans[loan.0 as usize].push(flow),
        }
    }
    let mut reaches = Vec::with_capacity(body.loans.len());
    for (index, loan) in body.loans.iter().enumerate() {
        let mut reached = vec![false; body.locals.len()];
        // The positions reached, by local.
        let mut at = vec![Vec::<Path>::new(); body.locals.len()];
        let mut stack = vec![(loan.temporary, Path::ROOT)];
        stack.extend(
            from_loans[LoanId(index as u32).0 as usize]
                .iter()
                .map(|flow| (flow.into, flow.path)),
        );
        while let Some((local, path)) = stack.pop() {
            let positions = &mut at[local.0 as usize];
            // Held at a position above already.
            if positions.iter().any(|above| path.starts_with(*above)) {
                continue;
            }
            positions.retain(|below| !below.starts_with(path));
            positions.push(path);
            reached[local.0 as usize] = true;
            for flow in &by_source[local.0 as usize] {
                let Origin::Local(_, from) = flow.from else {
                    continue;
                };
                let into = if path.starts_with(from) {
                    flow.path.join(path.strip(from))
                } else if from.starts_with(path) || from.is_own() && path.starts_with(from.parent())
                {
                    flow.path
                } else {
                    continue;
                };
                stack.push((flow.into, into));
            }
        }
        reaches.push(reached);
    }
    reaches
}

/// Whether each loan reaches one of `sinks`.
pub(crate) fn reaches_any(reach: &[Vec<bool>], sinks: &[Local]) -> Vec<bool> {
    reach
        .iter()
        .map(|reached| sinks.iter().any(|sink| reached[sink.0 as usize]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A0: Step = Step::Arg(0);
    const A1: Step = Step::Arg(1);

    #[test]
    fn paths_pack_and_compare() {
        let path = Path::of(&[A0, Step::Lifetime(2), Step::Own]);
        assert_eq!(path.len(), 3);
        assert_eq!(
            path.steps().collect::<Vec<_>>(),
            [A0, Step::Lifetime(2), Step::Own]
        );
        assert!(path.starts_with(Path::of(&[A0])));
        assert!(!path.starts_with(Path::of(&[A1])));
        assert_eq!(
            path.strip(Path::of(&[A0])),
            Path::of(&[Step::Lifetime(2), Step::Own])
        );
        assert_eq!(path.parent(), Path::of(&[A0, Step::Lifetime(2)]));
        assert!(path.is_own());
        let deep = Path::of(&[A0; 9]);
        assert_eq!(deep.len(), 8);
    }

    #[test]
    fn sub_reads_positions() {
        let loan = Origin::Loan(LoanId(0));
        let v = Origin::Local(Local(1), Path::ROOT);
        // `&v` of a `Vec<&T>`: the loan's own region, and `v` below it.
        let reference: Held = vec![(Path::OWN, loan), (Path::of(&[A0]), v)];
        // The element's position: `v`'s element, without the loan.
        assert_eq!(
            sub(&reference, Path::of(&[A0, A0])),
            vec![(Path::ROOT, Origin::Local(Local(1), Path::of(&[A0])))]
        );
        // The own region: all of it.
        assert_eq!(
            sub(&reference, Path::OWN),
            vec![
                (Path::ROOT, loan),
                (Path::ROOT, Origin::Local(Local(1), Path::OWN))
            ]
        );
    }
}
