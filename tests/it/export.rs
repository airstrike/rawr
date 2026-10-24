//! `#[rawr::export]`: an app's struct, methods, fields and free functions,
//! called from typed scripts through `Engine::compile` and `call_fn`.

use rawr::{Engine, EvalAltResult, Exports, Scope};

#[derive(Clone)]
#[rawr::export]
pub struct Order {
    pub customer: String,
    pub items: i64,
    discount: f64,
}

#[rawr::export]
impl Order {
    pub fn new(customer: &str) -> Self {
        Self { customer: customer.into(), items: 0, discount: 0.0 }
    }

    pub fn total(&self, price: f64) -> f64 {
        price * self.items as f64 * (1.0 - self.discount)
    }

    pub fn add(&mut self, n: i64) {
        self.items += n;
    }

    pub fn discounted(mut self, discount: f64) -> Self {
        self.discount = discount;
        self
    }

    pub fn note(&self) -> Option<String> {
        (self.items > 2).then(|| format!("bulk for {}", self.customer))
    }

    fn hidden(&self) -> i64 {
        self.items
    }
}

#[rawr::export]
fn tax_rate(state: &str) -> f64 {
    if state == "NY" { 0.08875 } else { 0.0 }
}

#[rawr::export]
pub fn label(order: &Order, suffix: String) -> String {
    format!("{} x{}{suffix}", order.customer, order.items)
}

fn engine() -> Engine {
    let mut shop = Exports::new("shop");
    shop.add::<Order>().add::<tax_rate>().add::<label>();
    let mut engine = Engine::new();
    engine.export(shop).unwrap();
    engine
}

fn order() -> Order {
    Order { customer: "Ada".into(), items: 2, discount: 0.0 }
}

fn call<T: Clone + 'static>(source: &str, name: &str, args: impl rawr::FuncArgs) -> T {
    let engine = engine();
    let ast = engine.compile(source).unwrap_or_else(|error| panic!("{error}"));
    engine
        .call_fn::<T>(&mut Scope::new(), &ast, name, args)
        .unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn export_fields_read_from_typed_scripts() {
    let text: String = call(
        "use shop::Order;
         fn label(order: Order) -> String { format!(\"{} x{}\", order.customer, order.items) }",
        "label",
        (order(),),
    );
    assert_eq!(text, "Ada x2");
}

#[test]
fn export_fields_assign_through_a_place() {
    let items: i64 = call(
        "use shop::Order;
         fn bump(mut order: Order) -> i64 { order.items = order.items + 5; order.items }",
        "bump",
        (order(),),
    );
    assert_eq!(items, 7);
}

#[test]
fn export_methods_keep_their_receivers() {
    let total: f64 = call(
        "use shop::Order;
         fn run(mut order: Order) -> f64 {
             order.add(2);
             let order = order.discounted(0.5);
             order.total(10.0)
         }",
        "run",
        (order(),),
    );
    assert_eq!(total, 20.0);
}

#[test]
fn export_associated_functions_and_options() {
    let note: String = call(
        "use shop::Order;
         fn run() -> String {
             let mut order = Order::new(\"Bo\");
             order.add(3);
             order.note().unwrap_or(String::from(\"none\"))
         }",
        "run",
        (),
    );
    assert_eq!(note, "bulk for Bo");
}

#[test]
fn export_free_functions_borrow_their_arguments() {
    let text: String = call(
        "use shop::{label, tax_rate, Order};
         fn run(order: Order) -> String { label(order, format!(\" at {}\", tax_rate(\"NY\"))) }",
        "run",
        (order(),),
    );
    assert_eq!(text, "Ada x2 at 0.08875");
}

#[test]
fn export_hides_private_items() {
    let engine = engine();
    for source in [
        "use shop::Order; fn f(order: Order) -> i64 { order.hidden() }",
        "use shop::Order; fn f(order: Order) -> f64 { order.discount }",
    ] {
        engine.compile(source).expect_err("a private item is not exported");
    }
}

#[test]
fn export_rejects_the_wrong_receiver() {
    let engine = engine();
    engine
        .compile("use shop::Order; fn f(order: Order) { order.add(1); }")
        .expect_err("`add` takes `&mut self`, and `order` is not mutable");
}

#[test]
fn export_unknown_members_read_like_rustc() {
    let engine = engine();
    let error = engine
        .compile("use shop::Order;\nfn f(order: Order) -> i64 {\n    order.itmes\n}\n")
        .unwrap_err();
    assert!(matches!(
        error.err_type(),
        rawr::ParseErrorType::Resolution(rawr::resolve::Error::UnknownMember { .. })
    ));
    let display = error.to_string();
    assert!(
        display.starts_with("error[E0609]: no field `itmes` on type `shop::Order`\n --> <script>:3:11"),
        "{display}"
    );
    assert_eq!(format!("{error:?}"), display);

    let error = engine
        .compile("use shop::Order;\nfn f(order: Order) -> f64 {\n    order.totl(1.0)\n}\n")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("error[E0599]: no method named `totl` found for `shop::Order` in the current scope"),
        "{error}"
    );

    // `?` in `fn main() -> Result<(), Box<EvalAltResult>>` prints the Debug.
    let boxed: Box<EvalAltResult> = error.clone().into();
    assert_eq!(format!("{boxed:?}"), error.to_string());
    assert_eq!(boxed.to_string(), error.to_string());
}

#[test]
fn export_rejects_a_taken_root() {
    let mut engine = engine();
    let mut again = Exports::new("shop");
    again.add::<tax_rate>();
    engine.export(again).expect_err("the root `shop` is taken");
}
