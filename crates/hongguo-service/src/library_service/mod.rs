mod actions;
mod fetch;
pub mod model;

pub use actions::{Removal, remove};
pub use fetch::load;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Follows,
    Collections,
    Liked,
    Reservations,
}
