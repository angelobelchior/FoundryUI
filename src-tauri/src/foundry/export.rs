use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::chat::ChatParams;
use super::db::{ChatMessage, Conversation, MessageRole};
use super::error::{FoundryError, FoundryResult};
use super::jobs::now_millis;

const FILE_STEM_LIMIT: usize = 64;
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Markdown,
    Json,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReceipt {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportLabels {
    pub untitled: String,
    pub user: String,
    pub assistant: String,
    pub thinking: String,
    pub model: String,
    pub messages: String,
    pub model_changed: String,
    pub tokens: String,
    pub image_attachments_one: String,
    pub image_attachments_other: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConversationExport<'a> {
    format: &'static str,
    version: u32,
    exported_at: i64,
    conversation: &'a Conversation,
    params: &'a ChatParams,
    messages: &'a [ChatMessage],
}

pub fn markdown(
    conversation: &Conversation,
    messages: &[ChatMessage],
    labels: &ExportLabels,
) -> String {
    let title = title_of(conversation, labels);
    let mut output = String::new();

    output.push_str(&format!("# {title}\n\n"));
    output.push_str(&format!(
        "- **{}:** `{}`\n- **{}:** {}\n",
        labels.model,
        conversation.model_alias,
        labels.messages,
        messages.len()
    ));

    for message in messages {
        match message.role {
            MessageRole::User => {
                output.push_str(&format!("\n## {}\n\n", labels.user));
                push_body(&mut output, &message.content);
                push_attachments(&mut output, message, labels);
            }
            MessageRole::Assistant => {
                output.push_str(&format!("\n## {}\n\n", labels.assistant));
                push_reasoning(&mut output, &message.reasoning, labels);
                push_body(&mut output, &message.content);
                push_usage(&mut output, message, labels);
            }
            MessageRole::Divider => {
                output.push_str(&format!(
                    "\n---\n\n_{}_\n",
                    fill(
                        &labels.model_changed,
                        &[("model", message.model_alias.as_deref().unwrap_or_default())],
                    )
                ));
            }
        }
    }

    output
}

pub fn json(
    conversation: &Conversation,
    params: &ChatParams,
    messages: &[ChatMessage],
) -> FoundryResult<String> {
    serde_json::to_string_pretty(&ConversationExport {
        format: "foundryui.conversation",
        version: FORMAT_VERSION,
        exported_at: now_millis(),
        conversation,
        params,
        messages,
    })
    .map_err(|error| FoundryError::ExportFailed {
        detail: error.to_string(),
    })
}

pub fn file_stem(title: &str, conversation_id: i64) -> String {
    let mut stem = String::new();

    for character in title.chars() {
        if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_' | '(' | ')') {
            stem.push(character);
        } else {
            stem.push('-');
        }
    }

    let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    let stem = stem
        .trim_matches(|character| character == '-' || character == ' ')
        .chars()
        .take(FILE_STEM_LIMIT)
        .collect::<String>();
    let stem = stem
        .trim_matches(|character| character == '-' || character == ' ')
        .to_string();

    if stem.is_empty() {
        format!("conversation-{conversation_id}")
    } else {
        stem
    }
}

pub fn write_export(
    directory: &Path,
    stem: &str,
    format: ExportFormat,
    contents: &str,
) -> FoundryResult<PathBuf> {
    std::fs::create_dir_all(directory).map_err(export_error)?;

    let extension = format.extension();
    let mut candidate = directory.join(format!("{stem}.{extension}"));
    let mut index = 2;

    while candidate.exists() {
        if index > 999 {
            return Err(FoundryError::ExportFailed {
                detail: format!("too many files named {stem} in {}", directory.display()),
            });
        }

        candidate = directory.join(format!("{stem} ({index}).{extension}"));
        index += 1;
    }

    std::fs::write(&candidate, contents).map_err(export_error)?;

    Ok(candidate)
}

fn title_of(conversation: &Conversation, labels: &ExportLabels) -> String {
    let title = conversation.title.trim();

    if title.is_empty() {
        labels.untitled.clone()
    } else {
        title.to_string()
    }
}

fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(template.to_string(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

fn fill_number(template: &str, name: &str, value: usize) -> String {
    fill(template, &[(name, &value.to_string())])
}

fn push_body(output: &mut String, content: &str) {
    if content.trim().is_empty() {
        return;
    }

    output.push_str(content.trim_end());
    output.push('\n');
}

fn push_reasoning(output: &mut String, reasoning: &str, labels: &ExportLabels) {
    if reasoning.trim().is_empty() {
        return;
    }

    output.push_str(&format!(
        "<details>\n<summary>{}</summary>\n\n",
        labels.thinking
    ));
    output.push_str(reasoning.trim());
    output.push_str("\n\n</details>\n\n");
}

fn push_attachments(output: &mut String, message: &ChatMessage, labels: &ExportLabels) {
    let images = message
        .attachments
        .iter()
        .filter(|attachment| attachment.kind == "image")
        .count();

    if images == 0 {
        return;
    }

    let template = if images == 1 {
        &labels.image_attachments_one
    } else {
        &labels.image_attachments_other
    };
    let line = fill_number(template, "count", images);

    output.push_str(&format!("\n_{line}_\n"));
}

fn push_usage(output: &mut String, message: &ChatMessage, labels: &ExportLabels) {
    let Some(usage) = &message.usage else {
        return;
    };

    let line = fill(
        &labels.tokens,
        &[
            ("input", &usage.prompt_tokens.to_string()),
            ("output", &usage.completion_tokens.to_string()),
        ],
    );

    output.push_str(&format!("\n_{line}_\n"));
}

fn export_error(error: std::io::Error) -> FoundryError {
    FoundryError::ExportFailed {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundry::db::{Attachment, MessageStatus, TokenUsage};

    fn conversation() -> Conversation {
        Conversation {
            id: 4,
            title: "Explain FTS5".to_string(),
            model_alias: "qwen3-0.6b".to_string(),
            model_variant_id: None,
            created_at: 1,
            updated_at: 2,
            favorite: false,
            archived: false,
        }
    }

    fn message(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
            id: 1,
            conversation_id: 4,
            seq: 1,
            role,
            content: content.to_string(),
            reasoning: String::new(),
            model_alias: Some("qwen3-0.6b".to_string()),
            params: None,
            usage: None,
            error: None,
            status: MessageStatus::Complete,
            attachments: Vec::new(),
            created_at: 1,
            updated_at: 2,
        }
    }

    fn labels() -> ExportLabels {
        ExportLabels {
            untitled: "Untitled conversation".to_string(),
            user: "User".to_string(),
            assistant: "Assistant".to_string(),
            thinking: "Thinking".to_string(),
            model: "Model".to_string(),
            messages: "Messages".to_string(),
            model_changed: "Model changed to `{model}`".to_string(),
            tokens: "Tokens: {input} in, {output} out".to_string(),
            image_attachments_one: "{count} image attachment".to_string(),
            image_attachments_other: "{count} image attachments".to_string(),
        }
    }

    #[test]
    fn markdown_keeps_code_tables_reasoning_and_usage() {
        let messages = vec![
            message(
                MessageRole::User,
                "Show me the query:\n\n```sql\nSELECT 1;\n```",
            ),
            ChatMessage {
                reasoning: "weighing options".to_string(),
                usage: Some(TokenUsage {
                    prompt_tokens: 12,
                    completion_tokens: 34,
                    total_tokens: 46,
                }),
                content: "| a | b |\n| - | - |\n| 1 | 2 |".to_string(),
                ..message(MessageRole::Assistant, "")
            },
        ];

        let output = markdown(&conversation(), &messages, &labels());

        assert!(output.starts_with("# Explain FTS5\n"));
        assert!(output.contains("- **Model:** `qwen3-0.6b`"));
        assert!(output.contains("```sql\nSELECT 1;\n```"));
        assert!(output.contains("| a | b |\n| - | - |\n| 1 | 2 |"));
        assert!(output.contains("<summary>Thinking</summary>\n\nweighing options"));
        assert!(output.contains("_Tokens: 12 in, 34 out_"));
    }

    #[test]
    fn markdown_names_the_untitled_conversation_and_model_changes() {
        let messages = vec![ChatMessage {
            role: MessageRole::Divider,
            model_alias: Some("qwen3.5-0.8b".to_string()),
            content: String::new(),
            ..message(MessageRole::Divider, "")
        }];
        let untitled = Conversation {
            title: "   ".to_string(),
            ..conversation()
        };

        let output = markdown(&untitled, &messages, &labels());

        assert!(output.contains("# Untitled conversation"));
        assert!(output.contains("_Model changed to `qwen3.5-0.8b`_"));
    }

    #[test]
    fn markdown_uses_the_supplied_labels() {
        let messages = vec![
            message(MessageRole::User, "mostre a consulta"),
            ChatMessage {
                reasoning: "pesando".to_string(),
                usage: Some(TokenUsage {
                    prompt_tokens: 1,
                    completion_tokens: 2,
                    total_tokens: 3,
                }),
                content: "pronto".to_string(),
                ..message(MessageRole::Assistant, "")
            },
        ];
        let labels = ExportLabels {
            untitled: "Conversa sem título".to_string(),
            user: "Usuário".to_string(),
            assistant: "Assistente".to_string(),
            thinking: "Raciocínio".to_string(),
            model: "Modelo".to_string(),
            messages: "Mensagens".to_string(),
            model_changed: "Modelo alterado para `{model}`".to_string(),
            tokens: "Tokens: {input} entrada, {output} saída".to_string(),
            image_attachments_one: "{count} imagem anexada".to_string(),
            image_attachments_other: "{count} imagens anexadas".to_string(),
        };

        let output = markdown(&conversation(), &messages, &labels);

        assert!(output.contains("- **Modelo:** `qwen3-0.6b`\n- **Mensagens:** 2"));
        assert!(output.contains("## Usuário"));
        assert!(output.contains("## Assistente"));
        assert!(output.contains("<summary>Raciocínio</summary>"));
        assert!(output.contains("_Tokens: 1 entrada, 2 saída_"));
    }

    #[test]
    fn json_contains_the_full_structure() {
        let messages = vec![
            ChatMessage {
                attachments: vec![Attachment {
                    id: 9,
                    message_id: 1,
                    kind: "image".to_string(),
                    mime: "image/png".to_string(),
                    path: "/tmp/image.png".to_string(),
                    bytes: 128,
                    width: Some(1),
                    height: Some(1),
                }],
                ..message(MessageRole::User, "describe")
            },
            ChatMessage {
                usage: Some(TokenUsage {
                    prompt_tokens: 1,
                    completion_tokens: 2,
                    total_tokens: 3,
                }),
                ..message(MessageRole::Assistant, "a pixel")
            },
        ];
        let params = ChatParams {
            temperature: Some(0.2),
            ..ChatParams::default()
        };

        let output = json(&conversation(), &params, &messages).expect("json");
        let value: serde_json::Value = serde_json::from_str(&output).expect("valid json");

        assert_eq!(value["format"], "foundryui.conversation");
        assert_eq!(value["version"], 1);
        assert_eq!(value["conversation"]["title"], "Explain FTS5");
        assert_eq!(value["params"]["temperature"], 0.2);
        assert_eq!(value["messages"][0]["attachments"][0]["mime"], "image/png");
        assert_eq!(value["messages"][1]["usage"]["totalTokens"], 3);
    }

    #[test]
    fn file_names_are_safe_and_fall_back_to_the_id() {
        assert_eq!(
            file_stem("Explain FTS5: search / export?", 4),
            "Explain FTS5- search - export"
        );
        assert_eq!(file_stem("   ", 7), "conversation-7");
        assert_eq!(file_stem("a".repeat(120).as_str(), 8).chars().count(), 64);
        assert_eq!(file_stem("../../etc/passwd", 9), "etc-passwd");
    }

    #[test]
    fn exports_never_overwrite_an_existing_file() {
        let directory = tempfile::tempdir().expect("temp dir");
        let first = write_export(directory.path(), "chat", ExportFormat::Markdown, "one")
            .expect("first export");
        let second = write_export(directory.path(), "chat", ExportFormat::Markdown, "two")
            .expect("second export");

        assert_eq!(first.file_name().expect("name"), "chat.md");
        assert_eq!(second.file_name().expect("name"), "chat (2).md");
        assert_eq!(std::fs::read_to_string(&first).expect("first"), "one");
        assert_eq!(std::fs::read_to_string(&second).expect("second"), "two");
    }
}
