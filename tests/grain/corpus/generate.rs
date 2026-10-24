// Random typed Rust scripts, for comparing the VM against the walker on
// programs nobody wrote.
//
// The hand-written corpus proves the constructs someone thought to test. This
// covers the combinations they did not: a `match` inside a `for` inside a
// function whose argument is an indexed vector, and the several thousand other
// shapes of that kind. Every one is run both ways and the results compared, so
// a divergence names a script that reproduces it.
//
// Every script is well typed. Each expression is built for the type its place
// needs, and each variable carries its type, so the type checker accepts what
// is written and run time is where the two sides can disagree. Errors still
// happen there, and on purpose: dividing by zero, overflowing, indexing past
// the end and calling `unwrap` on nothing are all generated, and both sides
// have to report them the same way.
//
// Two constraints follow from `fuzz/fuzz_targets/generated.rs` pulling this
// file in with `include!` rather than depending on it: nothing may be imported
// beyond `std`, and the header is `//` rather than `//!`, because an included
// file cannot open with an inner attribute.
//
// What is deliberately not generated:
//
//   - Recursion. A function may only call one defined before it, so no script
//     can recurse. Running out of call levels is a limit both sides enforce
//     but not in lockstep, and it would drown real findings.
//   - An unbounded loop. Every `while` counts a guard variable the body cannot
//     name, and counts it before the body runs, so a `continue` cannot skip it.
//   - A moved value read again. A `String` or `Vec` variable is read by value
//     only through `.clone()`, so no script uses a value after a move.

/// A source of choices: xorshift64*, optionally front-loaded with bytes
/// somebody else picked.
///
/// The bytes are for `cargo fuzz`. Every grammar decision takes one, so
/// flipping byte *k* of an input changes decision *k* and leaves the rest of
/// the script alone — which is the locality a coverage-guided fuzzer needs to
/// make progress. From a plain seed, or once the bytes run out, it is an
/// ordinary reproducible PRNG.
pub struct Rng {
    state: u64,
    choices: Vec<u8>,
    at: usize,
}

impl Rng {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self {
            // A zero state is a fixed point for xorshift, and zero is exactly
            // the seed somebody reaches for first.
            state: if seed == 0 { 0x9e37_79b9_7f4a_7c15 } else { seed },
            choices: Vec::new(),
            at: 0,
        }
    }

    /// Draws from `choices` until they are used up, then from the PRNG.
    ///
    /// Running out is not a failure and not a truncation: a script that stops
    /// being fuzzer-directed part way through is still a valid script, and
    /// still worth comparing.
    #[must_use]
    pub fn from_bytes(choices: &[u8]) -> Self {
        let mut seed = 0u64;
        for byte in choices.iter().take(8) {
            seed = (seed << 8) | u64::from(*byte);
        }
        Self { choices: choices.to_vec(), at: 0, ..Self::new(seed) }
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    pub fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        if let Some(byte) = self.choices.get(self.at) {
            self.at += 1;
            return usize::from(*byte) % bound;
        }
        (self.next() % bound as u64) as usize
    }

    fn one_of<'a, T>(&mut self, choices: &'a [T]) -> &'a T {
        &choices[self.below(choices.len())]
    }

    fn chance(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
}

/// How deep an expression may nest.
///
/// Rawr's parser has its own limit and rejects anything past it. Staying well
/// under means a rejected parse is a bug in this generator rather than the
/// expected outcome for half the corpus.
const MAX_DEPTH: usize = 3;

/// How deep blocks and control flow may nest, for the same reason.
const MAX_NESTING: usize = 2;

/// The types a generated value can have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    Int,
    Bool,
    Text,
    List,
}

const TYPES: &[Ty] = &[Ty::Int, Ty::Int, Ty::Bool, Ty::Text, Ty::List];

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Self::Int => "i64",
            Self::Bool => "bool",
            Self::Text => "String",
            Self::List => "Vec<i64>",
        }
    }
}

/// A variable that can be named, with what it holds and whether it may be
/// assigned to.
#[derive(Clone)]
struct Var {
    name: String,
    ty: Ty,
    mutable: bool,
}

