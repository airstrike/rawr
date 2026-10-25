//! An iced snippet, as a preview or a full app.

/// The snippet shape [`wrap`] recognized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    /// The source declares its own `#[preview]`; refused.
    Preview,
    /// A type with `fn view(&self)`.
    App,
    /// A stateless `fn view()`.
    View,
    /// A bare expression.
    Expression,
}

/// Classify a snippet without wrapping it.
#[must_use]
pub fn shape(source: &str) -> Shape {
    if source.contains("#[preview") {
        Shape::Preview
    } else if runs_itself(source) {
        // `iced::run(|state: &mut u32, message: Message| .., |state: &u32| ..)`
        // has no `fn view` of its own.
        Shape::App
    } else if let Some(at) = find_view(source) {
        let parameters = source[at..]
            .split_once('(')
            .and_then(|(_, rest)| rest.split_once(')'))
            .map_or("", |(parameters, _)| parameters.trim());
        if !parameters.is_empty() {
            Shape::App
        } else {
            Shape::View
        }
    } else {
        Shape::Expression
    }
}

/// How [`wrap`] and [`live`] wrap a snippet.
#[derive(Clone, Copy, Debug)]
pub struct Options<'a> {
    /// The preview's logical `(width, height)`.
    pub size: (u32, u32),
    /// The built-in theme's `iced::Theme` variant, such as `Dark`.
    pub theme: &'a str,
    /// Expressions, each a full app's Message, wrapped as functions named
    /// [`MESSAGE`] and their index.
    pub messages: &'a [String],
    /// `use` items for a snippet with no top-level `use` of its own: the
    /// prelude rawr-iced's manifest names.
    pub prelude: &'a str,
}

/// The function a full app's `index`th message is wrapped as.
pub const MESSAGE: &str = "rawr_play_message_";

/// Wrap a snippet into a program: a view or an expression gets one
/// `#[preview]`; a full app runs as itself, given a `main` when it has none
/// and each message as a function returning its Message.
///
/// User lines keep their line numbers: added imports and attributes share
/// the first line or the `fn view` line, and helpers go after the source.
pub fn wrap(source: &str, options: &Options<'_>) -> Result<String, String> {
    let (width, height) = options.size;
    let size = format!("size = ({width}, {height})");
    let source = &without_tests(source);
    let source = source.as_str();
    let theme_fn = format!(
        "\nfn rawr_play_theme() -> iced::Theme {{ iced::Theme::{} }}\n",
        options.theme
    );
    let themes = "themes = [rawr_play_theme]";
    let shape = shape(source);
    if !options.messages.is_empty() && shape != Shape::App {
        return Err(NEEDS_APP.into());
    }
    let prelude = prelude(source, options);
    Ok(match shape {
        // A hand-written `#[preview]` could pick any canvas size.
        Shape::Preview => {
            return Err("?play adds `#[preview]` itself; remove it and use size=WxH".into());
        }
        Shape::Expression => format!(
            "{prelude}#[preview({size}, {themes})] fn view() -> impl iced::widget::Widget<()> {{ {source}\n}}{theme_fn}"
        ),
        Shape::View => {
            let at = find_view(source).expect("view shape has a view fn");
            format!(
                "{prelude}{}#[preview({size}, {themes})] {}{theme_fn}",
                &source[..at],
                &source[at..]
            )
        }
        Shape::App => app(source, options)?,
    })
}

/// Wrap a snippet into a program that runs live, as a browser runs it: a
/// full app as [`wrap`] wraps it, and a view or an expression as an app
/// whose State is nothing and whose view is the snippet. Size and theme are
/// the host's.
pub fn live(source: &str, options: &Options<'_>) -> Result<String, String> {
    let source = &without_tests(source);
    let source = source.as_str();
    let shape = shape(source);
    if !options.messages.is_empty() && shape != Shape::App {
        return Err(NEEDS_APP.into());
    }
    let prelude = prelude(source, options);
    let host = |message: &str| {
        format!(
            "\n#[derive(Default)]\nstruct RawrPlayView;\n\
             impl RawrPlayView {{\n\
                 fn update(&mut self, _message: {message}) {{}}\n\
                 fn view(&self) -> impl iced::widget::Widget<{message}> {{ view() }}\n\
             }}\n\
             pub fn main() -> iced::Result {{ iced::run(RawrPlayView::update, RawrPlayView::view) }}\n"
        )
    };
    Ok(match shape {
        Shape::Preview => {
            return Err("remove `#[preview]`: a snippet is its own preview".into());
        }
        Shape::Expression => format!(
            "{prelude}fn view() -> impl iced::widget::Widget<()> {{ {source}\n}}{}",
            host("()")
        ),
        Shape::View => {
            let at = find_view(source).expect("view shape has a view fn");
            let message = view_message(&source[at..]).unwrap_or_else(|| "()".into());
            format!("{prelude}{source}{}", host(&message))
        }
        Shape::App => app(source, options)?,
    })
}

