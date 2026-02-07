use chrono::{DateTime, Utc};
use serde::Serialize;

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

impl Serialize for ContentType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Serialize)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn content_type_serializes_as_string() {
        assert_eq!(serde_json::to_string(&ContentType::Text).unwrap(), "\"text\"");
        assert_eq!(serde_json::to_string(&ContentType::Image).unwrap(), "\"image\"");
        assert_eq!(serde_json::to_string(&ContentType::FileRef).unwrap(), "\"fileref\"");
    }

    #[test]
    fn clip_serializes_to_json() {
        let clip = Clip {
            id: 1,
            content_type: ContentType::Text,
            text_content: Some("hello".into()),
            image_path: None,
            image_width: None,
            image_height: None,
            hash: "abc123".into(),
            size_bytes: 5,
            pinned: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            tags: vec!["test".into()],
        };
        let json: serde_json::Value = serde_json::to_value(&clip).unwrap();
        assert_eq!(json["id"], 1);
        assert_eq!(json["content_type"], "text");
        assert_eq!(json["text_content"], "hello");
        assert_eq!(json["pinned"], false);
        assert_eq!(json["tags"], serde_json::json!(["test"]));
    }

    #[test]
    fn storage_stats_serializes() {
        let stats = StorageStats {
            total_clips: 10,
            text_clips: 7,
            image_clips: 2,
            fileref_clips: 1,
            total_size: 4096,
            oldest: None,
            newest: None,
        };
        let json: serde_json::Value = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_clips"], 10);
        assert_eq!(json["oldest"], serde_json::Value::Null);
    }

    #[test]
    fn empty_clip_vec_serializes() {
        let clips: Vec<Clip> = vec![];
        assert_eq!(serde_json::to_string(&clips).unwrap(), "[]");
    }
}
