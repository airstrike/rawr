//! What a script widget's call borrows from iced, for the length of the call.
//!
//! iced hands `Widget::layout`, `draw` and `update` a `&mut Tree`, a
//! `&Renderer` and a `&mut Shell` that live only for that call. The script
//! reaches them through handles (`Tree`, `Shell`) and the wrapped child
//! widgets it forwards to. A [`Loan`] makes them reachable: it pushes one
//! [`Frame`] onto this thread's stack and pops it when it drops, so no
//! pointer outlives the call that lent it. A handle names its frame by a
//! [`Nonce`]; a handle kept past its call finds no frame and fails.
//!
//! The evaluator runs on the thread that called it, so a script hook that
//! forwards to a child which is itself a script widget pushes a second
//! frame above the first and pops it before the outer call resumes.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::Shell;

use crate::runtime;
use crate::widget;

/// A lowered child widget, as iced keeps it.
pub(crate) type Element = iced::Element<'static, runtime::Message>;

/// The wrapped child widgets of one script widget: each `Element` value its
/// hooks forward to, lowered the first time a hook reaches it and kept for
/// the rest of this view, by the value's identity.
pub(crate) struct Children {
    context: Option<widget::OwnedContext>,
    lowered: RefCell<Vec<(widget::Identity, Element)>>,
}

/// The tree a call receives: `&Tree` for `draw`, `&mut Tree` for `layout`,
/// none for `size`, `tag` and `state`.
pub(crate) enum Lent<'a> {
    None,
    Shared(&'a Tree),
    Mutable(&'a mut Tree),
}

/// The identity of one lent call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Nonce(u64);

/// What one call lends. Every pointer is valid while the frame is on the
/// stack, and only then.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    nonce: Nonce,
    tree: *mut Tree,
    mutable: bool,
    renderer: *const iced::Renderer,
    shell: *mut Shell<'static, runtime::Message>,
    operation: Option<*mut (dyn Operation + 'static)>,
    children: *const Children,
}

/// One call's borrows, lent while it lives. It holds them for `'a`, so the
/// lender cannot touch them until the loan ends.
pub(crate) struct Loan<'a> {
    nonce: Nonce,
    borrow: PhantomData<&'a mut ()>,
}

/// Why a handle could not reach what it names.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum Error {
    #[error("`{0}` is used outside the call that received it")]
    Expired(&'static str),
    #[error("`{0}` is not lent to this call")]
    Missing(&'static str),
    #[error("this call lends `Tree` read-only")]
    ReadOnly,
    #[error("the tree has no child at {0:?}")]
    Path(Vec<usize>),
    #[error("the wrapped widget is already in use by this call")]
    Busy,
    #[error("the wrapped widget panicked: {0}")]
    Panicked(String),
}

thread_local! {
    static FRAMES: RefCell<Vec<Frame>> = const { RefCell::new(Vec::new()) };
}

impl Children {
    pub(crate) fn new(context: widget::OwnedContext) -> Self {
        Self {
            context: Some(context),
            lowered: RefCell::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub(crate) fn none() -> Self {
        Self {
            context: None,
            lowered: RefCell::new(Vec::new()),
        }
    }

    /// The index of `value`'s lowered widget, lowering it when no hook has
    /// reached it yet in this view.
    fn index(&self, value: &widget::Value) -> Result<usize, Error> {
        let identity = value.identity();
        let mut lowered = self.lowered.try_borrow_mut().map_err(|_| Error::Busy)?;
        if let Some(index) = lowered.iter().position(|(known, _)| *known == identity) {
            return Ok(index);
        }
        let context = self.context.as_ref().ok_or(Error::Missing("Element"))?;
        let element = value.lower(&context.context());
        lowered.push((identity, element));
        Ok(lowered.len() - 1)
    }

    /// The lowered children, for `overlay`, which borrows one for as long
    /// as its overlay lives.
    pub(crate) fn lowered_mut(&mut self) -> &mut Vec<(widget::Identity, Element)> {
        self.lowered.get_mut()
    }

    /// Run `f` on the child `index` lowered earlier in this view. A panic
    /// in the child's native code is an error of the call that reached it,
    /// so it never unwinds through the script.
    pub(crate) fn with<R>(&self, index: usize, f: impl FnOnce(&mut Element) -> R) -> Result<R, Error> {
        let mut lowered = self.lowered.try_borrow_mut().map_err(|_| Error::Busy)?;
        let (_, element) = lowered.get_mut(index).ok_or(Error::Busy)?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(element))).map_err(|panic| {
            Error::Panicked(
                panic
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_owned())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "a panic without a message".to_owned()),
            )
        })
    }
}

