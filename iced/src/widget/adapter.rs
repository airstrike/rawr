//! Retained iced lifecycle adapter for authority-bound scripted widgets.

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{tree, Meta, Operation, Tree, Widget};
use iced::advanced::Shell;
use iced::{Event, Length, Rectangle, Size, Vector};
use rawr::{program, Dynamic};

use crate::{runtime, widget};

use super::script;

mod state {
    use rawr::{program, type_def, widget, Dynamic};

    pub(super) struct State {
        pub(super) key: Key,
        pub(super) local: Local,
        pub(super) layout: Vec<usize>,
        pub(super) overlay: Option<super::surface::State>,
        pub(super) children: Vec<super::widget::Value>,
        pub(super) mode: super::widget::key::Mode,
    }

    #[derive(Eq, PartialEq)]
    pub(super) struct Key {
        site: widget::Site,
        owner: type_def::Identity,
        schema: program::schema::Fingerprint,
        state: widget::state::Type,
    }

    pub(super) enum Local {
        Uninitialized,
        Ready(Option<Dynamic>),
        Failed,
    }

    impl State {
        pub(super) fn bind(promotion: &widget::Promotion) -> Self {
            Self {
                key: Key::bind(promotion),
                local: Local::Uninitialized,
                layout: Vec::new(),
                overlay: None,
                children: Vec::new(),
                mode: super::widget::key::Mode::default(),
            }
        }
    }

    impl Key {
        pub(super) fn bind(promotion: &widget::Promotion) -> Self {
            Self {
                site: promotion.site().clone(),
                owner: promotion.definition().owner().clone(),
                schema: promotion.definition().schema().clone(),
                state: promotion.definition().state().clone(),
            }
        }
    }

    impl Local {
        pub(super) fn value(&self) -> Option<&Dynamic> {
            match self {
                Self::Ready(value) => value.as_ref(),
                Self::Uninitialized | Self::Failed => None,
            }
        }

        pub(super) fn value_mut(&mut self) -> Option<&mut Dynamic> {
            match self {
                Self::Ready(value) => value.as_mut(),
                Self::Uninitialized | Self::Failed => None,
            }
        }

        pub(super) fn is_ready(&self) -> bool {
            matches!(self, Self::Ready(_))
        }

        pub(super) fn retry(&mut self) {
            if matches!(self, Self::Failed) {
                *self = Self::Uninitialized;
            }
        }
    }
}

mod surface {
    use iced::advanced::layout::{self, Layout};
    use iced::advanced::mouse;
    use iced::advanced::overlay;
    use iced::advanced::renderer::{self, Renderer as _};
    use iced::advanced::widget::{Operation, Tree, Widget as _};
    use iced::advanced::Shell;
    use iced::{Event, Rectangle, Size, Vector};

    use crate::{runtime, widget};

    pub(super) struct State {
        identity: widget::Identity,
        element: iced::Element<'static, runtime::Message>,
        pub(super) tree: Tree,
    }

    /// A scripted overlay, laid out within the window when it is created
    /// and placed at the widget's viewport origin plus its translation.
    pub(super) struct Overlay<'a> {
        state: &'a mut State,
        layout: Layout,
        viewport: Rectangle,
        translation: Vector,
        window: Size,
    }

    impl State {
        pub(super) fn bind(
            identity: widget::Identity,
            mut element: iced::Element<'static, runtime::Message>,
        ) -> Self {
            let mut tree = Tree::new(&element);
            tree.diff(&mut element);
            Self {
                identity,
                element,
                tree,
            }
        }

        pub(super) fn reconcile(
            &mut self,
            identity: widget::Identity,
            mut element: iced::Element<'static, runtime::Message>,
        ) -> Result<(), (widget::Identity, iced::Element<'static, runtime::Message>)> {
            if self.identity != identity {
                return Err((identity, element));
            }
            self.tree.diff(&mut element);
            self.element = element;
            Ok(())
        }
    }

    impl<'a> Overlay<'a> {
        pub(super) fn bind(
            state: &'a mut State,
            renderer: &iced::Renderer,
            viewport: Rectangle,
            translation: Vector,
            window: Size,
        ) -> Self {
            let limits = layout::Limits::new(Size::ZERO, window);
            state.element.layout(&mut state.tree, renderer, &limits);
            let layout = Layout::new(state.tree.size).move_to(viewport.position() + translation);
            Self {
                state,
                layout,
                viewport,
                translation,
                window,
            }
        }
    }

    impl overlay::Overlay<runtime::Message, iced::Theme, iced::Renderer> for Overlay<'_> {
        fn draw(
            &self,
            renderer: &mut iced::Renderer,
            theme: &iced::Theme,
            style: &renderer::Style,
            cursor: mouse::Cursor,
        ) {
            renderer.with_layer(self.layout.bounds(), |renderer| {
                self.state.element.draw(
                    &self.state.tree,
                    renderer,
                    theme,
                    style,
                    self.layout,
                    cursor,
                    &self.viewport,
                );
            });
        }

        fn operate(&mut self, renderer: &iced::Renderer, operation: &mut dyn Operation) {
            self.state.element.operate(
                &mut self.state.tree,
                self.layout,
                &self.viewport,
                renderer,
                operation,
            );
        }

        fn update(
            &mut self,
            event: &Event,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, runtime::Message>,
        ) {
            self.state.element.update(
                &mut self.state.tree,
                event,
                self.layout,
                cursor,
                renderer,
                shell,
                &self.viewport,
            );
        }

        fn mouse_interaction(
            &self,
            cursor: mouse::Cursor,
            renderer: &iced::Renderer,
        ) -> mouse::Interaction {
            self.state.element.mouse_interaction(
                &self.state.tree,
                self.layout,
                cursor,
                &self.viewport,
                renderer,
            )
        }

        fn overlay<'a>(
            &'a mut self,
            renderer: &iced::Renderer,
        ) -> Vec<overlay::Element<'a, runtime::Message, iced::Theme, iced::Renderer>> {
            self.state.element.overlay(
                &mut self.state.tree,
                self.layout,
                renderer,
                &self.viewport,
                self.translation,
                self.window,
            )
        }
    }
}

impl script::Bound {
    fn reconcile(&mut self, tree: &mut Tree) {
        let key = state::Key::bind(self.binding.promotion());
        let state = tree.state.downcast_mut::<state::State>();
        if state.key != key {
            *state = state::State::bind(self.binding.promotion());
            tree.children.clear();
            self.children.clear();
            self.children_ready = false;
        }
    }

    fn initialize(&mut self, tree: &mut Tree) -> bool {
        let state = tree.state.downcast_mut::<state::State>();
        match state.local {
            state::Local::Ready(_) => return true,
            state::Local::Failed => return false,
            state::Local::Uninitialized => {}
        }

        let kind = rawr::widget::hook::Kind::State;
        match self.binding.state() {
            Ok(Some(value))
                if self
                    .binding
                    .promotion()
                    .definition()
                    .state()
                    .nominal_type()
                    .is_some() =>
            {
                state.local = state::Local::Ready(Some(value));
                self.clear(kind);
                true
            }
            Ok(None)
                if self
                    .binding
                    .promotion()
                    .definition()
                    .state()
                    .nominal_type()
                    .is_none() =>
            {
                state.local = state::Local::Ready(None);
                true
            }
            Ok(_) => {
                state.local = state::Local::Failed;
                self.record_message(kind, "the state hook returned an incompatible local state");
                false
            }
            Err(error) => {
                state.local = state::Local::Failed;
                self.record(kind, &error);
                false
            }
        }
    }

    fn refresh_children(&mut self, tree: &mut Tree) {
        let kind = rawr::widget::hook::Kind::Children;
        let values = match self.binding.children() {
            Ok(Some(value)) => match value.try_cast_result::<widget::children::List>() {
                Ok(list) => list.values().to_vec(),
                Err(value) => {
                    self.record_message(
                        kind,
                        format!(
                            "children hook returned `{}`, expected `rawr_iced::widget::children::List`",
                            value.type_name()
                        ),
                    );
                    self.restore_children(tree);
                    return;
                }
            },
            Ok(None) => Vec::new(),
            Err(error) => {
                self.record(kind, &error);
                self.restore_children(tree);
                return;
            }
        };
        let mode = match widget::key::Mode::bind(&values) {
            Ok(mode) => mode,
            Err(error) => {
                self.record_message(kind, error.to_string());
                self.restore_children(tree);
                return;
            }
        };
        self.install_children(tree, values, mode);
        self.clear(kind);
    }

    fn install_children(
        &mut self,
        tree: &mut Tree,
        values: Vec<widget::Value>,
        mode: widget::key::Mode,
    ) {
        let context = self
            .services
            .context(self.instance, self.binding.generation());
        let mut children = values
            .iter()
            .map(|value| value.lower(&context))
            .collect::<Vec<_>>();
        drop(context);
        let previous = tree.state.downcast_ref::<state::State>().mode.clone();
        mode.reconcile(&previous, &mut tree.children);
        tree.diff_children(&mut children);
        let state = tree.state.downcast_mut::<state::State>();
        state.children = values;
        state.mode = mode;
        self.children = children;
        self.children_ready = true;
    }

    fn restore_children(&mut self, tree: &mut Tree) {
        if self.children_ready {
            return;
        }
        let values = tree.state.downcast_ref::<state::State>().children.clone();
        let context = self
            .services
            .context(self.instance, self.binding.generation());
        let children = values
            .iter()
            .map(|value| value.lower(&context))
            .collect::<Vec<_>>();
        drop(context);
        self.children = children;
        self.children_ready = true;
    }

    fn clear(&self, kind: rawr::widget::hook::Kind) {
        if let Some(key) = self.key(kind) {
            self.services.diagnostics().clear(&key);
        }
    }

    fn record(&self, kind: rawr::widget::hook::Kind, error: &program::Error) {
        if let Some(key) = self.key(kind) {
            self.services
                .diagnostics()
                .record(key, self.binding.report(kind, error));
        }
    }

    fn record_message(&self, kind: rawr::widget::hook::Kind, message: impl Into<String>) {
        if let Some(key) = self.key(kind) {
            self.services
                .diagnostics()
                .record(key, self.binding.report_message(kind, message));
        }
    }

    fn key(&self, kind: rawr::widget::hook::Kind) -> Option<widget::diagnostic::Key> {
        self.binding
            .promotion()
            .definition()
            .hook(kind)
            .map(|hook| {
                widget::diagnostic::Key::new(
                    self.binding.promotion().site().clone(),
                    kind,
                    hook.location().span(),
                )
            })
    }

    fn local(tree: &Tree) -> Option<Dynamic> {
        tree.state
            .downcast_ref::<state::State>()
            .local
            .value()
            .cloned()
    }

    fn layout_indices(tree: &Tree) -> Vec<usize> {
        tree.state.downcast_ref::<state::State>().layout.clone()
    }

    /// A widget whose layout failed: empty, with no positioned children.
    fn collapse(tree: &mut Tree) {
        tree.state.downcast_mut::<state::State>().layout.clear();
        tree.size = Size::ZERO;
    }

