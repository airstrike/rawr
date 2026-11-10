pub use tracing::warn;

/// A subscriber that prints each event's level, target, line and fields.
struct Printer;

impl tracing::Subscriber for Printer {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        struct Fields(String);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write as _;
                let _ = write!(self.0, " {field}={value:?}");
            }
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        let metadata = event.metadata();
        println!(
            "{} {} line {:?}:{}",
            metadata.level(),
            metadata.target(),
            metadata.line(),
            fields.0
        );
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

/// Sets the subscriber, as inboard's `log::init` does.
pub fn init() {
    tracing::subscriber::set_global_default(Printer).unwrap();
}
