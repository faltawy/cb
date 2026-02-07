use sha2::{Digest, Sha256};

pub fn hash_content(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_deterministic() {
        let h1 = hash_content(b"hello");
        let h2 = hash_content(b"hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_different_inputs() {
        let h1 = hash_content(b"hello");
        let h2 = hash_content(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_is_hex_sha256() {
        let h = hash_content(b"hello");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
