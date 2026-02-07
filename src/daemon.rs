use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rusqlite::Connection;

use crate::clipboard::{clipboard_content_to_new_clip, read_clipboard, save_image_to_file};
use crate::config::AppPaths;
use crate::errors::{CbError, Result};
use crate::storage::sqlite::SqliteStorage;
use crate::storage::ClipStorage;

const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub fn write_pid_file(path: &Path) -> Result<()> {
    let pid = std::process::id();
    fs::write(path, pid.to_string()).map_err(|e| CbError::Daemon(e.to_string()))
}

pub fn read_pid_file(path: &Path) -> Result<Option<u32>> {
    match fs::read_to_string(path) {
        Ok(contents) => match contents.trim().parse::<u32>() {
            Ok(pid) => Ok(Some(pid)),
            Err(_) => Ok(None),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CbError::Daemon(e.to_string())),
    }
}

pub fn remove_pid_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CbError::Daemon(e.to_string())),
    }
}

pub fn is_process_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

pub fn stop_daemon(paths: &AppPaths) -> Result<bool> {
    match read_pid_file(&paths.pid_file)? {
        Some(pid) if is_process_running(pid) => {
            unsafe { libc::kill(pid as i32, libc::SIGTERM); }
            remove_pid_file(&paths.pid_file)?;
            Ok(true)
        }
        Some(_) => {
            remove_pid_file(&paths.pid_file)?;
            Ok(false)
        }
        None => Ok(false),
    }
}

pub fn daemon_status(paths: &AppPaths) -> Result<Option<u32>> {
    match read_pid_file(&paths.pid_file)? {
        Some(pid) if is_process_running(pid) => Ok(Some(pid)),
        Some(_) => {
            remove_pid_file(&paths.pid_file)?;
            Ok(None)
        }
        None => Ok(None),
    }
}

pub fn run_watcher(paths: &AppPaths) -> Result<()> {
    fs::create_dir_all(&paths.base_dir).map_err(|e| CbError::Daemon(e.to_string()))?;
    fs::create_dir_all(&paths.images_dir).map_err(|e| CbError::Daemon(e.to_string()))?;

    write_pid_file(&paths.pid_file)?;

    let conn = Connection::open(&paths.db_path).map_err(CbError::Storage)?;
    let storage = SqliteStorage::new(conn)?;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc_handler(r);

    let mut last_hash: Option<String> = None;

    eprintln!("cb: watching clipboard (pid {})", std::process::id());

    while running.load(Ordering::Relaxed) {
        if let Err(e) = poll_once(&storage, paths, &mut last_hash) {
            eprintln!("cb: poll error: {}", e);
        }
        thread::sleep(POLL_INTERVAL);
    }

    eprintln!("cb: shutting down");
    remove_pid_file(&paths.pid_file)?;
    Ok(())
}

fn ctrlc_handler(running: Arc<AtomicBool>) {
    let _ = ctrlc::set_handler(move || {
        running.store(false, Ordering::Relaxed);
    });
}

fn poll_once(
    storage: &SqliteStorage,
    paths: &AppPaths,
    last_hash: &mut Option<String>,
) -> Result<()> {
    let content = match read_clipboard()? {
        Some(c) => c,
        None => return Ok(()),
    };

    if last_hash.as_deref() == Some(&content.hash) {
        return Ok(());
    }

    let new_hash = content.hash.clone();

    if storage.find_by_hash(&content.hash)?.is_some() {
        *last_hash = Some(new_hash);
        return Ok(());
    }

    let image_path = if content.image_data.is_some() {
        let filename = format!("{}.png", &content.hash[..16]);
        let full_path = paths.images_dir.join(&filename);
        save_image_to_file(
            content.image_data.as_ref().unwrap(),
            content.width.unwrap() as u32,
            content.height.unwrap() as u32,
            &full_path,
        )?;
        Some(full_path.to_string_lossy().to_string())
    } else {
        None
    };

    let new_clip = clipboard_content_to_new_clip(content, image_path);
    storage.insert(new_clip)?;

    *last_hash = Some(new_hash);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_write_and_read_pid_file() {
        let dir = TempDir::new().unwrap();
        let pid_path = dir.path().join("test.pid");
        write_pid_file(&pid_path).unwrap();
        let pid = read_pid_file(&pid_path).unwrap();
        assert_eq!(pid, Some(std::process::id()));
    }

    #[test]
    fn test_read_missing_pid_file() {
        let dir = TempDir::new().unwrap();
        let pid_path = dir.path().join("nonexistent.pid");
        let pid = read_pid_file(&pid_path).unwrap();
        assert!(pid.is_none());
    }

    #[test]
    fn test_remove_pid_file() {
        let dir = TempDir::new().unwrap();
        let pid_path = dir.path().join("test.pid");
        write_pid_file(&pid_path).unwrap();
        remove_pid_file(&pid_path).unwrap();
        assert!(!pid_path.exists());
    }

    #[test]
    fn test_remove_missing_pid_file_ok() {
        let dir = TempDir::new().unwrap();
        let pid_path = dir.path().join("nonexistent.pid");
        let result = remove_pid_file(&pid_path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_is_process_running_self() {
        assert!(is_process_running(std::process::id()));
    }

    #[test]
    fn test_is_process_running_invalid() {
        assert!(!is_process_running(99999));
    }

    #[test]
    fn test_daemon_status_not_running() {
        let dir = TempDir::new().unwrap();
        let paths = AppPaths::from_base(dir.path().to_path_buf());
        let status = daemon_status(&paths).unwrap();
        assert!(status.is_none());
    }

    #[test]
    fn test_daemon_status_stale_pid() {
        let dir = TempDir::new().unwrap();
        let paths = AppPaths::from_base(dir.path().to_path_buf());
        fs::write(&paths.pid_file, "99999").unwrap();
        let status = daemon_status(&paths).unwrap();
        assert!(status.is_none());
        assert!(!paths.pid_file.exists());
    }
}
