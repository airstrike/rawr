//! A stored `&mut` of a range of an array's elements (D1113): the
//! reference `[handle, part, .., low..high]`, whose last step is the range,
//! normalized where the reference is made. A view of a view, an element of
//! one, and the views `split_at_mut`, `chunks_mut` and `iter_mut` give are
//! references below the same handle, so a write through any of them
//! reaches the array itself.

use super::{Target, Window};
use crate::ast::Expr;
use crate::{
    Array, Dynamic, Engine, ExclusiveRange, InclusiveRange, RawrResultOf, StaticVec, ERR, INT,
};

/// `PreludeOp::View`'s code for the reference to a part of a view: an
/// element by its index, or a range of elements.
pub const PART: INT = 0;
/// `PreludeOp::View`'s code for `split_at_mut(mid)`: the two views.
pub const SPLIT: INT = 1;
/// `PreludeOp::View`'s code for `iter_mut()`: each element's reference.
pub const ELEMENTS: INT = 2;
/// `PreludeOp::View`'s code for `chunks_mut(size)`: each chunk's view.
pub const CHUNKS: INT = 3;

/// A place's path (`Engine::windowed_path`): where it starts, the parts
/// below, and the range of elements it ends with, if any.
pub(crate) type Path = (Dynamic, StaticVec<usize>, Option<(usize, usize)>);

/// Where a reference reaches: below its shared handle, the parts at
/// `steps`, then the elements `window` names, if any.
struct Reached {
    handle: Dynamic,
    steps: StaticVec<usize>,
    window: Option<(usize, usize)>,
}

impl Reached {
    /// The place `path` names below `handle`: each index is a part, and a
    /// range is a window of the array reached so far, checked against its
    /// length as `Index` on a slice checks it. An index or a range after a
    /// window is one of the window's own elements.
    fn new(handle: Dynamic, path: &[Dynamic]) -> Result<Self, String> {
        let mut reached = Self {
            handle,
            steps: StaticVec::new(),
            window: None,
        };
        for step in path {
            reached.step(step)?;
        }
        Ok(reached)
    }

    /// `self` reaching `step` further: an index, or a range.
    fn step(&mut self, step: &Dynamic) -> Result<(), String> {
        if let Ok(index) = step.as_int() {
            let index = usize::try_from(index).unwrap_or(usize::MAX);
            match self.window.take() {
                Some((low, high)) if index >= high - low => {
                    return Err(format!(
                        "index out of bounds: the len is {} but the index is {index}",
                        high - low
                    ));
                }
                Some((low, _)) => self.steps.push(low + index),
                None => self.steps.push(index),
            }
            return Ok(());
        }
        let (low, high, inclusive) =
            bounds(step).ok_or_else(|| "a view's step is no index or range".to_string())?;
        let (base, len) = match self.window {
            Some((low, high)) => (low, high - low),
            None => (0, self.length()?),
        };
        let (low, high) = crate::resolve::lower_nominal::slice_window(low, high, inclusive, len)?;
        self.window = Some((base + low, base + high));
        Ok(())
    }

    /// The number of elements `self` reaches: its window's, or the array's.
    fn len(&self) -> Result<usize, String> {
        match self.window {
            Some((low, high)) => Ok(high - low),
            None => self.length(),
        }
    }

    /// The length of the array at `steps` below the handle.
    fn length(&self) -> Result<usize, String> {
        let missing = || "a view's place is no array".to_string();
        let guard = self.handle.read_lock::<Dynamic>().ok_or_else(missing)?;
        let mut place = &*guard;
        for &step in &self.steps {
            place = place.part(step).ok_or_else(missing)?;
        }
        let array = place.as_array_ref().map_err(|_| missing())?;
        Ok(array.len())
    }

    /// The reference `self` is: `[handle, part, .., low..high]`.
    fn reference(&self) -> Dynamic {
        let mut reference = Array::with_capacity(self.steps.len() + 2);
        reference.push(self.handle.clone());
        reference.extend(
            self.steps
                .iter()
                .map(|&step| Dynamic::from_int(step as INT)),
        );
        if let Some((low, high)) = self.window {
            let range: ExclusiveRange = (low as INT)..(high as INT);
            reference.push(Dynamic::from(range));
        }
        Dynamic::from_array(reference)
    }

    /// The reference to `step` below `self`.
    fn part(&self, step: &Dynamic) -> Result<Dynamic, String> {
        let mut part = Self {
            handle: self.handle.clone(),
            steps: self.steps.clone(),
            window: self.window,
        };
        part.step(step)?;
        Ok(part.reference())
    }

    /// The reference to the elements `low..high` of `self`.
    fn window(&self, low: usize, high: usize) -> Result<Dynamic, String> {
        let range: ExclusiveRange = (low as INT)..(high as INT);
        self.part(&Dynamic::from(range))
    }
}

/// A range step's bounds: `low`, `high` (`None` for an open end, which a
/// place's text writes as `-1`) and whether `high` is included.
fn bounds(step: &Dynamic) -> Option<(usize, Option<usize>, bool)> {
    let at = |bound: INT| usize::try_from(bound).unwrap_or(usize::MAX);
    if let Some(range) = step.read_lock::<ExclusiveRange>() {
        let high = (range.end != -1).then(|| at(range.end));
        return Some((at(range.start), high, false));
    }
    let range = step.read_lock::<InclusiveRange>()?;
    Some((at(*range.start()), Some(at(*range.end())), true))
}

