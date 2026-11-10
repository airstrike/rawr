// rawr
// A crate's trait objects in a slice of `Option`s (#1591): numbers, a
// `bool` and a crate value lent as `&dyn Value`, and the set a field set
// makes of them, which borrows them while it lives, read, given to a
// function and to a closure, as tracing's `event!` gives its `ValueSet`,
// and `format_args!` as such an object.

fn main() {
    let fields = fields::FieldSet::new(&["a", "b", "c"]);
    let n = 5i64;
    let point = fields::Point { x: 2, y: 3 };
    let values = [
        Some(&n as &dyn fields::Value),
        None,
        Some(&point as &dyn fields::Value),
    ];
    let set = fields.value_set_all(&values);
    println!("{}", set.render());
    println!("{}", fields::dispatch("event", &set));
    (|set: fields::ValueSet| println!("{}", fields::dispatch("event", &set)))(
        fields.value_set_all(&[Some(&true as &dyn fields::Value), None, None]),
    );
    // `format_args!` lent as an object, alone and among a set's values in
    // the block a crate's macro makes, as tracing's `warn!("..")` lends its
    // message (#1561).
    println!(
        "{}",
        fields::one(&format_args!("one {}", n) as &dyn fields::Value)
    );
    // And given uncoerced, which rustc coerces at the call, and a slice
    // lent to a call at a lifetime of its own that its result doesn't
    // keep (#1592).
    println!("{}", fields::one(&format_args!("uncoerced {}", n)));
    println!(
        "{}",
        fields.count(&[
            Some(&n as &dyn fields::Value),
            None,
            Some(&point as &dyn fields::Value)
        ])
    );
    (|set: fields::ValueSet| println!("{}", fields::dispatch("event", &set)))(fields::values!(
        fields,
        [format_args!("n {}", n), n, format_args!("x {n} y")]
    ));
}
