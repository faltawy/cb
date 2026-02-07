pub const CREATE_CLIPS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS clips (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        content_type TEXT NOT NULL,
        text_content TEXT,
        image_path TEXT,
        image_width INTEGER,
        image_height INTEGER,
        hash TEXT NOT NULL UNIQUE,
        size_bytes INTEGER NOT NULL,
        pinned INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )
";

pub const CREATE_TAGS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS tags (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        clip_id INTEGER NOT NULL,
        tag TEXT NOT NULL,
        FOREIGN KEY (clip_id) REFERENCES clips(id) ON DELETE CASCADE,
        UNIQUE(clip_id, tag)
    )
";

pub const CREATE_INDEX_HASH: &str =
    "CREATE INDEX IF NOT EXISTS idx_clips_hash ON clips(hash)";

pub const CREATE_INDEX_CREATED_AT: &str =
    "CREATE INDEX IF NOT EXISTS idx_clips_created_at ON clips(created_at)";

pub const CREATE_INDEX_TAG: &str =
    "CREATE INDEX IF NOT EXISTS idx_tags_tag ON tags(tag)";

pub const CREATE_INDEX_CLIP_ID: &str =
    "CREATE INDEX IF NOT EXISTS idx_tags_clip_id ON tags(clip_id)";
