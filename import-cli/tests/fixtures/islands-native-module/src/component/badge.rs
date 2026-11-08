// rawr
//! A leaf of an existing app, marked to hot-reload: it names the native
//! `page` module's trait, function, constants and nested alias by path and
//! through `use`, and takes and gives the native trait's objects and
//! values of the aliases, which cross as their targets do.

use crate::page;
use crate::page::size::Width;
use crate::page::Page;

pub fn keep(page: Box<dyn crate::page::Page>) -> Box<dyn Page> {
    pass(page)
}

fn pass(page: Box<dyn Page>) -> Box<dyn page::Page> {
    let kept: Box<dyn crate::page::Page> = page;
    let named: Box<dyn page::Page> = kept;
    let again: Box<dyn Page> = named;
    again
}

pub fn pick(first: Box<dyn Page>, second: Box<dyn page::Page>, wide: bool) -> Box<dyn Page> {
    if wide {
        second
    } else {
        first
    }
}

pub fn scaled(by: usize) -> usize {
    widened(by) + crate::page::SCALE
}

fn widened(by: Width) -> Width {
    crate::page::scale(by) + page::size::BASE
}

pub fn widen(by: page::size::Width) -> Width {
    widened(by)
}

pub fn measure(by: &Width) -> page::size::Width {
    *by * 2
}

pub fn titled(page: page::Landing) -> String {
    page.title()
}

pub fn grow(by: &mut page::size::Width, tally: &mut page::Tally, again: &mut Width) {
    *by += 3;
    tally.add(*by);
    *again *= *by;
}
