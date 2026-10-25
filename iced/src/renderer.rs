//! Evaluator-local authority for scripted drawing operations, and the
//! recorded drawing both kinds of script widget replay: a `#[rawr::widget]`'s
//! (D82) and an `advanced::Widget`'s.

use std::mem;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use rawr::host;
use rawr::{application, program, widget};

/// One exact scripted draw invocation.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Context {
    instance: application::Instance,
    generation: program::GenerationId,
    site: widget::Site,
    protocol: widget::Fingerprint,
    nonce: Nonce,
}

/// The invocation-scoped source value received as `&mut rawr_iced::Renderer`.
#[derive(Clone)]
pub(crate) struct Lease {
    context: Context,
    state: Arc<Mutex<State>>,
}

/// One open renderer transaction and its facade-authenticated call proof.
pub(crate) struct Transaction {
    lease: Lease,
    invocation: rawr::host::runtime::invocation::Invocation<Context>,
}

/// Drawing operations drained from one successful transaction.
pub(crate) struct Replay {
    commands: Vec<Command>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Nonce(usize);

enum State {
    Open(Vec<Command>),
    Committed,
    Aborted,
}

/// One recorded drawing operation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
    FillQuad {
        bounds: iced::Rectangle,
        color: iced::Color,
        radius: f32,
    },
    Quad(iced::advanced::renderer::Quad, iced::Background),
    Mesh(iced::advanced::graphics::Mesh),
    StartLayer(iced::Rectangle),
    EndLayer,
    StartTransformation(iced::Transformation),
    EndTransformation,
    /// A wrapped child's own drawing, in its place among the rest: the
    /// recorder's index of it, which the replay hands back.
    Child(usize),
}

/// A scripted renderer operation did not retain exact invocation authority.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// The static extension facade could not produce its Renderer class proof.
    #[error("the rawr_iced Renderer facade proof is unavailable: {0}")]
    Facade(#[from] host::Error),
    /// The Renderer class does not permit invocation-scoped storage.
    #[error("the rawr_iced Renderer invocation proof is invalid: {0}")]
    Invocation(#[from] host::runtime::invocation::Error),
    /// A receiver lease belongs to another exact draw invocation.
    #[error("the renderer lease belongs to another draw invocation")]
    Context,
    /// A renderer lease was used after its transaction closed.
    #[error("the renderer lease is no longer open")]
    Closed,
    /// A panic poisoned the transaction's command buffer.
    #[error("the renderer transaction command buffer is poisoned")]
    Poisoned,
    /// A drawing value bypassed the facade's canonical validation.
    #[error("the renderer operation contains invalid canonical geometry or color")]
    InvalidValue,
}

impl Context {
    fn bind(
        instance: application::Instance,
        generation: program::GenerationId,
        site: widget::Site,
        protocol: widget::Fingerprint,
    ) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);

        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        Self {
            instance,
            generation,
            site,
            protocol,
            nonce: Nonce(nonce),
        }
    }
}

impl Lease {
    pub(crate) fn fill_quad(
        &self,
        expected: &Context,
        bounds: iced::Rectangle,
        color: iced::Color,
    ) -> Result<(), Error> {
        self.push_quad(expected, bounds, color, 0.0)
    }

    pub(crate) fn fill_rounded_quad(
        &self,
        expected: &Context,
        bounds: iced::Rectangle,
        color: iced::Color,
        radius: f32,
    ) -> Result<(), Error> {
        self.push_quad(expected, bounds, color, radius)
    }

    fn push_quad(
        &self,
        expected: &Context,
        bounds: iced::Rectangle,
        color: iced::Color,
        radius: f32,
    ) -> Result<(), Error> {
        if &self.context != expected {
            return Err(Error::Context);
        }
        if !valid(bounds, color) || !radius.is_finite() || radius < 0.0 {
            return Err(Error::InvalidValue);
        }

        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        match &mut *state {
            State::Open(commands) => {
                commands.push(Command::FillQuad {
                    bounds,
                    color,
                    radius,
                });
                Ok(())
            }
            State::Committed | State::Aborted => Err(Error::Closed),
        }
    }
}

