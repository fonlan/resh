use std::sync::Arc;
use std::time::Instant;

use super::types::FileEntry;

pub(super) const DIRECTORY_LISTING_CACHE_MAX_ENTRIES: usize = 32;
pub(super) const DIRECTORY_LISTING_PAGE_LIMIT_DEFAULT: usize = 400;
pub(super) const DIRECTORY_LISTING_PAGE_LIMIT_MAX: usize = 2_000;

#[derive(Clone)]
pub(super) struct CachedDirectoryListing {
    pub(super) session_id: String,
    pub(super) files: Arc<Vec<FileEntry>>,
    pub(super) created_at: Instant,
}
