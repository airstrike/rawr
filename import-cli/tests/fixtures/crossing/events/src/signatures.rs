//! std's types in a crate's signature, in every position (#707, D711
//! step 5): tuples taken and given, `&String`, `&Vec<T>`, `Range<usize>`,
//! `&Path` and `Box<T>`, and a crate type's std trait impls (#709):
//! `Index`, used through `[]`, and `FromStr`, through `.parse()`.

use std::ops::{Index, Range};
use std::path::Path;
use std::str::FromStr;

pub fn area(size: (u32, u32)) -> u32 {
    size.0 * size.1
}

pub fn swap(pair: (i32, String)) -> (String, i32) {
    (pair.1, pair.0)
}

pub fn shout(text: &String) -> String {
    text.to_uppercase()
}

pub fn count(words: &Vec<String>) -> usize {
    words.len()
}

pub fn total(values: &Vec<i64>) -> i64 {
    values.iter().sum()
}

pub fn span(range: Range<usize>) -> usize {
    range.end - range.start
}

pub fn halves(n: usize) -> Range<usize> {
    0..n / 2
}

pub fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn boxed(n: i64) -> Box<i64> {
    Box::new(n * 2)
}

pub fn unbox(n: Box<i64>) -> i64 {
    *n + 1
}

#[derive(Debug, Clone, PartialEq)]
pub struct Coords {
    pub x: i64,
    pub y: i64,
}

impl FromStr for Coords {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let (x, y) = text
            .split_once(',')
            .ok_or_else(|| format!("no comma in {text}"))?;
        let number = |part: &str| part.trim().parse().map_err(|_| format!("bad {part}"));
        Ok(Coords {
            x: number(x)?,
            y: number(y)?,
        })
    }
}

pub struct Grid {
    cells: Vec<i64>,
}

impl Grid {
    pub fn new(n: i64) -> Self {
        Grid {
            cells: (0..n).map(|i| i * 10).collect(),
        }
    }
}

impl Index<usize> for Grid {
    type Output = i64;

    fn index(&self, at: usize) -> &i64 {
        &self.cells[at]
    }
}
