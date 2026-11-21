//! The function a place in a file is written in, by its tokens: a hover
//! reads the frame of that function, never a binding of the same name in
//! another.

use crate::tokenizer::Token;
use crate::Position;

/// The name of the innermost function whose signature or body holds line
/// `line` and character `column` (both from 1) of the file `lexed` lexes:
/// a closure's place is in the function it is written in. `None` outside
/// every function.
pub(super) fn function(lexed: &[(Token, Position)], line: usize, column: usize) -> Option<String> {
    // Each `{` open before the place, with the function whose body it
    // opens, if it opens one.
    let mut open: Vec<Option<String>> = Vec::new();
    // A function whose signature is under way, and whether its name is
    // the next token.
    let mut signature: Option<String> = None;
    let mut naming = false;
    // How deep in `(` and `[` the tokens are: a `;` there is no item's end.
    let mut nested = 0usize;
    for (token, position) in lexed {
        let (Some(at_line), Some(at_column)) = (position.line(), position.position()) else {
            continue;
        };
        if (at_line, at_column) > (line, column) {
            break;
        }
        match token {
            Token::Fn => naming = true,
            Token::Identifier(name) if naming => {
                signature = Some(name.to_string());
                naming = false;
            }
            Token::LeftParen | Token::LeftBracket => {
                naming = false;
                nested += 1;
            }
            Token::RightParen | Token::RightBracket => nested = nested.saturating_sub(1),
            Token::LeftBrace | Token::MapStart => open.push(signature.take()),
            Token::RightBrace => {
                open.pop();
            }
            Token::SemiColon if nested == 0 => signature = None,
            _ => naming = false,
        }
    }
    signature.or_else(|| open.iter().rev().find_map(Clone::clone))
}

#[cfg(test)]
mod tests {
    use super::function;

    const FILE: &str = "fn other(count: u32) -> u32 {\n\
                        \x20   count\n\
                        }\n\
                        impl Game {\n\
                        \x20   fn tick(&mut self, by: [u8; 2]) {\n\
                        \x20       let add = |n: u32| n + self.count;\n\
                        \x20   }\n\
                        }\n";

    #[test]
    fn a_place_is_in_the_innermost_function_around_it() {
        let lexed = crate::resolve::lower_nominal::lex_rust(&crate::Engine::new(), FILE);
        assert_eq!(function(&lexed, 2, 5).as_deref(), Some("other"));
        assert_eq!(
            function(&lexed, 1, 10).as_deref(),
            Some("other"),
            "a parameter"
        );
        assert_eq!(
            function(&lexed, 5, 30).as_deref(),
            Some("tick"),
            "past a `;` in a type"
        );
        assert_eq!(
            function(&lexed, 6, 40).as_deref(),
            Some("tick"),
            "a closure's body"
        );
        assert_eq!(function(&lexed, 4, 6), None, "an impl is no function");
    }
}
