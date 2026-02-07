use std::fs;
use std::path::Path;

use arboard::Clipboard;
use image::ImageEncoder;

use crate::errors::{CbError, Result};
use crate::hash::hash_content;
use crate::storage::models::{ContentType, NewClip};

pub struct ClipboardContent {
    pub content_type: ContentType,
    pub text: Option<String>,
    pub image_data: Option<Vec<u8>>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub hash: String,
    pub size_bytes: i64,
}

pub fn read_clipboard() -> Result<Option<ClipboardContent>> {
    let mut cb = Clipboard::new().map_err(|e| CbError::Clipboard(e.to_string()))?;

    if let Ok(text) = cb.get_text()
        && !text.is_empty()
    {
        let hash = hash_content(text.as_bytes());
        let size = text.len() as i64;
        return Ok(Some(ClipboardContent {
            content_type: ContentType::Text,
            text: Some(text),
            image_data: None,
            width: None,
            height: None,
            hash,
            size_bytes: size,
        }));
    }

    if let Ok(img) = cb.get_image() {
        let hash = hash_content(&img.bytes);
        let size = img.bytes.len() as i64;
        return Ok(Some(ClipboardContent {
            content_type: ContentType::Image,
            text: None,
            image_data: Some(img.bytes.into_owned()),
            width: Some(img.width as i32),
            height: Some(img.height as i32),
            hash,
            size_bytes: size,
        }));
    }

    Ok(None)
}

pub fn write_text_to_clipboard(text: &str) -> Result<()> {
    let mut cb = Clipboard::new().map_err(|e| CbError::Clipboard(e.to_string()))?;
    cb.set_text(text).map_err(|e| CbError::Clipboard(e.to_string()))
}

pub fn write_image_to_clipboard(path: &Path) -> Result<()> {
    let img = image::open(path).map_err(|e| CbError::Image(e.to_string()))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut cb = Clipboard::new().map_err(|e| CbError::Clipboard(e.to_string()))?;
    let img_data = arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: rgba.into_raw().into(),
    };
    cb.set_image(img_data)
        .map_err(|e| CbError::Clipboard(e.to_string()))
}

pub fn save_image_to_file(data: &[u8], width: u32, height: u32, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CbError::Image(e.to_string()))?;
    }
    let file = fs::File::create(path).map_err(|e| CbError::Image(e.to_string()))?;
    let encoder = image::codecs::png::PngEncoder::new(file);
    encoder
        .write_image(data, width, height, image::ColorType::Rgba8.into())
        .map_err(|e| CbError::Image(e.to_string()))
}

pub fn clipboard_content_to_new_clip(
    content: ClipboardContent,
    image_path: Option<String>,
) -> NewClip {
    NewClip {
        content_type: content.content_type,
        text_content: content.text,
        image_path,
        image_width: content.width,
        image_height: content.height,
        hash: content.hash,
        size_bytes: content.size_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_clipboard_content_to_new_clip_text() {
        let content = ClipboardContent {
            content_type: ContentType::Text,
            text: Some("hello".to_string()),
            image_data: None,
            width: None,
            height: None,
            hash: "abc123".to_string(),
            size_bytes: 5,
        };
        let clip = clipboard_content_to_new_clip(content, None);
        assert_eq!(clip.content_type, ContentType::Text);
        assert_eq!(clip.text_content.as_deref(), Some("hello"));
        assert_eq!(clip.hash, "abc123");
        assert!(clip.image_path.is_none());
    }

    #[test]
    fn test_clipboard_content_to_new_clip_image() {
        let content = ClipboardContent {
            content_type: ContentType::Image,
            text: None,
            image_data: Some(vec![0u8; 100]),
            width: Some(10),
            height: Some(10),
            hash: "img_hash".to_string(),
            size_bytes: 100,
        };
        let clip = clipboard_content_to_new_clip(content, Some("/images/test.png".to_string()));
        assert_eq!(clip.content_type, ContentType::Image);
        assert_eq!(clip.image_path.as_deref(), Some("/images/test.png"));
        assert_eq!(clip.image_width, Some(10));
    }

    #[test]
    fn test_save_image_to_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("subdir/test.png");
        // 2x2 RGBA image (16 bytes)
        let data = vec![255u8; 16];
        save_image_to_file(&data, 2, 2, &path).unwrap();
        assert!(path.exists());
    }
}
