use serde::Serialize;
use std::io::BufRead;

use crate::enrichment;

const MAX_LINE_SIZE: usize = 10 * 1024 * 1024;
const TOOL_RESULT_MAX: usize = 500;
const THINKING_MAX: usize = 300;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMessage {
    pub role: String,
    pub message_type: String,
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input_summary: Option<String>,
    pub tool_result_content: Option<String>,
    pub is_error: Option<bool>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationData {
    pub session_id: String,
    pub messages: Vec<ConversationMessage>,
    pub total_entries: usize,
}

pub fn parse_conversation(session_id: &str, raw_cwd: &str) -> Option<ConversationData> {
    let path = enrichment::find_jsonl_path(session_id, raw_cwd)?;
    let file = std::fs::File::open(&path).ok()?;
    let reader = std::io::BufReader::new(file);

    let mut messages: Vec<ConversationMessage> = Vec::new();
    let mut total_entries = 0;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        if line.is_empty() || line.len() > MAX_LINE_SIZE {
            continue;
        }

        let entry: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let entry_type = entry.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if entry_type == "progress" || entry_type == "file-history-snapshot" {
            continue;
        }

        let timestamp = entry.get("timestamp").and_then(|t| t.as_str()).map(|s| s.to_string());

        match entry_type {
            "user" => {
                parse_user_entry(&entry, &timestamp, &mut messages);
                total_entries += 1;
            }
            "assistant" => {
                parse_assistant_entry(&entry, &timestamp, &mut messages);
                total_entries += 1;
            }
            "system" => {
                if let Some(text) = extract_system_text(&entry) {
                    messages.push(ConversationMessage {
                        role: "system".into(),
                        message_type: "text".into(),
                        text: Some(text),
                        tool_name: None,
                        tool_input_summary: None,
                        tool_result_content: None,
                        is_error: None,
                        timestamp: timestamp.clone(),
                    });
                    total_entries += 1;
                }
            }
            _ => {}
        }
    }

    Some(ConversationData {
        session_id: session_id.to_string(),
        messages,
        total_entries,
    })
}

fn parse_user_entry(
    entry: &serde_json::Value,
    timestamp: &Option<String>,
    messages: &mut Vec<ConversationMessage>,
) {
    let content = match entry.get("message").and_then(|m| m.get("content")) {
        Some(c) => c,
        None => return,
    };

    match content {
        serde_json::Value::String(text) => {
            if !text.is_empty() {
                messages.push(ConversationMessage {
                    role: "user".into(),
                    message_type: "text".into(),
                    text: Some(text.clone()),
                    tool_name: None,
                    tool_input_summary: None,
                    tool_result_content: None,
                    is_error: None,
                    timestamp: timestamp.clone(),
                });
            }
        }
        serde_json::Value::Array(blocks) => {
            for block in blocks {
                let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

                match block_type {
                    "tool_result" => {
                        let is_error = block.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false);
                        let result_content = extract_block_text(block);
                        let result_content = truncate_or_persisted(&result_content, TOOL_RESULT_MAX);

                        messages.push(ConversationMessage {
                            role: "user".into(),
                            message_type: "tool_result".into(),
                            text: None,
                            tool_name: None,
                            tool_input_summary: None,
                            tool_result_content: Some(result_content),
                            is_error: Some(is_error),
                            timestamp: timestamp.clone(),
                        });
                    }
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                messages.push(ConversationMessage {
                                    role: "user".into(),
                                    message_type: "text".into(),
                                    text: Some(text.to_string()),
                                    tool_name: None,
                                    tool_input_summary: None,
                                    tool_result_content: None,
                                    is_error: None,
                                    timestamp: timestamp.clone(),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn parse_assistant_entry(
    entry: &serde_json::Value,
    timestamp: &Option<String>,
    messages: &mut Vec<ConversationMessage>,
) {
    let content = match entry.get("message").and_then(|m| m.get("content")) {
        Some(serde_json::Value::Array(arr)) => arr,
        _ => return,
    };

    for block in content {
        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match block_type {
            "text" => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    if !text.is_empty() {
                        messages.push(ConversationMessage {
                            role: "assistant".into(),
                            message_type: "text".into(),
                            text: Some(text.to_string()),
                            tool_name: None,
                            tool_input_summary: None,
                            tool_result_content: None,
                            is_error: None,
                            timestamp: timestamp.clone(),
                        });
                    }
                }
            }
            "tool_use" => {
                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                let input = block.get("input");
                let summary = extract_tool_summary(name, input);

                messages.push(ConversationMessage {
                    role: "assistant".into(),
                    message_type: "tool_use".into(),
                    text: None,
                    tool_name: Some(name.to_string()),
                    tool_input_summary: Some(summary),
                    tool_result_content: None,
                    is_error: None,
                    timestamp: timestamp.clone(),
                });
            }
            "thinking" => {
                if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                    let truncated = if text.len() > THINKING_MAX {
                        truncate_str(text, THINKING_MAX)
                    } else {
                        text.to_string()
                    };
                    messages.push(ConversationMessage {
                        role: "assistant".into(),
                        message_type: "thinking".into(),
                        text: Some(truncated),
                        tool_name: None,
                        tool_input_summary: None,
                        tool_result_content: None,
                        is_error: None,
                        timestamp: timestamp.clone(),
                    });
                }
            }
            _ => {}
        }
    }
}

fn extract_tool_summary(name: &str, input: Option<&serde_json::Value>) -> String {
    let target = input.and_then(|v| {
        match name {
            "Bash" => v.get("command").and_then(|c| c.as_str()),
            "Edit" | "Write" | "Read" => v.get("file_path").and_then(|f| f.as_str()),
            "Grep" | "Glob" => v.get("pattern").and_then(|p| p.as_str()),
            "WebFetch" => v.get("url").and_then(|u| u.as_str()),
            "WebSearch" => v.get("query").and_then(|q| q.as_str()),
            "Agent" => v.get("prompt").and_then(|p| p.as_str()),
            _ => None,
        }
    });

    match target {
        Some(t) => format!("{}: {}", name, truncate_str(t, 80)),
        None => name.to_string(),
    }
}

fn extract_block_text(block: &serde_json::Value) -> String {
    // Tool result content can be a string or array of content blocks
    if let Some(content) = block.get("content") {
        match content {
            serde_json::Value::String(s) => return s.clone(),
            serde_json::Value::Array(blocks) => {
                let texts: Vec<&str> = blocks
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                            b.get("text").and_then(|t| t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                return texts.join("\n");
            }
            _ => {}
        }
    }
    String::new()
}

fn extract_system_text(entry: &serde_json::Value) -> Option<String> {
    // System entries have various subtypes — extract meaningful text
    let subtype = entry.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
    match subtype {
        "api_error" => {
            let cause = entry.get("cause").and_then(|c| c.get("code")).and_then(|c| c.as_str()).unwrap_or("unknown");
            Some(format!("API error: {}", cause))
        }
        "turn_duration" => None, // Not useful to display
        _ => {
            let level = entry.get("level").and_then(|l| l.as_str()).unwrap_or("");
            let message = entry.get("message").and_then(|m| m.as_str()).unwrap_or("");
            if !message.is_empty() {
                Some(format!("[{}] {}", level, message))
            } else {
                None
            }
        }
    }
}

fn truncate_str(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    // Walk backwards from max to find a valid char boundary
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

fn truncate_or_persisted(text: &str, max: usize) -> String {
    if text.contains("<persisted-output>") || text.contains("persisted-output") {
        return "[Output saved to file]".to_string();
    }
    truncate_str(text, max)
}
