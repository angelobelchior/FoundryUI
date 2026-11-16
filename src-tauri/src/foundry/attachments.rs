use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;

use super::db::Database;
use super::error::{FoundryError, FoundryResult};
use super::jobs::now_millis;

pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_IMAGES_PER_MESSAGE: usize = 4;

static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedAttachment {
    pub token: String,
    pub mime: String,
    pub bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct StagedFile {
    pub meta: StagedAttachment,
    pub path: PathBuf,
}

pub struct AttachmentStore {
    root: PathBuf,
    staged: Mutex<HashMap<String, StagedFile>>,
}

impl AttachmentStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            staged: Mutex::new(HashMap::new()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store(&self, data_url: &str) -> FoundryResult<StagedAttachment> {
        let (declared, bytes) = decode_data_url(data_url)?;

        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(rejected(format!(
                "the image is larger than {} MB",
                MAX_IMAGE_BYTES / (1024 * 1024)
            )));
        }

        let Some((mime, extension)) = sniff_image(&bytes) else {
            return Err(rejected(format!(
                "the file is not a supported image ({declared})"
            )));
        };
        let token = format!(
            "att-{}-{}",
            now_millis(),
            TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = self.root.join(format!("{token}.{extension}"));

        std::fs::create_dir_all(&self.root).map_err(storage)?;
        std::fs::write(&path, &bytes).map_err(storage)?;

        let (width, height) = image_size(&bytes);
        let meta = StagedAttachment {
            token: token.clone(),
            mime: mime.to_string(),
            bytes: bytes.len() as u64,
            width,
            height,
        };

        lock(&self.staged).insert(
            token,
            StagedFile {
                meta: meta.clone(),
                path,
            },
        );

        Ok(meta)
    }

    pub fn discard(&self, tokens: &[String]) {
        for token in tokens {
            if let Some(staged) = lock(&self.staged).remove(token) {
                let _ = std::fs::remove_file(staged.path);
            }
        }
    }

    pub fn remove_paths(&self, paths: &[String]) {
        for path in paths {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn take(&self, tokens: &[String]) -> FoundryResult<Vec<StagedFile>> {
        if tokens.len() > MAX_IMAGES_PER_MESSAGE {
            return Err(rejected(format!(
                "a message accepts at most {MAX_IMAGES_PER_MESSAGE} images"
            )));
        }

        let mut unique: Vec<&String> = Vec::new();

        for token in tokens {
            if !unique.contains(&token) {
                unique.push(token);
            }
        }

        let mut staged = lock(&self.staged);
        let mut files = Vec::new();

        for token in unique {
            let Some(file) = staged.remove(token) else {
                return Err(rejected("the image is no longer available"));
            };

            files.push(file);
        }

        Ok(files)
    }

    pub fn sweep(&self, database: &Database) -> FoundryResult<u64> {
        if !self.root.is_dir() {
            return Ok(0);
        }

        let referenced = database.attachment_paths()?;
        let staged: HashSet<String> = lock(&self.staged)
            .values()
            .map(|file| file.path.to_string_lossy().to_string())
            .collect();
        let mut removed = 0;

        for entry in std::fs::read_dir(&self.root).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            let path = entry.path();
            let path = path.to_string_lossy().to_string();

            if !Path::new(&path).is_file() || referenced.contains(&path) || staged.contains(&path) {
                continue;
            }

            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }

        Ok(removed)
    }
}

pub fn file_data_url(path: &Path, mime: &str) -> FoundryResult<String> {
    let bytes = std::fs::read(path).map_err(storage)?;

    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn decode_data_url(input: &str) -> FoundryResult<(String, Vec<u8>)> {
    let trimmed = input.trim();
    let Some(rest) = trimmed.strip_prefix("data:") else {
        return Err(rejected("the image must be sent as a data URL"));
    };
    let Some((meta, payload)) = rest.split_once(',') else {
        return Err(rejected("the data URL is malformed"));
    };
    let Some(declared) = meta.strip_suffix(";base64") else {
        return Err(rejected("the data URL is not base64 encoded"));
    };
    let declared = declared.split(';').next().unwrap_or_default();

    if !declared.starts_with("image/") {
        return Err(rejected("only images can be attached to a message"));
    }

    let bytes = STANDARD
        .decode(payload.trim())
        .map_err(|_| rejected("the image data is not valid base64"))?;

    if bytes.is_empty() {
        return Err(rejected("the image is empty"));
    }

    Ok((declared.to_string(), bytes))
}

fn sniff_image(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    if bytes.starts_with(PNG) {
        return Some(("image/png", "png"));
    }

    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(("image/jpeg", "jpg"));
    }

    if bytes.starts_with(b"GIF8") {
        return Some(("image/gif", "gif"));
    }

    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(("image/webp", "webp"));
    }

    None
}

fn image_size(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return png_size(bytes);
    }

    if bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg_size(bytes);
    }

    if bytes.starts_with(b"GIF8") {
        return gif_size(bytes);
    }

    (None, None)
}

