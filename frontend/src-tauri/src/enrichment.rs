use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct SessionInference {
    pub status: String,
    pub activity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonlEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    message: Option<MessageInfo>,
    #[serde(rename = "toolUseResult")]
    tool_use_result: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MessageInfo {
    stop_reason: Option<String>,
    content: Option<serde_json::Value>,
}

pub fn infer_session_status(session_id: &str, raw_cwd: &str) -> SessionInference {
    let default = SessionInference {
        status: "Unknown".to_string(),
        activity: None,
    };

    let jsonl_path = match find_jsonl_path(session_id, raw_cwd) {
        Some(p) => p,
        None => return default,
    };

    let mtime_age = file_age_secs(&jsonl_path).unwrap_or(9999.0);

    let tail = match read_tail(&jsonl_path, 16384) {
        Some(t) => t,
        None => return default,
    };

    let lines: Vec<&str> = tail.lines().rev().collect();

    let mut last_entry: Option<JsonlEntry> = None;
    let mut last_tool_name: Option<String> = None;

    for line in &lines {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<JsonlEntry>(line) {
            let entry_type = entry.entry_type.as_deref().unwrap_or("");

            if entry_type == "progress" || entry_type == "file-history-snapshot" {
                continue;
            }

            if entry_type == "assistant" && last_tool_name.is_none() {
                last_tool_name = extract_tool_name(&entry);
            }

            if last_entry.is_none() && (entry_type == "assistant" || entry_type == "user") {
                last_entry = Some(entry);
            }

            if last_entry.is_some() && (last_tool_name.is_some() || lines.len() > 20) {
                break;
            }
        }
    }

    let entry = match last_entry {
        Some(e) => e,
        None => return default,
    };

    infer_from_entry(entry, mtime_age, last_tool_name).unwrap_or(default)
}

fn extract_tool_name(entry: &JsonlEntry) -> Option<String> {
    let content = entry.message.as_ref()?.content.as_ref()?;
    let arr = content.as_array()?;
    arr.iter().rev()
        .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .and_then(|b| b.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

fn infer_from_entry(
    entry: JsonlEntry,
    mtime_age: f64,
    last_tool_name: Option<String>,
) -> Option<SessionInference> {
    let entry_type = entry.entry_type.as_deref()?;

    match entry_type {
        "assistant" => {
            let stop_reason = entry.message.as_ref()
                .and_then(|m| m.stop_reason.as_deref())
                .unwrap_or("");

            let inference = match stop_reason {
                "end_turn" if mtime_age < 5.0 => {
                    SessionInference { status: "Idle".into(), activity: Some("Finished responding".into()) }
                }
                "end_turn" => {
                    SessionInference { status: "Idle".into(), activity: Some("Waiting for prompt".into()) }
                }
                "tool_use" => {
                    let tool_label = last_tool_name.as_deref().unwrap_or("tool");
                    if mtime_age < 10.0 {
                        SessionInference { status: "Working".into(), activity: Some(format!("Tool: {}", tool_label)) }
                    } else {
                        SessionInference { status: "WaitingInput".into(), activity: Some(format!("Permission: {}", tool_label)) }
                    }
                }
                _ if mtime_age < 5.0 => {
                    SessionInference { status: "Working".into(), activity: last_tool_name.map(|t| format!("Tool: {}", t)) }
                }
                _ => {
                    SessionInference { status: "Unknown".into(), activity: None }
                }
            };
            Some(inference)
        }
        "user" => {
            let inference = if entry.tool_use_result.is_some() {
                if mtime_age < 10.0 {
                    SessionInference { status: "Working".into(), activity: Some("Processing tool result".into()) }
                } else {
                    SessionInference { status: "WaitingInput".into(), activity: Some("May need input".into()) }
                }
            } else if mtime_age < 30.0 {
                SessionInference { status: "Working".into(), activity: Some("Processing prompt".into()) }
            } else {
                SessionInference { status: "Idle".into(), activity: Some("Waiting for prompt".into()) }
            };
            Some(inference)
        }
        _ => None,
    }
}

fn find_jsonl_path(session_id: &str, raw_cwd: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let encoded = encode_cwd(raw_cwd);
    let path = home
        .join(".claude")
        .join("projects")
        .join(&encoded)
        .join(format!("{}.jsonl", session_id));

    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn encode_cwd(cwd: &str) -> String {
    cwd.replace('/', "-").replace(' ', "-")
}

fn file_age_secs(path: &Path) -> Option<f64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).unwrap_or(Duration::from_secs(9999));
    Some(age.as_secs_f64())
}

fn read_tail(path: &Path, bytes: u64) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let file_size = file.metadata().ok()?.len();

    let offset = file_size.saturating_sub(bytes);
    file.seek(SeekFrom::Start(offset)).ok()?;

    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;

    if offset > 0 {
        if let Some(nl) = buf.find('\n') {
            buf = buf[nl + 1..].to_string();
        }
    }

    Some(buf)
}
