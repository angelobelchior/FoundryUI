use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::error::{FoundryError, FoundryResult};

pub const EMBEDDINGS_TIMEOUT_SECONDS: u64 = 180;
const CHUNK_TARGET_CHARS: usize = 700;
const CHUNK_MAX_CHARS: usize = 920;
const CHUNK_OVERLAP_CHARS: usize = 140;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingUsage {
    pub prompt_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Debug, Clone)]
pub struct EmbeddingBatch {
    pub vectors: Vec<Vec<f32>>,
    pub usage: EmbeddingUsage,
}

pub async fn embed_texts(
    endpoint: &super::local_inference::VerifiedLocalEndpoint,
    model: &str,
    input: &[String],
) -> FoundryResult<EmbeddingBatch> {
    if input.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "embeddings_embed".to_string(),
            message: "the embeddings input is empty".to_string(),
            hint: None,
        });
    }

    #[derive(Serialize)]
    struct EmbeddingsRequest<'a> {
        model: &'a str,
        input: &'a [String],
    }

    let response = endpoint
        .post_json(
            "/v1/embeddings",
            &EmbeddingsRequest { model, input },
            Duration::from_secs(EMBEDDINGS_TIMEOUT_SECONDS),
        )
        .await
        .map_err(super::http::local_inference_error)?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        if status == reqwest::StatusCode::NOT_FOUND || is_missing_model(&body) {
            return Err(FoundryError::ModelNotLoaded {
                alias: model.to_string(),
            });
        }

        return Err(FoundryError::BadResponse {
            detail: format!("HTTP {status}: {body}"),
        });
    }

    parse_embeddings(&body)
}

pub fn normalize_vector(vector: &[f32]) -> Vec<f32> {
    if vector.is_empty() {
        return Vec::new();
    }

    let squared: f32 = vector.iter().map(|value| value * value).sum();

    if !squared.is_finite() || squared <= 0.0 {
        return vec![0.0; vector.len()];
    }

    let magnitude = squared.sqrt();

    vector.iter().map(|value| value / magnitude).collect()
}

pub fn vector_blob(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);

    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    bytes
}

pub fn vector_from_blob(blob: &[u8]) -> Option<Vec<f32>> {
    let (chunks, remainder) = blob.as_chunks::<4>();

    if !remainder.is_empty() {
        return None;
    }

    let mut vector = Vec::with_capacity(chunks.len());

    for chunk in chunks {
        vector.push(f32::from_le_bytes(*chunk));
    }

    Some(vector)
}

pub fn similarity_score(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }

    let dot = left
        .iter()
        .zip(right.iter())
        .map(|(a, b)| a * b)
        .sum::<f32>();

    ((dot + 1.0) * 0.5).clamp(0.0, 1.0)
}

pub fn lexical_overlap_score(text: &str, query: &str) -> f32 {
    let terms = query_terms(query);

    if terms.is_empty() {
        return 0.0;
    }

    let haystack = text.to_lowercase();
    let mut matched = 0usize;

    for term in &terms {
        if haystack.contains(term) {
            matched += 1;
        }
    }

    (matched as f32 / terms.len() as f32).clamp(0.0, 1.0)
}

pub fn parse_document_lines(input: &str) -> Vec<String> {
    chunk_paragraphs(parse_text_paragraphs(input))
}

pub fn parse_pdf_document_lines(input: &str) -> Vec<String> {
    let normalized = input.replace('\u{000C}', "\n\n");

    chunk_paragraphs(parse_text_paragraphs(&normalized))
}

pub fn read_document_lines(path: &Path) -> FoundryResult<Vec<String>> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_lowercase())
        .unwrap_or_default();

    let content = match extension.as_str() {
        "txt" | "md" => fs::read_to_string(path).map_err(|error| FoundryError::CommandFailed {
            command: "embeddings_import_file".to_string(),
            message: error.to_string(),
            hint: None,
        })?,
        "pdf" => pdf_extract::extract_text(path).map_err(|error| FoundryError::CommandFailed {
            command: "embeddings_import_file".to_string(),
            message: error.to_string(),
            hint: None,
        })?,
        _ => {
            return Err(FoundryError::CommandFailed {
                command: "embeddings_import_file".to_string(),
                message: "the file must be .txt, .md or .pdf".to_string(),
                hint: None,
            });
        }
    };

    Ok(if extension == "pdf" {
        parse_pdf_document_lines(&content)
    } else {
        parse_document_lines(&content)
    })
}

