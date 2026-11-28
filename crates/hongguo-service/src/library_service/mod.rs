mod actions;
mod fetch;
mod folder_actions;
mod likes_removal;
mod metadata;
pub mod model;
mod reservation_removal;

pub use actions::{Removal, RemovalMode, remove};
pub use fetch::{collection_folders, create_collection_folder, load, update_collection_folder};
pub use folder_actions::{
    delete_collection_folder, move_collection_items, update_collection_folder_settings,
};
pub use hongguo_api::interact::{BookshelfFolder, FolderMembership};
pub use metadata::{CollectionMetadata, METADATA_WINDOW_BATCH, collection_metadata};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Follows,
    Collections,
    Liked,
    Reservations,
}