const NEEDS_APP: &str =
    "messages need a full app with `fn view(&self)` and `fn update(&mut self, ..)`";

/// rawr-iced's prelude, for a snippet that imports nothing itself. A `use`
/// inside a function body imports only there.
fn prelude<'a>(source: &str, options: &Options<'a>) -> &'a str {
    if has_top_level_use(source) {
        ""
    } else {
        options.prelude
    }
}

/// A full app runs as itself: given a `main` when it has none, and each
/// message as a function of its Message type.
fn app(source: &str, options: &Options<'_>) -> Result<String, String> {
    let mut program = source.to_owned();
    if !runs_itself(source) {
        let at = find_view(source).expect("app shape has a view fn");
        let state = app_state(source, at)
            .ok_or_else(|| "cannot find the State type that owns `fn view`".to_owned())?;
        let callbacks = if view_signature(&source[at..]).contains("self") {
            format!("{state}::update, {state}::view")
        } else {
            "update, view".into()
        };
        program.push_str(&format!(
            "\npub fn main() -> iced::Result {{ iced::run({callbacks}) }}\n"
        ));
    }
    if !options.messages.is_empty() {
        let message = message_type(source)
            .ok_or_else(|| "cannot find the Message type `fn update` takes".to_owned())?;
        for (index, expression) in options.messages.iter().enumerate() {
            program.push_str(&format!(
                "\nfn {MESSAGE}{index}() -> {message} {{ {expression} }}\n"
            ));
        }
    }
    Ok(program)
}

/// The Message of a stateless view's `Widget<M>` or `Element<'_, M>`.
fn view_message(view: &str) -> Option<String> {
    let returns = view.split_once("->")?.1;
    let returns = &returns[..returns.find('{')?];
    let (_, generics) = returns.split_once('<')?;
    let generics = generics.trim_end().strip_suffix('>')?;
    let message = generics
        .split(',')
        .map(str::trim)
        .rfind(|part| !part.starts_with('\''))?;
    (!message.is_empty()).then(|| message.to_owned())
}

/// Whether `source` starts its own app with `iced::run` or
/// `iced::application`.
fn runs_itself(source: &str) -> bool {
    source.contains("iced::run(") || source.contains("iced::application(")
}

/// Whether a line of `source` outside every brace declares an inline
/// module: `mod name {` or `pub mod name {`.
pub fn has_top_level_mod(source: &str) -> bool {
    let mut depth = 0usize;
    for line in source.lines() {
        let item = line.trim_start();
        let item = item.strip_prefix("pub ").unwrap_or(item);
        if depth == 0 && item.starts_with("mod ") && item.contains('{') {
            return true;
        }
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    false
}

/// Whether a line of `source` outside every brace starts with `use `.
fn has_top_level_use(source: &str) -> bool {
    let mut depth = 0usize;
    for line in source.lines() {
        if depth == 0 && line.trim_start().starts_with("use ") {
            return true;
        }
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    false
}

/// The parameters of the `fn view` at the start of `view`.
fn view_signature(view: &str) -> &str {
    view.split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map_or("", |(parameters, _)| parameters)
}

/// The type of `fn update`'s last parameter, or of the update closure's
/// that `iced::run` takes: the app's Message.
fn message_type(source: &str) -> Option<String> {
    let Some((_, rest)) = source.split_once("fn update(") else {
        return closure_message_type(source);
    };
    let mut depth = 0usize;
    let end = rest.char_indices().find_map(|(at, c)| match c {
        '(' | '<' | '[' => {
            depth += 1;
            None
        }
        ')' if depth == 0 => Some(at),
        ')' | '>' | ']' => {
            depth = depth.saturating_sub(1);
            None
        }
        _ => None,
    })?;
    last_parameter_type(&rest[..end])
}

/// The last parameter type of the first closure after `iced::run(` or
/// `iced::application(` that takes `&mut` state: its update's Message.
fn closure_message_type(source: &str) -> Option<String> {
    let start = source
        .find("iced::run(")
        .or_else(|| source.find("iced::application("))?;
    let rest = &source[start..];
    let mut from = 0;
    while let Some(open) = rest[from..].find('|') {
        let open = from + open + 1;
        let close = open + rest[open..].find('|')?;
        let parameters = &rest[open..close];
        if parameters.contains("&mut") {
            return last_parameter_type(parameters);
        }
        from = close + 1;
    }
    None
}

/// The type of the last of `parameters`, as `name: Type` or `_: Type`.
fn last_parameter_type(parameters: &str) -> Option<String> {
    let mut depth = 0usize;
    let last = parameters
        .char_indices()
        .filter(|(_, c)| {
            match c {
                '(' | '<' | '[' => depth += 1,
                ')' | '>' | ']' => depth = depth.saturating_sub(1),
                _ => {}
            }
            *c == ',' && depth == 0
        })
        .last()
        .map_or(parameters, |(at, _)| &parameters[at + 1..]);
    let (_, ty) = last.split_once(':')?;
    let ty = ty.trim();
    (!ty.is_empty()).then(|| ty.to_owned())
}

/// `source` with each `#[cfg(test)]` item (such as `mod tests { .. }`)
/// blanked, keeping line numbers: a pasted example often ends with tests.
pub fn without_tests(source: &str) -> String {
    const ATTRIBUTE: &str = "#[cfg(test)]";
    let mut out = source.to_owned();
    let mut from = 0;
    while let Some(found) = out[from..].find(ATTRIBUTE) {
        let start = from + found;
        let mut depth = 0usize;
        let mut end = out.len();
        for (offset, c) in out[start + ATTRIBUTE.len()..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + ATTRIBUTE.len() + offset + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = start + ATTRIBUTE.len() + offset + 1;
                    break;
                }
                _ => {}
            }
        }
        let blank: String = out[start..end]
            .chars()
            .map(|c| if c == '\n' { c } else { ' ' })
            .collect();
        out.replace_range(start..end, &blank);
        from = start + blank.len();
    }
    out
}