fn parse_embeddings(body: &str) -> FoundryResult<EmbeddingBatch> {
    #[derive(Debug, Deserialize)]
    struct EmbeddingItem {
        #[serde(default)]
        index: usize,
        #[serde(default)]
        embedding: Vec<f32>,
    }

    #[derive(Debug, Deserialize)]
    struct UsagePayload {
        #[serde(default, alias = "input_tokens")]
        prompt_tokens: Option<i64>,
        #[serde(default)]
        total_tokens: Option<i64>,
    }

    #[derive(Debug, Deserialize)]
    struct EmbeddingsResponse {
        #[serde(default)]
        data: Vec<EmbeddingItem>,
        #[serde(default)]
        usage: Option<UsagePayload>,
    }

    let parsed: EmbeddingsResponse =
        serde_json::from_str(body).map_err(|error| FoundryError::BadResponse {
            detail: error.to_string(),
        })?;

    if parsed.data.is_empty() {
        return Err(FoundryError::BadResponse {
            detail: "the embeddings response has no vectors".to_string(),
        });
    }

    let mut items = parsed.data;

    items.sort_by_key(|item| item.index);

    let vectors = items
        .into_iter()
        .map(|item| {
            if item.embedding.is_empty() {
                return Err(FoundryError::BadResponse {
                    detail: "the embeddings response has an empty vector".to_string(),
                });
            }

            Ok(item.embedding)
        })
        .collect::<FoundryResult<Vec<Vec<f32>>>>()?;

    let usage = parsed.usage.unwrap_or(UsagePayload {
        prompt_tokens: None,
        total_tokens: None,
    });

    Ok(EmbeddingBatch {
        vectors,
        usage: EmbeddingUsage {
            prompt_tokens: usage.prompt_tokens.unwrap_or_default().max(0),
            total_tokens: usage.total_tokens.unwrap_or_default().max(0),
        },
    })
}

fn is_missing_model(detail: &str) -> bool {
    let lower = detail.to_lowercase();

    lower.contains("not loaded")
        || lower.contains("model not found")
        || lower.contains("no model matching")
        || lower.contains("model is not loaded")
}

fn parse_text_paragraphs(input: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for line in input.lines() {
        let compact = normalize_text(line);

        if compact.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }

            continue;
        }

        current.push(compact);
    }

    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }

    if paragraphs.is_empty() {
        return input
            .lines()
            .map(normalize_text)
            .filter(|line| !line.is_empty())
            .collect();
    }

    paragraphs
}

