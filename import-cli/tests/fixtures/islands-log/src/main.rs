//! Native `main`, as inboard's: it sets `log`'s level and a `tracing`
//! subscriber that prints each event, then calls the marked `inbox`
//! module, which compares `tracing`'s and `log`'s levels and logs.
mod workspace;

use workspace::inbox;

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

fn main() {
    log::set_max_level(log::LevelFilter::Info);
    tracing::subscriber::set_global_default(Printer).unwrap();
    println!("{}", inbox::levels());
    println!("{}", inbox::reexported());
    println!("{}", inbox::here());
    println!("{}", inbox::name());
    println!("{}", inbox::traced());
    println!("{}", inbox::modules());
    println!("{}", inbox::crossing());
    println!("{}", inbox::site());
    println!("{}", inbox::events());
}
