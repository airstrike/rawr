//! Native: the app's workspace, as inboard's: it keeps the marked home
//! screen's `State` between calls, gives it messages, polls the tasks its
//! `update` gives and feeds their messages back.
pub mod home;
pub mod inbox;
pub mod thumbs;

pub struct Workspace {
    pub home: home::State,
    pub inbox: inbox::State,
    pub updates: Vec<inbox::Update>,
    /// What the wizard's catalog reads, as inboard's source pool.
    pub sources: inbox::State,
    pub thumbs: thumbs::Renderer,
    pub theme: crate::Theme,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            home: home::State::new(),
            inbox: inbox::State::default(),
            updates: Vec::new(),
            sources: inbox::State::default(),
            thumbs: thumbs::Renderer::default(),
            theme: crate::Theme::new("dark"),
        }
    }

    /// Give `message` to the home screen, and every message its tasks give
    /// after it, as iced's runtime does, and say where its wizard
    /// commits. How many messages it took.
    pub fn update(&mut self, message: home::Message) -> usize {
        let mut pending = vec![message];
        let mut taken = 0;
        while let Some(message) = pending.pop() {
            taken += 1;
            let catalog = home::wizard::View::new(&self.sources);
            let action = home::update(
                &mut self.home,
                message,
                &mut self.inbox,
                catalog,
                &mut self.thumbs,
            );
            if let Some(home::Instruction::Commit(step)) = action.instruction {
                println!("committed at step {step}");
            }
            pending.extend(action.task.poll());
        }
        taken
    }

    /// The home screen's element, which borrows the screen, the inbox and
    /// its updates for as long as native code holds it, as iced's `view`
    /// does.
    pub fn show(&self) -> ui::Element<'_, home::Message> {
        self.show_over(None)
    }

    /// The home screen's element over `grid`, an element of its own, as
    /// inboard's workspace gives `home_grid`.
    pub fn show_over<'a>(
        &'a self,
        grid: Option<ui::Element<'a, home::wizard::Message>>,
    ) -> ui::Element<'a, home::Message> {
        let catalog = home::wizard::View::new(&self.inbox);
        self.home
            .view(&self.inbox, &self.updates, catalog, grid, &self.theme)
    }

    pub fn press(&mut self) -> usize {
        let element = home::view(&self.home);
        println!("view {}", element.label);
        match element.on_press {
            Some(message) => self.update(message),
            None => 0,
        }
    }
}
