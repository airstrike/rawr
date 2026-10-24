// The script behind `tests/grain/fixtures/golden.rgrn`. See
// `a_golden_artifact_written_by_an_older_build_still_runs` in
// `tests/grain/format.rs`.
//
// Deliberately wide rather than deliberately short: every encoder branch this
// reaches is a branch the golden artifact pins against silent drift. Editing it
// means regenerating the artifact, which is the point — the pair only proves
// anything while the artifact is older than the reader.
//
// Every integer is an `i64` and nothing formats a float or a `usize`, because
// those still lower through compiler-owned support, and a program holding that
// support cannot be written at all. The caller supplies `supplied`, an `i64`,
// and `w`, a host value only registered functions can use.

fn tally(items: &Vec<i64>, bias: i64) -> i64 {
    let mut total: i64 = bias;
    for item in items {
        total += item;
    }
    total
}

const STRIDE: i64 = 2;
let mut counts: Vec<i64> = vec![1, 2, 3];
let label = "measure";
let ratio: f64 = 2.5;
// Not all-constant, so the optimizer leaves it to be built at run time.
let computed: Vec<i64> = vec![STRIDE, counts.len() as i64, supplied];

// A chain rooted at a slot, one on a temporary, and a write to the name the
// caller supplied.
counts.push(tally(&counts, 10));
let widest = vec![9, 8].len() as i64;
supplied += counts.len() as i64;

// Registered functions taking the caller's value by reference, the second
// with an argument the receiver is resolved after.
bump(w);
raise(w, STRIDE);

// A closure, which shares what it captures and curries it onto a pointer. Made
// and spent inside a block, so the scope this is compared on does not end up
// holding a pointer — the two sides render one differently on purpose, which
// `a_closure_pointer_is_late_bound` in `tests/grain/scope.rs` pins.
let mut captured: i64 = 0;
{
    let mut add = |n: i64| captured += n;
    add(supplied);
    let peek = |k: i64| k + supplied;
    captured += peek(1);
}

let mut doubled: i64 = 0;
let mut i: i64 = 0;
while i < counts.len() as i64 {
    doubled += counts[i as usize];
    i += 1;
}

let mut squares: Vec<i64> = Vec::new();
for n in counts.clone() {
    if n > 3 {
        break;
    }
    squares.push(n * STRIDE);
}

// A closure handed to a native, which calls it back.
let mapped: Vec<i64> = squares.iter().map(|x| x + 1).collect();
let big = ratio * 2.0 > 4.0;
let size = match doubled {
    0 => "none",
    1..=9 => "few",
    _ => "many",
};
let text = format!("{} of {} is {}", label, counts.len() as i64, size);

format!(
    "{} {} {} {} {} {} {} {:?} {:?} {:?}",
    doubled, text, big, supplied, widest, captured, level_of(w), computed, squares, mapped,
)
