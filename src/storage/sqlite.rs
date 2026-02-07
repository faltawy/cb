use chrono::{DateTime, Utc};
use rusqlite::{Connection, params, Row};

use crate::errors::{CbError, Result};
use super::ClipStorage;
use super::models::{Clip, ClipFilter, ContentType, NewClip, StorageStats};
use super::schema;

const BASE_SELECT: &str = "
    SELECT clips.id, clips.content_type, clips.text_content, clips.image_path,
           clips.image_width, clips.image_height, clips.hash, clips.size_bytes,
           clips.pinned, clips.created_at, clips.updated_at,
           GROUP_CONCAT(t.tag) as tags
    FROM clips
    LEFT JOIN tags t ON t.clip_id = clips.id
";

pub struct SqliteStorage {
    conn: Connection,
}

fn row_to_clip(row: &Row) -> rusqlite::Result<Clip> {
    let type_str: String = row.get(1)?;
    let pinned_int: i32 = row.get(8)?;
    let tags_str: Option<String> = row.get(11)?;
    let tags = match tags_str {
        Some(s) if !s.is_empty() => s.split(',').map(String::from).collect(),
        _ => Vec::new(),
    };
    Ok(Clip {
        id: row.get(0)?,
        content_type: ContentType::parse(&type_str).unwrap_or(ContentType::Text),
        text_content: row.get(2)?,
        image_path: row.get(3)?,
        image_width: row.get(4)?,
        image_height: row.get(5)?,
        hash: row.get(6)?,
        size_bytes: row.get(7)?,
        pinned: pinned_int != 0,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        tags,
    })
}

impl SqliteStorage {
    pub fn new(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute(schema::CREATE_CLIPS_TABLE, [])?;
        conn.execute(schema::CREATE_TAGS_TABLE, [])?;
        conn.execute(schema::CREATE_INDEX_HASH, [])?;
        conn.execute(schema::CREATE_INDEX_CREATED_AT, [])?;
        conn.execute(schema::CREATE_INDEX_TAG, [])?;
        conn.execute(schema::CREATE_INDEX_CLIP_ID, [])?;
        Ok(Self { conn })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::new(conn)
    }

    #[cfg(test)]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}

impl ClipStorage for SqliteStorage {
    fn insert(&self, clip: NewClip) -> Result<Clip> {
        let now = Utc::now();
        self.conn.execute(
            "INSERT INTO clips (content_type, text_content, image_path, image_width, image_height, hash, size_bytes, pinned, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
            params![
                clip.content_type.as_str(),
                clip.text_content,
                clip.image_path,
                clip.image_width,
                clip.image_height,
                clip.hash,
                clip.size_bytes,
                now,
                now,
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.get_by_id(id)
    }

    fn get_by_id(&self, id: i64) -> Result<Clip> {
        let sql = format!("{} WHERE clips.id = ? GROUP BY clips.id", BASE_SELECT);
        self.conn
            .query_row(&sql, params![id], row_to_clip)
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CbError::NotFound(format!("Clip with id {} not found", id))
                }
                other => CbError::Storage(other),
            })
    }

    fn list(&self, filter: ClipFilter) -> Result<Vec<Clip>> {
        // TODO: This is the query builder — see below for your contribution spot
        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut use_tag_join = false;

        if let Some(ref ct) = filter.content_type {
            conditions.push("clips.content_type = ?".to_string());
            param_values.push(Box::new(ct.as_str().to_string()));
        }
        if let Some(pinned) = filter.pinned {
            conditions.push("clips.pinned = ?".to_string());
            param_values.push(Box::new(pinned as i32));
        }
        if let Some(ref tag) = filter.tag {
            use_tag_join = true;
            param_values.push(Box::new(tag.clone()));
        }

        let from_clause = if use_tag_join {
            "FROM clips
             INNER JOIN tags t1 ON t1.clip_id = clips.id AND t1.tag = ?
             LEFT JOIN tags t2 ON t2.clip_id = clips.id"
        } else {
            "FROM clips
             LEFT JOIN tags t ON t.clip_id = clips.id"
        };

        let tag_col = if use_tag_join {
            "GROUP_CONCAT(t2.tag)"
        } else {
            "GROUP_CONCAT(t.tag)"
        };

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT clips.id, clips.content_type, clips.text_content, clips.image_path,
                    clips.image_width, clips.image_height, clips.hash, clips.size_bytes,
                    clips.pinned, clips.created_at, clips.updated_at,
                    {} as tags
             {} {} GROUP BY clips.id ORDER BY clips.id DESC LIMIT ? OFFSET ?",
            tag_col, from_clause, where_clause
        );

