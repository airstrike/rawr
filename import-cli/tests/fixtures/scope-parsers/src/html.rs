// rawr
//! pulldown-cmark's README example, as written there: the parser a frame
//! holds goes to `push_html`, which writes into a `String` it borrows
//! mutably. Then the same with `html::push_html(&mut s, parser)` on a
//! `String`'s text, into a string that already holds some.

use pulldown_cmark::{html, Options, Parser};

pub fn main() {
    let markdown_input = "Hello world, this is a ~~complicated~~ *very simple* example.";

    // Set up options and parser. Strikethroughs are not part of the CommonMark standard
    // and we therefore must enable it explicitly.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown_input, options);

    // Write to String buffer.
    let mut html_output = String::new();
    pulldown_cmark::html::push_html(&mut html_output, parser);

    // Check that the output is what we expected.
    let expected_html =
        "<p>Hello world, this is a <del>complicated</del> <em>very simple</em> example.</p>\n";
    assert_eq!(expected_html, &html_output);
    print!("{html_output}");

    let text = String::from("# Title\n\n- one\n- *two*\n");
    let mut s = String::from("<!-- before -->\n");
    let parser = Parser::new(&text);
    html::push_html(&mut s, parser);
    print!("{s}");
    println!("html bytes: {}", s.len());
}
