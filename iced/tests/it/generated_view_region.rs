use iced::widget::Widget as _;
use rawr_iced::profile::runtime::Region;

struct Borrowed<'view>(&'view str);

impl iced::advanced::widget::Meta for Borrowed<'_> {}

impl<Message, Theme, Renderer> iced::advanced::Widget<Message, Theme, Renderer> for Borrowed<'_>
where
    Renderer: iced::advanced::Renderer,
{
    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size::new(iced::Length::Shrink, iced::Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        _renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) {
        let width = (self.0.len() as f32).max(1.0);
        tree.size = limits.resolve(
            iced::Length::Shrink,
            iced::Length::Shrink,
            iced::Size::new(width, 1.0),
        );
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        _layout: iced::advanced::Layout,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
    }
}

fn borrowed_text<'view>(
    region: &'view mut Region<'view>,
    plan: &'view String,
) -> iced::Element<'view, ()> {
    region.lower(plan, |_context, text| {
        iced::widget::text(text.as_str()).boxed()
    })
}

fn borrowed_input<'view>(
    region: &'view mut Region<'view>,
    plan: &'view (String, String),
) -> iced::Element<'view, String> {
    region.lower(plan, |_context, (placeholder, value)| {
        iced::widget::text_input(placeholder, value)
            .on_input(|value| value)
            .boxed()
    })
}

fn borrowed_custom<'view>(
    region: &'view mut Region<'view>,
    plan: &'view String,
) -> iced::Element<'view, ()> {
    region.lower(plan, |_context, label| Borrowed(label).boxed())
}

fn input_only_callback<'view>(
    region: &'view mut Region<'view>,
    plan: &'view String,
) -> iced::Element<'view, usize> {
    region.lower(plan, |_context, value| {
        fn length(input: &str) -> usize {
            input.len()
        }
        iced::widget::text_input("Value", value)
            .on_input(|input| length(&input))
            .boxed()
    })
}

fn owned_shared_callback<'view>(
    region: &'view mut Region<'view>,
    plan: &'view (
        String,
        std::sync::Arc<dyn Fn(String) -> usize + Send + Sync>,
    ),
) -> iced::Element<'view, usize> {
    region.lower(plan, |_context, (value, callback)| {
        let callback = std::sync::Arc::clone(callback);
        iced::widget::text_input("Value", value)
            .on_input(move |input| callback(input))
            .boxed()
    })
}

#[test]
fn borrowed_widgets_are_confined_by_the_region_signature() {
    let _ = borrowed_text;
    let _ = borrowed_input;
    let _ = borrowed_custom;
    let _ = input_only_callback;
    let _ = owned_shared_callback;
    assert!(!std::any::type_name::<Region<'static>>().is_empty());
}
