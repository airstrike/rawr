//! This script tests string operations.

println!("hello");
println!("this\nis \\ nice");       // escape sequences
println!("0x40 hex is \x40");       // hex escape sequence
println!("Unicode fun: \u{2764}");  // Unicode escape sequence
println!("more fun: \u{1F603}");    // Unicode escape sequence
println!("{}", "foo".to_string() + " " + "bar");    // string building using strings
println!("{}", "foo" < "bar");      // string comparison
println!("{}", "foo" >= "bar");     // string comparison
println!("{}", format!("the answer is {}", 42));    // string building using non-string types

let s = "\u{2764} hello, world! \u{1F603}"; // string variable
println!("length={}", s.chars().count());   // should be 17

let s: String = s.chars().skip(2).take(12).chain(['?']).collect();  // change the string
println!("Question: {s}");          // should print 'Question: hello, world?'

// Line continuation:
let s = "This is a long \
         string constructed using \
         line continuation";

// Inline format arguments
println!("One string: {s}");

// Raw multi-line string literal:
let s = r#"
          This is a multi-line
                "string" with no \t escapes
made using raw string
      syntax.
"#;

println!("{s}");

// Nested formatting
let inner = format!("within {} formatting", format!("yet another level of {}", "formatting!"));
let s = format!("This is formatting {inner} within a format string.");

println!("{s}");

println!(">>> END <<<");