impl Transaction {
    pub(crate) fn bind(
        instance: application::Instance,
        generation: program::GenerationId,
        site: widget::Site,
        protocol: widget::Fingerprint,
    ) -> Result<Self, Error> {
        let context = Context::bind(instance, generation, site, protocol);
        let invocation = host::runtime::invocation::Invocation::bind(class()?, context.clone())?;
        Ok(Self {
            lease: Lease {
                context,
                state: Arc::new(Mutex::new(State::Open(Vec::new()))),
            },
            invocation,
        })
    }

    pub(crate) fn lease(&self) -> Lease {
        self.lease.clone()
    }

    pub(crate) const fn invocation(&self) -> &host::runtime::invocation::Invocation<Context> {
        &self.invocation
    }

    pub(crate) fn commit(self) -> Result<Replay, Error> {
        let mut state = self.lease.state.lock().map_err(|_| Error::Poisoned)?;
        let commands = match mem::replace(&mut *state, State::Committed) {
            State::Open(commands) => commands,
            State::Committed => {
                *state = State::Committed;
                return Err(Error::Closed);
            }
            State::Aborted => {
                *state = State::Aborted;
                return Err(Error::Closed);
            }
        };
        drop(state);
        Ok(Replay { commands })
    }

    pub(crate) fn abort(mut self) {
        self.close();
    }

    fn close(&mut self) {
        if let Ok(mut state) = self.lease.state.lock()
            && matches!(*state, State::Open(_))
        {
            *state = State::Aborted;
        }
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        self.close();
    }
}

impl Replay {
    pub(crate) fn replay<R>(self, renderer: &mut R)
    where
        R: iced::advanced::Renderer + iced::advanced::graphics::mesh::Renderer,
    {
        replay(self.commands, renderer, |_, _| {});
    }

    #[cfg(test)]
    pub(crate) const fn len(&self) -> usize {
        self.commands.len()
    }
}

