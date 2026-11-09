// rawr
// Statics declared inside bodies, of native types: two written in one
// block, the callsite built from the metadata before it, and the same pair
// a crate's macro expands at each call, naming `__CALLSITE` and `META` each
// time. Each is one place for the program's life. tracing's own shape,
// whose metadata points back at its callsite, is a cycle rawr refuses:
// `probe::event!` in place of `probe::counted!` is the refusal test's.

fn written() -> String {
    static SITE_META: probe::Metadata = probe::Metadata::named("written");
    static SITE: probe::Callsite = probe::Callsite::new(&SITE_META);
    SITE.interest();
    probe::describe(&SITE)
}

fn step(tick: i64) -> String {
    let scale = 1;
    let event = probe::counted!("tick");
    format!("{tick} value {} {event}", tick * scale)
}

fn main() {
    println!("{}", written());
    println!("{}", written());
    println!("{}", probe::counted!("first"));
    probe::run(step);
}
