pub mod inventory {
    pub struct Product {
        pub name: String,
        pub quantity: u32,
    }

    pub enum Availability {
        InStock,
        BackOrdered(u32),
        Discontinued { replacement: Option<String> },
    }

    pub const DEFAULT_QUANTITY: u32 = 1;

    pub fn product(name: impl Into<String>) -> Product {
        Product {
            name: name.into(),
            quantity: DEFAULT_QUANTITY,
        }
    }

    impl Product {
        pub fn restock(&mut self, quantity: u32) {
            self.quantity = self.quantity.saturating_add(quantity);
        }

        pub fn is_available(&self) -> bool {
            self.quantity > 0
        }
    }
}

pub use inventory::{Availability, Product, product};