/// Byte offset of `fn view` (or `pub fn view`) followed by `(`.
fn find_view(source: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = source[from..].find("fn view") {
        let at = from + found;
        let after = source[at + "fn view".len()..].trim_start();
        let boundary = at == 0
            || !source[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if boundary && after.starts_with('(') {
            let before = source[..at].trim_end();
            return Some(match before.strip_suffix("pub") {
                Some(rest) => rest.len(),
                None => at,
            });
        }
        from = at + 1;
    }
    None
}

/// The path an app passes to `.theme(..)`, such as `App::theme` or `ink`.
pub fn app_theme(source: &str) -> Option<&str> {
    let (_, rest) = source.split_once(".theme(")?;
    let path = rest[..rest.find(')')?].trim();
    let valid = !path.is_empty()
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == ':');
    valid.then_some(path)
}

/// The State type for a view at `at`: the nearest preceding `impl Name`, or
/// the `&Name` parameter of a free `fn view(state: &Name)`.
fn app_state(source: &str, at: usize) -> Option<String> {
    let ident = |text: &str| -> String {
        text.chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect()
    };
    let view = view_signature(&source[at..]);
    if !view.contains("self") {
        let (_, rest) = view.split_once('&')?;
        let name = ident(rest.trim_start());
        return (!name.is_empty()).then_some(name);
    }
    let impl_at = source[..at].rfind("impl ")?;
    let name = ident(source[impl_at + "impl ".len()..].trim_start());
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::{has_top_level_use, live, view_message, wrap, Options};

    const OPTIONS: Options<'static> = Options {
        size: (480, 360),
        theme: "Dark",
        messages: &[],
        prelude: "use iced::widget::text; ",
    };

    #[test]
    fn a_use_in_a_function_body_keeps_the_prelude() {
        assert!(has_top_level_use("use iced::widget::text;\nfn view() {}"));
        assert!(!has_top_level_use(
            "fn view() {\n    use iced::widget::text;\n}"
        ));
    }

    #[test]
    fn a_live_view_or_expression_is_an_app_on_the_same_lines() {
        let expression = live("text(\"hi\")", &OPTIONS).unwrap();
        assert!(expression.starts_with(
            "use iced::widget::text; fn view() -> impl iced::widget::Widget<()> { text(\"hi\")\n}"
        ));
        assert!(expression.contains("fn update(&mut self, _message: ()) {}"));
        assert!(expression.contains("iced::run(RawrPlayView::update, RawrPlayView::view)"));
        let view = "fn view() -> Element<'static, Message> {\n    text(\"hi\").into()\n}\n";
        let program = live(view, &OPTIONS).unwrap();
        assert!(program.starts_with(&format!("use iced::widget::text; {view}")));
        assert!(program.contains("_message: Message"));
        assert!(!program.contains("#[preview"));
    }

    #[test]
    fn a_live_app_is_wrapped_as_the_server_wraps_it() {
        let app = "struct App;\nimpl App {\n    fn update(&mut self, message: Message) {}\n    fn view(&self) -> Element<'_, Message> { text(\"\").into() }\n}\n";
        let messages = ["Message::Go".to_owned()];
        let options = Options {
            messages: &messages,
            ..OPTIONS
        };
        assert_eq!(live(app, &options), wrap(app, &options));
        let program = live(app, &options).unwrap();
        assert!(program.contains("iced::run(App::update, App::view)"));
        assert!(program.contains("fn rawr_play_message_0() -> Message { Message::Go }"));
        assert!(live("text(\"hi\")", &options).is_err());
    }

    #[test]
    fn a_view_message_comes_from_its_return_type() {
        assert_eq!(
            view_message("fn view() -> impl Widget<Message> {").as_deref(),
            Some("Message")
        );
        assert_eq!(
            view_message("fn view() -> Element<'static, Msg> {").as_deref(),
            Some("Msg")
        );
        assert_eq!(view_message("fn view() -> impl Widget {"), None);
    }
}