impl Engine {
    /// The reference `__rawr_ref` makes of the place `path` names below
    /// the shared handle `handle`: its steps, with any range of elements
    /// normalized and checked, as Rust checks `&mut v[low..high]` where it
    /// is borrowed. `Err` is Rust's panic.
    pub(crate) fn reference(handle: Dynamic, path: &[Dynamic]) -> Result<Dynamic, String> {
        Ok(Reached::new(handle, path)?.reference())
    }

    /// A stored `&mut [T]` (`KEYWORD_REF`), `[handle, part, .., low..high]`:
    /// its handle, the parts below it, and the range of the array's
    /// elements its last step names. `None` for another value.
    pub(crate) fn reference_window(
        reference: &Dynamic,
    ) -> Option<(Dynamic, StaticVec<usize>, usize, usize)> {
        // A reference a closure captured is shared: its steps are read out.
        if reference.is_shared() && !reference.is_array() {
            return Self::reference_window(&reference.flatten_clone());
        }
        let path = reference.as_array_ref().ok()?;
        let (window, path) = path.split_last()?;
        let (handle, steps) = path.split_first()?;
        if !handle.is_shared() {
            return None;
        }
        let range = window.read_lock::<ExclusiveRange>()?;
        let low = usize::try_from(range.start).ok()?;
        let high = usize::try_from(range.end).ok()?;
        let steps = steps
            .iter()
            .map(|step| usize::try_from(step.as_int().ok()?).ok())
            .collect::<Option<_>>()?;
        Some((handle.clone(), steps, low, high))
    }

    /// The target of the elements `low..high` of the array at `steps`
    /// below `handle`, which a stored `&mut [T]` reaches (`Window`). `name`
    /// is what reached it, for an error.
    pub(crate) fn reach_window(
        handle: Dynamic,
        steps: StaticVec<usize>,
        (low, high): (usize, usize),
        name: &str,
        expr: &Expr,
    ) -> RawrResultOf<Target<'static>> {
        Window::take(handle, steps, low, high)
            .map(|window| Target::Window(Box::new(window)))
            .ok_or_else(|| ERR::ErrorDataRace(name.to_string(), expr.position()).into())
    }

    /// A place's path as `held_path` reads it (where its local is, or its
    /// handle, then the parts below it), and the range of elements a
    /// stored `&mut [T]`'s path ends with (`reference_window`).
    pub(crate) fn windowed_path(held: &Dynamic) -> Option<Path> {
        if let Some((handle, steps, low, high)) = Self::reference_window(held) {
            return Some((handle, steps, Some((low, high))));
        }
        let (root, steps) = Self::held_path(held)?;
        Some((root, steps, None))
    }

    /// `PreludeOp::View`: what `code` makes of the stored `&mut` `reference`
    /// (`PART`, `SPLIT`, `ELEMENTS`, `CHUNKS`), given `argument`. `Err` is
    /// Rust's panic.
    pub(crate) fn view(
        reference: &Dynamic,
        code: INT,
        argument: &Dynamic,
    ) -> Result<Dynamic, String> {
        let reference = reference.flatten_clone();
        let path = reference
            .as_array_ref()
            .map_err(|_| "a view is no stored reference".to_string())?;
        let (handle, path) = path
            .split_first()
            .filter(|(handle, _)| handle.is_shared())
            .ok_or_else(|| "a view is no stored reference".to_string())?;
        // A native vector a stored `&mut` names, as a crate's `&mut
        // Vec<Tree>` field lent where it is (D651): its elements are the
        // places of its own elements, each claimed for its use.
        if path.is_empty()
            && let Some(place) = handle
                .read_lock::<Dynamic>()
                .and_then(|held| crate::place::get(&held))
        {
            return native_view(&place, code, argument);
        }
        let reached = Reached::new(handle.clone(), path)?;
        let len = reached.len()?;
        let count = || usize::try_from(argument.as_int().unwrap_or(-1)).ok();
        let views = match code {
            PART => return reached.part(argument),
            SPLIT => {
                let mid = count().filter(|mid| *mid <= len).ok_or("mid > len")?;
                vec![reached.window(0, mid)?, reached.window(mid, len)?]
            }
            ELEMENTS => (0..len)
                .map(|index| reached.part(&Dynamic::from_int(index as INT)))
                .collect::<Result<_, _>>()?,
            CHUNKS => {
                let size = count().filter(|size| *size > 0);
                let size = size.ok_or("chunk size must be non-zero")?;
                (0..len)
                    .step_by(size)
                    .map(|low| reached.window(low, (low + size).min(len)))
                    .collect::<Result<_, _>>()?
            }
            _ => return Err("an unknown view".into()),
        };
        Ok(Dynamic::from_array(views))
    }
}

/// [`Engine::view`] of a native place: an element by its index, or the
/// place of each element. A native vector's range has no view yet.
fn native_view(
    place: &crate::place::Place,
    code: INT,
    argument: &Dynamic,
) -> Result<Dynamic, String> {
    let failed = |error: Box<ERR>| error.to_string();
    match code {
        PART if argument.as_int().is_ok() => place.element(argument).map_err(failed),
        ELEMENTS => {
            let len = place
                .invoke(crate::place::Operation::Len, &mut [])
                .map_err(failed)?
                .as_int()
                .map_err(|_| "a native vector's length is no integer".to_string())?;
            let elements = (0..len)
                .map(|index| place.element(&Dynamic::from_int(index)).map_err(failed))
                .collect::<Result<Array, _>>()?;
            Ok(Dynamic::from_array(elements))
        }
        _ => Err("a range of a native vector lent where it is isn't supported yet".into()),
    }
}
