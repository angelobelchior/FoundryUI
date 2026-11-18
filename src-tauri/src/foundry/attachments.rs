use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;

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
    pub data: Vec<u8>,
}

#[derive(Default)]
pub struct AttachmentStore {
    staged: Mutex<HashMap<String, StagedFile>>,
}

impl AttachmentStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store(&self, data_url: &str) -> FoundryResult<StagedAttachment> {
        let (declared, data) = decode_data_url(data_url)?;

        if data.len() > MAX_IMAGE_BYTES {
            return Err(rejected(format!(
                "the image is larger than {} MB",
                MAX_IMAGE_BYTES / (1024 * 1024)
            )));
        }

        let Some(mime) = sniff_image(&data) else {
            return Err(rejected(format!(
                "the file is not a supported image ({declared})"
            )));
        };
        let token = format!(
            "att-{}-{}",
            now_millis(),
            TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let (width, height) = image_size(&data);
        let meta = StagedAttachment {
            token: token.clone(),
            mime: mime.to_string(),
            bytes: data.len() as u64,
            width,
            height,
        };

        lock(&self.staged).insert(
            token,
            StagedFile {
                meta: meta.clone(),
                data,
            },
        );

        Ok(meta)
    }

    pub fn discard(&self, tokens: &[String]) {
        let mut staged = lock(&self.staged);

        for token in tokens {
            staged.remove(token);
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

        if unique.iter().any(|token| !staged.contains_key(*token)) {
            return Err(rejected("the image is no longer available"));
        }

        let mut files = Vec::new();

        for token in unique {
            let Some(file) = staged.remove(token) else {
                return Err(rejected("the image is no longer available"));
            };

            files.push(file);
        }

        Ok(files)
    }
}

pub fn image_data_url(data: &[u8], mime: &str) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(data))
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

fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    if bytes.starts_with(PNG) {
        return Some("image/png");
    }

    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }

    if bytes.starts_with(b"GIF8") {
        return Some("image/gif");
    }

    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
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

    #[test]
    fn stores_png_images_with_owned_bytes_size_and_dimensions() {
        let store = AttachmentStore::new();
        let mut input = PIXEL.to_owned();
        let staged = store.store(&input).expect("stored");
        input.clear();

        assert_eq!(staged.mime, "image/png");
        assert_eq!(staged.width, Some(1));
        assert_eq!(staged.height, Some(1));
        assert_eq!(staged.bytes, 70);

        let files = store
            .take(std::slice::from_ref(&staged.token))
            .expect("taken");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].meta.token, staged.token);
        assert_eq!(&files[0].data[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(image_data_url(&files[0].data, &files[0].meta.mime), PIXEL);
        assert!(store.take(&[staged.token]).is_err());
    }

    #[test]
    fn default_store_stages_images() {
        let store = AttachmentStore::default();
        let staged = store.store(PIXEL).expect("stored");

        assert_eq!(store.take(&[staged.token]).expect("taken").len(), 1);
    }

    #[test]
    fn reads_jpeg_dimensions_from_staged_bytes() {
        let store = AttachmentStore::new();
        let data = jpeg(640, 480);
        let staged = store
            .store(&image_data_url(&data, "image/jpeg"))
            .expect("stored");

        assert_eq!(staged.mime, "image/jpeg");
        assert_eq!((staged.width, staged.height), (Some(640), Some(480)));
        assert_eq!(store.take(&[staged.token]).expect("taken")[0].data, data);
    }

    #[test]
    fn rejects_files_that_are_not_images() {
        let store = AttachmentStore::new();
        let text = image_data_url(b"hello", "text/plain");
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
        let store = AttachmentStore::new();

        for input in [
            "image/png;base64,AAAA",
            "data:image/png,AAAA",
            "data:image/png;base64,",
            "data:image/png;base64,???",
            "data:image/png;base64",
        ] {
            assert_eq!(
                store.store(input).expect_err("rejected").code(),
                "AttachmentRejected"
            );
        }
    }

    #[test]
    fn limits_the_number_of_images_without_consuming_staged_bytes() {
        let store = AttachmentStore::new();
        let tokens: Vec<String> = (0..=MAX_IMAGES_PER_MESSAGE)
            .map(|_| store.store(PIXEL).expect("stored").token)
            .collect();
        let error = store.take(&tokens).expect_err("too many images");

        assert_eq!(error.code(), "AttachmentRejected");
        assert_eq!(
            store
                .take(&tokens[..MAX_IMAGES_PER_MESSAGE])
                .expect("first four")
                .len(),
            MAX_IMAGES_PER_MESSAGE
        );
        assert_eq!(
            store
                .take(&tokens[MAX_IMAGES_PER_MESSAGE..])
                .expect("last image")
                .len(),
            1
        );
    }

    #[test]
    fn discarding_makes_only_selected_tokens_unavailable() {
        let store = AttachmentStore::new();
        let discarded = store.store(PIXEL).expect("stored");
        let retained = store.store(PIXEL).expect("stored");

        store.discard(&[
            discarded.token.clone(),
            discarded.token.clone(),
            "missing".to_owned(),
        ]);

        assert!(store.take(&[discarded.token]).is_err());
        assert_eq!(store.take(&[retained.token]).expect("retained").len(), 1);
    }

    #[test]
    fn taking_deduplicates_tokens_and_preserves_requested_order() {
        let store = AttachmentStore::new();
        let first = store.store(PIXEL).expect("stored");
        let second = store.store(PIXEL).expect("stored");
        let files = store
            .take(&[
                second.token.clone(),
                first.token.clone(),
                second.token.clone(),
            ])
            .expect("taken");

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].meta.token, second.token);
        assert_eq!(files[1].meta.token, first.token);
        assert!(store.take(&[first.token]).is_err());
        assert!(store.take(&[second.token]).is_err());
    }

    #[test]
    fn invalid_take_preserves_every_valid_token() {
        let store = AttachmentStore::new();
        let first = store.store(PIXEL).expect("stored");
        let second = store.store(PIXEL).expect("stored");

        let error = store
            .take(&[
                first.token.clone(),
                "missing".to_owned(),
                second.token.clone(),
            ])
            .expect_err("missing token");

        assert_eq!(error.code(), "AttachmentRejected");
        let files = store
            .take(&[first.token, second.token])
            .expect("valid tokens retained");
        assert_eq!(files.len(), 2);
        assert!(
            files
                .iter()
                .all(|file| image_data_url(&file.data, &file.meta.mime) == PIXEL)
        );
    }

    #[test]
    fn data_urls_round_trip_the_stored_image() {
        let store = AttachmentStore::new();
        let staged = store.store(PIXEL).expect("stored");
        let files = store.take(&[staged.token]).expect("taken");

        assert_eq!(image_data_url(&files[0].data, &files[0].meta.mime), PIXEL);
    }

    #[test]
    fn image_data_urls_encode_binary_bytes_without_io() {
        assert_eq!(
            image_data_url(&[0, 1, 2, 253, 254, 255], "image/webp"),
            "data:image/webp;base64,AAEC/f7/"
        );
        assert_eq!(image_data_url(&[], "image/png"), "data:image/png;base64,");
    }

    #[test]
    fn enforces_the_inclusive_five_megabyte_limit() {
        let store = AttachmentStore::new();
        let mut data = vec![0; MAX_IMAGE_BYTES];
        data[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        let staged = store
            .store(&image_data_url(&data, "image/png"))
            .expect("maximum image");

        assert_eq!(staged.bytes, MAX_IMAGE_BYTES as u64);
        data.push(0);
        assert_eq!(
            store
                .store(&image_data_url(&data, "image/png"))
                .expect_err("oversized")
                .code(),
            "AttachmentRejected"
        );
        assert_eq!(
            store.take(&[staged.token]).expect("maximum image retained")[0]
                .data
                .len(),
            MAX_IMAGE_BYTES
        );
    }

    #[test]
    fn sniffs_supported_formats_instead_of_trusting_declared_mime() {
        let store = AttachmentStore::new();

        for (data, mime, dimensions) in [
            (
                &b"GIF89a\x02\x00\x03\x00"[..],
                "image/gif",
                (Some(2), Some(3)),
            ),
            (
                &b"RIFF\x05\x00\x00\x00WEBPx"[..],
                "image/webp",
                (None, None),
            ),
        ] {
            let staged = store
                .store(&image_data_url(data, "image/png"))
                .expect("sniffed");

            assert_eq!(staged.mime, mime);
            assert_eq!((staged.width, staged.height), dimensions);
            assert_eq!(store.take(&[staged.token]).expect("taken")[0].data, data);
        }
    }

    #[test]
    fn empty_take_and_discard_preserve_available_images() {
        let store = AttachmentStore::new();
        let staged = store.store(PIXEL).expect("stored");

        assert!(store.take(&[]).expect("empty take").is_empty());
        store.discard(&[]);
        assert_eq!(store.take(&[staged.token]).expect("retained").len(), 1);
    }

    #[test]
    fn unavailable_tokens_cannot_consume_other_staged_images() {
        let store = AttachmentStore::new();
        let consumed = store.store(PIXEL).expect("stored");
        let discarded = store.store(PIXEL).expect("stored");
        let retained = store.store(PIXEL).expect("stored");
        store
            .take(std::slice::from_ref(&consumed.token))
            .expect("consumed");
        store.discard(std::slice::from_ref(&discarded.token));

        assert!(
            store
                .take(&[retained.token.clone(), consumed.token])
                .is_err()
        );
        assert!(
            store
                .take(&[retained.token.clone(), discarded.token])
                .is_err()
        );
        assert_eq!(store.take(&[retained.token]).expect("retained").len(), 1);
    }

    #[test]
    fn staged_bytes_are_independent_of_subsequent_source_replacement() {
        let directory = tempfile::tempdir().expect("temporary sources");
        let source = directory.path().join("source.png");
        let original = STANDARD
            .decode(PIXEL.split_once(',').expect("payload").1)
            .expect("png");
        std::fs::write(&source, &original).expect("source");
        let store = AttachmentStore::new();
        let staged = store
            .store(&image_data_url(
                &std::fs::read(&source).expect("import"),
                "image/png",
            ))
            .expect("stored");
        std::fs::write(&source, b"replacement source").expect("replace source");

        let files = store.take(&[staged.token]).expect("taken");

        assert_eq!(files[0].data, original);
        assert_eq!(
            std::fs::read(&source).expect("untouched replacement"),
            b"replacement source"
        );
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("sources")
                .count(),
            1
        );
    }

    #[test]
    fn staging_taking_discarding_and_dropping_do_not_modify_source_files() {
        let directory = tempfile::tempdir().expect("temporary sources");
        let project = directory.path().join("project");
        let source = directory.path().join("att-123-1.png");
        let imported = project.join("imported-source.png");
        std::fs::create_dir(&project).expect("project");
        std::fs::write(&source, b"external source").expect("external source");
        std::fs::write(&imported, b"imported source").expect("imported source");

        {
            let store = AttachmentStore::new();
            let taken = store.store(PIXEL).expect("stored");
            let discarded = store.store(PIXEL).expect("stored");
            let _dropped = store.store(PIXEL).expect("stored");
            store.take(&[taken.token]).expect("taken");
            store.discard(&[
                discarded.token,
                source.to_string_lossy().into_owned(),
                imported.to_string_lossy().into_owned(),
            ]);
        }

        assert_eq!(
            std::fs::read(&source).expect("external source"),
            b"external source"
        );
        assert_eq!(
            std::fs::read(&imported).expect("imported source"),
            b"imported source"
        );
        assert_eq!(
            std::fs::read_dir(directory.path())
                .expect("sources")
                .count(),
            2
        );
        assert_eq!(
            std::fs::read_dir(project).expect("project sources").count(),
            1
        );
    }
}