fn chunk_paragraphs(paragraphs: Vec<String>) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for paragraph in paragraphs {
        for piece in chunk_paragraph(&paragraph) {
            if current.is_empty() {
                current = piece;
                continue;
            }

            let candidate_len = current.chars().count() + 2 + piece.chars().count();

            if candidate_len <= CHUNK_TARGET_CHARS {
                current.push_str("\n\n");
                current.push_str(&piece);
            } else {
                chunks.push(current);
                current = piece;
            }
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn chunk_paragraph(paragraph: &str) -> Vec<String> {
    let normalized = normalize_text(paragraph);

    if normalized.is_empty() {
        return Vec::new();
    }

    if normalized.chars().count() <= CHUNK_MAX_CHARS {
        return vec![normalized];
    }

    let words: Vec<&str> = normalized.split_whitespace().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < words.len() {
        let mut end = start;
        let mut count = 0usize;

        while end < words.len() {
            let word_len = words[end].chars().count() + usize::from(end > start);

            if count + word_len > CHUNK_MAX_CHARS && end > start {
                break;
            }

            count += word_len;
            end += 1;
        }

        if end == start {
            end += 1;
        }

        if end < words.len() && count > CHUNK_TARGET_CHARS {
            let mut candidate = end;
            let mut candidate_count = count;

            while candidate > start + 1 && candidate_count > CHUNK_TARGET_CHARS {
                let index = candidate - 1;
                let token = words[index];
                let token_len = token.chars().count() + usize::from(index > start);

                candidate_count = candidate_count.saturating_sub(token_len);
                candidate -= 1;

                if ends_sentence(token) {
                    end = candidate;
                    break;
                }
            }
        }

        let chunk = words[start..end].join(" ");

        if !chunk.is_empty() {
            chunks.push(chunk);
        }

        if end >= words.len() {
            break;
        }

        let mut overlap_words = 0usize;
        let mut overlap_chars = 0usize;
        let mut cursor = end;

        while cursor > start {
            let index = cursor - 1;
            let token_len = words[index].chars().count() + usize::from(overlap_words > 0);

            if overlap_chars + token_len > CHUNK_OVERLAP_CHARS {
                break;
            }

            overlap_chars += token_len;
            overlap_words += 1;
            cursor -= 1;
        }

        start = end.saturating_sub(overlap_words);

        if start >= end {
            start = end;
        }
    }

    chunks
}

fn query_terms(input: &str) -> Vec<String> {
    let mut terms = Vec::new();

    for token in input
        .to_lowercase()
        .split(|char: char| !char.is_alphanumeric())
        .filter(|term| term.chars().count() >= 3)
    {
        if terms.iter().any(|existing| existing == token) {
            continue;
        }

        terms.push(token.to_string());

        if terms.len() >= 10 {
            break;
        }
    }

    terms
}

fn normalize_text(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ends_sentence(input: &str) -> bool {
    input.ends_with('.')
        || input.ends_with('!')
        || input.ends_with('?')
        || input.ends_with(':')
        || input.ends_with(';')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_restores_vectors() {
        let normalized = normalize_vector(&[3.0, 4.0]);
        let blob = vector_blob(&normalized);
        let restored = vector_from_blob(&blob).expect("vector");

        assert!((normalized[0] - 0.6).abs() < 0.0001);
        assert!((normalized[1] - 0.8).abs() < 0.0001);
        assert_eq!(normalized.len(), restored.len());
    }

    #[test]
    fn maps_dot_product_to_zero_and_one() {
        assert_eq!(similarity_score(&[1.0, 0.0], &[1.0, 0.0]), 1.0);
        assert_eq!(similarity_score(&[1.0, 0.0], &[-1.0, 0.0]), 0.0);
        assert_eq!(similarity_score(&[1.0, 0.0], &[0.0, 1.0]), 0.5);
    }

    #[test]
    fn document_lines_ignore_blanks() {
        let lines = parse_document_lines("first\n\n second \n");

        assert_eq!(lines, vec!["first\n\nsecond"]);
    }

    #[test]
    fn pdf_lines_group_paragraphs_and_join_wrapped_lines() {
        let lines = parse_pdf_document_lines(
            "first line\ncontinues here\n\nsecond\tparagraph\n  with   spaces \n\u{000C}\nthird page",
        );

        assert_eq!(
            lines,
            vec!["first line continues here\n\nsecond paragraph with spaces\n\nthird page"]
        );
    }

    #[test]
    fn long_paragraph_is_chunked_with_overlap() {
        let input = std::iter::repeat_n(
            "vector search relevance quality retrieval chunk indexing",
            120,
        )
        .collect::<Vec<_>>()
        .join(" ");
        let lines = parse_document_lines(&input);

        assert!(lines.len() > 1);
        assert!(
            lines
                .iter()
                .all(|line| line.chars().count() <= CHUNK_MAX_CHARS)
        );
    }

    #[test]
    fn lexical_overlap_matches_query_terms() {
        let score = lexical_overlap_score(
            "FoundryUI stores semantic chunks from markdown and pdf files.",
            "semantic markdown ranking",
        );

        assert!(score > 0.5);
    }
}
