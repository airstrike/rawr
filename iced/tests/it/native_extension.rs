use std::sync::atomic::{AtomicUsize, Ordering};

use rawr::{host, Engine, Shared};
use rawr_iced::{program, runtime};

static CUSTOM_LAYOUTS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn external_facade_lowers_a_real_custom_widget_without_builtin_dispatch() {
    let (mut adapter, dependencies) = boot();

    assert_eq!(
        dependencies
            .iter()
            .map(host::facade::Dependency::root)
            .collect::<Vec<_>>(),
        ["gallery", "iced"]
    );

    let increment = click(&adapter, "Native badge 0");
    assert_eq!(increment.instance(), adapter.instance());
    assert_eq!(increment.generation(), adapter.generation());
    assert_eq!(
        adapter.update(increment).unwrap(),
        program::Delivery::Applied
    );
    assert_text(&adapter, "Native badge 1");
    assert!(CUSTOM_LAYOUTS.load(Ordering::Relaxed) >= 2);
    assert!(adapter.diagnostics().is_empty());
}

#[test]
fn foreign_custom_widget_message_is_dropped_without_mutation_or_diagnostic() {
    let (mut adapter, _) = boot();
    let (foreign, _) = boot();
    let stale = click(&foreign, "Native badge 0");
    let diagnostics = adapter.diagnostics().to_vec();

    let transition = adapter.update(stale).unwrap();
    assert_eq!(transition.delivery(), program::Delivery::Dropped);
    assert!(transition.plan().is_none());
    assert_eq!(adapter.diagnostics(), diagnostics);
    adapter.refresh_view().unwrap();
    assert_text(&adapter, "Native badge 0");
    assert_eq!(adapter.diagnostics(), diagnostics);
}

#[test]
fn direct_native_value_construction_is_unattested_at_a_typed_rawr_boundary() {
    let (adapter, _) = boot_with(false);

    assert!(adapter.last_good_output().is_none());
    assert_eq!(adapter.diagnostics().len(), 1);
    assert!(
        adapter.diagnostics()[0]
            .message()
            .contains("failed exact type attestation"),
        "{}",
        adapter.diagnostics()[0].message()
    );
}

#[test]
fn scripted_keyed_parent_accepts_an_external_native_direct_child() {
    CUSTOM_LAYOUTS.store(0, Ordering::Relaxed);
    let (adapter, _) = boot_source(keyed_parent_source(), true);

    let _simulator = iced_test::simulator(adapter.view());
    assert!(CUSTOM_LAYOUTS.load(Ordering::Relaxed) >= 1);
    assert!(adapter.diagnostics().is_empty());
}

fn boot() -> (program::Adapter, Vec<host::facade::Dependency>) {
    boot_with(true)
}

fn boot_with(authenticated_projection: bool) -> (program::Adapter, Vec<host::facade::Dependency>) {
    boot_source(source(), authenticated_projection)
}

fn boot_source(
    source: &str,
    authenticated_projection: bool,
) -> (program::Adapter, Vec<host::facade::Dependency>) {
    let iced = rawr_iced::facade().unwrap();
    let value = iced.export_type("Element").unwrap();
    let rawr = rawr_iced::extension(&iced).unwrap();
    let gallery = gallery::facade(value);

    let mut engine = Engine::new();
    engine.register_host_facade(iced).unwrap();
    engine.register_host_facade(rawr).unwrap();
    engine.register_host_facade(gallery.clone()).unwrap();
    let descriptor = engine.compile_application_descriptor(source).unwrap();
    let engine = Shared::new(engine);
    let launch = engine.launch_application(descriptor).unwrap();
    let dependencies = launch.dependencies().to_vec();
    let mut registry = host::runtime::Registry::new();
    for facade in rawr_iced::runtime_facades().unwrap() {
        registry.install(facade).unwrap();
    }
    registry
        .install(gallery::runtime(gallery, authenticated_projection))
        .unwrap();
    let adapter = program::Adapter::boot(launch.bind(registry).unwrap()).unwrap();
    (adapter, dependencies)
}

fn keyed_parent_source() -> &'static str {
    r#"
use gallery::badge;
use iced::Result;
use iced::Element;
use rawr_iced::widget::children::{List, list};
use rawr_iced::widget::key;
use rawr_iced::widget::layout::Size;

#[derive(Default)]
struct Counter { value: i64 }
struct Scripted;
struct Parent { value: i64 }
#[derive(Debug, Clone, Copy)]
enum Message { Increment }

pub fn scripted_value() -> Element<Message> { Scripted {} }
pub fn scripted() -> Element<Message> { key(scripted_value(), "scripted") }
pub fn native_value(value: i64) -> Element<Message> {
    badge(value, Message::Increment)
}
pub fn native(value: i64) -> Element<Message> { key(native_value(value), "native") }

#[widget]
impl Scripted {
    fn layout(
        &mut self,
        input: rawr_iced::widget::layout::Input,
    ) -> rawr_iced::widget::layout::Node {
        input.resolve(Size::fixed(20, 20), 20, 20)
    }
}

