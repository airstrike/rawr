//! Owner-scoped scripted-widget lifecycle failures and their native layer.

use std::sync::{Arc, Mutex, MutexGuard};

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{tree, Meta, Operation, Tree, Widget};
use iced::advanced::Shell;
use iced::widget::Widget as _;
use iced::{Event, Length, Rectangle, Size, Vector};
use rawr::{application, program, widget, Span};

use crate::runtime;

/// One exact lifecycle invocation location within an owner-scoped sink.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Key {
    site: widget::Site,
    stage: Stage,
    span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Stage {
    Binding,
    Hook(widget::hook::Kind),
}

/// One exact canonical lifecycle failure retained by the host session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Entry {
    key: Key,
    report: rawr::diagnostic::Report,
}

/// The exact application/preview session receiving lifecycle observations.
pub(crate) struct Sink {
    owner: Owner,
    inner: Mutex<State>,
    /// Style tables computed for this owner's generation.
    styles: Mutex<super::style::Tables>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Owner {
    instance: application::Instance,
    generation: program::GenerationId,
}

struct State {
    entries: Vec<Entry>,
    revision: u64,
    pending_redraw: bool,
    waker: Option<iced_runtime::core::shell::Waker>,
}

struct Layer<'a> {
    children: [iced::Element<'a, runtime::Message>; 2],
    sink: Arc<Sink>,
}

struct Report {
    sink: Arc<Sink>,
    fallback: iced::Element<'static, runtime::Message>,
}

struct Snapshot {
    owner: Owner,
    revision: u64,
    entries: Vec<Entry>,
}

mod report {
    use super::Owner;
    use crate::runtime;

    pub(super) struct State {
        pub(super) owner: Owner,
        pub(super) revision: u64,
        pub(super) full: iced::Element<'static, runtime::Message>,
        #[cfg(test)]
        pub(super) rebuilds: usize,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) enum Presentation {
        None,
        Full,
        Fallback,
    }
}

impl Key {
    /// Bundle compiler-owned site, hook, and source proofs into one failure key.
    pub(crate) fn new(site: widget::Site, hook: widget::hook::Kind, span: Span) -> Self {
        Self {
            site,
            stage: Stage::Hook(hook),
            span,
        }
    }

    pub(crate) fn binding(site: widget::Site, span: Span) -> Self {
        Self {
            site,
            stage: Stage::Binding,
            span,
        }
    }
}

impl Entry {
    pub(crate) const fn report(&self) -> &rawr::diagnostic::Report {
        &self.report
    }

    pub(crate) fn context(&self) -> String {
        match &self.key.stage {
            Stage::Binding => format!("widget binding at {}", self.key.span),
            Stage::Hook(hook) => format!("widget {} at {}", hook.name(), self.key.span),
        }
    }
}

impl Sink {
    /// Mint a lifecycle sink for one exact host session owner.
    pub(crate) fn new(instance: application::Instance, generation: program::GenerationId) -> Self {
        Self {
            owner: Owner {
                instance,
                generation,
            },
            inner: Mutex::new(State {
                entries: Vec::new(),
                revision: 0,
                pending_redraw: false,
                waker: None,
            }),
            styles: Mutex::default(),
        }
    }

    pub(crate) fn styles(&self) -> MutexGuard<'_, super::style::Tables> {
        self.styles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn owns(
        &self,
        instance: application::Instance,
        generation: program::GenerationId,
    ) -> bool {
        self.owner
            == (Owner {
                instance,
                generation,
            })
    }

    /// Retain one failure once and wake the runtime when presentation changed.
    pub(crate) fn record(&self, key: Key, report: rawr::diagnostic::Report) -> bool {
        let wake = {
            let mut state = self.state();
            if state
                .entries
                .iter()
                .any(|entry| entry.key == key && entry.report == report)
            {
                return false;
            }

            state.entries.retain(|entry| entry.key != key);
            state.entries.push(Entry { key, report });
            Self::changed(&mut state)
        };
        if let Some(waker) = wake {
            waker.wake();
        }
        true
    }