/// Draw `commands` on `renderer` in order, with `child` drawing each wrapped
/// child in its place. A layer or transformation left open is closed at the
/// end, and an end without its start is dropped, so the renderer's own
/// stacks stay balanced whatever the script did.
pub(crate) fn replay<R>(
    commands: Vec<Command>,
    renderer: &mut R,
    mut child: impl FnMut(&mut R, usize),
) where
    R: iced::advanced::Renderer + iced::advanced::graphics::mesh::Renderer,
{
    #[derive(PartialEq)]
    enum Open {
        Layer,
        Transformation,
    }
    let mut open = Vec::new();
    for command in commands {
        match command {
            Command::FillQuad {
                bounds,
                color,
                radius,
            } => renderer.fill_quad(
                iced::advanced::renderer::Quad {
                    bounds,
                    border: iced::Border {
                        radius: radius.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                color,
            ),
            Command::Quad(quad, background) => renderer.fill_quad(quad, background),
            Command::Mesh(mesh) => renderer.draw_mesh(mesh),
            Command::StartLayer(bounds) => {
                renderer.start_layer(bounds);
                open.push(Open::Layer);
            }
            Command::StartTransformation(transformation) => {
                renderer.start_transformation(transformation);
                open.push(Open::Transformation);
            }
            Command::EndLayer => {
                if open.last() == Some(&Open::Layer) {
                    open.pop();
                    renderer.end_layer();
                }
            }
            Command::EndTransformation => {
                if open.last() == Some(&Open::Transformation) {
                    open.pop();
                    renderer.end_transformation();
                }
            }
            Command::Child(index) => child(renderer, index),
        }
    }
    for open in open.into_iter().rev() {
        match open {
            Open::Layer => renderer.end_layer(),
            Open::Transformation => renderer.end_transformation(),
        }
    }
}

fn class() -> Result<host::type_def::Export, host::Error> {
    static CLASS: OnceLock<Result<host::type_def::Export, host::Error>> = OnceLock::new();

    CLASS
        .get_or_init(|| {
            let iced = crate::facade()?;
            crate::extension(&iced)?.export_type("Renderer")
        })
        .clone()
}

fn valid(bounds: iced::Rectangle, color: iced::Color) -> bool {
    [bounds.x, bounds.y, bounds.width, bounds.height]
        .into_iter()
        .all(f32::is_finite)
        && bounds.width >= 0.0
        && bounds.height >= 0.0
        && [color.r, color.g, color.b, color.a]
            .into_iter()
            .all(|channel| channel.is_finite() && (0.0..=1.0).contains(&channel))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use rawr::{application, host, program, widget, Dynamic, Engine, Shared};

    use super::{Context, Error, Transaction};

    #[test]
    fn successful_unit_transaction_replays_once_in_source_order() {
        let proofs = proofs();
        let transaction = transaction(&proofs, 0);
        let lease = transaction.lease();
        let expected = lease.context.clone();
        let first = iced::Rectangle::new(iced::Point::new(1.0, 2.0), iced::Size::new(3.0, 4.0));
        let second = iced::Rectangle::new(iced::Point::new(5.0, 6.0), iced::Size::new(7.0, 8.0));

        lease
            .fill_quad(&expected, first, iced::Color::BLACK)
            .unwrap();
        lease
            .fill_rounded_quad(&expected, second, iced::Color::WHITE, 12.0)
            .unwrap();
        let replay = transaction.commit().unwrap();

        assert!(matches!(
            lease.fill_quad(&expected, first, iced::Color::BLACK),
            Err(Error::Closed)
        ));
        let mut renderer = Recording::default();
        replay.replay(&mut renderer);
        assert_eq!(
            renderer.fills,
            vec![
                (
                    first,
                    iced::Background::Color(iced::Color::BLACK),
                    iced::border::Radius::default(),
                ),
                (
                    second,
                    iced::Background::Color(iced::Color::WHITE),
                    12.0.into(),
                ),
            ]
        );
    }

    #[test]
    fn abort_and_drop_discard_commands_and_stale_every_lease() {
        let proofs = proofs();

        let aborted = transaction(&proofs, 0);
        let lease = aborted.lease();
        let expected = lease.context.clone();
        lease
            .fill_quad(&expected, iced::Rectangle::default(), iced::Color::BLACK)
            .unwrap();
        aborted.abort();
        assert!(matches!(
            lease.fill_quad(&expected, iced::Rectangle::default(), iced::Color::BLACK),
            Err(Error::Closed)
        ));

        let dropped = transaction(&proofs, 0);
        let lease = dropped.lease();
        let expected = lease.context.clone();
        lease
            .fill_quad(&expected, iced::Rectangle::default(), iced::Color::BLACK)
            .unwrap();
        drop(dropped);
        assert!(matches!(
            lease.fill_quad(&expected, iced::Rectangle::default(), iced::Color::BLACK),
            Err(Error::Closed)
        ));
    }

    #[test]
    fn lease_rejects_every_context_tuple_mismatch() {
        let proofs = proofs();
        let active = transaction(&proofs, 0);
        let lease = active.lease();
        let exact = lease.context.clone();
        let other = transaction(&proofs, 0).lease.context.clone();
        let bounds = iced::Rectangle::default();

        for mismatch in [
            Context {
                instance: application::Instance::new(),
                ..exact.clone()
            },
            Context {
                generation: proofs.other_generation,
                ..exact.clone()
            },
            Context {
                site: proofs.sites[1].clone(),
                ..exact.clone()
            },
            Context {
                protocol: proofs.other_protocol.clone(),
                ..exact.clone()
            },
            Context {
                nonce: other.nonce,
                ..exact.clone()
            },
        ] {
            assert!(matches!(
                lease.fill_quad(&mismatch, bounds, iced::Color::BLACK),
                Err(Error::Context)
            ));
        }

        lease.fill_quad(&exact, bounds, iced::Color::BLACK).unwrap();
        assert_eq!(active.commit().unwrap().commands.len(), 1);
    }

    #[test]
    fn concurrent_direct_cross_use_never_moves_authority() {
        let proofs = proofs();
        let first = transaction(&proofs, 0);
        let second = transaction(&proofs, 1);
        let first_lease = first.lease();
        let first_context = first_lease.context.clone();
        let second_lease = second.lease();
        let second_context = second_lease.context.clone();

        assert!(matches!(
            first_lease.fill_quad(
                &second_context,
                iced::Rectangle::default(),
                iced::Color::BLACK,
            ),
            Err(Error::Context)
        ));
        second_lease
            .fill_quad(
                &second_context,
                iced::Rectangle::default(),
                iced::Color::WHITE,
            )
            .unwrap();
        assert!(matches!(
            second_lease.fill_quad(
                &first_context,
                iced::Rectangle::default(),
                iced::Color::BLACK,
            ),
            Err(Error::Context)
        ));

        let lease = Arc::new(first_lease);
        let context = Arc::new(first_context);
        let workers = (0..4)
            .map(|index| {
                let lease = lease.clone();
                let context = context.clone();
                thread::spawn(move || {
                    lease.fill_quad(
                        &context,
                        iced::Rectangle::new(
                            iced::Point::new(index as f32, 0.0),
                            iced::Size::new(1.0, 1.0),
                        ),
                        iced::Color::BLACK,
                    )
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap().unwrap();
        }

        assert_eq!(first.commit().unwrap().commands.len(), 4);
        assert_eq!(second.commit().unwrap().commands.len(), 1);
    }

    #[test]
    fn registry_calls_keep_interleaved_and_concurrent_contexts_exact() {
        let (binding, instance) = binding();
        let promotion = binding.promotion();
        let first = Transaction::bind(
            instance,
            binding.generation(),
            promotion.site().clone(),
            promotion.definition().fingerprint().clone(),
        )
        .unwrap();
        let second = Transaction::bind(
            instance,
            binding.generation(),
            promotion.site().clone(),
            promotion.definition().fingerprint().clone(),
        )
        .unwrap();

        let error = binding
            .draw(
                None,
                Dynamic::from(second.lease()),
                Dynamic::from(input()),
                first.invocation(),
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("renderer lease belongs to another draw invocation"));
        assert_eq!(
            binding
                .draw(
                    None,
                    Dynamic::from(second.lease()),
                    Dynamic::from(input()),
                    second.invocation(),
                )
                .unwrap(),
            Some(())
        );

        let lease = first.lease();
        let invocation = first.invocation().clone();
        let workers = (0..4)
            .map(|_| {
                let binding = binding.clone();
                let lease = lease.clone();
                let invocation = invocation.clone();
                thread::spawn(move || {
                    binding.draw(
                        None,
                        Dynamic::from(lease),
                        Dynamic::from(input()),
                        &invocation,
                    )
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            assert_eq!(worker.join().unwrap().unwrap(), Some(()));
        }

        assert_eq!(first.commit().unwrap().commands.len(), 4);
        assert_eq!(second.commit().unwrap().commands.len(), 1);
    }

    #[test]
    fn invalid_callback_value_falls_through_drop_to_abort() {
        let proofs = proofs();
        let transaction = transaction(&proofs, 0);
        let lease = transaction.lease();
        let expected = lease.context.clone();

        assert!(matches!(
            lease.fill_quad(
                &expected,
                iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(f32::NAN, 1.0),),
                iced::Color::BLACK,
            ),
            Err(Error::InvalidValue)
        ));
        assert!(matches!(
            lease.fill_rounded_quad(
                &expected,
                iced::Rectangle::default(),
                iced::Color::BLACK,
                -1.0,
            ),
            Err(Error::InvalidValue)
        ));
        drop(transaction);
        assert!(matches!(
            lease.fill_quad(&expected, iced::Rectangle::default(), iced::Color::BLACK),
            Err(Error::Closed)
        ));
    }

    fn transaction(proofs: &Proofs, site: usize) -> Transaction {
        let transaction = Transaction::bind(
            application::Instance::new(),
            proofs.generation,
            proofs.sites[site].clone(),
            proofs.protocol.clone(),
        )
        .unwrap();
        let _ = transaction.invocation();
        transaction
    }

    fn binding() -> (widget::Binding, application::Instance) {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(application())
            .unwrap();
        let launch = engine.launch_application(descriptor).unwrap();
        let mut registry = host::runtime::Registry::new();
        crate::install_test_runtime(&mut registry);
        let launch = launch.bind(registry).unwrap();
        let data = launch.boot_default().unwrap();
        let descriptor = launch
            .promote_widget(&launch.widget_promotions()[0], data)
            .unwrap();
        let binding = launch.bind_widget(&descriptor).unwrap();
        (binding, application::Instance::new())
    }

    fn input() -> crate::widget::draw::Input {
        crate::widget::draw::Input::new(
            iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(10.0, 12.0)),
            None,
            iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(10.0, 12.0)),
            iced::Color::BLACK,
        )
    }

    struct Proofs {
        generation: program::GenerationId,
        other_generation: program::GenerationId,
        sites: Vec<widget::Site>,
        protocol: widget::Fingerprint,
        other_protocol: widget::Fingerprint,
    }

    fn proofs() -> Proofs {
        let mut engine = Engine::new();
        crate::install(&mut engine).unwrap();
        let engine = Shared::new(engine);
        let descriptor = engine
            .compile_application_descriptor(source("i64", "1", "App"))
            .unwrap();
        let executable = engine.launch_application(descriptor).unwrap();
        let descriptor = engine
            .compile_application_descriptor(source("i64", "1", "Other"))
            .unwrap();
        let other = engine.launch_application(descriptor).unwrap();
        let descriptor = engine
            .compile_application_descriptor(source("bool", "false", "App"))
            .unwrap();
        let drifted = engine.launch_application(descriptor).unwrap();
        let sites = vec![
            executable.authority().widget_promotions()[0].site().clone(),
            other.authority().widget_promotions()[0].site().clone(),
        ];
        assert_eq!(sites.len(), 2);
        Proofs {
            generation: executable.authority().generation(),
            other_generation: other.authority().generation(),
            sites,
            protocol: executable.authority().widgets()[0].fingerprint().clone(),
            other_protocol: drifted.authority().widgets()[0].fingerprint().clone(),
        }
    }

    fn source(field: &str, value: &str, app: &str) -> String {
        format!(
            r#"
                use iced::Element;

                struct Dial {{ radius: {field} }}
                #[derive(Default)]
                struct {app};
                #[derive(Debug, Clone, Copy)]
                enum Message {{ Tick }}

                #[widget]
                impl Dial {{
                    fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) {{
                        ()
                    }}
                }}

                impl {app} {{
                    fn update(&mut self, message: Message) {{
                        match message {{ Message::Tick => () }}
                    }}

                    fn view(&self) -> Element<Message> {{
                        Dial {{ radius: {value} }}
                    }}
                }}

                pub fn main() -> iced::Result {{ iced::run({app}::update, {app}::view) }}
            "#
        )
    }

    fn application() -> &'static str {
        r#"
            use iced::{Color, Result};
            use iced::Element;

            #[derive(Default)]
            struct Dial { radius: i64 }
            #[derive(Debug, Clone, Copy)]
            enum Message { Tick }

            #[widget]
            impl Dial {
                fn draw(&self, renderer: &mut rawr_iced::Renderer, input: rawr_iced::widget::draw::Input) {
                    renderer.fill_quad(input.bounds(), Color::BLACK);
                }
            }

            impl Dial {
                fn update(&mut self, message: Message) {
                    match message { Message::Tick => () }
                }

                fn view(&self) -> Element<Message> { Dial { radius: self.radius } }
            }

            pub fn main() -> Result { iced::run(Dial::update, Dial::view) }
        "#
    }

    #[derive(Default)]
    struct Recording {
        fills: Vec<(iced::Rectangle, iced::Background, iced::border::Radius)>,
    }

    impl iced::advanced::graphics::mesh::Renderer for Recording {
        fn draw_mesh(&mut self, _mesh: iced::advanced::graphics::Mesh) {}

        fn draw_mesh_cache(&mut self, _cache: iced::advanced::graphics::mesh::Cache) {}
    }

    impl iced::advanced::Renderer for Recording {
        fn start_layer(&mut self, _bounds: iced::Rectangle) {}

        fn end_layer(&mut self) {}

        fn start_transformation(&mut self, _transformation: iced::Transformation) {}

        fn end_transformation(&mut self) {}

        fn fill_quad(
            &mut self,
            quad: iced::advanced::renderer::Quad,
            background: impl Into<iced::Background>,
        ) {
            self.fills
                .push((quad.bounds, background.into(), quad.border.radius));
        }

        fn allocate_image(
            &self,
            _handle: &iced::advanced::image::Handle,
            _callback: impl FnOnce(Result<iced::advanced::image::Allocation, iced::advanced::image::Error>)
                + Send
                + 'static,
        ) {
        }

        fn hint(&mut self, _scale: iced::advanced::renderer::Scale) {}

        fn scale(&self) -> Option<iced::advanced::renderer::Scale> {
            None
        }

        fn reset(&mut self, _new_bounds: iced::Rectangle) {}

        fn settings(&self) -> iced::advanced::renderer::Settings {
            iced::advanced::renderer::Settings::default()
        }
    }
}
