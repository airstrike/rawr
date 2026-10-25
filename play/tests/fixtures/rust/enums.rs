// An enum per command, `match` to run it, and `?` to stop at the first error.
#[derive(Debug)]
enum Command {
    Push(i64),
    Add,
    Mul,
}

fn parse(word: &str) -> Result<Command, String> {
    match word {
        "+" => Ok(Command::Add),
        "*" => Ok(Command::Mul),
        _ => word
            .parse()
            .map(Command::Push)
            .map_err(|_| format!("not a command: {word:?}")),
    }
}

fn calc(program: &str) -> Result<i64, String> {
    let mut stack = Vec::new();
    for word in program.split_whitespace() {
        match parse(word)? {
            Command::Push(n) => stack.push(n),
            Command::Add | Command::Mul => {
                let (Some(b), Some(a)) = (stack.pop(), stack.pop()) else {
                    return Err(format!("{word} needs two values"));
                };
                stack.push(if word == "+" { a + b } else { a * b });
            }
        }
    }
    stack.pop().ok_or_else(|| "empty program".to_string())
}

fn main() {
    for program in ["2 3 + 4 *", "1 +", "6 7 x", ""] {
        match calc(program) {
            Ok(value) => println!("{program:>9} = {value}"),
            Err(error) => println!("{program:>9} : {error}"),
        }
    }
}
