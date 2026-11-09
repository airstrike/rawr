//! Native: a tab of the marked `home`, which can't be cloned and which
//! `home` keeps in a `Vec`.

#[derive(Debug)]
pub struct Tab {
    title: String,
}

impl Tab {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn rename(&mut self, title: &str) {
        self.title = title.to_owned();
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        println!("tab {} closed", self.title);
    }
}