/// Integer operators. Each one can fail at run time on some operands —
/// overflow, or a zero on the right of `/` and `%` — which is a disagreement
/// worth generating rather than worth avoiding.
const ARITHMETIC: &[&str] = &["+", "-", "*", "/", "%", "&", "|", "^"];

const COMPARISONS: &[&str] = &["==", "!=", "<", "<=", ">", ">="];

pub struct Generator {
    rng: Rng,
    /// Variables that can be named, innermost last.
    vars: Vec<Var>,
    /// Functions defined so far, with their parameter types and their result.
    /// Only these can be called, and only from after their definition, which
    /// is what rules out recursion.
    functions: Vec<(String, Vec<Ty>, Ty)>,
    /// Loop nesting, so `break` and `continue` appear only inside a loop.
    loops: usize,
    depth: usize,
    /// Nesting of blocks and control flow, capped for the same reason `depth`
    /// is.
    nesting: usize,
    /// Supplies names that cannot collide with each other.
    counter: usize,
}

impl Generator {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { rng: Rng::new(seed), vars: Vec::new(), functions: Vec::new(), loops: 0, depth: 0, nesting: 0, counter: 0 }
    }

    /// A generator whose choices are made by somebody else's bytes, for
    /// `cargo fuzz`. See [`Rng::from_bytes`].
    #[must_use]
    pub fn from_bytes(choices: &[u8]) -> Self {
        Self { rng: Rng::from_bytes(choices), ..Self::new(0) }
    }

    fn name(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn any_type(&mut self) -> Ty {
        *self.rng.one_of(TYPES)
    }

    /// One whole script.
    pub fn script(&mut self) -> String {
        let mut out = String::new();

        for _ in 0..self.rng.below(3) {
            out.push_str(&self.function());
            out.push(' ');
        }

        // A scope of its own per script, so nothing leaks between them.
        self.vars.clear();
        for _ in 0..1 + self.rng.below(5) {
            out.push_str(&self.statement());
            out.push(' ');
        }

        // Ending on an expression gives the script a value, so a divergence in
        // what it computed shows up and not only a divergence in what it left
        // behind.
        let ty = self.any_type();
        out.push_str(&self.expression(ty));
        out
    }

    fn function(&mut self) -> String {
        let name = self.name("f");
        let params: Vec<Ty> = (0..self.rng.below(3)).map(|_| self.any_type()).collect();
        let result = self.any_type();

        // A body sees its parameters and nothing else, which is also what rawr
        // gives it.
        let declared: Vec<Var> = params.iter().enumerate().map(|(i, ty)| Var { name: format!("p{i}"), ty: *ty, mutable: false }).collect();
        let outer = std::mem::replace(&mut self.vars, declared);
        let outer_loops = std::mem::replace(&mut self.loops, 0);
        // A function body starts one level in, so its statements stay inside
        // the expression depth rawr allows a body.
        let outer_nesting = std::mem::replace(&mut self.nesting, 1);
        let mut body = String::new();
        for _ in 0..1 + self.rng.below(3) {
            body.push_str(&self.statement());
            body.push(' ');
        }
        body.push_str(&self.expression(result));
        self.vars = outer;
        self.loops = outer_loops;
        self.nesting = outer_nesting;

        let list: Vec<String> = params.iter().enumerate().map(|(i, ty)| format!("p{i}: {}", ty.name())).collect();
        // Registered after the body is generated, so the body cannot call it.
        self.functions.push((name.clone(), params, result));
        format!("fn {name}({}) -> {} {{ {body} }}", list.join(", "), result.name())
    }

    fn statement(&mut self) -> String {
        // Weighted by hand: `let` more often than anything else, because a
        // script with no variables cannot exercise slots, writes or capture.
        // Anything that nests is dropped from the choices once deep enough,
        // which is why the arms are ordered with those last.
        let nesting_kinds = if self.nesting < MAX_NESTING { 5 } else { 0 };
        let loop_kinds = if self.loops > 0 { 2 } else { 0 };
        match self.rng.below(6 + nesting_kinds + loop_kinds) {
            0..=2 => {
                let ty = self.any_type();
                let value = self.expression(ty);
                let name = self.name("v");
                let mutable = !self.rng.chance(4);
                self.vars.push(Var { name: name.clone(), ty, mutable });
                format!("let {}{name}: {} = {value};", if mutable { "mut " } else { "" }, ty.name())
            }
            3 | 4 => self.assignment(),
            5 => {
                let ty = self.any_type();
                format!("{};", self.expression(ty))
            }

            // Nesting, reachable only while there is budget for it.
            6 => {
                let cond = self.expression(Ty::Bool);
                self.nesting += 1;
                let then = self.block();
                let otherwise = self.rng.chance(2).then(|| self.block());
                self.nesting -= 1;
                match otherwise {
                    Some(otherwise) => format!("if {cond} {then} else {otherwise}"),
                    None => format!("if {cond} {then}"),
                }
            }
            7 => self.nested(Self::bounded_while),
            8 => self.nested(Self::for_loop),
            9 => self.nested(Self::match_statement),
            10 => self.nested(Self::block),

            // Only reachable inside a loop, per `loop_kinds` above.
            11 => "break;".to_string(),
            _ => "continue;".to_string(),
        }
    }

    fn nested(&mut self, build: fn(&mut Self) -> String) -> String {
        self.nesting += 1;
        let out = build(self);
        self.nesting -= 1;
        out
    }

    /// A run of statements, with the scope they declared popped afterwards
    /// exactly as the VM pops it.
    ///
    /// Returned without braces so a caller can add to it.
    fn statements(&mut self) -> String {
        let mark = self.vars.len();
        let mut out = String::new();
        for _ in 0..1 + self.rng.below(2) {
            out.push_str(&self.statement());
            out.push(' ');
        }
        self.vars.truncate(mark);
        out
    }

    fn block(&mut self) -> String {
        format!("{{ {} }}", self.statements())
    }

    fn assignment(&mut self) -> String {
        let targets: Vec<Var> = self.vars.iter().filter(|var| var.mutable).cloned().collect();
        if targets.is_empty() {
            let ty = self.any_type();
            return format!("{};", self.expression(ty));
        }
        let target = targets[self.rng.below(targets.len())].clone();
        let name = &target.name;
        match target.ty {
            Ty::Int => {
                let op = self.rng.one_of(&["=", "+=", "-=", "*="]).to_string();
                format!("{name} {op} {};", self.expression(Ty::Int))
            }
            Ty::Bool => format!("{name} = {};", self.expression(Ty::Bool)),
            Ty::Text => match self.rng.below(3) {
                0 => format!("{name} = {};", self.expression(Ty::Text)),
                1 => format!("{name}.push_str(&{});", self.expression(Ty::Text)),
                _ => format!("{name}.push('{}');", self.rng.one_of(&['a', 'z', '!'])),
            },
            Ty::List => match self.rng.below(8) {
                0 => format!("{name} = {};", self.expression(Ty::List)),
                1 | 2 => format!("{name}.push({});", self.expression(Ty::Int)),
                // Past the end some of the time, which both sides must refuse
                // the same way.
                3 => format!("{name}[{}] = {};", self.rng.below(4), self.expression(Ty::Int)),
                4 => format!("{name}.pop();"),
                5 => format!("{name}.truncate({});", self.rng.below(4)),
                6 => format!("{name}.sort();"),
                _ => format!("{name}.reverse();"),
            },
        }
    }

    /// `while`, with a guard the body cannot reach.
    ///
    /// The guard is not pushed onto `vars`, so nothing generated inside can
    /// assign to it, and it is counted before the body, so a `continue`
    /// cannot skip it. The loop always ends.
    fn bounded_while(&mut self) -> String {
        let guard = self.name("g");
        let limit = 1 + self.rng.below(4);
        self.loops += 1;
        let body = self.statements();
        self.loops -= 1;
        format!("let mut {guard}: i64 = 0; while {guard} < {limit} {{ {guard} += 1; {body} }}")
    }

    fn for_loop(&mut self) -> String {
        let item = self.name("it");
        let iterable = if self.rng.chance(2) { format!("0..{}", 1 + self.rng.below(4)) } else { self.expression(Ty::List) };

        self.vars.push(Var { name: item.clone(), ty: Ty::Int, mutable: false });
        self.loops += 1;
        let body = self.block();
        self.loops -= 1;
        self.vars.pop();

        format!("for {item} in {iterable} {body}")
    }

    fn match_statement(&mut self) -> String {
        let subject = self.expression(Ty::Int);
        let mut arms = Vec::new();
        for value in 0..1 + self.rng.below(3) {
            arms.push(format!("{value} => {}", self.block()));
        }
        if self.rng.chance(3) {
            arms.push(format!("5..=9 => {}", self.block()));
        }
        arms.push(format!("_ => {}", self.block()));
        format!("match {subject} {{ {} }}", arms.join(" "))
    }

    fn variable(&mut self, ty: Ty) -> Option<String> {
        let names: Vec<String> = self.vars.iter().filter(|var| var.ty == ty).map(|var| var.name.clone()).collect();
        if names.is_empty() {
            return None;
        }
        Some(names[self.rng.below(names.len())].clone())
    }

    /// A leaf of the type asked for: cheap, and always available however deep
    /// the nesting has got.
    fn atom(&mut self, ty: Ty) -> String {
        if self.rng.chance(2)
            && let Some(var) = self.variable(ty)
        {
            // A `String` or a `Vec` read by value is cloned, so the variable
            // is never moved out of.
            return match ty {
                Ty::Int | Ty::Bool => var,
                Ty::Text | Ty::List => format!("{var}.clone()"),
            };
        }
        match ty {
            Ty::Int => format!("{}", self.rng.below(64)),
            Ty::Bool => self.rng.one_of(&["true", "false"]).to_string(),
            Ty::Text => format!("\"s{}\".to_string()", self.rng.below(8)),
            Ty::List => {
                let items: Vec<String> = (0..self.rng.below(4)).map(|_| format!("{}", self.rng.below(16))).collect();
                if items.is_empty() { "Vec::<i64>::new()".to_string() } else { format!("vec![{}]", items.join(", ")) }
            }
        }
    }

    /// An expression of the type asked for.
    fn expression(&mut self, ty: Ty) -> String {
        if self.depth >= MAX_DEPTH {
            return self.atom(ty);
        }
        self.depth += 1;
        let out = match ty {
            Ty::Int => self.int(),
            Ty::Bool => self.boolean(),
            Ty::Text => self.text(),
            Ty::List => self.list(),
        };
        self.depth -= 1;
        out
    }

    fn int(&mut self) -> String {
        match self.rng.below(16) {
            0..=3 => self.atom(Ty::Int),
            4..=6 => {
                let lhs = self.expression(Ty::Int);
                let op = self.rng.one_of(ARITHMETIC);
                format!("({lhs} {op} {})", self.expression(Ty::Int))
            }
            7 => format!("(-{})", self.expression(Ty::Int)),
            8 => {
                let cond = self.expression(Ty::Bool);
                let then = self.expression(Ty::Int);
                format!("(if {cond} {{ {then} }} else {{ {} }})", self.expression(Ty::Int))
            }
            9 => {
                let subject = self.expression(Ty::Int);
                let zero = self.expression(Ty::Int);
                let digit = self.expression(Ty::Int);
                format!("(match {subject} {{ 0 => {zero}, 1..=9 => {digit}, _ => {} }})", self.expression(Ty::Int))
            }
            10 => format!("({}.len() as i64)", self.expression(Ty::List)),
            11 => format!("({}.len() as i64)", self.expression(Ty::Text)),
            // Past the end some of the time, which both sides must refuse the
            // same way.
            12 => format!("{}[{}]", self.expression(Ty::List), self.rng.below(4)),
            13 => {
                let method = self.rng.one_of(&["abs", "max", "min", "signum"]).to_string();
                let receiver = self.expression(Ty::Int);
                // Cast, so a literal receiver is an `i64` rather than an
                // integer whose type is still to be decided.
                match method.as_str() {
                    "max" | "min" => format!("({receiver} as i64).{method}({})", self.expression(Ty::Int)),
                    _ => format!("({receiver} as i64).{method}()"),
                }
            }
            14 => self.call(Ty::Int),
            _ => self.closure(Ty::Int),
        }
    }

    fn boolean(&mut self) -> String {
        match self.rng.below(9) {
            0..=1 => self.atom(Ty::Bool),
            2..=4 => {
                let lhs = self.expression(Ty::Int);
                let op = self.rng.one_of(COMPARISONS);
                format!("({lhs} {op} {})", self.expression(Ty::Int))
            }
            5 => {
                let lhs = self.expression(Ty::Bool);
                let op = self.rng.one_of(&["&&", "||"]);
                format!("({lhs} {op} {})", self.expression(Ty::Bool))
            }
            6 => format!("(!{})", self.expression(Ty::Bool)),
            7 => {
                let list = self.expression(Ty::List);
                format!("{list}.contains(&{})", self.expression(Ty::Int))
            }
            _ => {
                if self.rng.chance(2) {
                    format!("{}.is_empty()", self.expression(Ty::List))
                } else {
                    format!("{}.is_empty()", self.expression(Ty::Text))
                }
            }
        }
    }

    fn text(&mut self) -> String {
        match self.rng.below(7) {
            0..=1 => self.atom(Ty::Text),
            2 => {
                let lhs = self.expression(Ty::Int);
                format!("format!(\"{{}}-{{}}\", {lhs}, {})", self.expression(Ty::Text))
            }
            3 => format!("format!(\"{{:?}}\", {})", self.expression(Ty::List)),
            // `format!` rather than `.to_string()`: the VM refuses
            // `(s.len() as i64).to_string()`, which
            // `to_string_on_a_string_length_is_refused_by_the_vm` pins, and
            // this would reach it in a few scripts in every thousand.
            4 => format!("format!(\"{{}}\", {})", self.expression(Ty::Int)),
            5 => format!("{}.to_uppercase()", self.expression(Ty::Text)),
            _ => self.call(Ty::Text),
        }
    }

    fn list(&mut self) -> String {
        match self.rng.below(7) {
            0..=1 => self.atom(Ty::List),
            2 => {
                let items: Vec<String> = (0..1 + self.rng.below(3)).map(|_| self.expression(Ty::Int)).collect();
                format!("vec![{}]", items.join(", "))
            }
            3 => format!("(0..{}).collect::<Vec<i64>>()", self.rng.below(5)),
            // A closure handed to a native, which calls it back.
            4 => {
                let param = self.name("x");
                let list = self.expression(Ty::List);
                self.vars.push(Var { name: param.clone(), ty: Ty::Int, mutable: false });
                let body = self.expression(Ty::Int);
                self.vars.pop();
                format!("{list}.into_iter().map(|{param}| {body}).collect::<Vec<i64>>()")
            }
            5 => {
                let param = self.name("x");
                let list = self.expression(Ty::List);
                self.vars.push(Var { name: param.clone(), ty: Ty::Int, mutable: false });
                let body = self.expression(Ty::Bool);
                self.vars.pop();
                format!("{list}.into_iter().filter(|{param}| {{ let {param} = *{param}; {body} }}).collect::<Vec<i64>>()")
            }
            _ => self.call(Ty::List),
        }
    }

    /// A closure called on the spot.
    fn closure(&mut self, ty: Ty) -> String {
        let param = self.name("c");
        let argument = self.atom(Ty::Int);
        self.vars.push(Var { name: param.clone(), ty: Ty::Int, mutable: false });
        let body = self.expression(ty);
        self.vars.pop();
        format!("(|{param}: i64| -> {} {{ {body} }})({argument})", ty.name())
    }

    fn call(&mut self, ty: Ty) -> String {
        let candidates: Vec<(String, Vec<Ty>)> = self.functions.iter().filter(|(.., result)| *result == ty).map(|(name, params, _)| (name.clone(), params.clone())).collect();
        if candidates.is_empty() {
            return self.atom(ty);
        }
        let (name, params) = candidates[self.rng.below(candidates.len())].clone();
        let args: Vec<String> = params.iter().map(|param| self.atom(*param)).collect();
        format!("{name}({})", args.join(", "))
    }
}