        param_values.push(Box::new(filter.effective_limit()));
        param_values.push(Box::new(filter.offset));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql)?;
        let clips = stmt
            .query_map(param_refs.as_slice(), row_to_clip)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(clips)
    }

    fn search(&self, query: &str, limit: i64) -> Result<Vec<Clip>> {
        let sql = format!(
            "{} WHERE clips.text_content LIKE '%' || ? || '%' COLLATE NOCASE
             GROUP BY clips.id ORDER BY clips.id DESC LIMIT ?",
            BASE_SELECT
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let clips = stmt
            .query_map(params![query, limit], row_to_clip)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(clips)
    }

    fn delete(&self, id: i64) -> Result<bool> {
        let changes = self.conn.execute("DELETE FROM clips WHERE id = ?", params![id])?;
        Ok(changes > 0)
    }

    fn find_by_hash(&self, hash: &str) -> Result<Option<Clip>> {
        let sql = format!("{} WHERE clips.hash = ? GROUP BY clips.id", BASE_SELECT);
        match self.conn.query_row(&sql, params![hash], row_to_clip) {
            Ok(clip) => Ok(Some(clip)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CbError::Storage(e)),
        }
    }

    fn add_tag(&self, clip_id: i64, tag: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO tags (clip_id, tag) VALUES (?, ?)",
            params![clip_id, tag],
        )?;
        Ok(())
    }

    fn remove_tag(&self, clip_id: i64, tag: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM tags WHERE clip_id = ? AND tag = ?",
            params![clip_id, tag],
        )?;
        Ok(())
    }

    fn set_pinned(&self, id: i64, pinned: bool) -> Result<()> {
        let now = Utc::now();
        self.conn.execute(
            "UPDATE clips SET pinned = ?, updated_at = ? WHERE id = ?",
            params![pinned as i32, now, id],
        )?;
        Ok(())
    }

    fn clear_older_than(&self, before: DateTime<Utc>) -> Result<i64> {
        let changes = self.conn.execute(
            "DELETE FROM clips WHERE updated_at < ? AND pinned = 0",
            params![before],
        )?;
        Ok(changes as i64)
    }

    fn stats(&self) -> Result<StorageStats> {
        self.conn.query_row(
            "SELECT
                COUNT(*),
                COUNT(CASE WHEN content_type = 'text' THEN 1 END),
                COUNT(CASE WHEN content_type = 'image' THEN 1 END),
                COUNT(CASE WHEN content_type = 'fileref' THEN 1 END),
                COALESCE(SUM(size_bytes), 0),
                MIN(created_at),
                MAX(created_at)
             FROM clips",
            [],
            |row| {
                Ok(StorageStats {
                    total_clips: row.get(0)?,
                    text_clips: row.get(1)?,
                    image_clips: row.get(2)?,
                    fileref_clips: row.get(3)?,
                    total_size: row.get(4)?,
                    oldest: row.get(5)?,
                    newest: row.get(6)?,
                })
            },
        ).map_err(CbError::Storage)
    }

    fn touch(&self, id: i64) -> Result<()> {
        let now = Utc::now();
        let changes = self.conn.execute(
            "UPDATE clips SET updated_at = ? WHERE id = ?",
            params![now, id],
        )?;
        if changes == 0 {
            return Err(CbError::NotFound(format!("Clip with id {} not found", id)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::CbError;
    use crate::hash::hash_content;
    use super::super::models::ContentType;
    use chrono::{Duration, Utc};

    fn test_storage() -> SqliteStorage {
        SqliteStorage::in_memory().unwrap()
    }

    fn text_clip(content: &str) -> NewClip {
        NewClip {
            content_type: ContentType::Text,
            text_content: Some(content.to_string()),
            image_path: None,
            image_width: None,
            image_height: None,
            hash: hash_content(content.as_bytes()),
            size_bytes: content.len() as i64,
        }
    }

    fn image_clip(path: &str, w: i32, h: i32) -> NewClip {
        let hash_input = format!("{}:{}x{}", path, w, h);
        NewClip {
            content_type: ContentType::Image,
            text_content: None,
            image_path: Some(path.to_string()),
            image_width: Some(w),
            image_height: Some(h),
            hash: hash_content(hash_input.as_bytes()),
            size_bytes: 1024,
        }
    }

    // --- Schema ---

    #[test]
    fn test_in_memory_creates_tables() {
        let storage = test_storage();
        let count: i64 = storage
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('clips', 'tags')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    // --- Insert ---

    #[test]
    fn test_insert_text_clip() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("hello world")).unwrap();
        assert_eq!(clip.content_type, ContentType::Text);
        assert_eq!(clip.text_content.as_deref(), Some("hello world"));
        assert_eq!(clip.size_bytes, 11);
        assert!(!clip.pinned);
        assert!(clip.tags.is_empty());
    }

    #[test]
    fn test_insert_image_clip() {
        let storage = test_storage();
        let clip = storage.insert(image_clip("/tmp/img.png", 800, 600)).unwrap();
        assert_eq!(clip.content_type, ContentType::Image);
        assert_eq!(clip.image_path.as_deref(), Some("/tmp/img.png"));
        assert_eq!(clip.image_width, Some(800));
        assert_eq!(clip.image_height, Some(600));
    }

    #[test]
    fn test_insert_returns_incrementing_ids() {
        let storage = test_storage();
        let c1 = storage.insert(text_clip("first")).unwrap();
        let c2 = storage.insert(text_clip("second")).unwrap();
        let c3 = storage.insert(text_clip("third")).unwrap();
        assert_eq!(c1.id, 1);
        assert_eq!(c2.id, 2);
        assert_eq!(c3.id, 3);
    }

    // --- Get ---

    #[test]
    fn test_get_by_id() {
        let storage = test_storage();
        let inserted = storage.insert(text_clip("find me")).unwrap();
        let found = storage.get_by_id(inserted.id).unwrap();
        assert_eq!(found.id, inserted.id);
        assert_eq!(found.text_content.as_deref(), Some("find me"));
    }

    #[test]
    fn test_get_by_id_not_found() {
        let storage = test_storage();
        let result = storage.get_by_id(999);
        assert!(matches!(result, Err(CbError::NotFound(_))));
    }

    // --- List ---

    #[test]
    fn test_list_empty() {
        let storage = test_storage();
        let clips = storage.list(ClipFilter::default()).unwrap();
        assert!(clips.is_empty());
    }

    #[test]
    fn test_list_returns_clips() {
        let storage = test_storage();
        storage.insert(text_clip("one")).unwrap();
        storage.insert(text_clip("two")).unwrap();
        let clips = storage.list(ClipFilter::default()).unwrap();
        assert_eq!(clips.len(), 2);
    }

    #[test]
    fn test_list_with_limit() {
        let storage = test_storage();
        for i in 0..5 {
            storage.insert(text_clip(&format!("clip {}", i))).unwrap();
        }
        let clips = storage.list(ClipFilter { limit: 3, ..Default::default() }).unwrap();
        assert_eq!(clips.len(), 3);
    }

    #[test]
    fn test_list_with_offset() {
        let storage = test_storage();
        for i in 0..5 {
            storage.insert(text_clip(&format!("clip {}", i))).unwrap();
        }
        let clips = storage.list(ClipFilter { limit: 2, offset: 3, ..Default::default() }).unwrap();
        assert_eq!(clips.len(), 2);
    }

    #[test]
    fn test_list_filter_by_type() {
        let storage = test_storage();
        storage.insert(text_clip("text")).unwrap();
        storage.insert(image_clip("/img.png", 100, 100)).unwrap();
        let clips = storage.list(ClipFilter {
            content_type: Some(ContentType::Text),
            ..Default::default()
        }).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].content_type, ContentType::Text);
    }

    #[test]
    fn test_list_filter_by_pinned() {
        let storage = test_storage();
        let c1 = storage.insert(text_clip("pinned")).unwrap();
        storage.insert(text_clip("not pinned")).unwrap();
        storage.set_pinned(c1.id, true).unwrap();
        let clips = storage.list(ClipFilter {
            pinned: Some(true),
            ..Default::default()
        }).unwrap();
        assert_eq!(clips.len(), 1);
        assert!(clips[0].pinned);
    }

    #[test]
    fn test_list_filter_by_tag() {
        let storage = test_storage();
        let c1 = storage.insert(text_clip("tagged")).unwrap();
        storage.insert(text_clip("untagged")).unwrap();
        storage.add_tag(c1.id, "important").unwrap();
        let clips = storage.list(ClipFilter {
            tag: Some("important".to_string()),
            ..Default::default()
        }).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].text_content.as_deref(), Some("tagged"));
    }

    #[test]
    fn test_list_order_desc() {
        let storage = test_storage();
        storage.insert(text_clip("first")).unwrap();
        storage.insert(text_clip("second")).unwrap();
        let clips = storage.list(ClipFilter::default()).unwrap();
        assert!(clips[0].id > clips[1].id);
    }

    // --- Search ---

    #[test]
    fn test_search_finds_match() {
        let storage = test_storage();
        storage.insert(text_clip("hello world")).unwrap();
        storage.insert(text_clip("goodbye world")).unwrap();
        let results = storage.search("hello", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text_content.as_deref(), Some("hello world"));
    }

    #[test]
    fn test_search_no_results() {
        let storage = test_storage();
        storage.insert(text_clip("hello")).unwrap();
        let results = storage.search("xyz", 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_case_insensitive() {
        let storage = test_storage();
        storage.insert(text_clip("Hello World")).unwrap();
        let results = storage.search("hello", 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_respects_limit() {
        let storage = test_storage();
        for i in 0..5 {
            storage.insert(text_clip(&format!("match {}", i))).unwrap();
        }
        let results = storage.search("match", 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    // --- Delete ---

    #[test]
    fn test_delete_existing() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("delete me")).unwrap();
        let deleted = storage.delete(clip.id).unwrap();
        assert!(deleted);
        assert!(matches!(storage.get_by_id(clip.id), Err(CbError::NotFound(_))));
    }

    #[test]
    fn test_delete_nonexistent() {
        let storage = test_storage();
        let deleted = storage.delete(999).unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_delete_cascades_tags() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("tagged")).unwrap();
        storage.add_tag(clip.id, "tag1").unwrap();
        storage.delete(clip.id).unwrap();
        let tag_count: i64 = storage
            .conn()
            .query_row("SELECT COUNT(*) FROM tags WHERE clip_id = ?", [clip.id], |row| row.get(0))
            .unwrap();
        assert_eq!(tag_count, 0);
    }

    // --- Hash ---

    #[test]
    fn test_find_by_hash_found() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("unique")).unwrap();
        let found = storage.find_by_hash(&clip.hash).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, clip.id);
    }

    #[test]
    fn test_find_by_hash_not_found() {
        let storage = test_storage();
        let found = storage.find_by_hash("nonexistent_hash").unwrap();
        assert!(found.is_none());
    }

    // --- Tags ---

    #[test]
    fn test_add_tag() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("taggable")).unwrap();
        storage.add_tag(clip.id, "work").unwrap();
        let fetched = storage.get_by_id(clip.id).unwrap();
        assert_eq!(fetched.tags, vec!["work"]);
    }

    #[test]
    fn test_add_multiple_tags() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("multi-tag")).unwrap();
        storage.add_tag(clip.id, "alpha").unwrap();
        storage.add_tag(clip.id, "beta").unwrap();
        let fetched = storage.get_by_id(clip.id).unwrap();
        let mut tags = fetched.tags.clone();
        tags.sort();
        assert_eq!(tags, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_add_duplicate_tag() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("dup tag")).unwrap();
        storage.add_tag(clip.id, "same").unwrap();
        storage.add_tag(clip.id, "same").unwrap();
        let fetched = storage.get_by_id(clip.id).unwrap();
        assert_eq!(fetched.tags, vec!["same"]);
    }

    #[test]
    fn test_remove_tag() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("removable tag")).unwrap();
        storage.add_tag(clip.id, "temp").unwrap();
        storage.remove_tag(clip.id, "temp").unwrap();
        let fetched = storage.get_by_id(clip.id).unwrap();
        assert!(fetched.tags.is_empty());
    }

    #[test]
    fn test_remove_nonexistent_tag() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("no tags")).unwrap();
        let result = storage.remove_tag(clip.id, "ghost");
        assert!(result.is_ok());
    }

    // --- Pin ---

    #[test]
    fn test_set_pinned_true() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("pin me")).unwrap();
        storage.set_pinned(clip.id, true).unwrap();
        let fetched = storage.get_by_id(clip.id).unwrap();
        assert!(fetched.pinned);
    }

    #[test]
    fn test_set_pinned_false() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("unpin me")).unwrap();
        storage.set_pinned(clip.id, true).unwrap();
        storage.set_pinned(clip.id, false).unwrap();
        let fetched = storage.get_by_id(clip.id).unwrap();
        assert!(!fetched.pinned);
    }

    // --- Clear ---

    #[test]
    fn test_clear_older_than() {
        let storage = test_storage();
        storage.insert(text_clip("old")).unwrap();
        storage.insert(text_clip("also old")).unwrap();
        let cutoff = Utc::now() + Duration::seconds(1);
        let removed = storage.clear_older_than(cutoff).unwrap();
        assert_eq!(removed, 2);
        let clips = storage.list(ClipFilter::default()).unwrap();
        assert!(clips.is_empty());
    }

    #[test]
    fn test_clear_older_than_skips_pinned() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("pinned old")).unwrap();
        storage.set_pinned(clip.id, true).unwrap();
        storage.insert(text_clip("unpinned old")).unwrap();
        let cutoff = Utc::now() + Duration::seconds(1);
        let removed = storage.clear_older_than(cutoff).unwrap();
        assert_eq!(removed, 1);
        let clips = storage.list(ClipFilter::default()).unwrap();
        assert_eq!(clips.len(), 1);
        assert!(clips[0].pinned);
    }

    // --- Stats ---

    #[test]
    fn test_stats_empty() {
        let storage = test_storage();
        let stats = storage.stats().unwrap();
        assert_eq!(stats.total_clips, 0);
        assert_eq!(stats.text_clips, 0);
        assert_eq!(stats.image_clips, 0);
        assert_eq!(stats.total_size, 0);
        assert!(stats.oldest.is_none());
        assert!(stats.newest.is_none());
    }

    #[test]
    fn test_stats_counts() {
        let storage = test_storage();
        storage.insert(text_clip("text1")).unwrap();
        storage.insert(text_clip("text2")).unwrap();
        storage.insert(image_clip("/img.png", 100, 100)).unwrap();
        let stats = storage.stats().unwrap();
        assert_eq!(stats.total_clips, 3);
        assert_eq!(stats.text_clips, 2);
        assert_eq!(stats.image_clips, 1);
        assert!(stats.oldest.is_some());
        assert!(stats.newest.is_some());
    }

    // --- Touch ---

    #[test]
    fn test_touch_updates_timestamp() {
        let storage = test_storage();
        let clip = storage.insert(text_clip("touch me")).unwrap();
        let original_updated = clip.updated_at;
        std::thread::sleep(std::time::Duration::from_millis(10));
        storage.touch(clip.id).unwrap();
        let fetched = storage.get_by_id(clip.id).unwrap();
        assert!(fetched.updated_at >= original_updated);
    }

    #[test]
    fn test_touch_nonexistent() {
        let storage = test_storage();
        let result = storage.touch(999);
        assert!(matches!(result, Err(CbError::NotFound(_))));
    }
}
