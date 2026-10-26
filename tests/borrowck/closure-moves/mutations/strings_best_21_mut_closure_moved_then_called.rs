// Parse `name = score` lines with split_once and let else.
fn parse(line: &str) -> Option<(String, i32)> {
    let Some((name, score)) = line.split_once('=') else {
        return None;
    };
    Some((name.trim().to_string(), score.trim().parse().ok()?))
}

fn main() {
    let mut best = (String::from("nobody"), i32::MIN);
    for line in ["ferris = 42", "say \"hi\"", "dyno=99", "gopher=7"] {
        let Some(entry) = parse(line) else {
            println!("skipping {line:?}");
            continue;
        };
        if entry.1 > best.1 {
            println!("{} takes the lead from {}", entry.0, best.0);
            best = entry;
        }
    }
    let mut __f = || __touch(&mut best);
    let mut __g = __f;
    __g();
    __f();
    println!("{} wins with {}", best.0, best.1);

    let bytes = best.0.as_bytes();
    let vowels = bytes.iter().filter(|&&b| b"aeiouy".contains(&b)).count();
    let first = bytes[0] as char;
    println!("{bytes:?} starts with b'{first}' and has {vowels} vowels");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
