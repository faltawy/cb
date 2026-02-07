use std::path::PathBuf;

pub struct AppPaths {
    pub base_dir: PathBuf,
    pub db_path: PathBuf,
    pub images_dir: PathBuf,
    pub pid_file: PathBuf,
    pub log_file: PathBuf,
}

impl Default for AppPaths {
    fn default() -> Self {
        Self::new()
    }
}

impl AppPaths {
    pub fn new() -> Self {
        let base = dirs::home_dir()
            .expect("Could not determine home directory")
            .join(".cb");
        Self::from_base(base)
    }

    pub fn from_base(base: PathBuf) -> Self {
        Self {
            db_path: base.join("cb.db"),
            images_dir: base.join("images"),
            pid_file: base.join("cb.pid"),
            log_file: base.join("cb.log"),
            base_dir: base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_base() {
        let paths = AppPaths::from_base(PathBuf::from("/tmp/test-cb"));
        assert_eq!(paths.base_dir, PathBuf::from("/tmp/test-cb"));
        assert_eq!(paths.db_path, PathBuf::from("/tmp/test-cb/cb.db"));
        assert_eq!(paths.images_dir, PathBuf::from("/tmp/test-cb/images"));
        assert_eq!(paths.pid_file, PathBuf::from("/tmp/test-cb/cb.pid"));
        assert_eq!(paths.log_file, PathBuf::from("/tmp/test-cb/cb.log"));
    }

    #[test]
    fn test_new_uses_home_dir() {
        let paths = AppPaths::new();
        assert!(paths.base_dir.ends_with(".cb"));
    }
}
