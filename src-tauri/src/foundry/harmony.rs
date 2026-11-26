const TOKENS: [&str; 7] = [
    "<|start|>",
    "<|end|>",
    "<|message|>",
    "<|channel|>",
    "<|constrain|>",
    "<|return|>",
    "<|call|>",
];

const CHANNEL_OPEN_TOKENS: [&str; 3] = ["<|channel>", "<|turn>", "<|think|>"];
const CHANNEL_CLOSE_TOKENS: [&str; 2] = ["<channel|>", "<turn|>"];

const ROLES: [&str; 4] = ["assistant", "user", "system", "tool"];

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HarmonyDelta {
    pub content: String,
    pub reasoning: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    Content,
    Reasoning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Header,
    Channel,
    Body(Channel),
    Constraint(Channel),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Detection {
    Pending,
    Harmony,
    Channel,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelMode {
    Text,
    Name,
    Body { reasoning: bool },
}

pub struct HarmonyStream {
    mode: Mode,
    pending: String,
    detection: Detection,
    channel_mode: ChannelMode,
}

impl Default for HarmonyStream {
    fn default() -> Self {
        Self::new()
    }
}

impl HarmonyStream {
    pub fn new() -> Self {
        Self {
            mode: Mode::Header,
            pending: String::new(),
            detection: Detection::Pending,
            channel_mode: ChannelMode::Text,
        }
    }

    pub fn push(&mut self, chunk: &str) -> HarmonyDelta {
        if self.detection == Detection::Plain {
            return HarmonyDelta {
                content: chunk.to_string(),
                reasoning: String::new(),
            };
        }

        self.pending.push_str(chunk);

        if self.detection == Detection::Pending {
            match detect(&self.pending) {
                Detection::Pending => return HarmonyDelta::default(),
                Detection::Plain => {
                    self.detection = Detection::Plain;

                    return HarmonyDelta {
                        content: std::mem::take(&mut self.pending),
                        reasoning: String::new(),
                    };
                }
                Detection::Harmony => self.detection = Detection::Harmony,
                Detection::Channel => self.detection = Detection::Channel,
            }
        }

        self.drain(false)
    }

    pub fn finish(&mut self) -> HarmonyDelta {
        match self.detection {
            Detection::Plain => HarmonyDelta::default(),
            Detection::Pending => {
                self.detection = Detection::Plain;

                HarmonyDelta {
                    content: std::mem::take(&mut self.pending),
                    reasoning: String::new(),
                }
            }
            Detection::Harmony => self.drain_harmony(true),
            Detection::Channel => self.drain_channel(true),
        }
    }

    fn drain(&mut self, at_end: bool) -> HarmonyDelta {
        match self.detection {
            Detection::Channel => self.drain_channel(at_end),
            _ => self.drain_harmony(at_end),
        }
    }

    fn drain_harmony(&mut self, at_end: bool) -> HarmonyDelta {
        let mut delta = HarmonyDelta::default();

        loop {
            match self.mode {
                Mode::Header => {
                    let Some((index, token)) = next_token(&self.pending) else {
                        let keep = if at_end {
                            0
                        } else {
                            trailing_prefix_len(&self.pending)
                        };
                        let drop = self.pending.len() - keep;

                        self.pending.drain(..drop);
                        break;
                    };

                    self.pending.drain(..index + token.len());
                    self.mode = if token == "<|channel|>" {
                        Mode::Channel
                    } else {
                        Mode::Header
                    };
                }
                Mode::Channel => {
                    let Some((index, token)) = next_token(&self.pending) else {
                        if at_end {
                            self.pending.clear();
                        }

                        break;
                    };

                    let name = self.pending[..index].trim().to_ascii_lowercase();
                    let channel = channel_for(&name);

                    self.pending.drain(..index + token.len());
                    self.mode = match token {
                        "<|message|>" => Mode::Body(channel),
                        "<|constrain|>" => Mode::Constraint(channel),
                        _ => Mode::Header,
                    };
                }
                Mode::Body(channel) => {
                    let Some((index, token)) = next_token(&self.pending) else {
                        let keep = if at_end {
                            0
                        } else {
                            trailing_prefix_len(&self.pending)
                        };
                        let emit = self.pending.len() - keep;
                        let text: String = self.pending.drain(..emit).collect();

                        push(&mut delta, channel, &text);
                        break;
                    };

                    let text: String = self.pending.drain(..index).collect();

                    push(&mut delta, channel, &text);
                    self.pending.drain(..token.len());
                    self.mode = match token {
                        "<|start|>" | "<|end|>" => Mode::Header,
                        "<|channel|>" => Mode::Channel,
                        _ => Mode::Body(channel),
                    };
                }
                Mode::Constraint(channel) => {
                    let Some((index, token)) = next_token(&self.pending) else {
                        if at_end {
                            self.pending.clear();
                        }

                        break;
                    };

                    self.pending.drain(..index + token.len());
                    self.mode = match token {
                        "<|message|>" => Mode::Body(channel),
                        "<|channel|>" => Mode::Channel,
                        "<|start|>" | "<|end|>" => Mode::Header,
                        _ => Mode::Constraint(channel),
                    };
                }
            }
        }

        delta
    }

    fn drain_channel(&mut self, at_end: bool) -> HarmonyDelta {
        let mut delta = HarmonyDelta::default();

        loop {
            match self.channel_mode {
                ChannelMode::Text => {
                    let Some((index, token)) = next_channel_token(&self.pending) else {
                        let keep = if at_end {
                            0
                        } else {
                            trailing_channel_prefix_len(&self.pending)
                        };
                        let emit = self.pending.len() - keep;
                        let text: String = self.pending.drain(..emit).collect();

                        push(&mut delta, Channel::Content, &text);
                        break;
                    };

                    let text: String = self.pending.drain(..index).collect();

                    push(&mut delta, Channel::Content, &text);
                    self.pending.drain(..token.len());

                    if token == "<|channel>" {
                        self.channel_mode = ChannelMode::Name;
                    }
                }
                ChannelMode::Name => {
                    let leading = self
                        .pending
                        .find(|c: char| !c.is_whitespace())
                        .unwrap_or(self.pending.len());

                    self.pending.drain(..leading);

                    if self.pending.is_empty() {
                        if at_end {
                            self.channel_mode = ChannelMode::Text;
                        }

                        break;
                    }

                    let Some(end) = self.pending.find(|c: char| c.is_whitespace() || c == '<') else {
                        if at_end {
                            let name = std::mem::take(&mut self.pending);

                            push(&mut delta, Channel::Content, &name);
                            self.channel_mode = ChannelMode::Text;
                        }

                        break;
                    };

                    let name = self.pending[..end].to_ascii_lowercase();

                    self.pending.drain(..end);

                    let whitespace = self
                        .pending
                        .find(|c: char| !c.is_whitespace())
                        .unwrap_or(self.pending.len());

                    self.pending.drain(..whitespace);

                    let reasoning = matches!(name.as_str(), "thought" | "analysis");

                    self.channel_mode = ChannelMode::Body { reasoning };
                }
                ChannelMode::Body { reasoning } => {
                    let Some((index, token)) = next_channel_token(&self.pending) else {
                        let keep = if at_end {
                            0
                        } else {
                            trailing_channel_prefix_len(&self.pending)
                        };
                        let emit = self.pending.len() - keep;
                        let text: String = self.pending.drain(..emit).collect();

                        push(&mut delta, reasoning_channel(reasoning), &text);
                        break;
                    };

                    let text: String = self.pending.drain(..index).collect();

                    push(&mut delta, reasoning_channel(reasoning), &text);
                    self.pending.drain(..token.len());

                    if token == "<channel|>" {
                        self.channel_mode = ChannelMode::Text;
                    }
                }
            }
        }

        delta
    }
}

fn detect(value: &str) -> Detection {
    let Some(index) = value.find("<|") else {
        if trailing_prefix_len(value) > 0
            || trailing_channel_prefix_len(value) > 0
            || is_role_prefix(value.trim())
        {
            return Detection::Pending;
        }

        return Detection::Plain;
    };

    let header = value[..index].trim();

    if !header.is_empty() && !is_role(header) {
        return Detection::Plain;
    }

    let suffix = &value[index..];

    if TOKENS.iter().any(|token| suffix.starts_with(token)) {
        return Detection::Harmony;
    }

    if CHANNEL_OPEN_TOKENS.iter().any(|token| suffix.starts_with(token)) {
        return Detection::Channel;
    }

    if TOKENS.iter().any(|token| token.starts_with(suffix)) {
        return Detection::Pending;
    }

    if CHANNEL_OPEN_TOKENS.iter().any(|token| token.starts_with(suffix)) {
        return Detection::Pending;
    }

    Detection::Plain
}

fn is_role(value: &str) -> bool {
    ROLES.contains(&value)
}

fn is_role_prefix(value: &str) -> bool {
    ROLES.iter().any(|role| role.starts_with(value))
}

fn channel_for(name: &str) -> Channel {
    if name == "analysis" {
        Channel::Reasoning
    } else {
        Channel::Content
    }
}

fn reasoning_channel(reasoning: bool) -> Channel {
    if reasoning {
        Channel::Reasoning
    } else {
        Channel::Content
    }
}

fn push(delta: &mut HarmonyDelta, channel: Channel, text: &str) {
    if text.is_empty() {
        return;
    }

    match channel {
        Channel::Content => delta.content.push_str(text),
        Channel::Reasoning => delta.reasoning.push_str(text),
    }
}

fn next_token(value: &str) -> Option<(usize, &'static str)> {
    TOKENS
        .iter()
        .filter_map(|token| value.find(token).map(|index| (index, *token)))
        .min_by_key(|(index, _)| *index)
}

fn next_channel_token(value: &str) -> Option<(usize, &'static str)> {
    CHANNEL_OPEN_TOKENS
        .iter()
        .chain(CHANNEL_CLOSE_TOKENS.iter())
        .filter_map(|token| value.find(token).map(|index| (index, *token)))
        .min_by_key(|(index, _)| *index)
}

fn trailing_prefix_len(value: &str) -> usize {
    let Some(index) = value.rfind('<') else {
        return 0;
    };
    let suffix = &value[index..];

    if TOKENS.iter().any(|token| token.starts_with(suffix)) {
        suffix.len()
    } else {
        0
    }
}

fn trailing_channel_prefix_len(value: &str) -> usize {
    let Some(index) = value.rfind('<') else {
        return 0;
    };
    let suffix = &value[index..];
    let tokens = CHANNEL_OPEN_TOKENS.iter().chain(CHANNEL_CLOSE_TOKENS.iter());

    if tokens.into_iter().any(|token| token.starts_with(suffix)) {
        suffix.len()
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(chunks: &[&str]) -> HarmonyDelta {
        let mut stream = HarmonyStream::new();
        let mut delta = HarmonyDelta::default();

        for chunk in chunks {
            let next = stream.push(chunk);

            delta.content.push_str(&next.content);
            delta.reasoning.push_str(&next.reasoning);
        }

        let tail = stream.finish();

        delta.content.push_str(&tail.content);
        delta.reasoning.push_str(&tail.reasoning);

        delta
    }

    #[test]
    fn splits_analysis_from_the_final_answer() {
        let delta = collect(&[
            "<|channel|>analysis<|message|>We need to answer in Portuguese.<|end|>",
            "<|start|>assistant<|channel|>final<|message|>Aqui vai o código.",
        ]);

        assert_eq!(delta.reasoning, "We need to answer in Portuguese.");
        assert_eq!(delta.content, "Aqui vai o código.");
    }

    #[test]
    fn keeps_channel_text_clean_when_tokens_are_split_across_chunks() {
        let delta = collect(&[
            "<|chan",
            "nel|>analy",
            "sis<|mess",
            "age|>thinking",
            "<|en",
            "d|><|start|>assistant<|channel|>fin",
            "al<|message|>done",
            "<|end|>",
        ]);

        assert_eq!(delta.reasoning, "thinking");
        assert_eq!(delta.content, "done");
    }

    #[test]
    fn passes_through_content_without_harmony_tokens() {
        let delta = collect(&["Hello", " world", "!"]);

        assert_eq!(delta.content, "Hello world!");
        assert!(delta.reasoning.is_empty());
    }

    #[test]
    fn keeps_plain_text_that_looks_like_angle_brackets() {
        let delta = collect(&["a < b", " and c > d"]);

        assert_eq!(delta.content, "a < b and c > d");
    }

    #[test]
    fn does_not_treat_harmony_tokens_inside_plain_text_as_a_header() {
        let delta = collect(&["Here is <|start|> a token"]);

        assert_eq!(delta.content, "Here is <|start|> a token");
        assert!(delta.reasoning.is_empty());
    }

    #[test]
    fn emits_a_trailing_plain_fragment_when_the_stream_ends() {
        let delta = collect(&["partial answer"]);

        assert_eq!(delta.content, "partial answer");
    }

    #[test]
    fn routes_commentary_to_content() {
        let delta = collect(&["<|channel|>commentary<|message|>calling a tool<|end|>"]);

        assert_eq!(delta.content, "calling a tool");
        assert!(delta.reasoning.is_empty());
    }

    #[test]
    fn handles_the_token_per_chunk_sequence_from_gpt_oss() {
        let delta = collect(&[
            "<|channel|>",
            "analysis",
            "<|message|>",
            "The",
            " user",
            " asks",
            "<|end|>",
            "<|start|>",
            "assistant",
            "<|channel|>",
            "final",
            "<|message|>",
            "ola",
            "<|return|>",
        ]);

        assert_eq!(delta.reasoning, "The user asks");
        assert_eq!(delta.content, "ola");
    }

    #[test]
    fn ignores_constraints_between_channel_and_message() {
        let delta = collect(&["<|channel|>final<|constrain|>json<|message|>{\"ok\":true}"]);

        assert_eq!(delta.content, "{\"ok\":true}");
    }

    #[test]
    fn handles_a_message_without_an_explicit_start_token() {
        let delta = collect(&["assistant<|channel|>final<|message|>hello"]);

        assert_eq!(delta.content, "hello");
    }

    #[test]
    fn routes_gemma_empty_thought_block_to_content() {
        let delta = collect(&["<|channel>thought\n<channel|>Olá! Como posso ajudar?"]);

        assert!(delta.reasoning.is_empty());
        assert_eq!(delta.content, "Olá! Como posso ajudar?");
    }

    #[test]
    fn routes_gemma_thought_to_reasoning_and_answer_to_content() {
        let delta = collect(&["<|channel>thought\npensando na resposta<channel|>Aqui está a resposta."]);

        assert_eq!(delta.reasoning, "pensando na resposta");
        assert_eq!(delta.content, "Aqui está a resposta.");
    }

    #[test]
    fn handles_gemma_tokens_split_across_chunks() {
        let delta = collect(&[
            "<|chan",
            "nel>tho",
            "ught\n",
            "<chan",
            "nel|>",
            "resposta",
            " final",
        ]);

        assert!(delta.reasoning.is_empty());
        assert_eq!(delta.content, "resposta final");
    }

    #[test]
    fn discards_gemma_turn_markers() {
        let delta = collect(&["<|channel>thought\n<channel|>ok<turn|>"]);

        assert!(delta.reasoning.is_empty());
        assert_eq!(delta.content, "ok");
    }
}
