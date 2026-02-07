pub mod models;
pub mod schema;
pub mod sqlite;

use chrono::{DateTime, Utc};

use crate::errors::Result;
use models::{Clip, ClipFilter, NewClip, StorageStats};

pub trait ClipStorage {
    fn insert(&self, clip: NewClip) -> Result<Clip>;
    fn get_by_id(&self, id: i64) -> Result<Clip>;
    fn list(&self, filter: ClipFilter) -> Result<Vec<Clip>>;
    fn search(&self, query: &str, limit: i64) -> Result<Vec<Clip>>;
    fn delete(&self, id: i64) -> Result<bool>;
    fn find_by_hash(&self, hash: &str) -> Result<Option<Clip>>;
    fn add_tag(&self, clip_id: i64, tag: &str) -> Result<()>;
    fn remove_tag(&self, clip_id: i64, tag: &str) -> Result<()>;
    fn set_pinned(&self, id: i64, pinned: bool) -> Result<()>;
    fn clear_older_than(&self, before: DateTime<Utc>) -> Result<i64>;
    fn stats(&self) -> Result<StorageStats>;
    fn touch(&self, id: i64) -> Result<()>;
}
