//! Helpers that make frames, as iced's `widget::helpers` makes widgets: a
//! private module whose items the crate's root re-exports by a glob, so
//! `events::framed` is both the module `framed` and the function here.

use crate::core;
use crate::framed::{self, Framed, Shown};

/// `item` framed by hashes, through the `framed` module's own generic code.
pub fn framed<S: Shown>(item: S) -> Framed<S> {
    framed::framed(item).edge('#')
}

/// `item`'s frame tinted red by `color`, a type named through the crate
/// `core` names, as iced's `helpers` names `core::text::parser::PlainText`.
pub fn reddened<S: Shown>(item: S, color: core::graphics::color::Color) -> Framed<S> {
    framed(item).edge(if color.r > 0.5 { '!' } else { '#' })
}

/// `item` painted by `palette` with `class`, whose bound names the
/// `framed` module's generic alias by its path, as iced's `helpers` names
/// `container::StyleFn`.
pub fn painted<'a, S, P: framed::Palette>(
    item: S,
    palette: P,
    class: framed::EdgeFn<'a, P>,
) -> framed::Painted<'a, S, P>
where
    P::Class<'a>: From<framed::EdgeFn<'a, P>>,
{
    framed::Painted::new(item, palette, class.into())
}

/// A button of `content`, a crate generic type no opened module holds,
/// whose `Message` and `Theme` only the view's later uses and output
/// give, as iced's `button(..)` leaves them.
pub fn pressed<'a, Message, W, Theme>(content: W) -> crate::widgets::Button<'a, Message, W, Theme> {
    crate::widgets::button(content)
}

/// `item` painted by `palette` with a class of a closure, whose bound
/// names its parameter's trait through a qualified projection by the
/// trait's path, and the `framed` module's generic alias by its module,
/// as iced's `helpers::iced` names `container::StyleFn`.
pub fn badge<'a, S, P>(item: S, palette: P) -> framed::Painted<'a, S, P>
where
    P: framed::Palette + 'a,
    <P as framed::Palette>::Class<'a>: From<framed::EdgeFn<'a, P>>,
{
    let class: framed::EdgeFn<'a, P> = Box::new(|_| '+');
    framed::Painted::new(item, palette, class.into())
}
