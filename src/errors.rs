use thiserror::Error;

#[derive(Debug, Error)]
pub enum CbError {
    #[error("Storage error: {0}")]
    Storage(#[from] rusqlite::Error),

    #[error("Clipboard error: {0}")]
    Clipboard(String),

    #[error("Image error: {0}")]
    Image(String),

    #[error("Daemon error: {0}")]
    Daemon(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, CbError>;