#[widget]
impl Parent {
    fn children(&self) -> List<Message> { list![scripted(), native(self.value)] }

    fn layout(
        &mut self,
        input: rawr_iced::widget::layout::Input,
    ) -> rawr_iced::widget::layout::Node {
        input
            .resolve(Size::fixed(180, 40), 180, 40)
            .child(0, 0, 0, 20, 20)
            .child(1, 20, 0, 160, 40)
    }
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message { Message::Increment => self.value += 1 }
    }

    fn view(&self) -> Element<Message> { Parent { value: self.value } }
}

pub fn main() -> Result { iced::run(Counter::update, Counter::view) }
"#
}

fn assert_text(adapter: &program::Adapter, expected: &str) {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.find(expected).unwrap();
}

fn click(adapter: &program::Adapter, label: &str) -> runtime::Message {
    let mut simulator = iced_test::simulator(adapter.view());
    simulator.click(label).unwrap();
    let mut messages = simulator.into_messages();
    let message = messages
        .next()
        .expect("the external native widget must emit once");
    assert!(messages.next().is_none());
    message
}

fn source() -> &'static str {
    r#"
use gallery::badge;

#[derive(Default)]
struct Counter {
    value: i64,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    Increment,
}

impl Counter {
    fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
        }
    }

    fn view(&self) -> gallery::Badge<Message> {
        badge(self.value, Message::Increment)
    }
}

pub fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}
"#
}

mod gallery {
    use rawr::{capability, host, Dynamic, EvalAltResult, Position};

    use super::badge;

    pub fn facade(target: host::type_def::Export) -> host::facade::Definition {
        let mut builder =
            host::facade::Builder::new("gallery", host::facade::Abi::new(1).unwrap()).unwrap();
        builder
            .type_::<badge::Descriptor>("Badge", &["Message"], &[])
            .unwrap();
        builder.project_to("Badge", target, &["Message"]).unwrap();

        let mut signature = host::signature::Builder::new();
        signature
            .type_parameter(
                "Message",
                &[
                    capability::Kind::Clone,
                    capability::Kind::Send,
                    capability::Kind::Static,
                ],
            )
            .unwrap();
        signature.parameter(host::type_ref::Type::builtin(host::type_ref::Builtin::I64));
        signature.parameter(host::type_ref::Type::parameter("Message").unwrap());
        signature.output(
            host::type_ref::Type::applied(
                "Badge",
                [host::type_ref::Type::parameter("Message").unwrap()],
            )
            .unwrap(),
        );
        builder
            .item(host::item::Definition::function("badge", signature.finish()).unwrap())
            .unwrap();
        builder.finish().unwrap()
    }

    pub fn runtime(
        definition: host::facade::Definition,
        authenticated_projection: bool,
    ) -> host::runtime::Facade {
        let mut builder = host::runtime::Builder::new(definition);
        builder
            .bind_type_attestation("gallery::Badge", |value, expected| {
                let badge = value.read_lock::<badge::Descriptor>()?;
                if expected.argument_count() != 1 || !expected.argument_matches(0, badge.payload())
                {
                    return None;
                }
                expected.attest_witness(Some(badge.witness()))
            })
            .unwrap();
        builder
            .bind_with_context("gallery::badge", |call, args| {
                if args.len() != 2 {
                    return Err(runtime_error(format!(
                        "gallery::badge received {} operand(s), expected 2",
                        args.len()
                    )));
                }
                let count = args[0].as_int().map_err(|_| {
                    runtime_error(format!(
                        "gallery::badge expected `i64`, found `{}`",
                        args[0].type_name()
                    ))
                })?;
                Ok(Dynamic::from(badge::Descriptor::new(
                    count,
                    args[1].take(),
                    call.output_witness()
                        .map_err(|error| runtime_error(error.to_string()))?,
                )))
            })
            .unwrap();
        if authenticated_projection {
            builder
                .bind_projection_with_context("gallery::Badge", "iced::Element", |call, args| {
                    if args.len() != 1 {
                        return Err(runtime_error(format!(
                            "gallery widget projection received {} operand(s), expected 1",
                            args.len()
                        )));
                    }
                    let actual = args[0].type_name();
                    let descriptor = args[0]
                        .take()
                        .try_cast_result::<badge::Descriptor>()
                        .map_err(|_| {
                            runtime_error(format!(
                            "gallery widget projection expected `gallery::Badge`, found `{actual}`"
                        ))
                        })?;
                    Ok(Dynamic::from(
                        rawr_iced::widget::Value::from_projection(
                            descriptor.clone(),
                            call,
                            descriptor.witness(),
                        )
                        .map_err(|error| runtime_error(error.to_string()))?,
                    ))
                })
                .unwrap();
        } else {
            builder
                .bind_projection("gallery::Badge", "iced::Element", |args| {
                    let descriptor = args[0]
                        .take()
                        .try_cast_result::<badge::Descriptor>()
                        .map_err(|_| runtime_error("expected gallery::Badge".to_owned()))?;
                    Ok(Dynamic::from(rawr_iced::widget::Value::new(descriptor)))
                })
                .unwrap();
        }
        builder.finish()
    }

