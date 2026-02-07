use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq)]
pub enum ContentType {
    Text,
    Image,
    FileRef,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::Text => "text",
            ContentType::Image => "image",
            ContentType::FileRef => "fileref",
        }
    }

    pub fn parse(s: &str) -> Option<ContentType> {
        match s {
            "text" => Some(ContentType::Text),
            "image" => Some(ContentType::Image),
            "fileref" => Some(ContentType::FileRef),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Clip {
    pub id: i64,
    pub content_type: ContentType,
    pub text_content: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<i32>,
    pub image_height: Option<i32>,
    pub hash: String,
    pub size_bytes: i64,
    pub pinned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NewClip {
    pub content_type: ContentType,
    pub text_content: Option<String>,
    pub image_path: Option<String>,
    pub image_width: Option<i32>,
    pub image_height: Option<i32>,
    pub hash: String,
    pub size_bytes: i64,
}

#[derive(Debug)]
pub struct StorageStats {
    pub total_clips: i64,
    pub text_clips: i64,
    pub image_clips: i64,
    pub fileref_clips: i64,
    pub total_size: i64,
    pub oldest: Option<DateTime<Utc>>,
    pub newest: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
pub struct ClipFilter {
    pub content_type: Option<ContentType>,
    pub pinned: Option<bool>,
    pub tag: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

impl ClipFilter {
    pub fn effective_limit(&self) -> i64 {
        if self.limit <= 0 { 50 } else { self.limit }
    }
}
