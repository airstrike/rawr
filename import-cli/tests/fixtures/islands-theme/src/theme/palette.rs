//! Native: the colors a theme is made of, as inboard's `themedef`.

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub colors: Colors,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    pub accent: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hue {
    Warm,
    Cool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "light".to_owned(),
            colors: Colors { accent: 120 },
        }
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            name: "dark".to_owned(),
            colors: Colors { accent: 250 },
        }
    }
}