    fn runtime_error(message: String) -> Box<EvalAltResult> {
        EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
    }
}

mod badge {
    use std::sync::atomic::Ordering;

    use iced::advanced::{self, Layout, Shell};
    use iced::widget::Widget as _;
    use rawr::Dynamic;
    use rawr_iced::{runtime, widget};

    use super::CUSTOM_LAYOUTS;

    #[derive(Clone)]
    pub struct Descriptor {
        witness: rawr::host::runtime::Witness,
        identity: widget::Identity,
        count: i64,
        payload: Dynamic,
    }

    struct Widget {
        content: iced::Element<'static, runtime::Message>,
    }

    impl Descriptor {
        pub fn new(count: i64, payload: Dynamic, witness: rawr::host::runtime::Witness) -> Self {
            Self {
                witness,
                identity: widget::Identity::unique(),
                count,
                payload,
            }
        }

        pub const fn payload(&self) -> &Dynamic {
            &self.payload
        }

        pub const fn witness(&self) -> &rawr::host::runtime::Witness {
            &self.witness
        }
    }

    impl widget::Descriptor for Descriptor {
        fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
            let content =
                iced::widget::button(iced::widget::text(format!("Native badge {}", self.count)))
                    .on_press(context.message(self.payload.clone()))
                    .boxed();
            Widget { content }.boxed()
        }

        fn identity(&self) -> widget::Identity {
            self.identity.clone()
        }
    }

    impl advanced::widget::Meta for Widget {}

    impl iced::advanced::Widget<runtime::Message, iced::Theme, iced::Renderer> for Widget {
        fn diff(&mut self, tree: &mut advanced::widget::Tree) {
            tree.diff_children(std::slice::from_mut(&mut self.content));
        }

        fn size(&self) -> iced::Size<iced::Length> {
            self.content.size()
        }

        fn layout(
            &mut self,
            tree: &mut advanced::widget::Tree,
            renderer: &iced::Renderer,
            limits: &advanced::layout::Limits,
        ) {
            CUSTOM_LAYOUTS.fetch_add(1, Ordering::Relaxed);
            advanced::layout::padded(
                tree,
                limits,
                iced::Length::Shrink,
                iced::Length::Shrink,
                6,
                |tree, limits| {
                    self.content.layout(tree, renderer, limits);
                    tree.size
                },
            );
        }

        fn operate(
            &mut self,
            tree: &mut advanced::widget::Tree,
            layout: Layout,
            viewport: &iced::Rectangle,
            renderer: &iced::Renderer,
            operation: &mut dyn advanced::widget::Operation,
        ) {
            operation.container(None, layout.bounds(), viewport);
            operation.traverse(&mut |operation| {
                let (layout, tree) = layout
                    .iter_mut(&mut tree.children)
                    .next()
                    .expect("badge content layout");
                self.content
                    .operate(tree, layout, viewport, renderer, operation);
            });
        }

        fn update(
            &mut self,
            tree: &mut advanced::widget::Tree,
            event: &iced::Event,
            layout: Layout,
            cursor: advanced::mouse::Cursor,
            renderer: &iced::Renderer,
            shell: &mut Shell<'_, runtime::Message>,
            viewport: &iced::Rectangle,
        ) {
            let (layout, tree) = layout
                .iter_mut(&mut tree.children)
                .next()
                .expect("badge content layout");
            self.content
                .update(tree, event, layout, cursor, renderer, shell, viewport);
        }

        fn mouse_interaction(
            &self,
            tree: &advanced::widget::Tree,
            layout: Layout,
            cursor: advanced::mouse::Cursor,
            viewport: &iced::Rectangle,
            renderer: &iced::Renderer,
        ) -> advanced::mouse::Interaction {
            let (layout, tree) = layout
                .iter(&tree.children)
                .next()
                .expect("badge content layout");
            self.content
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }

        fn draw(
            &self,
            tree: &advanced::widget::Tree,
            renderer: &mut iced::Renderer,
            theme: &iced::Theme,
            style: &advanced::renderer::Style,
            layout: Layout,
            cursor: advanced::mouse::Cursor,
            viewport: &iced::Rectangle,
        ) {
            let (layout, tree) = layout
                .iter(&tree.children)
                .next()
                .expect("badge content layout");
            self.content
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }

        fn overlay<'a>(
            &'a mut self,
            tree: &'a mut advanced::widget::Tree,
            layout: Layout,
            renderer: &iced::Renderer,
            viewport: &iced::Rectangle,
            translation: iced::Vector,
            window: iced::Size,
        ) -> Vec<advanced::overlay::Element<'a, runtime::Message, iced::Theme, iced::Renderer>> {
            let (layout, tree) = layout
                .iter_mut(&mut tree.children)
                .next()
                .expect("badge content layout");
            self.content
                .overlay(tree, layout, renderer, viewport, translation, window)
        }
    }
}
