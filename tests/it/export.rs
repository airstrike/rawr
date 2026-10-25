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

fn call<T: Clone + Send + Sync + 'static>(source: &str, name: &str, args: impl rawr::FuncArgs) -> T {
    let engine = engine();
    let ast = engine.compile(source).unwrap_or_else(|error| panic!("{error}"));
    engine.call_fn::<T>(&mut Scope::new(), &ast, name, args).unwrap_or_else(|error| panic!("{error}"))
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
    for source in ["use shop::Order; fn f(order: Order) -> i64 { order.hidden() }", "use shop::Order; fn f(order: Order) -> f64 { order.discount }"] {
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
    let error = engine.compile("use shop::Order;\nfn f(order: Order) -> i64 {\n    order.itmes\n}\n").unwrap_err();
    assert!(matches!(error.err_type(), rawr::ParseErrorType::Resolution(rawr::resolve::Error::UnknownMember { .. })));
    let display = error.to_string();
    assert!(display.starts_with("error[E0609]: no field `itmes` on type `Order`\n --> <script>:3:11-3:16"), "{display}");
    assert_eq!(format!("{error:?}"), display);

    let error = engine.compile("use shop::Order;\nfn f(order: Order) -> f64 {\n    order.totl(1.0)\n}\n").unwrap_err();
    assert!(error.to_string().starts_with("error[E0599]: no method named `totl` found for `Order` in the current scope"), "{error}");

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

static GUARDED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[rawr::export]
fn guarded() -> i64 {
    GUARDED.fetch_add(1, std::sync::atomic::Ordering::SeqCst) as i64
}

static COUNTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[rawr::export]
fn counted() -> i64 {
    COUNTED.fetch_add(1, std::sync::atomic::Ordering::SeqCst) as i64
}

fn guarding() -> Engine {
    let mut exports = Exports::new("count");
    exports.add::<guarded>();
    let mut engine = Engine::new();
    engine.export(exports).unwrap();
    engine
}

fn counting(root: &str) -> Engine {
    let mut exports = Exports::new(root);
    exports.add::<counted>();
    let mut engine = Engine::new();
    engine.export(exports).unwrap();
    engine
}

#[test]
fn export_freezes_at_the_first_compile_that_calls_it() {
    let mut engine = engine();
    // A compile that calls no export leaves the exports open.
    engine.compile("fn f() -> i64 { 1 }").unwrap();
    let mut more = Exports::new("more");
    more.add::<counted>();
    engine.export(more).unwrap();

    engine.compile("use shop::tax_rate; fn f() -> f64 { tax_rate(\"NY\") }").unwrap();
    let mut late = Exports::new("late");
    late.add::<counted>();
    let error = engine.export(late).expect_err("the exports are frozen");
    assert!(error.to_string().contains("export everything before compiling"), "{error}");
}

#[test]
fn export_binding_fails_before_a_callback_on_another_engine() {
    let compiler = guarding();
    let ast = compiler.compile("use count::guarded; fn f() -> i64 { guarded() }").unwrap();
    let before = GUARDED.load(std::sync::atomic::Ordering::SeqCst);
    let runner = guarding();
    runner.call_fn::<i64>(&mut Scope::new(), &ast, "f", ()).expect_err("the program was compiled by another engine");
    assert_eq!(GUARDED.load(std::sync::atomic::Ordering::SeqCst), before);
}

#[test]
fn export_binding_fails_before_a_callback_when_a_facade_is_missing() {
    // The engine knows `count` at compile time but implements only `other`.
    let mut engine = Engine::new();
    let mut builder = rawr::host::facade::Builder::new("count", rawr::host::facade::Abi::new(1).unwrap()).unwrap();
    let mut signature = rawr::host::signature::Builder::new();
    signature.output(rawr::host::type_ref::Type::builtin(rawr::host::type_ref::Builtin::I64));
    builder.item(rawr::host::item::Definition::function("guarded", signature.finish()).unwrap()).unwrap();
    engine.register_host_facade(builder.finish().unwrap()).unwrap();
    let mut other = Exports::new("other");
    other.add::<counted>();
    engine.export(other).unwrap();

    let ast = engine.compile("use count::guarded; fn f() -> i64 { guarded() }").unwrap();
    let before = GUARDED.load(std::sync::atomic::Ordering::SeqCst);
    let error = engine.call_fn::<i64>(&mut Scope::new(), &ast, "f", ()).expect_err("`count` has no runtime facade");
    assert!(error.to_string().contains("count"), "{error}");
    assert_eq!(GUARDED.load(std::sync::atomic::Ordering::SeqCst), before);
}

#[test]
fn export_without_a_registry_reports_the_missing_binding() {
    let mut engine = Engine::new();
    let mut builder = rawr::host::facade::Builder::new("count", rawr::host::facade::Abi::new(1).unwrap()).unwrap();
    let mut signature = rawr::host::signature::Builder::new();
    signature.output(rawr::host::type_ref::Type::builtin(rawr::host::type_ref::Builtin::I64));
    builder.item(rawr::host::item::Definition::function("counted", signature.finish()).unwrap()).unwrap();
    engine.register_host_facade(builder.finish().unwrap()).unwrap();
    let ast = engine.compile("use count::counted; fn f() -> i64 { counted() }").unwrap();
    let error = engine.call_fn::<i64>(&mut Scope::new(), &ast, "f", ()).expect_err("nothing implements `count::counted`");
    assert!(error.to_string().contains("missing runtime binding"), "{error}");
}

#[test]
fn export_programs_on_one_engine_share_its_host() {
    let engine = counting("count");
    let first = engine.compile("use count::counted; fn f() -> i64 { counted() }").unwrap();
    let second = engine.compile("use count::counted; fn g() -> i64 { counted() + 100 }").unwrap();
    let a = engine.call_fn::<i64>(&mut Scope::new(), &first, "f", ()).unwrap();
    let b = engine.call_fn::<i64>(&mut Scope::new(), &second, "g", ()).unwrap();
    assert!(b > 100 && a < b, "{a} {b}");
}

#[derive(Clone)]
#[rawr::export]
pub struct Tally {
    pub count: i64,
}

#[rawr::export]
impl Tally {
    pub fn count(&self) -> i64 {
        self.count * 10
    }
}

fn tallying() -> Engine {
    let mut exports = Exports::new("tally");
    exports.add::<Tally>();
    let mut engine = Engine::new();
    engine.export(exports).unwrap();
    engine
}

#[test]
fn export_field_called_like_a_method_is_rustc_e0599() {
    let error = engine().compile("use shop::Order;\nfn f(order: Order) -> i64 {\n    order.items()\n}\n").unwrap_err();
    let text = error.to_string();
    assert!(text.starts_with("error[E0599]: no method named `items` found for `Order` in the current scope\n --> <script>:3:11-3:16"), "{text}");
    assert!(text.ends_with("= help: to use the field `items`, remove the parentheses"), "{text}");
}

#[test]
fn export_method_read_like_a_field_is_rustc_e0615() {
    let error = engine().compile("use shop::Order;\nfn f(order: Order) -> f64 {\n    order.total\n}\n").unwrap_err();
    let text = error.to_string();
    assert!(text.starts_with("error[E0615]: attempted to take value of method `total` on type `Order`\n --> <script>:3:11-3:16"), "{text}");
}

#[test]
fn export_field_and_method_of_one_name_both_work() {
    let engine = tallying();
    let ast = engine
        .compile(
            "use tally::Tally;
             fn f(mut tally: Tally) -> i64 {
                 let before = tally.count + tally.count();
                 tally.count = 5;
                 before * 1000 + tally.count + tally.count()
             }",
        )
        .unwrap_or_else(|error| panic!("{error}"));
    let value: i64 = engine.call_fn(&mut Scope::new(), &ast, "f", (Tally { count: 1 },)).unwrap();
    assert_eq!(value, 11 * 1000 + 5 + 50);
}