    /// Clear the failure for one successful exact lifecycle hook.
    pub(crate) fn clear(&self, key: &Key) -> bool {
        let wake = {
            let mut state = self.state();
            let before = state.entries.len();
            state.entries.retain(|entry| entry.key != *key);
            if state.entries.len() == before {
                return false;
            }

            Self::changed(&mut state)
        };
        if let Some(waker) = wake {
            waker.wake();
        }
        true
    }

    fn snapshot(&self) -> Snapshot {
        let state = self.state();
        Snapshot {
            owner: self.owner,
            revision: state.revision,
            entries: state.entries.clone(),
        }
    }

    fn status(&self) -> (Owner, u64, bool) {
        let state = self.state();
        (self.owner, state.revision, state.entries.is_empty())
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.state().entries.len()
    }

    #[cfg(test)]
    pub(crate) fn messages(&self) -> Vec<String> {
        self.state()
            .entries
            .iter()
            .map(|entry| entry.report.message().to_owned())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn reports(&self) -> Vec<rawr::diagnostic::Report> {
        self.state()
            .entries
            .iter()
            .map(|entry| entry.report.clone())
            .collect()
    }

    /// Attach the exact runtime waker and convert one pending wake into redraw.
    pub(crate) fn synchronize(&self, shell: &mut Shell<'_, runtime::Message>) {
        let (redraw, previous) = {
            let mut state = self.state();
            let previous = state.waker.replace(shell.waker().clone());
            (std::mem::take(&mut state.pending_redraw), previous)
        };
        drop(previous);
        if redraw {
            shell.request_redraw();
        }
    }

    fn changed(state: &mut State) -> Option<iced_runtime::core::shell::Waker> {
        state.revision = state
            .revision
            .checked_add(1)
            .expect("lifecycle diagnostic revisions cannot be exhausted");
        if state.pending_redraw {
            return None;
        }

        state.pending_redraw = true;
        state.waker.clone()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Keep lifecycle reports live above an otherwise ordinary iced element.
pub(crate) fn overlay<'a>(
    content: iced::Element<'a, runtime::Message>,
    sink: Arc<Sink>,
) -> iced::Element<'a, runtime::Message> {
    let content = iced::widget::container(content)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .boxed();
    Layer::bind(content, sink).boxed()
}

impl<'a> Layer<'a> {
    fn bind(content: iced::Element<'a, runtime::Message>, sink: Arc<Sink>) -> Self {
        Self {
            children: [
                content,
                Report {
                    sink: sink.clone(),
                    fallback: crate::diagnostic::lifecycle_fallback(),
                }
                .boxed(),
            ],
            sink,
        }
    }

    fn reconcile(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn ensure(&mut self, tree: &mut Tree) {
        if tree.children.len() != self.children.len() {
            self.reconcile(tree);
        }
    }
}

impl Meta for Layer<'_> {}

impl Widget<runtime::Message, iced::Theme, iced::Renderer> for Layer<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Layer<'static>>()
    }

    fn state(&self) -> tree::State {
        tree::State::None
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.reconcile(tree);
    }