fn png_size(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return (None, None);
    }

    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);

    (Some(width), Some(height))
}

fn gif_size(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if bytes.len() < 10 {
        return (None, None);
    }

    let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
    let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;

    (Some(width), Some(height))
}

fn jpeg_size(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    let mut index = 2;

    while index + 9 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }

        let marker = bytes[index + 1];
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;

        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]) as u32;

            return (Some(width), Some(height));
        }

        if length < 2 {
            return (None, None);
        }

        index += 2 + length;
    }

    (None, None)
}

fn rejected(reason: impl Into<String>) -> FoundryError {
    FoundryError::AttachmentRejected {
        reason: reason.into(),
    }
}

fn storage(error: impl std::fmt::Display) -> FoundryError {
    FoundryError::Storage {
        detail: error.to_string(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        bytes.extend_from_slice(&[0xFF, 0xD9]);
        bytes
    }

    fn store() -> (tempfile::TempDir, AttachmentStore) {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = AttachmentStore::new(directory.path().join("attachments"));

        (directory, store)
    }

    #[test]
    fn stores_png_images_with_size_and_dimensions() {
        let (_directory, store) = store();
        let staged = store.store(PIXEL).expect("stored");

        assert_eq!(staged.mime, "image/png");
        assert_eq!(staged.width, Some(1));
        assert_eq!(staged.height, Some(1));
        assert!(staged.bytes > 0);
        assert!(
            std::fs::read_dir(store.root())
                .expect("dir")
                .next()
                .is_some()
        );

        let files = store
            .take(std::slice::from_ref(&staged.token))
            .expect("taken");

        assert_eq!(files.len(), 1);
        assert!(files[0].path.is_file());
        assert!(store.take(&[staged.token]).is_err());
    }

    #[test]
    fn reads_jpeg_dimensions_from_the_frame_header() {
        let bytes = jpeg(640, 480);
        let (width, height) = image_size(&bytes);

        assert_eq!((width, height), (Some(640), Some(480)));
    }

    #[test]
    fn rejects_files_that_are_not_images() {
        let (_directory, store) = store();
        let text = format!("data:text/plain;base64,{}", STANDARD.encode(b"hello"));
        let error = store.store(&text).expect_err("text is rejected");

        assert_eq!(error.code(), "AttachmentRejected");
        assert!(
            store
                .store("data:image/png;base64,bm90IGFuIGltYWdl")
                .is_err()
        );
    }

    #[test]
    fn rejects_malformed_data_urls() {
        let (_directory, store) = store();

        assert!(store.store("image/png;base64,AAAA").is_err());
        assert!(store.store("data:image/png,AAAA").is_err());
        assert!(store.store("data:image/png;base64,").is_err());
    }

    #[test]
    fn limits_the_number_of_images_per_message() {
        let (_directory, store) = store();
        let tokens: Vec<String> = (0..=MAX_IMAGES_PER_MESSAGE)
            .map(|_| store.store(PIXEL).expect("stored").token)
            .collect();
        let error = store.take(&tokens).expect_err("too many images");

        assert_eq!(error.code(), "AttachmentRejected");
        store.discard(&tokens);
    }

    #[test]
    fn discarding_removes_the_staged_file() {
        let (_directory, store) = store();
        let staged = store.store(PIXEL).expect("stored");

        store.discard(std::slice::from_ref(&staged.token));

        assert!(store.take(&[staged.token]).is_err());
        assert_eq!(std::fs::read_dir(store.root()).expect("dir").count(), 0);
    }

    #[test]
    fn sweeping_removes_files_the_database_does_not_reference() {
        let (_directory, store) = store();
        let database = Database::open_in_memory().expect("database");
        let staged = store.store(PIXEL).expect("stored");
        let orphan = store.root().join("orphan.png");

        std::fs::write(&orphan, b"leftover").expect("orphan");

        let removed = store.sweep(&database).expect("sweep");

        assert_eq!(removed, 1);
        assert!(!orphan.exists());
        assert!(store.root().join(format!("{}.png", staged.token)).exists());
    }

    #[test]
    fn data_urls_round_trip_the_stored_image() {
        let (_directory, store) = store();
        let staged = store.store(PIXEL).expect("stored");
        let files = store.take(&[staged.token]).expect("taken");
        let url = file_data_url(&files[0].path, &files[0].meta.mime).expect("url");

        assert_eq!(url, PIXEL);
    }
}