impl<'a> Loan<'a> {
    /// Lend `tree`, the renderer, the Shell and
    /// the children to the calls made while the loan lives.
    pub(crate) fn new(
        tree: Lent<'a>,
        renderer: Option<&'a iced::Renderer>,
        shell: Option<&'a mut Shell<'_, runtime::Message>>,
        children: &'a Children,
    ) -> Self {
        Self::lend(tree, renderer, shell, None, children)
    }

    /// Lend `operate`'s tree, renderer and operation.
    pub(crate) fn operating(
        tree: &'a mut Tree,
        renderer: &'a iced::Renderer,
        operation: &'a mut dyn Operation,
        children: &'a Children,
    ) -> Self {
        Self::lend(Lent::Mutable(tree), Some(renderer), None, Some(operation), children)
    }

    fn lend(
        tree: Lent<'a>,
        renderer: Option<&'a iced::Renderer>,
        shell: Option<&'a mut Shell<'_, runtime::Message>>,
        operation: Option<&'a mut dyn Operation>,
        children: &'a Children,
    ) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);

        let nonce = Nonce(NEXT.fetch_add(1, Ordering::Relaxed));
        let (tree, mutable) = match tree {
            Lent::None => (std::ptr::null_mut(), false),
            Lent::Mutable(tree) => (std::ptr::from_mut(tree), true),
            Lent::Shared(tree) => (std::ptr::from_ref(tree).cast_mut(), false),
        };
        let frame = Frame {
            nonce,
            tree,
            mutable,
            renderer: renderer.map_or(std::ptr::null(), std::ptr::from_ref),
            shell: shell.map_or(std::ptr::null_mut(), |shell| {
                std::ptr::from_mut(shell).cast::<Shell<'static, runtime::Message>>()
            }),
            // SAFETY: only the lifetime is erased; the frame, and with it
            // the pointer, is popped before `'a` ends.
            operation: operation.map(|operation| unsafe {
                std::mem::transmute::<*mut (dyn Operation + '_), *mut (dyn Operation + 'static)>(
                    std::ptr::from_mut(operation),
                )
            }),
            children: std::ptr::from_ref(children),
        };
        FRAMES.with(|frames| frames.borrow_mut().push(frame));
        Self {
            nonce,
            borrow: PhantomData,
        }
    }

    pub(crate) const fn nonce(&self) -> Nonce {
        self.nonce
    }
}

impl Drop for Loan<'_> {
    fn drop(&mut self) {
        FRAMES.with(|frames| {
            let mut frames = frames.borrow_mut();
            if let Some(position) = frames.iter().rposition(|frame| frame.nonce == self.nonce) {
                frames.truncate(position);
            }
        });
    }
}

impl Frame {
    /// The frame `nonce` names, while its call runs.
    pub(crate) fn find(nonce: Nonce, what: &'static str) -> Result<Self, Error> {
        FRAMES.with(|frames| {
            frames
                .borrow()
                .iter()
                .rev()
                .find(|frame| frame.nonce == nonce)
                .copied()
                .ok_or(Error::Expired(what))
        })
    }

    /// The frame of the call running now: the newest.
    pub(crate) fn current(what: &'static str) -> Result<Self, Error> {
        FRAMES.with(|frames| frames.borrow().last().copied().ok_or(Error::Expired(what)))
    }

    pub(crate) const fn nonce(&self) -> Nonce {
        self.nonce
    }

    /// The subtree at `path` below the lent tree.
    pub(crate) fn tree<R>(&self, path: &[usize], f: impl FnOnce(&Tree) -> R) -> Result<R, Error> {
        // SAFETY: the frame is on the stack, so its call still holds the
        // borrow this pointer came from, and no other reference to the tree
        // is in use while the script runs.
        let mut tree = unsafe { self.tree.as_ref() }.ok_or(Error::Missing("Tree"))?;
        for &index in path {
            tree = tree.children.get(index).ok_or_else(|| Error::Path(path.to_vec()))?;
        }
        Ok(f(tree))
    }

