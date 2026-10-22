use iced::widget::{Renderer, Widget, button, column, component, sensor, space, text};

fn main() -> iced::Result {
    iced::application(|| false, update, view).run()
}

fn update(state: &mut bool, _: ()) {
    *state = true;
}

fn view(state: &bool) -> impl Widget<()> {
    column![
        button("show").on_press(()),
        state.then(|| component(Component))
    ]
}

struct Component;

impl<'a> component::Component<'a, ()> for Component {
    type State = bool;
    type Event = ();

    fn update(&self, state: &mut bool, _: (), _: &Renderer) -> Option<()> {
        *state = true;
        None
    }

    fn view(&self, state: &bool) -> impl Widget<()> + 'a {
        sensor(if *state {
            text("hi").height(20.8).boxed()
        } else {
            space().boxed()
        })
        .on_show(|_| ())
    }
}
