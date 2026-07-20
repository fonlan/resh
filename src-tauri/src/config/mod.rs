pub mod loader;
pub mod sync_manager;
pub mod sync_merge;
pub mod sync_protocol;
pub mod sync_state;
pub mod types;

pub use loader::ConfigManager;
pub use sync_manager::SyncManager;
pub use sync_protocol::{SyncConflict, SyncOutcome, SyncResolution, SyncResolutionChoice};
pub use types::Config;