    fn size(&self) -> Size<Length> {
        self.children[0].size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) {
        self.ensure(tree);
        self.children[0].layout(&mut tree.children[0], renderer, limits);
        let size = tree.children[0].size;
        self.children[1].layout(
            &mut tree.children[1],
            renderer,
            &layout::Limits::new(size, size),
        );
        tree.size = size;
        for child in &mut tree.children {
            child.translation = Vector::ZERO;
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let (content, tree) = layout
            .iter_mut(&mut tree.children)
            .next()
            .expect("diagnostic layer layout must retain content");
        self.children[0].operate(tree, content, viewport, renderer, operation);
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
        self.sink.synchronize(shell);
        let (content, tree) = layout
            .iter_mut(&mut tree.children)
            .next()
            .expect("diagnostic layer layout must retain content");
        self.children[0].update(tree, event, content, cursor, renderer, shell, viewport);
        self.sink.synchronize(shell);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let (content, tree) = layout
            .iter(&tree.children)
            .next()
            .expect("diagnostic layer layout must retain content");
        self.children[0].mouse_interaction(tree, content, cursor, viewport, renderer)
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
        let mut layouts = layout.iter(&tree.children);
        let (content, content_tree) = layouts
            .next()
            .expect("diagnostic layer layout must retain content");
        let (report, report_tree) = layouts
            .next()
            .expect("diagnostic layer layout must retain its report");
        self.children[0].draw(
            content_tree,
            renderer,
            theme,
            style,
            content,
            cursor,
            viewport,
        );
        self.children[1].draw(
            report_tree,
            renderer,
            theme,
            style,
            report,
            cursor,
            viewport,
        );
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
        let (content, tree) = layout
            .iter_mut(&mut tree.children)
            .next()
            .expect("diagnostic layer layout must retain content");
        self.children[0].overlay(tree, content, renderer, viewport, translation, window)
    }
}

impl Report {
    fn refresh(&mut self, tree: &mut Tree) {
        let (owner, revision, _) = self.sink.status();
        let state = tree.state.downcast_mut::<report::State>();
        let changed = state.owner != owner || state.revision != revision;
        if changed {
            let snapshot = self.sink.snapshot();
            state.owner = snapshot.owner;
            state.revision = snapshot.revision;
            state.full = crate::diagnostic::lifecycle(&snapshot.entries);
            #[cfg(test)]
            {
                state.rebuilds += 1;
            }
        }

        if tree.children.len() != 2 {
            tree.children = vec![Tree::new(&self.fallback), Tree::new(&state.full)];
            tree.children[0].diff(&mut self.fallback);
            tree.children[1].diff(&mut state.full);
        } else {
            tree.children[0].diff(&mut self.fallback);
            if changed {
                tree.children[1].diff(&mut state.full);
            }
        }
    }

    fn presentation(&self, tree: &Tree) -> report::Presentation {
        let (owner, revision, empty) = self.sink.status();
        if empty {
            return report::Presentation::None;
        }

        let state = tree.state.downcast_ref::<report::State>();
        if state.owner == owner && state.revision == revision {
            report::Presentation::Full
        } else {
            report::Presentation::Fallback
        }
    }
}

impl Meta for Report {}

impl Widget<runtime::Message, iced::Theme, iced::Renderer> for Report {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<report::State>()
    }

