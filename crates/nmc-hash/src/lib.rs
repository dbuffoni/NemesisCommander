//! # nmc-hash
//!
//! Streaming checksums for NemesisCommander.
//!
//! [`Hasher`] wraps the supported algorithms behind one type so callers can
//! pick an algorithm at run time (from configuration or from the CLI) and
//! feed data in chunks, which keeps memory use constant on huge files.

use std::io::Read;
use std::path::Path;

use nmc_vfs::HashAlgorithm;
use sha2::Digest;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Size of the read buffer used by the helpers in this crate.
pub const CHUNK_SIZE: usize = 256 * 1024;

/// An incremental hasher for any [`HashAlgorithm`].
pub enum Hasher {
    /// BLAKE3 state.
    Blake3(Box<blake3::Hasher>),
    /// XXH3-64 state.
    Xxh3(Box<xxhash_rust::xxh3::Xxh3>),
    /// SHA-256 state.
    Sha256(Box<sha2::Sha256>),
}

impl Hasher {
    /// A fresh hasher for `algorithm`.
    pub fn new(algorithm: HashAlgorithm) -> Self {
        match algorithm {
            HashAlgorithm::Blake3 => Hasher::Blake3(Box::default()),
            HashAlgorithm::Xxh3 => Hasher::Xxh3(Box::default()),
            HashAlgorithm::Sha256 => Hasher::Sha256(Box::default()),
        }
    }

    /// Feeds more data.
    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Blake3(h) => {
                h.update(data);
            }
            Hasher::Xxh3(h) => h.update(data),
            Hasher::Sha256(h) => h.update(data),
        }
    }

    /// Consumes the hasher and returns the lowercase hex digest.
    pub fn finish_hex(self) -> String {
        match self {
            Hasher::Blake3(h) => h.finalize().to_hex().to_string(),
            Hasher::Xxh3(h) => format!("{:016x}", h.digest()),
            Hasher::Sha256(h) => hex::encode(h.finalize()),
        }
    }
}

/// Parses an algorithm name as typed by users (`blake3`, `xxh3`, `sha256`).
pub fn parse_algorithm(name: &str) -> Option<HashAlgorithm> {
    match name.to_ascii_lowercase().replace('-', "").as_str() {
        "blake3" | "b3" => Some(HashAlgorithm::Blake3),
        "xxh3" | "xxhash" | "xxh364" => Some(HashAlgorithm::Xxh3),
        "sha256" => Some(HashAlgorithm::Sha256),
        _ => None,
    }
}

/// The canonical lowercase name of an algorithm.
pub fn algorithm_name(algorithm: HashAlgorithm) -> &'static str {
    match algorithm {
        HashAlgorithm::Blake3 => "blake3",
        HashAlgorithm::Xxh3 => "xxh3",
        HashAlgorithm::Sha256 => "sha256",
    }
}

/// Hashes everything an async reader produces.
pub async fn hash_reader<R: AsyncRead + Unpin + ?Sized>(
    reader: &mut R,
    algorithm: HashAlgorithm,
) -> std::io::Result<String> {
    let mut hasher = Hasher::new(algorithm);
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finish_hex())
}

/// Hashes a local file with blocking I/O. Call it from a blocking thread
/// (`tokio::task::spawn_blocking` or rayon), never from the async runtime.
///
/// BLAKE3 uses its memory-mapped, multi-threaded mode for large files.
pub fn hash_local_file(path: &Path, algorithm: HashAlgorithm) -> std::io::Result<String> {
    // Memory mapping only suits regular files (a pipe or device cannot be mapped).
    if algorithm == HashAlgorithm::Blake3 && std::fs::metadata(path)?.is_file() {
        let mut hasher = blake3::Hasher::new();
        hasher.update_mmap_rayon(path)?;
        return Ok(hasher.finalize().to_hex().to_string());
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Hasher::new(algorithm);
    let mut buf = vec![0u8; CHUNK_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finish_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [HashAlgorithm; 3] =
        [HashAlgorithm::Blake3, HashAlgorithm::Xxh3, HashAlgorithm::Sha256];

    #[test]
    fn known_vectors_for_abc() {
        let digest = |a| {
            let mut h = Hasher::new(a);
            h.update(b"abc");
            h.finish_hex()
        };
        assert_eq!(
            digest(HashAlgorithm::Sha256),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            digest(HashAlgorithm::Blake3),
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
        assert_eq!(digest(HashAlgorithm::Xxh3), "78af5f94892f3950");
    }

    #[test]
    fn chunked_equals_one_shot() {
        let data: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
        for algorithm in ALL {
            let mut one = Hasher::new(algorithm);
            one.update(&data);
            let mut chunked = Hasher::new(algorithm);
            for chunk in data.chunks(7919) {
                chunked.update(chunk);
            }
            assert_eq!(one.finish_hex(), chunked.finish_hex(), "{algorithm:?}");
        }
    }

    #[tokio::test]
    async fn file_reader_and_blocking_agree() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        std::fs::write(&path, vec![42u8; 3 * CHUNK_SIZE + 17]).unwrap();
        for algorithm in ALL {
            let blocking = hash_local_file(&path, algorithm).unwrap();
            let mut file = tokio::fs::File::open(&path).await.unwrap();
            let streamed = hash_reader(&mut file, algorithm).await.unwrap();
            assert_eq!(blocking, streamed, "{algorithm:?}");
        }
    }

    #[test]
    fn algorithm_names_roundtrip() {
        for algorithm in ALL {
            assert_eq!(parse_algorithm(algorithm_name(algorithm)), Some(algorithm));
        }
        assert_eq!(parse_algorithm("SHA-256"), Some(HashAlgorithm::Sha256));
        assert_eq!(parse_algorithm("md5"), None);
    }
}
