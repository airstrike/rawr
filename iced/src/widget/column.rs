//! Parent-owned keyed reconciliation for the built-in Column.

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{tree, Meta, Operation, Tree, Widget};
use iced::advanced::Shell;
use iced::widget::Widget as _;
use iced::{Event, Length, Rectangle, Size, Vector};

use crate::{runtime, widget};

/// A built-in Column that owns keyed direct-child Tree relocation.
pub(crate) struct Keyed<'a> {
    mode: widget::key::Mode,
    content: iced::Element<'a, runtime::Message>,
}

struct State {
    mode: widget::key::Mode,
}

impl<'a> Keyed<'a> {
    pub(crate) fn bind(
        mode: widget::key::Mode,
        children: Vec<iced::Element<'a, runtime::Message>>,
        spacing: f32,
        padding: iced::Padding,
        width: iced::Length,
        height: iced::Length,
        align_x: iced::Alignment,
    ) -> Self {
        let content = iced::widget::column(children)
            .spacing(spacing)
            .padding(padding)
            .width(width)
            .height(height)
            .align_x(align_x)
            .boxed();
        Self { mode, content }
    }

    /// Relocate the retained child trees by key, then diff the Column.
    ///
    /// The Column's own tree is this widget's single child: iced's Column
    /// keeps its flex cache in its tree state, and this widget keeps the
    /// keys it reconciled last in its own.
    fn reconcile(&mut self, tree: &mut Tree) {
        let previous = tree.state.downcast_ref::<State>().mode.clone();
        if tree.children.len() != 1 {
            tree.children = vec![Tree::new(&self.content)];
        }
        let content = &mut tree.children[0];
        if content.tag == self.content.tag() {
            self.mode.reconcile(&previous, &mut content.children);
        }
        content.diff(&mut self.content);
        tree.state.downcast_mut::<State>().mode = self.mode.clone();
    }

    fn ensure(&mut self, tree: &mut Tree) {
        if tree.children.len() != 1 || tree.children[0].children.len() != self.mode.len() {
            self.reconcile(tree);
        }
    }
}

impl Meta for Keyed<'_> {}

impl Widget<runtime::Message, iced::Theme, iced::Renderer> for Keyed<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            mode: self.mode.clone(),
        })
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.reconcile(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) {
        self.ensure(tree);
        let content = &mut tree.children[0];
        self.content.layout(content, renderer, limits);
        content.translation = Vector::ZERO;
        tree.size = content.size;
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let (layout, tree) = content(layout, tree);
        self.content
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, runtime::Message>,
        viewport: &Rectangle,
    ) {
        let (layout, tree) = content_mut(layout, tree);
        self.content
            .update(tree, event, layout, cursor, renderer, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let (layout, tree) = content(layout, tree);
        self.content
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let (layout, tree) = content_mut(layout, tree);
        self.content
            .operate(tree, layout, viewport, renderer, operation);
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'a, runtime::Message, iced::Theme, iced::Renderer>> {
        let (layout, tree) = content_mut(layout, tree);
        self.content
            .overlay(tree, layout, renderer, viewport, translation, window)
    }
}

/// The Column's layout and tree within this widget's.
fn content(layout: Layout, tree: &Tree) -> (Layout, &Tree) {
    layout
        .iter(&tree.children)
        .next()
        .expect("a keyed Column retains its content tree")
}

fn content_mut(layout: Layout, tree: &mut Tree) -> (Layout, &mut Tree) {
    layout
        .iter_mut(&mut tree.children)
        .next()
        .expect("a keyed Column retains its content tree")
}

#[cfg(test)]
mod tests {
    use iced::advanced::layout;
    use iced::advanced::renderer::{self, Headless};
    use iced::advanced::widget::{tree, Meta, Tree, Widget};
    use iced::widget::Widget as _;
    use iced::{Length, Size};

    use crate::{runtime, widget};

    use super::{Keyed, State};

    #[test]
    fn first_layout_reconciles_children_once_without_a_prior_diff() {
        let mut keyed = fixture(&[("first", 10), ("second", 20)]).boxed();
        let mut tree = Tree::new(&keyed);
        let renderer = renderer();
        let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0));

        assert!(tree.children.is_empty());
        keyed.layout(&mut tree, &renderer, &limits);
        let children = &tree.children[0].children;
        assert_eq!(children.len(), 2);
        let first = address(&children[0]);
        let second = address(&children[1]);

        keyed.layout(&mut tree, &renderer, &limits);
        let children = &tree.children[0].children;
        assert_eq!(children.len(), 2);
        assert_eq!(address(&children[0]), first);
        assert_eq!(address(&children[1]), second);
    }

    #[test]
    fn diff_moves_retained_children_by_key_and_inserts_and_drops() {
        let mut first = fixture(&[("first", 10), ("second", 20)]).boxed();
        let mut tree = Tree::new(&first);
        first.diff(&mut tree);
        let first = address(&tree.children[0].children[0]);
        let second = address(&tree.children[0].children[1]);

        let mut reordered = fixture(&[("second", 20), ("third", 30), ("first", 10)]).boxed();
        reordered.diff(&mut tree);
        let children = &tree.children[0].children;
        assert_eq!(children.len(), 3);
        assert_eq!(address(&children[0]), second);
        assert_ne!(address(&children[1]), first);
        assert_ne!(address(&children[1]), second);
        assert_eq!(address(&children[2]), first);

        let mut removed = fixture(&[("second", 20)]).boxed();
        removed.diff(&mut tree);
        let children = &tree.children[0].children;
        assert_eq!(children.len(), 1);
        assert_eq!(address(&children[0]), second);
    }

    fn fixture(children: &[(&str, usize)]) -> Keyed<'static> {
        Keyed::bind(
            widget::key::Mode::Keyed(
                children
                    .iter()
                    .map(|(key, _)| widget::key::Value::string((*key).into()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            children
                .iter()
                .map(|(_, value)| Stub(*value).boxed())
                .collect(),
            0.0,
            iced::Padding::ZERO,
            iced::Length::Fit,
            iced::Length::Shrink,
            iced::Alignment::Start,
        )
    }

    fn address(tree: &Tree) -> *const Marker {
        tree.state.downcast_ref::<Marker>() as *const Marker
    }

    fn renderer() -> iced::Renderer {
        iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("the pinned tiny-skia renderer must be available")
    }

    struct Marker(usize);

    struct Stub(usize);

    impl Meta for Stub {}

    impl Widget<runtime::Message, iced::Theme, iced::Renderer> for Stub {
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<Marker>()
        }

        fn state(&self) -> tree::State {
            tree::State::new(Marker(self.0))
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Fixed(10.0), Length::Fixed(10.0))
        }

        fn layout(
            &mut self,
            tree: &mut Tree,
            _renderer: &iced::Renderer,
            _limits: &layout::Limits,
        ) {
            assert_eq!(tree.state.downcast_ref::<Marker>().0, self.0);
            tree.size = Size::new(10.0, 10.0);
        }

        fn draw(
            &self,
            _tree: &Tree,
            _renderer: &mut iced::Renderer,
            _theme: &iced::Theme,
            _style: &renderer::Style,
            _layout: iced::advanced::Layout,
            _cursor: iced::mouse::Cursor,
            _viewport: &iced::Rectangle,
        ) {
        }
    }

    #[test]
    fn keyed_wrapper_owns_one_exact_reconciliation_mode() {
        let keyed = fixture(&[("first", 10)]).boxed();
        let tree = Tree::new(&keyed);
        assert_eq!(tree.tag, tree::Tag::of::<State>());
    }
}