    /// The subtree at `path` below the lent tree, to change.
    pub(crate) fn tree_mut<R>(&self, path: &[usize], f: impl FnOnce(&mut Tree) -> R) -> Result<R, Error> {
        if !self.mutable {
            return Err(Error::ReadOnly);
        }
        // SAFETY: as in `tree`; the call lent the tree mutably.
        let mut tree = unsafe { self.tree.as_mut() }.ok_or(Error::Missing("Tree"))?;
        for &index in path {
            tree = tree
                .children
                .get_mut(index)
                .ok_or_else(|| Error::Path(path.to_vec()))?;
        }
        Ok(f(tree))
    }

    pub(crate) fn renderer(&self) -> Result<&iced::Renderer, Error> {
        // SAFETY: as in `tree`.
        unsafe { self.renderer.as_ref() }.ok_or(Error::Missing("Renderer"))
    }

    /// The call's Shell, which a child's `update` publishes to directly.
    pub(crate) fn shell(&self) -> Result<&mut Shell<'static, runtime::Message>, Error> {
        // SAFETY: as in `tree`; the Shell's own lifetime outlives the call,
        // and the reference handed out is bounded by this frame's borrow.
        unsafe { self.shell.as_mut() }.ok_or(Error::Missing("Shell"))
    }

    /// The operation `operate` received.
    pub(crate) fn operation(&self) -> Result<&mut dyn Operation, Error> {
        // SAFETY: as in `tree`.
        self.operation
            .map(|operation| unsafe { &mut *operation })
            .ok_or(Error::Missing("Operation"))
    }

    pub(crate) fn children(&self) -> &Children {
        // SAFETY: as in `tree`.
        unsafe { &*self.children }
    }

    /// The index of `value` among the frame's children, lowered on first use.
    pub(crate) fn child(&self, value: &widget::Value) -> Result<usize, Error> {
        self.children().index(value)
    }
}

#[cfg(test)]
mod tests {
    use iced::advanced::widget::{Operation, Tree};

    use super::{Children, Error, Frame, Lent, Loan};

    #[test]
    fn a_loan_lends_the_tree_for_its_call_and_no_longer() {
        let children = Children::none();
        let mut tree = Tree::empty();
        tree.children.push(Tree::empty());
        let nonce = {
            let loan = Loan::new(Lent::Mutable(&mut tree), None, None, &children);
            let frame = Frame::find(loan.nonce(), "Tree").unwrap();
            frame
                .tree_mut(&[0], |child| child.size = iced::Size::new(3.0, 4.0))
                .unwrap();
            assert_eq!(
                frame.tree(&[0], |child| child.size).unwrap(),
                iced::Size::new(3.0, 4.0)
            );
            assert_eq!(frame.tree(&[1], |_| ()), Err(Error::Path(vec![1])));
            assert!(frame.renderer().is_err());
            loan.nonce()
        };
        assert_eq!(tree.children[0].size, iced::Size::new(3.0, 4.0));
        assert_eq!(Frame::find(nonce, "Tree").err(), Some(Error::Expired("Tree")));
    }

    #[test]
    fn a_read_only_loan_refuses_changes_and_nested_loans_pop_in_order() {
        let children = Children::none();
        let tree = Tree::empty();
        let outer = Loan::new(Lent::Shared(&tree), None, None, &children);
        assert_eq!(
            Frame::current("Tree").unwrap().tree_mut(&[], |_| ()),
            Err(Error::ReadOnly)
        );
        let inner_nonce = {
            let inner = Loan::new(Lent::Shared(&tree), None, None, &children);
            assert_eq!(Frame::current("Tree").unwrap().nonce(), inner.nonce());
            assert!(Frame::find(outer.nonce(), "Tree").is_ok());
            inner.nonce()
        };
        assert_eq!(Frame::current("Tree").unwrap().nonce(), outer.nonce());
        assert!(Frame::find(inner_nonce, "Tree").is_err());
        drop(outer);
        assert!(Frame::current("Tree").is_err());
    }
}
