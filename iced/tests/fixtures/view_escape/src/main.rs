use iced::widget::Widget as _;
use rawr_iced::profile::runtime::Region;

fn element_escape<'view>(
    region: &'view mut Region<'view>,
    plan: &'view String,
) -> iced::Element<'static, ()> {
    region.lower(plan, |_context, text| iced::widget::text(text).boxed())
}

fn callback_escape<'view>(value: &'view str) -> Box<dyn Fn() -> &'static str + 'static> {
    Box::new(move || value)
}

fn main() {
    let _ = element_escape;
    let _ = callback_escape;
}
