// Collect a string's letters into your own type.
struct Histogram([usize; 26]);

impl FromIterator<char> for Histogram {
    fn from_iter<I: IntoIterator<Item = char>>(iter: I) -> Self {
        let mut counts = [0; 26];
        for c in iter {
            if c.is_ascii_alphabetic() {
                counts[(c.to_ascii_lowercase() as u8 - b'a') as usize] += 1;
            }
        }
        Histogram(counts)
    }
}

fn main() {
    let text = "Jumpy Dyno quickly vexes the big frozen wolf at dawn.";
    let Histogram(counts) = text.chars().collect();
    for (i, &n) in counts.iter().enumerate() {
        if n > 0 {
            let letter = (b'a' + i as u8) as char;
            println!("{letter} {}", "#".repeat(n));
        }
    }
}