    fn refresh_overlay(&mut self, tree: &mut Tree, content: &widget::Value) {
        let identity = content.identity();
        let context = self
            .services
            .context(self.instance, self.binding.generation());
        let element = content.lower(&context);
        drop(context);
        let overlay = &mut tree.state.downcast_mut::<state::State>().overlay;
        match overlay {
            Some(state) => {
                if let Err((identity, element)) = state.reconcile(identity, element) {
                    *state = surface::State::bind(identity, element);
                }
            }
            None => *overlay = Some(surface::State::bind(identity, element)),
        }
    }
}

impl Meta for script::Bound {}

impl Widget<runtime::Message, iced::Theme, iced::Renderer> for script::Bound {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<state::State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(state::State::bind(self.binding.promotion()))
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.reconcile(tree);
        tree.state.downcast_mut::<state::State>().local.retry();
        if !self.initialize(tree) {
            self.children.clear();
            tree.children.clear();
            return;
        }

        let result = {
            let state = tree.state.downcast_mut::<state::State>();
            self.binding.diff(state.local.value_mut())
        };
        match result {
            Ok(Some(())) => self.clear(rawr::widget::hook::Kind::Diff),
            Ok(None) => {}
            Err(error) => self.record(rawr::widget::hook::Kind::Diff, &error),
        }
        self.refresh_children(tree);
    }

    fn size(&self) -> Size<Length> {
        let kind = rawr::widget::hook::Kind::Size;
        match self.binding.size() {
            Ok(Some(value)) => match value.try_cast_result::<Size<Length>>() {
                Ok(size) => {
                    self.clear(kind);
                    size
                }
                Err(value) => {
                    self.record_message(
                        kind,
                        format!(
                            "size hook returned `{}`, expected `iced::Size`",
                            value.type_name()
                        ),
                    );
                    Size::new(Length::Shrink, Length::Shrink)
                }
            },
            Ok(None) => Size::new(Length::Shrink, Length::Shrink),
            Err(error) => {
                self.record(kind, &error);
                Size::new(Length::Shrink, Length::Shrink)
            }
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) {
        self.reconcile(tree);
        if !self.initialize(tree) {
            Self::collapse(tree);
            return;
        }
        if !self.children_ready {
            self.refresh_children(tree);
        }

        let kind = rawr::widget::hook::Kind::Layout;
        let result = {
            let state = tree.state.downcast_ref::<state::State>();
            self.binding.layout(
                state.local.value(),
                Dynamic::from(widget::layout::Input::new(*limits)),
            )
        };
        let pending = match result {
            Ok(Some(pending)) => pending,
            Ok(None) => {
                Self::collapse(tree);
                return;
            }
            Err(error) => {
                self.record(kind, &error);
                Self::collapse(tree);
                return;
            }
        };
        let node = match pending.output::<widget::layout::Node>() {
            Ok(node) => node,
            Err(error) => {
                self.record_message(
                    kind,
                    format!(
                        "layout hook returned `{}`, expected `rawr_iced::widget::layout::Node`",
                        error.actual()
                    ),
                );
                Self::collapse(tree);
                return;
            }
        };

        // Each positioned child keeps its own size and its translation
        // within this widget in its tree; unpositioned children are skipped.
        let mut indices = Vec::with_capacity(node.children().len());
        for child in node.children() {
            let index = child.index();
            if index >= self.children.len() || indices.contains(&index) {
                self.record_message(
                    kind,
                    "layout hook positioned an unknown or duplicate child index",
                );
                Self::collapse(tree);
                return;
            }
            let bounds = child.bounds();
            let limits = layout::Limits::new(bounds.size(), bounds.size());
            let state = &mut tree.children[index];
            self.children[index].layout(state, renderer, &limits);
            state.translation = Vector::new(bounds.x, bounds.y);
            indices.push(index);
        }
        let commit = {
            let state = tree.state.downcast_mut::<state::State>();
            pending.commit(&mut self.binding, state.local.value_mut())
        };
        if let Err(error) = commit {
            self.record(kind, &error);
            Self::collapse(tree);
            return;
        }
        self.clear(kind);
        tree.state.downcast_mut::<state::State>().layout = indices;
        tree.size = node.size();
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
        if tree.state.downcast_ref::<state::State>().local.is_ready() {
            let kind = rawr::widget::hook::Kind::Draw;
            let input = widget::draw::Input::new(
                layout.bounds(),
                cursor.position_in(layout.bounds()),
                *viewport,
                style.text_color,
            );
            match self.draw(Self::local(tree), input) {
                Ok(Some(replay)) => {
                    self.clear(kind);
                    replay.replay(renderer);
                }
                Ok(None) => {}
                Err(script::Error::Hook(error)) => self.record(kind, &error),
                Err(error) => self.record_message(kind, error.to_string()),
            }
        }

        for &index in &tree.state.downcast_ref::<state::State>().layout {
            let Some(child) = self.children.get(index) else {
                continue;
            };
            let Some(state) = tree.children.get(index) else {
                continue;
            };
            let layout = child_layout(layout, state);
            child.draw(state, renderer, theme, style, layout, cursor, viewport);
        }
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
        let indices = Self::layout_indices(tree);
        for index in indices {
            let Some(child) = self.children.get_mut(index) else {
                continue;
            };
            let Some(state) = tree.children.get_mut(index) else {
                continue;
            };
            let layout = child_layout(layout, state);
            child.update(state, event, layout, cursor, renderer, shell, viewport);
        }
        if shell.is_event_captured() || !self.initialize(tree) {
            return;
        }

        let press = match event {
            Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => cursor.is_over(layout.bounds()),
            Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => {
                layout.bounds().contains(*position)
            }
            _ => false,
        };
        let kind = rawr::widget::hook::Kind::Update;
        let result = {
            let state = tree.state.downcast_mut::<state::State>();
            self.binding.update(
                state.local.value_mut(),
                Dynamic::from(widget::event::Input::new(press)),
            )
        };
        match result {
            Ok(Some(value)) => match value.try_cast_result::<widget::event::Outcome>() {
                Ok(outcome) => {
                    self.clear(kind);
                    if let Some(message) = outcome.message() {
                        shell.publish(runtime::Message::new(
                            self.instance,
                            self.binding.generation(),
                            message.clone(),
                        ));
                    }
                    if outcome.status() == iced::event::Status::Captured {
                        shell.capture_event();
                    }
                }
                Err(value) => self.record_message(
                    kind,
                    format!(
                        "update hook returned `{}`, expected `rawr_iced::widget::event::Outcome`",
                        value.type_name()
                    ),
                ),
            },
            Ok(None) => {}
            Err(error) => self.record(kind, &error),
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let mut interaction = mouse::Interaction::None;
        if tree.state.downcast_ref::<state::State>().local.is_ready() {
            let kind = rawr::widget::hook::Kind::MouseInteraction;
            let input = widget::mouse::Input::new(cursor.is_over(layout.bounds()));
            match self.binding.mouse_interaction(Self::local(tree), Dynamic::from(input)) {
                Ok(Some(value)) => match value.try_cast_result::<mouse::Interaction>() {
                    Ok(value) => {
                        self.clear(kind);
                        interaction = value;
                    }
                    Err(value) => self.record_message(
                        kind,
                        format!(
                            "mouse_interaction hook returned `{}`, expected `iced::mouse::Interaction`",
                            value.type_name()
                        ),
                    ),
                },
                Ok(None) => {}
                Err(error) => self.record(kind, &error),
            }
        }

        for &index in &tree.state.downcast_ref::<state::State>().layout {
            let Some(child) = self.children.get(index) else {
                continue;
            };
            let Some(state) = tree.children.get(index) else {
                continue;
            };
            let layout = child_layout(layout, state);
            interaction =
                interaction.max(child.mouse_interaction(state, layout, cursor, viewport, renderer));
        }
        interaction
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        if !self.initialize(tree) {
            return;
        }
        let kind = rawr::widget::hook::Kind::Operate;
        let result = {
            let state = tree.state.downcast_mut::<state::State>();
            self.binding.operate(
                state.local.value_mut(),
                Dynamic::from(widget::operation::Input::new(layout.bounds())),
            )
        };
        let outcome = match result {
            Ok(Some(value)) => match value.try_cast_result::<widget::operation::Outcome>() {
                Ok(outcome) => {
                    self.clear(kind);
                    outcome
                }
                Err(value) => {
                    self.record_message(
                        kind,
                        format!(
                            "operate hook returned `{}`, expected `rawr_iced::widget::operation::Outcome`",
                            value.type_name()
                        ),
                    );
                    widget::operation::Outcome::SKIP
                }
            },
            Ok(None) => widget::operation::Outcome::SKIP,
            Err(error) => {
                self.record(kind, &error);
                widget::operation::Outcome::SKIP
            }
        };
        if outcome.is_container() {
            operation.container(None, layout.bounds(), viewport);
        }
        if outcome.should_traverse() {
            let indices = Self::layout_indices(tree);
            operation.traverse(&mut |operation| {
                for &index in &indices {
                    let Some(child) = self.children.get_mut(index) else {
                        continue;
                    };
                    let Some(state) = tree.children.get_mut(index) else {
                        continue;
                    };
                    let layout = child_layout(layout, state);
                    child.operate(state, layout, viewport, renderer, operation);
                }
            });
        }
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
        let mut visual = None;
        if self.initialize(tree) {
            let kind = rawr::widget::hook::Kind::Overlay;
            let result = {
                let state = tree.state.downcast_mut::<state::State>();
                self.binding.overlay(
                    state.local.value_mut(),
                    Dynamic::from(widget::overlay::Input::new(*viewport, translation)),
                )
            };
            match result {
                Ok(Some(value)) => match value.try_cast_result::<widget::overlay::Value>() {
                    Ok(value) => {
                        self.clear(kind);
                        visual = value.content().cloned();
                    }
                    Err(value) => self.record_message(
                        kind,
                        format!(
                            "overlay hook returned `{}`, expected `rawr_iced::widget::overlay::Value`",
                            value.type_name()
                        ),
                    ),
                },
                Ok(None) => {}
                Err(error) => self.record(kind, &error),
            }
        }

        if let Some(content) = visual.as_ref() {
            self.refresh_overlay(tree, content);
        } else {
            tree.state.downcast_mut::<state::State>().overlay = None;
        }

        let indices = Self::layout_indices(tree);
        let scripted = tree
            .state
            .downcast_mut::<state::State>()
            .overlay
            .as_mut()
            .map(|state| {
                overlay::Element::new(Box::new(surface::Overlay::bind(
                    state,
                    renderer,
                    *viewport,
                    translation,
                    window,
                )))
            });
        let children = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .enumerate()
            .filter(|(index, _)| indices.contains(index))
            .flat_map(|(_, (child, state))| {
                let layout = child_layout(layout, state);
                child.overlay(state, layout, renderer, viewport, translation, window)
            });

        scripted.into_iter().chain(children).collect()
    }
}

/// The layout of a positioned child: its tree's size at its translation
/// within `parent`.
fn child_layout(parent: Layout, child: &Tree) -> Layout {
    parent
        .iter(std::slice::from_ref(child))
        .next()
        .map(|(layout, _)| layout)
        .expect("a child tree yields its layout")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use iced::advanced::layout::{self, Layout};
    use iced::advanced::mouse;
    use iced::advanced::renderer::{self, Headless};
    use iced::advanced::widget::{tree, Operation, Tree, Widget as _};
    use iced::advanced::Shell;
    use iced::{Event, Length, Point, Rectangle, Size, Vector};
    use iced_runtime::core::{shell, window};
    use rawr::{
        application, host, project, timing, Dynamic, Engine, EvalAltResult, Position, Shared,
    };

    use super::state;

    #[test]
    fn module_state_dial_runs_retained_size_layout_draw_and_update() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/widget_adapter");
        let target = project::Target::discover(root).unwrap();
        let prepared = crate::application::Compiler::new()
            .unwrap()
            .check_observed(&target)
            .into_result()
            .unwrap()
            .prepare()
            .unwrap();
        let mut adapter = prepared.into_adapter();
        drop(adapter.view());
        let identity = adapter.display().identity();
        let context = adapter.context();
        let diagnostics = context.diagnostics();
        let mut element = adapter.display().lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);

        assert!(matches!(
            tree.state.downcast_ref::<state::State>().local,
            state::Local::Ready(Some(_))
        ));
        assert_eq!(
            element.size(),
            Size::new(Length::Fixed(40.0), Length::Fixed(40.0))
        );

        let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
            renderer::Settings::default(),
            Some("tiny-skia"),
        ))
        .expect("the pinned tiny-skia renderer must be available");
        let available = Size::new(100.0, 100.0);
        let limits = layout::Limits::new(Size::ZERO, available);
        element.layout(&mut tree, &renderer, &limits);
        let node = Node::of(&tree);
        assert_eq!(
            node.size(),
            Size::new(40.0, 40.0),
            "{:?}",
            diagnostics.messages()
        );

        let viewport = Rectangle::with_size(available);
        let style = renderer::Style::default();
        element.draw(
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &style,
            node.layout(),
            mouse::Cursor::Available(Point::new(20.0, 20.0)),
            &viewport,
        );
        assert_eq!(diagnostics.len(), 0);

        let mut messages = shell::Bus::new();
        let mut shell = Shell::new(
            &window::Headless,
            iced_runtime::core::shell::Waker::noop(),
            &mut messages,
        );
        element.update(
            &mut tree,
            &Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            node.layout(),
            mouse::Cursor::Available(Point::new(20.0, 20.0)),
            &renderer,
            &mut shell,
            &viewport,
        );
        let captured = shell.is_event_captured();
        drop(shell);
        assert!(
            captured,
            "scripted update was not captured: {} messages, {:?}",
            messages.len(),
            diagnostics.messages(),
        );
        let messages = messages.into_iter().collect::<Vec<_>>();
        let [message] = messages.as_slice() else {
            panic!("the scripted update must publish exactly one message");
        };
        let message = message.clone();
        adapter.update(message).unwrap();

        assert_eq!(adapter.display().identity(), identity);
        assert!(adapter.diagnostics().is_empty());
        assert_eq!(diagnostics.len(), 0);
        let timing = adapter.timing();
        assert_eq!(timing.aggregate(timing::Phase::ViewEvaluation).count(), 2);
        assert_eq!(
            timing.aggregate(timing::Phase::DescriptorLowering).count(),
            1
        );
        assert_eq!(
            timing
                .aggregate(timing::Phase::ScriptedHookDispatch)
                .count(),
            5
        );
        assert!(timing
            .attempts()
            .iter()
            .filter(|attempt| attempt.phase() == timing::Phase::ScriptedHookDispatch)
            .all(|attempt| attempt.outcome() == timing::Outcome::Success));
    }

    #[test]
    fn rejected_layout_reports_exact_source_and_commits_nothing_until_valid() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/widget_report");
        let target = project::Target::discover(root).unwrap();
        let prepared = crate::application::Compiler::new()
            .unwrap()
            .check(&target)
            .unwrap()
            .prepare()
            .unwrap();
        let adapter = prepared.into_adapter();
        let context = adapter.context();
        let diagnostics = context.diagnostics();
        let mut element = adapter.display().lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);
        let renderer = fixture::renderer();

        let first = fixture::node(&mut element, &mut tree, &renderer);
        assert_eq!(first.size(), Size::ZERO);
        let reports = diagnostics.reports();
        let [report] = reports.as_slice() else {
            panic!("one exact layout validation report must be retained");
        };
        assert!(report
            .message()
            .contains("unknown or duplicate child index"));
        let [label] = report.labels() else {
            panic!("layout validation must retain one positioned hook label");
        };
        assert!(label.source().identity().ends_with("src/dial.rs"));
        assert_eq!(label.span().start().line(), Some(38));

        let second = fixture::node(&mut element, &mut tree, &renderer);
        assert_eq!(second.size(), Size::ZERO);
        assert_eq!(diagnostics.len(), 1);

        fixture::update(&mut element, &mut tree, &second, &renderer);
        let valid = fixture::node(&mut element, &mut tree, &renderer);
        assert_eq!(valid.size(), Size::new(20.0, 20.0));
        assert_eq!(valid.children().len(), 1);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn stateless_layout_commits_mutable_receiver_only_after_validation() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();
        let launch = fixture::launch(&engine, fixture::plain());
        let diagnostics = fixture::sink(instance, launch.generation());
        let context = crate::widget::Context::application(&launch, instance, &diagnostics, &theme);
        let value = fixture::view(&launch);
        let mut element = value.lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);

        let first = fixture::node(&mut element, &mut tree, &renderer);
        let second = fixture::node(&mut element, &mut tree, &renderer);
        assert_eq!(first.size(), Size::ZERO);
        assert_eq!(second.size(), Size::ZERO);
        assert_eq!(diagnostics.len(), 1);

        fixture::update(&mut element, &mut tree, &second, &renderer);
        let valid = fixture::node(&mut element, &mut tree, &renderer);
        assert_eq!(valid.size(), Size::new(20.0, 20.0));
        assert_eq!(valid.children().len(), 1);
        assert_eq!(diagnostics.len(), 0);

        let committed = fixture::node(&mut element, &mut tree, &renderer);
        assert_eq!(committed.size(), Size::ZERO);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn pending_layout_rejects_wrong_output_stale_and_cross_binding_commit() {
        let engine = fixture::engine();
        let launch = fixture::launch(&engine, fixture::direct());
        let data = launch.boot_default().unwrap();
        let descriptor = launch
            .promote_widget(&launch.widget_promotions()[0], data)
            .unwrap();
        let mut binding = launch.bind_widget(&descriptor).unwrap();
        let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0));
        let input = || Dynamic::from(crate::widget::layout::Input::new(limits));

        let wrong = binding.layout(None, input()).unwrap().unwrap();
        assert!(wrong.output::<i64>().is_err());
        drop(wrong);

        let cross = binding.layout(None, input()).unwrap().unwrap();
        let mut cloned = binding.clone();
        assert!(cross.commit(&mut cloned, None).is_err());

        let accepted = binding.layout(None, input()).unwrap().unwrap();
        let stale = binding.layout(None, input()).unwrap().unwrap();
        assert_eq!(
            accepted
                .output::<crate::widget::layout::Node>()
                .unwrap()
                .size(),
            Size::ZERO
        );
        accepted.commit(&mut binding, None).unwrap();
        assert!(stale.commit(&mut binding, None).is_err());

        let current = binding.layout(None, input()).unwrap().unwrap();
        assert_eq!(
            current
                .output::<crate::widget::layout::Node>()
                .unwrap()
                .size(),
            Size::new(1.0, 1.0)
        );
        current.commit(&mut binding, None).unwrap();
    }

    #[test]
    fn failed_state_retries_once_at_the_next_compatible_diff() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let first = fixture::launch(&engine, fixture::failing());
        let first_diagnostics = fixture::sink(instance, first.generation());
        let first_context =
            crate::widget::Context::application(&first, instance, &first_diagnostics, &theme);
        let first_value = fixture::view(&first);
        let mut first_element = first_value.lower(&first_context);
        let mut tree = Tree::new(&first_element);
        first_element.diff(&mut tree);

        assert!(matches!(
            tree.state.downcast_ref::<state::State>().local,
            state::Local::Failed
        ));
        assert_eq!(first_diagnostics.len(), 1);
        drop(first_element);

        let second = fixture::launch(&engine, fixture::ready(13, false, false));
        let second_diagnostics = fixture::sink(instance, second.generation());
        let second_context =
            crate::widget::Context::application(&second, instance, &second_diagnostics, &theme);
        let second_value = fixture::view(&second);
        let mut second_element = second_value.lower(&second_context);
        tree.diff(&mut second_element);

        assert!(matches!(
            tree.state.downcast_ref::<state::State>().local,
            state::Local::Ready(Some(_))
        ));
        assert_eq!(second_diagnostics.len(), 0);
    }

    #[test]
    fn compatible_reconstruction_preserves_tree_state_across_generations() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();
        let first = fixture::launch(&engine, fixture::ready(5, false, false));
        let first_diagnostics = fixture::sink(instance, first.generation());
        let first_context =
            crate::widget::Context::application(&first, instance, &first_diagnostics, &theme);
        let first_value = fixture::view(&first);
        let mut first_element = first_value.lower(&first_context);
        let mut tree = Tree::new(&first_element);
        first_element.diff(&mut tree);
        assert_eq!(
            fixture::layout(&mut first_element, &mut tree, &renderer),
            5.0
        );
        drop(first_element);

        let second = fixture::launch(&engine, fixture::ready(17, false, false));
        let second_diagnostics = fixture::sink(instance, second.generation());
        let second_context =
            crate::widget::Context::application(&second, instance, &second_diagnostics, &theme);
        let second_value = fixture::view(&second);
        let mut second_element = second_value.lower(&second_context);
        tree.diff(&mut second_element);

        assert_eq!(
            fixture::layout(&mut second_element, &mut tree, &renderer),
            5.0
        );
        assert_eq!(second_diagnostics.len(), 0);
    }

    #[test]
    fn reload_rebinds_scripted_hooks_preserves_tree_state_and_rejects_old_envelopes() {
        static NEXT_RELOAD: AtomicUsize = AtomicUsize::new(0);

        let engine = fixture::engine();
        let renderer = fixture::renderer();
        let id = NEXT_RELOAD.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("rawr-widget-reload-{}-{id}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        crate::write_marked(&root, "src/main.rs", fixture::ready(5, false, false));
        let target = project::Target::discover(&root).unwrap();
        let first = engine.compile_package_generation(&target).unwrap();
        let publication = Arc::new(rawr::reload::scheduler::publication::Slot::new(
            rawr::reload::source::Inputs::from_graph(first.source()),
        ));
        let source = first.source().entry_source().diagnostic().clone();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let bootstrap = first.bind(&engine, registry).unwrap();
        let mut host = crate::program::Host::bootstrap(bootstrap, Some(source)).unwrap();
        let old = crate::runtime::Message::new(
            host.adapter().instance(),
            host.adapter().generation(),
            Dynamic::UNIT,
        );
        let mut first_element = host.view();
        let mut tree = Tree::new(&first_element);
        first_element.diff(&mut tree);
        let first_layout = fixture::node(&mut first_element, &mut tree, &renderer);
        assert_eq!(first_layout.children()[0].children()[0].size().width, 5.0);
        drop(first_element);

        let source = fixture::ready(17, false, false).replace("state.value", "state.value + 1");
        crate::write_marked(&root, "src/main.rs", source);
        let second = engine.compile_package_generation(&target).unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let now = std::time::Instant::now();
        let gate = rawr::reload::scheduler::Gate::new(std::time::Duration::ZERO);
        gate.observe(now).unwrap();
        let snapshot = rawr::reload::source::Snapshot::from_graph(second.source());
        let candidate = gate
            .start(now)
            .unwrap()
            .complete((second, snapshot))
            .stage_inputs(&publication);
        let admission = host
            .install(&gate, &publication, candidate, &engine, registry)
            .unwrap();
        let rawr::reload::scheduler::Admission::Current(outcome) = admission else {
            panic!("fresh widget replacement must remain current");
        };
        let _task = outcome.into_task();
        let mut second_element = host.view();
        tree.diff(&mut second_element);

        let second_layout = fixture::node(&mut second_element, &mut tree, &renderer);
        assert_eq!(second_layout.children()[0].children()[0].size().width, 6.0);
        assert_eq!(
            host.update(old).unwrap().delivery(),
            crate::program::Delivery::Dropped
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn schema_change_remints_tree_state() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();
        let first = fixture::launch(&engine, fixture::ready(5, false, false));
        let first_diagnostics = fixture::sink(instance, first.generation());
        let first_context =
            crate::widget::Context::application(&first, instance, &first_diagnostics, &theme);
        let first_value = fixture::view(&first);
        let mut first_element = first_value.lower(&first_context);
        let mut tree = Tree::new(&first_element);
        first_element.diff(&mut tree);
        assert_eq!(
            fixture::layout(&mut first_element, &mut tree, &renderer),
            5.0
        );
        drop(first_element);

        let second = fixture::launch(&engine, fixture::ready(17, true, false));
        let second_diagnostics = fixture::sink(instance, second.generation());
        let second_context =
            crate::widget::Context::application(&second, instance, &second_diagnostics, &theme);
        let second_value = fixture::view(&second);
        let mut second_element = second_value.lower(&second_context);
        tree.diff(&mut second_element);

        assert_eq!(
            fixture::layout(&mut second_element, &mut tree, &renderer),
            17.0
        );
        assert_eq!(second_diagnostics.len(), 0);
    }

    #[test]
    fn fresh_elements_do_not_repeat_compatible_state_initialization() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();
        let launch = fixture::launch(&engine, fixture::ready(5, false, true));
        let diagnostics = fixture::sink(instance, launch.generation());
        let context = crate::widget::Context::application(&launch, instance, &diagnostics, &theme);
        let value = fixture::view(&launch);
        let mut element = value.lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);
        let node = fixture::node(&mut element, &mut tree, &renderer);
        fixture::update(&mut element, &mut tree, &node, &renderer);
        drop(element);

        for _ in 0..3 {
            let mut rebuilt = value.lower(&context);
            tree.diff(&mut rebuilt);
            assert_eq!(fixture::layout(&mut rebuilt, &mut tree, &renderer), 11.0);
        }
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn visual_overlay_owns_and_reconciles_its_content_tree() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();
        let launch = fixture::launch(&engine, fixture::overlay());
        let diagnostics = fixture::sink(instance, launch.generation());
        let context = crate::widget::Context::application(&launch, instance, &diagnostics, &theme);
        let value = fixture::view(&launch);
        let mut element = value.lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);
        let node = fixture::node(&mut element, &mut tree, &renderer);
        let viewport = Rectangle::with_size(Size::new(100.0, 100.0));

        for _ in 0..2 {
            let visual = element.overlay(
                &mut tree,
                node.layout(),
                &renderer,
                &viewport,
                Vector::ZERO,
                viewport.size(),
            );
            assert!(
                !visual.is_empty(),
                "the declared visual overlay must materialize"
            );
            drop(visual);
            let overlay = &tree
                .state
                .downcast_ref::<state::State>()
                .overlay
                .as_ref()
                .expect("the adapter Tree must own visual overlay state")
                .tree;
            assert_eq!(Node::of(overlay).children()[0].size(), Size::new(15.0, 15.0));
        }

        let retained = tree
            .state
            .downcast_ref::<state::State>()
            .overlay
            .as_ref()
            .expect("the adapter Tree must own visual overlay state");
        assert!(matches!(
            retained.tree.state.downcast_ref::<state::State>().local,
            state::Local::Ready(Some(_))
        ));
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn first_visual_overlay_diffs_exact_built_in_tree_before_layout() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();
        let launch = fixture::launch(&engine, fixture::native());
        let diagnostics = fixture::sink(instance, launch.generation());
        let context = crate::widget::Context::application(&launch, instance, &diagnostics, &theme);
        let value = fixture::view(&launch);
        let mut element = value.lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);
        let node = fixture::node(&mut element, &mut tree, &renderer);
        let viewport = Rectangle::with_size(Size::new(100.0, 100.0));

        let visual = element.overlay(
            &mut tree,
            node.layout(),
            &renderer,
            &viewport,
            Vector::ZERO,
            viewport.size(),
        );
        assert!(
            !visual.is_empty(),
            "the declared built-in visual overlay must materialize"
        );
        drop(visual);
        let retained = tree
            .state
            .downcast_ref::<state::State>()
            .overlay
            .as_ref()
            .expect("the adapter Tree must own built-in visual overlay state");
        assert!(!retained.tree.children.is_empty());
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn hook_failures_use_every_deterministic_adapter_fallback() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let mut renderer = fixture::renderer();
        let launch = fixture::launch(&engine, fixture::failures());
        let diagnostics = fixture::sink(instance, launch.generation());
        let context = crate::widget::Context::application(&launch, instance, &diagnostics, &theme);
        let value = fixture::view(&launch);
        let mut element = value.lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);
        assert!(tree.children.is_empty());
        assert_eq!(
            element.size(),
            Size::new(Length::Shrink, Length::Shrink)
        );

        let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0));
        element.layout(&mut tree, &renderer, &limits);
        let node = Node::of(&tree);
        assert_eq!(node.size(), Size::ZERO);
        let viewport = Rectangle::with_size(Size::new(100.0, 100.0));
        let style = renderer::Style::default();
        let layout = node.layout();
        element.draw(
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &style,
            layout,
            mouse::Cursor::Unavailable,
            &viewport,
        );

        let mut messages = shell::Bus::new();
        let mut shell = Shell::new(
            &window::Headless,
            iced_runtime::core::shell::Waker::noop(),
            &mut messages,
        );
        element.update(
            &mut tree,
            &Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            node.layout(),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut shell,
            &viewport,
        );
        assert!(!shell.is_event_captured());
        drop(shell);
        assert!(messages.is_empty());
        assert_eq!(
            element.mouse_interaction(
                &tree,
                node.layout(),
                mouse::Cursor::Unavailable,
                &viewport,
                &renderer,
            ),
            mouse::Interaction::None
        );

        let mut operation = Probe::default();
        element
            .operate(&mut tree, node.layout(), &viewport, &renderer, &mut operation);
        assert_eq!(operation.calls, 0);
        assert!(element
            .overlay(
                &mut tree,
                node.layout(),
                &renderer,
                &viewport,
                Vector::ZERO,
                viewport.size(),
            )
            .is_empty());
        assert_eq!(diagnostics.len(), 9, "{:?}", diagnostics.messages());
    }

    #[test]
    fn lifecycle_faults_are_deduplicated_positioned_and_recoverable() {
        let phase = Arc::new(AtomicUsize::new(0));
        let fault = fault_facade();
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        engine.register_host_facade(fault.clone()).unwrap();
        let engine = Shared::new(engine);
        let candidate = engine
            .compile_application(fixture::recoverable_failures())
            .unwrap();
        let faults = candidate
            .items()
            .find(|item| item.key().name() == "Faults")
            .unwrap()
            .clone();
        let recover = candidate
            .items()
            .find(|item| item.key().name() == "Recover")
            .unwrap()
            .clone();
        let launch = engine
            .launch_application(candidate.validate().unwrap())
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        registry
            .install(fault_runtime(fault, phase.clone()))
            .unwrap();
        let bound = launch.bind(registry).unwrap();
        let faults = bound.invoke(&faults, &mut []).unwrap();
        let recover = bound.invoke(&recover, &mut []).unwrap();
        let mut adapter = crate::program::Adapter::boot_observed(bound).unwrap();
        let context = adapter.context();
        let diagnostics = context.diagnostics();
        let wakes = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            fixture::redraw(&diagnostics, &wakes),
            window::RedrawRequest::Wait
        ));

        let mut element = adapter.display().lower(&context);
        let mut tree = Tree::new(&element);
        let retained = tree.state.downcast_ref::<state::State>() as *const state::State;
        element.diff(&mut tree);
        assert!(matches!(
            tree.state.downcast_ref::<state::State>().local,
            state::Local::Failed
        ));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        tree.diff(&mut element);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        let state_reports = diagnostics.reports();
        let [state_report] = state_reports.as_slice() else {
            panic!("one deduplicated state fault must be retained");
        };
        assert!(state_report.message().contains("state fault"));
        let source = state_report
            .labels()
            .iter()
            .find(|label| label.span().start().line().is_some())
            .map(|label| label.source().identity().to_owned())
            .expect("the state fault must retain positioned source evidence");
        assert!(!source.is_empty());
        drop(element);
        drop(context);

        assert!(matches!(
            fixture::redraw(&diagnostics, &wakes),
            window::RedrawRequest::NextFrame
        ));

        phase.store(1, Ordering::Relaxed);
        let message = adapter.context().message(faults);
        assert_eq!(
            adapter.update(message).unwrap(),
            crate::program::Delivery::Applied
        );
        let context = adapter.context();
        let mut element = adapter.display().lower(&context);
        tree.diff(&mut element);
        assert_eq!(
            tree.state.downcast_ref::<state::State>() as *const state::State,
            retained
        );
        assert!(matches!(
            tree.state.downcast_ref::<state::State>().local,
            state::Local::Ready(Some(_))
        ));
        assert_eq!(diagnostics.len(), 2, "{:?}", diagnostics.messages());

        let mut renderer = fixture::renderer();
        for _ in 0..2 {
            let cycle = fixture::cycle(&mut element, &mut tree, &mut renderer);
            assert_eq!(cycle.size, Size::new(Length::Shrink, Length::Shrink));
            assert_eq!(cycle.node.size(), Size::ZERO);
            assert!(cycle.node.children().is_empty());
            assert!(!cycle.captured);
            assert_eq!(cycle.published, 0);
            assert_eq!(cycle.interaction, mouse::Interaction::None);
            assert_eq!(cycle.operations, 0);
            assert!(!cycle.overlay);
        }
        assert_eq!(diagnostics.len(), 9, "{:?}", diagnostics.messages());
        assert_eq!(wakes.load(Ordering::Relaxed), 2);
        let reports = diagnostics.reports();
        for fault in [
            "children fault",
            "diff fault",
            "size fault",
            "layout fault",
            "draw fault",
            "update fault",
            "mouse fault",
            "operate fault",
            "overlay fault",
        ] {
            assert!(
                reports
                    .iter()
                    .any(|report| report.message().contains(fault)),
                "missing positioned report for {fault}: {:?}",
                diagnostics.messages(),
            );
        }
        let positions = reports
            .iter()
            .flat_map(|report| report.labels())
            .filter_map(|label| label.span().start().line())
            .collect::<BTreeSet<_>>();
        assert_eq!(positions.len(), 9, "{reports:#?}");
        assert!(reports
            .iter()
            .all(|report| report.labels().iter().any(|label| {
                label.source().identity() == source && label.span().start().line().is_some()
            })));
        drop(element);
        drop(context);

        assert!(matches!(
            fixture::redraw(&diagnostics, &wakes),
            window::RedrawRequest::NextFrame
        ));

        phase.store(2, Ordering::Relaxed);
        let message = adapter.context().message(recover);
        assert_eq!(
            adapter.update(message).unwrap(),
            crate::program::Delivery::Applied
        );
        let context = adapter.context();
        let mut element = adapter.display().lower(&context);
        tree.diff(&mut element);
        assert_eq!(
            tree.state.downcast_ref::<state::State>() as *const state::State,
            retained
        );
        let cycle = fixture::cycle(&mut element, &mut tree, &mut renderer);
        assert_eq!(
            cycle.size,
            Size::new(Length::Fixed(7.0), Length::Fixed(7.0))
        );
        assert_eq!(cycle.node.size(), Size::new(27.0, 27.0));
        assert!(!cycle.captured);
        assert_eq!(cycle.published, 0);
        assert_eq!(cycle.interaction, mouse::Interaction::Pointer);
        assert_eq!(cycle.operations, 1);
        assert!(!cycle.overlay);
        assert_eq!(diagnostics.len(), 0, "{:?}", diagnostics.messages());
        assert_eq!(wakes.load(Ordering::Relaxed), 3);

        assert!(matches!(
            fixture::redraw(&diagnostics, &wakes),
            window::RedrawRequest::NextFrame
        ));
    }

    #[test]
    fn direct_children_reconcile_positionally_and_layout_uses_declared_indices() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();
        let launch = fixture::launch(&engine, fixture::children());
        let diagnostics = fixture::sink(instance, launch.generation());
        let context = crate::widget::Context::application(&launch, instance, &diagnostics, &theme);
        let value = fixture::view(&launch);
        let mut element = value.lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);
        assert_eq!(tree.children.len(), 2);

        let node = fixture::node(&mut element, &mut tree, &renderer);
        assert_eq!(node.children().len(), 2);
        assert_eq!(node.children()[0].bounds().x, 20.0);
        assert_eq!(node.children()[1].bounds().x, 0.0);

        let mut rebuilt = value.lower(&context);
        tree.diff(&mut rebuilt);
        assert_eq!(tree.children.len(), 2);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn scripted_parent_relocates_keyed_trees_and_rejects_candidates_atomically() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();

        let (first, first_inspect) =
            fixture::launch_with_item(&engine, fixture::keyed(0), "inspect");
        let first_diagnostics = fixture::sink(instance, first.generation());
        let first_context =
            crate::widget::Context::application(&first, instance, &first_diagnostics, &theme);
        let first_value = fixture::view(&first);
        let mut first_element = first_value.lower(&first_context);
        let mut tree = Tree::new(&first_element);
        first_element.diff(&mut tree);
        assert_eq!(tree.children.len(), 2);
        let first_state =
            tree.children[0].state.downcast_ref::<state::State>() as *const state::State;
        let second_state =
            tree.children[1].state.downcast_ref::<state::State>() as *const state::State;
        let node = fixture::node(&mut first_element, &mut tree, &renderer);
        let messages = fixture::published(&mut first_element, &mut tree, &node, &renderer);
        assert_eq!(fixture::values(&first, &first_inspect, &messages), vec![11]);
        drop(first_element);

        for mode in [2, 3, 6] {
            let (invalid, inspect) =
                fixture::launch_with_item(&engine, fixture::keyed(mode), "inspect");
            let diagnostics = fixture::sink(instance, invalid.generation());
            let context =
                crate::widget::Context::application(&invalid, instance, &diagnostics, &theme);
            let value = fixture::view(&invalid);
            let mut element = value.lower(&context);
            tree.diff(&mut element);

            assert_eq!(
                tree.children[0].state.downcast_ref::<state::State>() as *const state::State,
                first_state
            );
            assert_eq!(
                tree.children[1].state.downcast_ref::<state::State>() as *const state::State,
                second_state
            );
            let node = fixture::node(&mut element, &mut tree, &renderer);
            let messages = fixture::published(&mut element, &mut tree, &node, &renderer);
            assert_eq!(fixture::values(&invalid, &inspect, &messages), vec![11]);
            let reports = diagnostics.reports();
            let [report] = reports.as_slice() else {
                panic!("one invalid keyed candidate report must be retained");
            };
            let expected = match mode {
                2 => "require a key",
                3 => "repeat key",
                _ => "children",
            };
            assert!(report.message().contains(expected), "{}", report.message());
            let [label] = report.labels() else {
                panic!("the parent children hook must position the keyed error");
            };
            assert_eq!(label.source().identity(), "<memory>");
            assert!(label.span().start().line().is_some());
            drop(element);
        }

        let (reversed, inspect) = fixture::launch_with_item(&engine, fixture::keyed(1), "inspect");
        let diagnostics = fixture::sink(instance, reversed.generation());
        let context =
            crate::widget::Context::application(&reversed, instance, &diagnostics, &theme);
        let value = fixture::view(&reversed);
        let mut element = value.lower(&context);
        tree.diff(&mut element);

        assert_eq!(
            tree.children[0].state.downcast_ref::<state::State>() as *const state::State,
            second_state
        );
        assert_eq!(
            tree.children[1].state.downcast_ref::<state::State>() as *const state::State,
            first_state
        );
        let node = fixture::node(&mut element, &mut tree, &renderer);
        let messages = fixture::published(&mut element, &mut tree, &node, &renderer);
        assert_eq!(fixture::values(&reversed, &inspect, &messages), vec![22]);
        assert_eq!(diagnostics.len(), 0);
        drop(element);

        let inserted = fixture::launch(&engine, fixture::keyed(4));
        let diagnostics = fixture::sink(instance, inserted.generation());
        let context =
            crate::widget::Context::application(&inserted, instance, &diagnostics, &theme);
        let value = fixture::view(&inserted);
        let mut element = value.lower(&context);
        tree.diff(&mut element);
        assert_eq!(tree.children.len(), 3);
        assert_eq!(
            tree.children[0].state.downcast_ref::<state::State>() as *const state::State,
            second_state
        );
        assert_eq!(
            tree.children[2].state.downcast_ref::<state::State>() as *const state::State,
            first_state
        );
        let inserted_state =
            tree.children[1].state.downcast_ref::<state::State>() as *const state::State;
        assert_ne!(inserted_state, first_state);
        assert_ne!(inserted_state, second_state);
        assert_eq!(diagnostics.len(), 0);
        drop(element);

        let removed = fixture::launch(&engine, fixture::keyed(5));
        let diagnostics = fixture::sink(instance, removed.generation());
        let context = crate::widget::Context::application(&removed, instance, &diagnostics, &theme);
        let value = fixture::view(&removed);
        let mut element = value.lower(&context);
        tree.diff(&mut element);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(
            tree.children[0].state.downcast_ref::<state::State>() as *const state::State,
            second_state
        );
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn nested_keyed_parents_accept_mixed_scripted_and_built_in_children() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let launch = fixture::launch(&engine, fixture::nested());
        let diagnostics = fixture::sink(instance, launch.generation());
        let context = crate::widget::Context::application(&launch, instance, &diagnostics, &theme);
        let value = fixture::view(&launch);
        let mut element = value.lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);

        assert_eq!(tree.children.len(), 2);
        assert_eq!(tree.children[0].children.len(), 1);
        assert!(tree.children[0]
            .state
            .downcast_ref::<state::State>()
            .local
            .is_ready());
        assert!(tree.children[0].children[0]
            .state
            .downcast_ref::<state::State>()
            .local
            .is_ready());
        assert_ne!(tree.children[1].tag, tree::Tag::of::<state::State>());
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn nested_maps_bind_scripted_child_message_before_final_application_message() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();
        let (launch, inspect) = fixture::launch_with_item(&engine, fixture::mapped(), "inspect");
        let diagnostics = fixture::sink(instance, launch.generation());
        let context = crate::widget::Context::application(&launch, instance, &diagnostics, &theme);
        let value = fixture::view(&launch);
        let mut element = value.lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);

        assert_eq!(diagnostics.len(), 0, "{:?}", diagnostics.messages());
        let node = fixture::node(&mut element, &mut tree, &renderer);
        let ignored = fixture::published_event(
            &mut element,
            &mut tree,
            &node,
            &renderer,
            Event::Mouse(iced::mouse::Event::CursorMoved {
                position: Point::new(2.0, 2.0),
            }),
        );
        assert!(ignored.is_empty());
        let messages = fixture::published(&mut element, &mut tree, &node, &renderer);
        assert_eq!(messages.len(), 1);
        assert_eq!(fixture::values(&launch, &inspect, &messages), [7]);
        assert!(messages[0].mapping_error().is_none());
        assert_eq!(diagnostics.len(), 0, "{:?}", diagnostics.messages());
    }

    #[test]
    fn key_on_an_unconsumed_root_value_does_not_change_widget_behavior() {
        let engine = fixture::engine();
        let instance = application::Instance::new();
        let theme = crate::theme::Value::default();
        let renderer = fixture::renderer();
        let launch = fixture::launch(&engine, fixture::ready(5, false, false));
        let diagnostics = fixture::sink(instance, launch.generation());
        let context = crate::widget::Context::application(&launch, instance, &diagnostics, &theme);
        let value = fixture::view(&launch).keyed(crate::widget::key::Value::string("root".into()));
        let mut element = value.lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);

        assert_eq!(fixture::layout(&mut element, &mut tree, &renderer), 5.0);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn built_in_column_retains_last_good_tree_across_invalid_view_and_recovers() {
        let engine = fixture::engine();
        let candidate = engine
            .compile_application(fixture::changing_column())
            .unwrap();
        let invalid = candidate
            .items()
            .find(|item| item.key().name() == "Invalid")
            .unwrap()
            .clone();
        let recover = candidate
            .items()
            .find(|item| item.key().name() == "Recover")
            .unwrap()
            .clone();
        let launch = engine
            .launch_application(candidate.validate().unwrap())
            .unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let bound = launch.bind(registry).unwrap();
        let invalid = bound.invoke(&invalid, &mut []).unwrap();
        let recover = bound.invoke(&recover, &mut []).unwrap();
        let mut adapter = crate::program::Adapter::boot_observed(bound).unwrap();
        let retained = adapter.last_good_output().unwrap().identity();

        let context = adapter.context();
        let mut element = adapter.display().lower(&context);
        let mut tree = Tree::new(&element);
        element.diff(&mut tree);
        let renderer = fixture::renderer();
        assert_eq!(fixture::layout(&mut element, &mut tree, &renderer), 1.0);
        // The keyed Column's own tree is the single child of the wrapper's.
        let first = tree.children[0].children[0]
            .state
            .downcast_ref::<state::State>() as *const state::State;
        let second = tree.children[0].children[1]
            .state
            .downcast_ref::<state::State>() as *const state::State;
        drop(element);
        drop(context);

        let message = adapter.context().message(invalid);
        assert_eq!(
            adapter.update(message).unwrap(),
            crate::program::Delivery::Applied
        );
        assert_eq!(adapter.last_good_output().unwrap().identity(), retained);
        assert_eq!(adapter.diagnostics().len(), 1);
        let context = adapter.context();
        let mut invalid = adapter.display().lower(&context);
        tree.diff(&mut invalid);
        assert_eq!(
            tree.children[0].children[0]
                .state
                .downcast_ref::<state::State>() as *const state::State,
            first
        );
        assert_eq!(
            tree.children[0].children[1]
                .state
                .downcast_ref::<state::State>() as *const state::State,
            second
        );
        drop(invalid);
        drop(context);

        let message = adapter.context().message(recover);
        assert_eq!(
            adapter.update(message).unwrap(),
            crate::program::Delivery::Applied
        );
        assert!(adapter.diagnostics().is_empty());
        assert_ne!(adapter.last_good_output().unwrap().identity(), retained);
        let context = adapter.context();
        let mut recovered = adapter.display().lower(&context);
        tree.diff(&mut recovered);
        assert_eq!(fixture::layout(&mut recovered, &mut tree, &renderer), 2.0);
        assert_eq!(
            tree.children[0].children[1]
                .state
                .downcast_ref::<state::State>() as *const state::State,
            second
        );
    }

    #[derive(Default)]
    struct Probe {
        calls: usize,
    }

    impl Operation for Probe {
        fn traverse(&mut self, _operate: &mut dyn FnMut(&mut dyn Operation)) {
            self.calls += 1;
        }

        fn container(
            &mut self,
            _id: Option<&iced::advanced::widget::Id>,
            _bounds: Rectangle,
            _viewport: &Rectangle,
        ) {
            self.calls += 1;
        }
    }

    fn fault_facade() -> host::facade::Definition {
        let mut builder =
            host::facade::Builder::new("fault", host::facade::Abi::new(1).unwrap()).unwrap();
        let mut value = host::signature::Builder::new();
        value.output(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
        builder
            .item(host::item::Definition::function("value", value.finish()).unwrap())
            .unwrap();
        let mut check = host::signature::Builder::new();
        check.parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
        builder
            .item(host::item::Definition::function("check", check.finish()).unwrap())
            .unwrap();
        builder.finish().unwrap()
    }

    fn fault_runtime(
        definition: host::facade::Definition,
        phase: Arc<AtomicUsize>,
    ) -> host::runtime::Facade {
        let mut builder = host::runtime::Builder::new(definition);
        let value_phase = phase.clone();
        builder
            .bind("fault::value", move |args| {
                if !args.is_empty() {
                    return Err(fault_error("fault::value expected no operands"));
                }
                if value_phase.load(Ordering::Relaxed) == 0 {
                    return Err(fault_error("state fault"));
                }
                Ok(Dynamic::from(7_i64))
            })
            .unwrap();
        builder
            .bind("fault::check", move |args| {
                let [value] = args else {
                    return Err(fault_error("fault::check expected one operand"));
                };
                let id = value
                    .as_int()
                    .map_err(|_| fault_error("fault::check expected i64"))?;
                if phase.load(Ordering::Relaxed) == 1 {
                    let message = match id {
                        1 => "children fault",
                        2 => "diff fault",
                        3 => "size fault",
                        4 => "layout fault",
                        5 => "update fault",
                        6 => "draw fault",
                        7 => "mouse fault",
                        8 => "operate fault",
                        9 => "overlay fault",
                        _ => "unknown lifecycle fault",
                    };
                    return Err(fault_error(message));
                }
                Ok(Dynamic::UNIT)
            })
            .unwrap();
        builder.finish()
    }

    fn fault_error(message: impl Into<String>) -> Box<EvalAltResult> {
        EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
    }

    /// What a layout left in a tree, as a node: its bounds within its
    /// parent and the children it positioned (a scripted widget's declared
    /// children, in their declared order).
    #[derive(Debug)]
    struct Node {
        bounds: Rectangle,
        children: Vec<Node>,
    }

    impl Node {
        fn of(tree: &Tree) -> Self {
            let children = if tree.tag == tree::Tag::of::<state::State>() {
                tree.state
                    .downcast_ref::<state::State>()
                    .layout
                    .iter()
                    .filter_map(|&index| tree.children.get(index))
                    .map(Self::of)
                    .collect()
            } else {
                tree.children.iter().map(Self::of).collect()
            };
            Self {
                bounds: Rectangle::new(Point::ORIGIN + tree.translation, tree.size),
                children,
            }
        }

        fn bounds(&self) -> Rectangle {
            self.bounds
        }

        fn size(&self) -> Size {
            self.bounds.size()
        }

        fn children(&self) -> &[Node] {
            &self.children
        }

        fn layout(&self) -> Layout {
            Layout::new(self.size())
        }
    }

    mod fixture {
        use super::{
            application, host, layout, mouse, renderer, shell, window, Arc, AtomicUsize, Engine,
            Event, Length, Node, Ordering, Point, Probe, Rectangle, Shared, Shell, Size, Tree,
            Vector,
        };
        use iced::advanced::Widget as _;

        pub(super) struct Cycle {
            pub(super) size: Size<Length>,
            pub(super) node: Node,
            pub(super) captured: bool,
            pub(super) published: usize,
            pub(super) interaction: mouse::Interaction,
            pub(super) operations: usize,
            pub(super) overlay: bool,
        }

        pub(super) fn engine() -> Shared<Engine> {
            let mut engine = Engine::new();
            crate::install(&mut engine).unwrap();
            Shared::new(engine)
        }

        pub(super) fn launch(
            engine: &Shared<Engine>,
            source: String,
        ) -> Arc<application::BoundLaunch> {
            let descriptor = engine.compile_application_descriptor(source).unwrap();
            let launch = engine.launch_application(descriptor).unwrap();
            let mut registry = host::runtime::Registry::new();
            crate::install_test_runtime(&mut registry);
            Arc::new(launch.bind(registry).unwrap())
        }

        pub(super) fn launch_with_item(
            engine: &Shared<Engine>,
            source: String,
            name: &str,
        ) -> (Arc<application::BoundLaunch>, rawr::item::Value) {
            let candidate = engine.compile_application(source).unwrap();
            let item = candidate
                .items()
                .find(|item| item.key().name() == name)
                .unwrap()
                .clone();
            let launch = engine
                .launch_application(candidate.validate().unwrap())
                .unwrap();
            let mut registry = host::runtime::Registry::new();
            crate::install_test_runtime(&mut registry);
            (Arc::new(launch.bind(registry).unwrap()), item)
        }

        pub(super) fn sink(
            instance: application::Instance,
            generation: rawr::program::GenerationId,
        ) -> Arc<crate::widget::diagnostic::Sink> {
            Arc::new(crate::widget::diagnostic::Sink::new(instance, generation))
        }

        pub(super) fn view(launch: &application::BoundLaunch) -> crate::widget::Value {
            let mut state = launch.boot_default().unwrap();
            crate::widget::Value::from_dynamic(
                launch
                    .invoke_view(std::slice::from_mut(&mut state))
                    .unwrap(),
            )
            .unwrap()
        }

        pub(super) fn renderer() -> iced::Renderer {
            iced::futures::executor::block_on(<iced::Renderer as renderer::Headless>::new(
                renderer::Settings::default(),
                Some("tiny-skia"),
            ))
            .expect("the pinned tiny-skia renderer must be available")
        }

        pub(super) fn cycle(
            element: &mut iced::Element<'static, crate::runtime::Message>,
            tree: &mut Tree,
            renderer: &mut iced::Renderer,
        ) -> Cycle {
            let size = element.size();
            let node = node(element, tree, renderer);
            let viewport = Rectangle::with_size(Size::new(100.0, 100.0));
            element.draw(
                tree,
                renderer,
                &iced::Theme::Light,
                &renderer::Style::default(),
                node.layout(),
                mouse::Cursor::Unavailable,
                &viewport,
            );
            let mut published = shell::Bus::new();
            let mut shell = Shell::new(
                &window::Headless,
                iced_runtime::core::shell::Waker::noop(),
                &mut published,
            );
            element.update(
                tree,
                &Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
                node.layout(),
                mouse::Cursor::Unavailable,
                renderer,
                &mut shell,
                &viewport,
            );
            let captured = shell.is_event_captured();
            drop(shell);
            let interaction = element.mouse_interaction(
                tree,
                node.layout(),
                mouse::Cursor::Unavailable,
                &viewport,
                renderer,
            );
            let mut operation = Probe::default();
            element
                .operate(tree, node.layout(), &viewport, renderer, &mut operation);
            let overlay = element
                .overlay(
                    tree,
                    node.layout(),
                    renderer,
                    &viewport,
                    Vector::ZERO,
                    viewport.size(),
                )
                .is_empty();
            let overlay = !overlay;
            Cycle {
                size,
                node,
                captured,
                published: published.len(),
                interaction,
                operations: operation.calls,
                overlay,
            }
        }

        pub(super) fn redraw(
            diagnostics: &Arc<crate::widget::diagnostic::Sink>,
            wakes: &Arc<AtomicUsize>,
        ) -> window::RedrawRequest {
            let observed = wakes.clone();
            let mut messages = shell::Bus::new();
            let mut shell = Shell::new(
                &window::Headless,
                iced_runtime::core::shell::Waker::new(move || {
                    observed.fetch_add(1, Ordering::Relaxed);
                }),
                &mut messages,
            );
            diagnostics.synchronize(&mut shell);
            shell.redraw_request()
        }

        pub(super) fn node(
            element: &mut iced::Element<'static, crate::runtime::Message>,
            tree: &mut Tree,
            renderer: &iced::Renderer,
        ) -> Node {
            let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0));
            element.layout(tree, renderer, &limits);
            Node::of(tree)
        }

        pub(super) fn layout(
            element: &mut iced::Element<'static, crate::runtime::Message>,
            tree: &mut Tree,
            renderer: &iced::Renderer,
        ) -> f32 {
            node(element, tree, renderer).size().width
        }

        pub(super) fn update(
            element: &mut iced::Element<'static, crate::runtime::Message>,
            tree: &mut Tree,
            node: &Node,
            renderer: &iced::Renderer,
        ) {
            let viewport = Rectangle::with_size(Size::new(100.0, 100.0));
            let mut messages = shell::Bus::new();
            let mut shell = Shell::new(
                &window::Headless,
                iced_runtime::core::shell::Waker::noop(),
                &mut messages,
            );
            element.update(
                tree,
                &Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
                node.layout(),
                mouse::Cursor::Available(Point::new(1.0, 1.0)),
                renderer,
                &mut shell,
                &viewport,
            );
        }

        pub(super) fn published(
            element: &mut iced::Element<'static, crate::runtime::Message>,
            tree: &mut Tree,
            node: &Node,
            renderer: &iced::Renderer,
        ) -> Vec<crate::runtime::Message> {
            published_event(
                element,
                tree,
                node,
                renderer,
                Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            )
        }

        pub(super) fn published_event(
            element: &mut iced::Element<'static, crate::runtime::Message>,
            tree: &mut Tree,
            node: &Node,
            renderer: &iced::Renderer,
            event: Event,
        ) -> Vec<crate::runtime::Message> {
            let viewport = Rectangle::with_size(Size::new(100.0, 100.0));
            let mut messages = shell::Bus::new();
            let mut shell = Shell::new(
                &window::Headless,
                iced_runtime::core::shell::Waker::noop(),
                &mut messages,
            );
            element.update(
                tree,
                &event,
                node.layout(),
                mouse::Cursor::Available(Point::new(1.0, 1.0)),
                renderer,
                &mut shell,
                &viewport,
            );
            drop(shell);
            messages.into_iter().collect()
        }

        pub(super) fn values(
            launch: &application::BoundLaunch,
            inspect: &rawr::item::Value,
            messages: &[crate::runtime::Message],
        ) -> Vec<i64> {
            messages
                .iter()
                .map(|message| {
                    launch
                        .invoke(inspect, &mut [message.payload().clone()])
                        .unwrap()
                        .as_int()
                        .unwrap()
                })
                .collect()
        }

        pub(super) fn failing() -> String {
            source("value: i64", "throw \"state failed\"", false)
        }

        pub(super) fn ready(value: i64, schema: bool, update: bool) -> String {
            let fields = if schema {
                "value: i64, reset: bool"
            } else {
                "value: i64"
            };
            let state = if schema {
                format!("State {{ value: {value}, reset: true }}")
            } else {
                format!("State {{ value: {value} }}")
            };
            source(fields, &state, update)
        }

        fn source(fields: &str, state: &str, update: bool) -> String {
            let update = update.then_some(
                r#"
                    fn update(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::event::Input,
                    ) -> rawr_iced::widget::event::Outcome<Message> {
                        state.value = 11;
                        ignore()
                    }
                "#,
            );
            format!(
                r#"
                    use iced::Result;
                    use rawr_iced::widget::layout::Size;
                    use iced::Element;
                    use rawr_iced::widget::event::ignore;

                    #[derive(Default)]
                    struct App;
                    struct State {{ {fields} }}
                    struct Dial;
                    #[derive(Debug, Clone, Copy)]
                    enum Message {{ Tick }}

                    #[widget]
                    impl Dial {{
                        fn state(&self) -> State {{ {state} }}

                        fn layout(
                            &mut self,
                            state: &mut State,
                            input: rawr_iced::widget::layout::Input,
                        ) -> rawr_iced::widget::layout::Node {{
                            input.resolve(
                                Size::fixed(state.value, state.value),
                                state.value,
                                state.value,
                            )
                        }}

                        {update}
                    }}

                    impl App {{
                        fn update(&mut self, message: Message) {{
                            match message {{ Message::Tick => () }}
                        }}

                        fn view(&self) -> Element<Message> {{ Dial {{}} }}
                    }}

                    pub fn main() -> Result {{ iced::run(App::update, App::view) }}
                "#,
                update = update.unwrap_or_default(),
            )
        }

        pub(super) fn overlay() -> String {
            r#"
                use iced::Result;
                use rawr_iced::widget::layout::Size;
                use iced::Element;
                use rawr_iced::widget::event::ignore;
                use rawr_iced::widget::overlay::some;

                #[derive(Default)]
                struct App;
                struct State { value: i64 }
                struct Popup;
                struct Dial;
                #[derive(Debug, Clone, Copy)]
                enum Message { Tick }

                #[widget]
                impl Popup {
                    fn state(&self) -> State { State { value: 15 } }

                    fn layout(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {
                        input.resolve(Size::fixed(state.value, state.value), 15, 15)
                    }

                    fn update(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::event::Input,
                    ) -> rawr_iced::widget::event::Outcome<Message> {
                        ignore()
                    }
                }

                pub fn popup() -> Element<Message> { Popup {} }

                #[widget]
                impl Dial {
                    fn layout(
                        &mut self,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {
                        input.resolve(Size::fixed(20, 20), 20, 20)
                    }

                    fn overlay(
                        &mut self,
                        input: rawr_iced::widget::overlay::Input,
                    ) -> rawr_iced::widget::overlay::Value<Message> {
                        some(popup())
                    }
                }

                impl App {
                    fn update(&mut self, message: Message) {
                        match message { Message::Tick => () }
                    }

                    fn view(&self) -> Element<Message> { Dial {} }
                }

                pub fn main() -> Result { iced::run(App::update, App::view) }
            "#
            .into()
        }

        pub(super) fn native() -> String {
            r#"
                use iced::Result;
                use rawr_iced::widget::layout::Size;
                use iced::Element;
                use iced::widget::{button, column};
                use rawr_iced::widget::overlay::some;

                #[derive(Default)]
                struct App;
                struct Dial;
                #[derive(Debug, Clone, Copy)]
                enum Message { Tick }

                pub fn popup() -> Element<Message> {
                    column![button("popup")]
                }

                #[widget]
                impl Dial {
                    fn layout(
                        &mut self,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {
                        input.resolve(Size::fixed(20, 20), 20, 20)
                    }

                    fn overlay(
                        &mut self,
                        input: rawr_iced::widget::overlay::Input,
                    ) -> rawr_iced::widget::overlay::Value<Message> {
                        some(popup())
                    }
                }

                impl App {
                    fn update(&mut self, message: Message) {
                        match message { Message::Tick => () }
                    }

                    fn view(&self) -> Element<Message> { Dial {} }
                }

                pub fn main() -> Result { iced::run(App::update, App::view) }
            "#
            .into()
        }

        pub(super) fn plain() -> String {
            r#"
                use iced::Result;
                use rawr_iced::widget::layout::Size;
                use iced::Element;
                use iced::widget::text;
                use rawr_iced::widget::children::{List, list};
                use rawr_iced::widget::event::{Outcome, ignore};

                #[derive(Default)]
                struct App;
                struct Dial { index: i64 }
                #[derive(Debug, Clone, Copy)]
                enum Message { Tick }

                pub fn label() -> Element<Message> { text("child") }

                #[widget]
                impl Dial {
                    fn children(&self) -> List<Message> { list![label()] }

                    fn layout(
                        &mut self,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {
                        let index = self.index;
                        self.index = 1;
                        input
                            .resolve(Size::fixed(20, 20), 20, 20)
                            .child(index, 0, 0, 20, 20)
                    }

                    fn update(
                        &mut self,
                        input: rawr_iced::widget::event::Input,
                    ) -> Outcome<Message> {
                        self.index = 0;
                        ignore()
                    }
                }

                impl App {
                    fn update(&mut self, message: Message) {
                        match message { Message::Tick => () }
                    }

                    fn view(&self) -> Element<Message> { Dial { index: 9 } }
                }

                pub fn main() -> Result { iced::run(App::update, App::view) }
            "#
            .into()
        }

        pub(super) fn direct() -> String {
            r#"
                use iced::Result;
                use rawr_iced::widget::layout::Size;
                use iced::Element;

                #[derive(Default)]
                struct Dial { value: i64 }
                #[derive(Debug, Clone, Copy)]
                enum Message { Tick }

                #[widget]
                impl Dial {
                    fn layout(
                        &mut self,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {
                        let value = self.value;
                        self.value += 1;
                        input.resolve(Size::fixed(value, value), value, value)
                    }
                }

                impl Dial {
                    fn update(&mut self, message: Message) {
                        match message { Message::Tick => () }
                    }

                    fn view(&self) -> Element<Message> {
                        Dial { value: self.value }
                    }
                }

                pub fn main() -> Result { iced::run(Dial::update, Dial::view) }
            "#
            .into()
        }

        pub(super) fn mapped() -> String {
            r#"
                use iced::Result;
                use rawr_iced::widget::layout::Size;
                use iced::Element;
                use rawr_iced::widget::event::{Outcome, publish};

                #[derive(Default)]
                struct App;
                struct Dial;
                #[derive(Debug, Clone, Copy)]
                enum Child { Changed(i64) }
                #[derive(Debug, Clone, Copy)]
                enum Mid { Child(Child) }
                #[derive(Debug, Clone, Copy)]
                enum Message { Mid(Mid) }

                pub fn inspect(message: Message) -> i64 {
                    match message {
                        Message::Mid(Mid::Child(Child::Changed(value))) => value,
                    }
                }

                #[widget]
                impl Dial {
                    fn layout(
                        &mut self,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {
                        input.resolve(Size::fixed(20, 20), 20, 20)
                    }

                    fn update(
                        &mut self,
                        input: rawr_iced::widget::event::Input,
                    ) -> Outcome<Child> {
                        publish(Child::Changed(7)).when(input.is_press())
                    }
                }

                pub fn dial() -> Element<Child> { Dial {} }

                impl App {
                    fn update(&mut self, message: Message) {
                        match message {
                            Message::Mid(Mid::Child(Child::Changed(_))) => (),
                        }
                    }

                    fn view(&self) -> Element<Message> {
                        dial().map(Mid::Child).map(Message::Mid)
                    }
                }

                pub fn main() -> Result { iced::run(App::update, App::view) }
            "#
            .into()
        }

        pub(super) fn failures() -> String {
            r#"
                use iced::Result;
                use rawr_iced::widget::layout::Size;
                use iced::mouse::Interaction;
                use iced::Element;
                use rawr_iced::widget::children::List;

                #[derive(Default)]
                struct App;
                struct State { value: i64 }
                struct Dial;
                #[derive(Debug, Clone, Copy)]
                enum Message { Tick }

                #[widget]
                impl Dial {
                    fn state(&self) -> State { State { value: 0 } }
                    fn children(&self) -> List<Message> { throw "children"; }
                    fn diff(&mut self, state: &mut State) { throw "diff"; }
                    fn size(&self) -> Size { throw "size"; }

                    fn layout(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node { throw "layout"; }

                    fn update(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::event::Input,
                    ) -> rawr_iced::widget::event::Outcome<Message> { throw "update"; }

                    fn draw(
                        &self,
                        state: &State,
                        renderer: &mut rawr_iced::Renderer,
                        input: rawr_iced::widget::draw::Input,
                    ) { throw "draw"; }

                    fn mouse_interaction(
                        &self,
                        state: &State,
                        input: rawr_iced::widget::mouse::Input,
                    ) -> Interaction { throw "mouse"; }

                    fn operate(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::operation::Input,
                    ) -> rawr_iced::widget::operation::Outcome { throw "operate"; }

                    fn overlay(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::overlay::Input,
                    ) -> rawr_iced::widget::overlay::Value<Message> { throw "overlay"; }
                }

                impl App {
                    fn update(&mut self, message: Message) {
                        match message { Message::Tick => () }
                    }

                    fn view(&self) -> Element<Message> { Dial {} }
                }

                pub fn main() -> Result { iced::run(App::update, App::view) }
            "#
            .into()
        }

        pub(super) fn recoverable_failures() -> String {
            r#"
                use iced::{Color, Result};
                use rawr_iced::widget::layout::Size;
                use iced::mouse::Interaction;
                use iced::Element;
                use rawr_iced::widget::children::{List, list};
                use rawr_iced::widget::event::ignore;
                use rawr_iced::widget::operation::container;
                use rawr_iced::widget::overlay::none;
                use fault::{check, value};

                #[derive(Default)]
                struct App { mode: i64 }
                struct State { value: i64 }
                struct Dial { mode: i64 }
                #[derive(Debug, Clone, Copy)]
                enum Message { Faults, Recover }

                #[widget]
                impl Dial {
                    fn state(&self) -> State { State { value: value() } }

                    fn children(&self) -> List<Message> {
                        check(1);
                        list![]
                    }

                    fn diff(&mut self, state: &mut State) {
                        self.mode += 1;
                        state.value += 10;
                        check(2)
                    }

                    fn size(&self) -> Size {
                        check(3);
                        Size::fixed(7, 7)
                    }

                    fn layout(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {
                        self.mode += 1;
                        state.value += 10;
                        check(4);
                        input.resolve(
                            Size::fixed(state.value, state.value),
                            state.value,
                            state.value,
                        )
                    }

                    fn update(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::event::Input,
                    ) -> rawr_iced::widget::event::Outcome<Message> {
                        self.mode += 1;
                        state.value += 10;
                        check(5);
                        ignore()
                    }

                    fn draw(
                        &self,
                        state: &State,
                        renderer: &mut rawr_iced::Renderer,
                        input: rawr_iced::widget::draw::Input,
                    ) {
                        renderer.fill_quad(input.bounds(), Color::BLACK);
                        check(6)
                    }

                    fn mouse_interaction(
                        &self,
                        state: &State,
                        input: rawr_iced::widget::mouse::Input,
                    ) -> Interaction {
                        check(7);
                        Interaction::Pointer
                    }

                    fn operate(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::operation::Input,
                    ) -> rawr_iced::widget::operation::Outcome {
                        self.mode += 1;
                        state.value += 10;
                        check(8);
                        container()
                    }

                    fn overlay(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::overlay::Input,
                    ) -> rawr_iced::widget::overlay::Value<Message> {
                        self.mode += 1;
                        state.value += 10;
                        check(9);
                        none()
                    }
                }

                impl App {
                    fn update(&mut self, message: Message) {
                        match message {
                            Message::Faults => self.mode = 1,
                            Message::Recover => self.mode = 2,
                        }
                    }

                    fn view(&self) -> Element<Message> { Dial { mode: self.mode } }
                }

                pub fn main() -> Result { iced::run(App::update, App::view) }
            "#
            .into()
        }

        pub(super) fn children() -> String {
            r#"
                use iced::Result;
                use rawr_iced::widget::layout::Size;
                use iced::Element;
                use iced::widget::text;
                use rawr_iced::widget::children::{List, list};

                #[derive(Default)]
                struct App;
                struct Dial;
                #[derive(Debug, Clone, Copy)]
                enum Message { Tick }

                pub fn first() -> Element<Message> { text("first") }
                pub fn second() -> Element<Message> { text("second") }

                #[widget]
                impl Dial {
                    fn children(&self) -> List<Message> {
                        list![first(), second()]
                    }

                    fn layout(
                        &mut self,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {
                        input
                            .resolve(Size::fixed(40, 20), 40, 20)
                            .child(1, 20, 0, 20, 20)
                            .child(0, 0, 0, 20, 20)
                    }
                }

                impl App {
                    fn update(&mut self, message: Message) {
                        match message { Message::Tick => () }
                    }

                    fn view(&self) -> Element<Message> { Dial {} }
                }

                pub fn main() -> Result { iced::run(App::update, App::view) }
            "#
            .into()
        }

        pub(super) fn nested() -> String {
            r#"
                use iced::Result;
                use iced::Element;
                use iced::widget::text;
                use rawr_iced::widget::children::{List, list};
                use rawr_iced::widget::key;

                #[derive(Default)]
                struct App;
                struct State { value: i64 }
                struct Leaf;
                struct Inner;
                struct Outer;
                #[derive(Debug, Clone, Copy)]
                enum Message { Tick }

                pub fn leaf_value() -> Element<Message> { Leaf {} }
                pub fn leaf() -> Element<Message> { key(leaf_value(), "leaf") }
                pub fn inner_value() -> Element<Message> { Inner {} }
                pub fn inner() -> Element<Message> { key(inner_value(), "inner") }
                pub fn label_value() -> Element<Message> { text("built-in") }
                pub fn label() -> Element<Message> { key(label_value(), "label") }

                #[widget]
                impl Leaf {
                    fn state(&self) -> State { State { value: 1 } }
                }

                #[widget]
                impl Inner {
                    fn children(&self) -> List<Message> { list![leaf()] }
                }

                #[widget]
                impl Outer {
                    fn children(&self) -> List<Message> { list![inner(), label()] }
                }

                impl App {
                    fn update(&mut self, message: Message) {
                        match message { Message::Tick => () }
                    }

                    fn view(&self) -> Element<Message> { Outer {} }
                }

                pub fn main() -> Result { iced::run(App::update, App::view) }
            "#
            .into()
        }

        pub(super) fn changing_column() -> String {
            r#"
                use iced::Result;
                use rawr_iced::widget::layout::Size;
                use iced::Element;
                use iced::widget::{Column, column};
                use rawr_iced::widget::key;

                #[derive(Default)]
                struct App { key: i64 }
                struct State { value: i64 }
                struct Child { value: i64 }
                #[derive(Debug, Clone, Copy)]
                enum Message { Invalid, Recover }

                pub fn first_value(id: i64) -> Element<Message> { Child { value: id } }
                pub fn second_value() -> Element<Message> { Child { value: 1 } }
                pub fn first(id: i64) -> Element<Message> {
                    key(first_value(id), id)
                }
                pub fn second() -> Element<Message> { key(second_value(), 1) }

                #[widget]
                impl Child {
                    fn state(&self) -> State { State { value: self.value } }

                    fn layout(
                        &mut self,
                        state: &mut State,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {
                        input.resolve(
                            Size::fixed(state.value, state.value),
                            state.value,
                            state.value,
                        )
                    }
                }

                impl App {
                    fn update(&mut self, message: Message) {
                        match message {
                            Message::Invalid => self.key = 1,
                            Message::Recover => self.key = 2,
                        }
                    }

                    fn view(&self) -> Column<'_, Message> {
                        column![first(self.key), second()]
                    }
                }

                pub fn main() -> Result { iced::run(App::update, App::view) }
            "#
            .into()
        }

        pub(super) fn keyed(mode: i64) -> String {
            let children = match mode {
                0 => "list![first(), second()]",
                1 => "list![second(), first()]",
                2 => "list![first(), unkeyed()]",
                3 => "list![first(), duplicate()]",
                4 => "list![second(), third(), first()]",
                5 => "list![second()]",
                _ => "throw \"children\"",
            };
            let children = format!("{children:<48}");
            format!(
                r#"
                use iced::Result;
                use rawr_iced::widget::layout::Size;
                use iced::Element;
                use rawr_iced::widget::children::{{List, list}};
                use rawr_iced::widget::event::{{Outcome, publish}};
                use rawr_iced::widget::key;

                #[derive(Default)]
                struct App;
                struct Local {{ value: i64 }}
                struct Child {{ value: i64 }}
                struct Parent;
                #[derive(Debug, Clone, Copy)]
                enum Message {{ Value(i64) }}

                pub fn first_value() -> Element<Message> {{ Child {{ value: 10 }} }}
                pub fn second_value() -> Element<Message> {{ Child {{ value: 20 }} }}
                pub fn third_value() -> Element<Message> {{ Child {{ value: 30 }} }}
                pub fn unkeyed() -> Element<Message> {{ Child {{ value: 30 }} }}
                pub fn first() -> Element<Message> {{ key(first_value(), "first") }}
                pub fn second() -> Element<Message> {{ key(second_value(), "second") }}
                pub fn third() -> Element<Message> {{ key(third_value(), "third") }}
                pub fn duplicate() -> Element<Message> {{ key(second_value(), "first") }}
                pub fn inspect(message: Message) -> i64 {{
                    match message {{ Message::Value(value) => value }}
                }}

                #[widget]
                impl Child {{
                    fn state(&self) -> Local {{ Local {{ value: self.value }} }}

                    fn diff(&mut self, state: &mut Local) {{ state.value += 1; }}

                    fn layout(
                        &mut self,
                        state: &mut Local,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {{
                        input.resolve(Size::fixed(20, 20), 20, 20)
                    }}

                    fn update(
                        &mut self,
                        state: &mut Local,
                        input: rawr_iced::widget::event::Input,
                    ) -> Outcome<Message> {{ publish(Message::Value(state.value)) }}
                }}

                #[widget]
                impl Parent {{
                    fn children(&self) -> List<Message> {{
                        {children}
                    }}

                    fn layout(
                        &mut self,
                        input: rawr_iced::widget::layout::Input,
                    ) -> rawr_iced::widget::layout::Node {{
                        input
                            .resolve(Size::fixed(40, 20), 40, 20)
                            .child(0, 0, 0, 20, 20)
                            .child(1, 20, 0, 20, 20)
                    }}
                }}

                impl App {{
                    fn update(&mut self, message: Message) {{
                        match message {{ Message::Value(_) => () }}
                    }}

                    fn view(&self) -> Element<Message> {{ Parent {{}} }}
                }}

                pub fn main() -> Result {{ iced::run(App::update, App::view) }}
            "#
            )
        }
    }
}
