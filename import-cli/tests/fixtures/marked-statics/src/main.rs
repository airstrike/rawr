// rawr
// Statics declared inside bodies, of native types: two written in one
// block that refer to each other, tracing's event shape, which a crate's
// macro expands at each call, naming `__CALLSITE` and `META` each time, and
// the same pair declared apart. Each is one place for the program's life.

fn written() -> String {
    static SITE: probe::Callsite = probe::Callsite::new(&SITE_META);
    static SITE_META: probe::Metadata = probe::Metadata::new("written", &SITE);
    SITE.interest();
    probe::describe(&SITE)
}

fn step(tick: i64) -> String {
    let scale = 1;
    let event = probe::event!("tick");
    format!("{tick} value {} {event}", tick * scale)
}

fn main() {
    println!("{}", written());
    println!("{}", written());
    println!("{}", probe::event!("first"));
    println!("{}", probe::counted!("counted"));
    probe::run(step);
}
