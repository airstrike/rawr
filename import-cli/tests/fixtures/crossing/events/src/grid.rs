//! A buffer made by calling a closure for each cell, as image's
//! `ImageBuffer::from_fn(width, height, |x, y| ..)` takes an `FnMut` with
//! no `'static` bound, and function pointers of any arity.

/// A grid of cells, as image's `ImageBuffer`.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    width: u32,
    height: u32,
    cells: Vec<u8>,
}

impl Grid {
    /// A `width` by `height` grid whose cell at `(x, y)` is `f(x, y)`,
    /// called once per cell, row by row.
    pub fn from_fn<F>(width: u32, height: u32, mut f: F) -> Grid
    where
        F: FnMut(u32, u32) -> u8,
    {
        let mut cells = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                cells.push(f(x, y));
            }
        }
        Grid {
            width,
            height,
            cells,
        }
    }

    /// Each cell replaced by `f` of it, through `impl FnMut`.
    pub fn map(mut self, mut f: impl FnMut(u8) -> u8) -> Grid {
        for cell in &mut self.cells {
            *cell = f(*cell);
        }
        self
    }

    /// The cell at `(x, y)`.
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.cells[(y * self.width + x) as usize]
    }

    /// The cells folded by `f`, a function pointer of two inputs.
    pub fn fold(&self, start: u32, f: fn(u32, u8) -> u32) -> u32 {
        self.cells.iter().fold(start, |sum, cell| f(sum, *cell))
    }

    /// Its size, as `width x height`.
    pub fn size(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }
}

/// `f` of `a`, `b` and `c`, a function pointer of three inputs.
pub fn combine(a: i64, b: i64, c: i64, f: fn(i64, i64, i64) -> i64) -> i64 {
    f(a, b, c)
}

/// `f` of nothing, a function pointer of no input.
pub fn produce(f: fn() -> i64) -> i64 {
    f()
}