    fn state(&self) -> tree::State {
        let snapshot = self.sink.snapshot();
        tree::State::new(report::State {
            owner: snapshot.owner,
            revision: snapshot.revision,
            full: crate::diagnostic::lifecycle(&snapshot.entries),
            #[cfg(test)]
            rebuilds: 1,
        })
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.refresh(tree);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) {
        self.refresh(tree);
        let size = limits.max;
        let limits = layout::Limits::new(size, size);
        self.fallback
            .layout(&mut tree.children[0], renderer, &limits);
        tree.state.downcast_mut::<report::State>().full.layout(
            &mut tree.children[1],
            renderer,
            &limits,
        );
        tree.size = size;
        for child in &mut tree.children {
            child.translation = Vector::ZERO;
        }
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
        let mut layouts = layout.iter(&tree.children);
        let (fallback, fallback_tree) = layouts
            .next()
            .expect("diagnostic report layout must retain its fallback");
        let (full, full_tree) = layouts
            .next()
            .expect("diagnostic report layout must retain its full panel");
        match self.presentation(tree) {
            report::Presentation::None => {}
            report::Presentation::Fallback => self.fallback.draw(
                fallback_tree,
                renderer,
                theme,
                style,
                fallback,
                cursor,
                viewport,
            ),
            report::Presentation::Full => tree
                .state
                .downcast_ref::<report::State>()
                .full
                .draw(full_tree, renderer, theme, style, full, cursor, viewport),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use iced::advanced::layout::{self, Layout};
    use iced::advanced::mouse;
    use iced::advanced::renderer::{self, Headless};
    use iced::advanced::widget::{Meta, Tree, Widget};
    use iced::advanced::Shell;
    use iced::widget::Widget as _;
    use iced::{Length, Rectangle, Size};
    use iced_runtime::core::{shell, window};
    use iced_runtime::user_interface::{self, UserInterface};
    use rawr::{diagnostic, Engine};

    use super::{overlay, report, Key, Layer, Report, Sink};
    use crate::runtime;

    struct Probe {
        states: Arc<AtomicUsize>,
        diffs: Arc<AtomicUsize>,
    }

    struct Shrink;

    impl Meta for Shrink {}

    impl Meta for Probe {}

    impl Widget<runtime::Message, iced::Theme, iced::Renderer> for Shrink {
        fn size(&self) -> Size<Length> {
            Size::new(Length::Fixed(80.0), Length::Fixed(40.0))
        }

        fn layout(&mut self, tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) {
            tree.size = limits.resolve(Length::Fixed(80.0), Length::Fixed(40.0), Size::ZERO);
        }

        fn draw(
            &self,
            _tree: &Tree,
            _renderer: &mut iced::Renderer,
            _theme: &iced::Theme,
            _style: &renderer::Style,
            _layout: Layout,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
        ) {
        }
    }

    impl Widget<runtime::Message, iced::Theme, iced::Renderer> for Probe {
        fn state(&self) -> iced::advanced::widget::tree::State {
            self.states.fetch_add(1, Ordering::Relaxed);
            iced::advanced::widget::tree::State::new(())
        }

        fn diff(&mut self, _tree: &mut Tree) {
            self.diffs.fetch_add(1, Ordering::Relaxed);
        }

        fn size(&self) -> Size<Length> {
            Size::new(Length::Fill, Length::Fill)
        }

        fn layout(&mut self, tree: &mut Tree, _renderer: &iced::Renderer, limits: &layout::Limits) {
            tree.size = limits.max;
        }

        fn draw(
            &self,
            _tree: &Tree,
            _renderer: &mut iced::Renderer,
            _theme: &iced::Theme,
            _style: &renderer::Style,
            _layout: Layout,
            _cursor: mouse::Cursor,
            _viewport: &Rectangle,
        ) {
        }
    }

    fn layer(sink: Arc<Sink>, states: Arc<AtomicUsize>, diffs: Arc<AtomicUsize>) -> Layer<'static> {
        Layer::bind(Probe { states, diffs }.boxed(), sink)
    }

    fn renderer() -> iced::Renderer {
        iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("the pinned tiny-skia renderer must be available")
    }

    #[test]
    fn layer_layout_repairs_and_retains_a_fresh_tree() {
        let (_, generation) = protocol();
        let sink = Arc::new(Sink::new(rawr::application::Instance::new(), generation));
        let states = Arc::new(AtomicUsize::new(0));
        let diffs = Arc::new(AtomicUsize::new(0));
        let mut layer = layer(sink, states.clone(), diffs.clone());
        let mut tree = Tree::new(&layer);
        assert!(tree.children.is_empty());

        let renderer = renderer();
        let size = Size::new(320.0, 200.0);
        let limits = layout::Limits::new(size, size);
        layer.layout(&mut tree, &renderer, &limits);
        assert_eq!(tree.children.len(), 2);
        assert_eq!(states.load(Ordering::Relaxed), 1);
        assert_eq!(diffs.load(Ordering::Relaxed), 1);

        layer.layout(&mut tree, &renderer, &limits);
        assert_eq!(states.load(Ordering::Relaxed), 1);
        assert_eq!(diffs.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn public_overlay_owns_the_window_even_when_application_content_shrinks() {
        let (_, generation) = protocol();
        let sink = Arc::new(Sink::new(rawr::application::Instance::new(), generation));
        let mut element = overlay(Shrink.boxed(), sink);
        let mut tree = Tree::new(&element);
        let renderer = renderer();
        let size = Size::new(320.0, 200.0);
        let limits = layout::Limits::new(Size::ZERO, size);
        element.layout(&mut tree, &renderer, &limits);

        assert_eq!(element.size(), Size::new(Length::Fill, Length::Fill));
        assert_eq!(tree.size, size);
        assert_eq!(tree.children[0].size, size);
        assert_eq!(tree.children[1].size, size);
    }

    #[test]
    fn first_user_interface_build_retains_layer_children_without_layout_diff() {
        let (_, generation) = protocol();
        let sink = Arc::new(Sink::new(rawr::application::Instance::new(), generation));
        let states = Arc::new(AtomicUsize::new(0));
        let diffs = Arc::new(AtomicUsize::new(0));
        let mut renderer = renderer();
        let size = Size::new(320.0, 200.0);

        let interface = UserInterface::build(
            layer(sink.clone(), states.clone(), diffs.clone()).boxed(),
            size,
            user_interface::Cache::default(),
            &mut renderer,
        );
        assert_eq!(states.load(Ordering::Relaxed), 1);
        assert_eq!(diffs.load(Ordering::Relaxed), 1);

        let cache = interface.into_cache();
        let _interface = UserInterface::build(
            layer(sink, states.clone(), diffs.clone()).boxed(),
            size,
            cache,
            &mut renderer,
        );
        assert_eq!(states.load(Ordering::Relaxed), 1);
        assert_eq!(diffs.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn sink_is_exact_deduplicated_clearable_reentrant_and_redraw_coalesced() {
        let instance = rawr::application::Instance::new();
        let (site, generation) = protocol();
        let sink = Arc::new(Sink::new(instance, generation));
        let other = rawr::application::Instance::new();

        assert!(sink.owns(instance, generation));
        assert!(!sink.owns(other, generation));

        let key = Key::new(site, rawr::widget::hook::Kind::Layout, rawr::Span::NONE);
        let report =
            diagnostic::Report::error(diagnostic::code::RUNTIME_EVALUATION, "layout hook failed")
                .finish();
        assert!(sink.record(key.clone(), report.clone()));
        assert!(!sink.record(key.clone(), report));
        assert_eq!(sink.len(), 1);

        let wakes = Arc::new(AtomicUsize::new(0));
        let observed = wakes.clone();
        let reentrant = sink.clone();
        let waker = iced_runtime::core::shell::Waker::new(move || {
            let _ = reentrant.len();
            observed.fetch_add(1, Ordering::Relaxed);
        });
        let mut messages = shell::Bus::new();
        let mut shell = Shell::new(&window::Headless, waker, &mut messages);
        sink.synchronize(&mut shell);
        assert!(matches!(
            shell.redraw_request(),
            window::RedrawRequest::NextFrame
        ));

        assert!(sink.clear(&key));
        assert!(!sink.clear(&key));
        assert_eq!(sink.len(), 0);
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        assert!(sink.record(
            key.clone(),
            diagnostic::Report::error(
                diagnostic::code::RUNTIME_EVALUATION,
                "layout hook failed again",
            )
            .finish(),
        ));
        assert!(sink.clear(&key));
        assert_eq!(wakes.load(Ordering::Relaxed), 1);

        let mut next_messages = shell::Bus::new();
        let mut next_shell = Shell::new(
            &window::Headless,
            iced_runtime::core::shell::Waker::noop(),
            &mut next_messages,
        );
        sink.synchronize(&mut next_shell);
        assert!(matches!(
            next_shell.redraw_request(),
            window::RedrawRequest::NextFrame
        ));
    }

    #[test]
    fn report_uses_same_pass_fallback_then_retains_one_full_revision() {
        let instance = rawr::application::Instance::new();
        let (site, generation) = protocol();
        let sink = Arc::new(Sink::new(instance, generation));
        let mut report = Report {
            sink: sink.clone(),
            fallback: crate::diagnostic::lifecycle_fallback(),
        };
        let mut tree = Tree {
            tag: report.tag(),
            state: report.state(),
            translation: iced::Vector::ZERO,
            size: Size::ZERO,
            children: Vec::new(),
        };
        report.diff(&mut tree);

        let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("the pinned tiny-skia renderer must be available");
        let size = Size::new(320.0, 200.0);
        let limits = layout::Limits::new(size, size);
        report.layout(&mut tree, &renderer, &limits);
        assert_eq!(report.presentation(&tree), report::Presentation::None);
        assert_eq!(tree.state.downcast_ref::<report::State>().rebuilds, 1);

        let key = Key::new(site, rawr::widget::hook::Kind::Draw, rawr::Span::NONE);
        let failure =
            diagnostic::Report::error(diagnostic::code::RUNTIME_EVALUATION, "draw hook failed")
                .finish();
        assert!(sink.record(key.clone(), failure));
        assert_eq!(report.presentation(&tree), report::Presentation::Fallback);

        report.layout(&mut tree, &renderer, &limits);
        assert_eq!(report.presentation(&tree), report::Presentation::Full);
        assert_eq!(tree.state.downcast_ref::<report::State>().rebuilds, 2);
        let theme = iced::Theme::Light;
        let style = renderer::Style::default();
        let viewport = Rectangle::with_size(size);
        report.draw(
            &tree,
            &mut renderer,
            &theme,
            &style,
            Layout::new(tree.size),
            mouse::Cursor::Unavailable,
            &viewport,
        );
        report.draw(
            &tree,
            &mut renderer,
            &theme,
            &style,
            Layout::new(tree.size),
            mouse::Cursor::Unavailable,
            &viewport,
        );
        assert_eq!(tree.state.downcast_ref::<report::State>().rebuilds, 2);

        assert!(sink.clear(&key));
        assert_eq!(report.presentation(&tree), report::Presentation::None);
    }

    #[test]
    fn report_rebuilds_when_a_new_owner_reuses_the_same_revision() {
        let first_instance = rawr::application::Instance::new();
        let (site, generation) = protocol();
        let first = Arc::new(Sink::new(first_instance, generation));
        let key = Key::new(site, rawr::widget::hook::Kind::Draw, rawr::Span::NONE);
        assert!(first.record(
            key.clone(),
            diagnostic::Report::error(diagnostic::code::RUNTIME_EVALUATION, "old owner failed",)
                .finish(),
        ));
        let mut report = Report {
            sink: first,
            fallback: crate::diagnostic::lifecycle_fallback(),
        };
        let mut tree = Tree {
            tag: report.tag(),
            state: report.state(),
            translation: iced::Vector::ZERO,
            size: Size::ZERO,
            children: Vec::new(),
        };
        report.diff(&mut tree);
        assert_eq!(report.presentation(&tree), report::Presentation::Full);
        assert_eq!(tree.state.downcast_ref::<report::State>().revision, 1);

        let second_instance = rawr::application::Instance::new();
        let second = Arc::new(Sink::new(second_instance, generation));
        assert!(second.record(
            key,
            diagnostic::Report::error(diagnostic::code::RUNTIME_EVALUATION, "new owner failed",)
                .finish(),
        ));
        report = Report {
            sink: second.clone(),
            fallback: crate::diagnostic::lifecycle_fallback(),
        };

        assert_eq!(report.presentation(&tree), report::Presentation::Fallback);
        report.diff(&mut tree);
        let (owner, revision, empty) = second.status();
        let state = tree.state.downcast_ref::<report::State>();
        assert!(state.owner == owner);
        assert_eq!(state.revision, revision);
        assert_eq!(revision, 1);
        assert!(!empty);
        assert_eq!(state.rebuilds, 2);
        assert_eq!(report.presentation(&tree), report::Presentation::Full);
    }

    #[test]
    fn binding_and_state_hook_failures_have_distinct_keys() {
        let (site, _) = protocol();
        let binding = Key::binding(site.clone(), rawr::Span::NONE);
        let state = Key::new(site, rawr::widget::hook::Kind::State, rawr::Span::NONE);

        assert_ne!(binding, state);
    }

    fn protocol() -> (rawr::widget::Site, rawr::program::GenerationId) {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let source = r#"
            use iced::Element;
            use iced::Length;
            use rawr_iced::widget::layout::Size;

            #[derive(Default)]
            struct Dial { radius: i64 }

            #[derive(Debug, Clone, Copy)]
            enum Message { Tick }

            #[widget]
            impl Dial {
                fn size(&self) -> Size {
                    Size::new(Length::Shrink, Length::Shrink)
                }
            }

            impl Dial {
                fn update(&mut self, message: Message) {
                    match message { Message::Tick => () }
                }

                fn view(&self) -> Element<Message> {
                    Dial { radius: self.radius }
                }
            }

            pub fn main() -> iced::Result {
                iced::run(Dial::update, Dial::view)
            }
        "#;
        let engine = rawr::Shared::new(engine);
        let descriptor = engine.compile_application_descriptor(source).unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = rawr::host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let launch = launch.bind(registry).unwrap();
        (
            launch.widget_promotions()[0].site().clone(),
            launch.generation(),
        )
    }
}
