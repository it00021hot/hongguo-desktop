mod actions;
mod fetch;
mod folder_actions;
pub mod model;

pub use actions::{Removal, remove};
pub use fetch::{collection_folders, create_collection_folder, load, update_collection_folder};
pub use folder_actions::{
    delete_collection_folder, move_collection_items, update_collection_folder_settings,
};
pub use hongguo_api::interact::{BookshelfFolder, FolderMembership};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Follows,
    Collections,
    Liked,
    Reservations,
}
