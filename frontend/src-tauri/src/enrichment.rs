use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const MAX_LINE_SIZE: usize = 10 * 1024 * 1024; // 10MB OOM guard
const TAIL_BYTES: u64 = 32768; // 32KB

#[derive(Debug, Clone)]
pub struct EnrichedData {
    pub status: String,
    pub activity: Option<String>,
    pub slug: Option<String>,
    pub model: Option<String>,
    pub context_used: Option<u64>,
    pub context_max: Option<u64>,
    pub git_branch: Option<String>,
    pub last_message: Option<String>,
}

impl Default for EnrichedData {
    fn default() -> Self {
        Self {
            status: "Unknown".into(),
            activity: None,
            slug: None,
            model: None,
            context_used: None,
            context_max: None,
            git_branch: None,
            last_message: None,
        }
    }
}

pub(crate) struct CacheEntry {
    mtime: SystemTime,
    data: EnrichedData,
}

pub type EnrichmentCache = Arc<Mutex<HashMap<String, CacheEntry>>>;

pub fn new_enrichment_cache() -> EnrichmentCache {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn prune_cache(cache: &EnrichmentCache, active_ids: &[String]) {
    if let Ok(mut map) = cache.lock() {
        map.retain(|k, _| active_ids.contains(k));
    }
}

pub fn enrich_session(session_id: &str, raw_cwd: &str, cache: &EnrichmentCache) -> EnrichedData {
    let jsonl_path = match find_jsonl_path(session_id, raw_cwd) {
        Some(p) => p,
        None => return EnrichedData::default(),
    };

    let current_mtime = match std::fs::metadata(&jsonl_path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return EnrichedData::default(),
    };

    // Check cache — return cached data if mtime unchanged
    if let Ok(map) = cache.lock() {
        if let Some(entry) = map.get(session_id) {
            if entry.mtime == current_mtime {
                return entry.data.clone();
            }
        }
    }

    // mtime changed — re-parse
    let mtime_age = SystemTime::now()
        .duration_since(current_mtime)
        .unwrap_or(Duration::from_secs(9999))
        .as_secs_f64();

    let data = match read_tail(&jsonl_path, TAIL_BYTES) {
        Some(tail) => parse_jsonl_tail(&tail, mtime_age),
        None => EnrichedData::default(),
    };

    // Update cache
    if let Ok(mut map) = cache.lock() {
        map.insert(
            session_id.to_string(),
            CacheEntry {
                mtime: current_mtime,
                data: data.clone(),
            },
        );
    }

    data
}

// ── JSONL types ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonlEntry {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    message: Option<MessageInfo>,
    tool_use_result: Option<serde_json::Value>,
    slug: Option<String>,
    git_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageInfo {
    model: Option<String>,
    stop_reason: Option<String>,
    content: Option<serde_json::Value>,
    usage: Option<UsageInfo>,
}

#[derive(Debug, Deserialize)]
struct UsageInfo {
    input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

// ── Parsing ──

fn parse_jsonl_tail(tail: &str, mtime_age: f64) -> EnrichedData {
    let mut result = EnrichedData::default();
    let mut last_entry: Option<JsonlEntry> = None;
    let mut last_tool_name: Option<String> = None;
    let mut found_assistant_for_message = false;

    for line in tail.lines().rev() {
        if line.is_empty() || line.len() > MAX_LINE_SIZE {
            continue;
        }

        let entry: JsonlEntry = match serde_json::from_str(line) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let entry_type = match entry.entry_type.as_deref() {
            Some(t) => t,
            None => continue,
        };

        if entry_type == "progress" || entry_type == "file-history-snapshot" {
            continue;
        }

        result.slug = result.slug.or_else(|| entry.slug.clone());
        result.git_branch = result.git_branch.or_else(|| entry.git_branch.clone());

        // Extract tool name from assistant entries
        if entry_type == "assistant" && last_tool_name.is_none() {
            last_tool_name = extract_tool_name(&entry);
        }

        // Extract model, context, last_message from assistant entries
        if entry_type == "assistant" && !found_assistant_for_message {
            if let Some(ref msg) = entry.message {
                // Model
                if result.model.is_none() {
                    if let Some(ref m) = msg.model {
                        let display = model_display_name(m);
                        result.context_max = Some(context_max_for_model(m));
                        result.model = Some(display);
                    }
                }

                // Context usage from usage block
                if result.context_used.is_none() {
                    if let Some(ref usage) = msg.usage {
                        let input = usage.input_tokens.unwrap_or(0);
                        let cache_create = usage.cache_creation_input_tokens.unwrap_or(0);
                        let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
                        let total = input + cache_create + cache_read;
                        if total > 0 {
                            result.context_used = Some(total);
                        }
                    }
                }

                // Last message text (from end_turn entries only)
                if result.last_message.is_none() && msg.stop_reason.as_deref() == Some("end_turn") {
                    result.last_message = extract_last_message(msg);
                    found_assistant_for_message = true;
                }
            }
        }

        // Status inference from last user/assistant entry
        if last_entry.is_none() && (entry_type == "assistant" || entry_type == "user") {
            last_entry = Some(entry);
            continue;
        }

        // Stop scanning once we have everything
        if last_entry.is_some()
            && result.slug.is_some()
            && result.model.is_some()
            && found_assistant_for_message
        {
            break;
        }
    }

    // Infer status from the last entry
    if let Some(entry) = last_entry {
        if let Some(inferred) = infer_from_entry(entry, mtime_age, last_tool_name) {
            result.status = inferred.status;
            result.activity = inferred.activity;
        }
    }

    result
}

fn extract_tool_name(entry: &JsonlEntry) -> Option<String> {
    let content = entry.message.as_ref()?.content.as_ref()?;
    let arr = content.as_array()?;
    arr.iter()
        .rev()
        .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .and_then(|b| b.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

fn extract_last_message(msg: &MessageInfo) -> Option<String> {
    let content = msg.content.as_ref()?;
    let arr = content.as_array()?;

    // Find the last text block
    let text = arr
        .iter()
        .rev()
        .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())?;

    Some(truncate_at_word(text, 120))
}

fn truncate_at_word(s: &str, max: usize) -> String {
    let normalized: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() <= max {
        return normalized;
    }
    // Find a valid char boundary at or before max
    let mut end = max;
    while end > 0 && !normalized.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &normalized[..end];
    match truncated.rfind(' ') {
        Some(pos) => format!("{}...", &truncated[..pos]),
        None => format!("{}...", truncated),
    }
}

struct StatusInference {
    status: String,
    activity: Option<String>,
}

fn infer_from_entry(
    entry: JsonlEntry,
    mtime_age: f64,
    last_tool_name: Option<String>,
) -> Option<StatusInference> {
    let entry_type = entry.entry_type.as_deref()?;

    match entry_type {
        "assistant" => {
            let stop_reason = entry
                .message
                .as_ref()
                .and_then(|m| m.stop_reason.as_deref())
                .unwrap_or("");

            let inference = match stop_reason {
                "end_turn" if mtime_age < 5.0 => StatusInference {
                    status: "Idle".into(),
                    activity: Some("Finished responding".into()),
                },
                "end_turn" => StatusInference {
                    status: "Idle".into(),
                    activity: Some("Waiting for prompt".into()),
                },
                "tool_use" => {
                    let tool_label = last_tool_name.as_deref().unwrap_or("tool");
                    if mtime_age < 10.0 {
                        StatusInference {
                            status: "Working".into(),
                            activity: Some(format!("Tool: {}", tool_label)),
                        }
                    } else {
                        StatusInference {
                            status: "WaitingInput".into(),
                            activity: Some(format!("Permission: {}", tool_label)),
                        }
                    }
                }
                _ if mtime_age < 5.0 => StatusInference {
                    status: "Working".into(),
                    activity: last_tool_name.map(|t| format!("Tool: {}", t)),
                },
                _ => StatusInference {
                    status: "Unknown".into(),
                    activity: None,
                },
            };
            Some(inference)
        }
        "user" => {
            let inference = if entry.tool_use_result.is_some() {
                if mtime_age < 10.0 {
                    StatusInference {
                        status: "Working".into(),
                        activity: Some("Processing tool result".into()),
                    }
                } else {
                    StatusInference {
                        status: "WaitingInput".into(),
                        activity: Some("May need input".into()),
                    }
                }
            } else if mtime_age < 30.0 {
                StatusInference {
                    status: "Working".into(),
                    activity: Some("Processing prompt".into()),
                }
            } else {
                StatusInference {
                    status: "Idle".into(),
                    activity: Some("Waiting for prompt".into()),
                }
            };
            Some(inference)
        }
        _ => None,
    }
}

// ── Helpers ──

fn model_display_name(raw: &str) -> String {
    if raw.contains("opus") {
        "Opus".into()
    } else if raw.contains("sonnet") {
        "Sonnet".into()
    } else if raw.contains("haiku") {
        "Haiku".into()
    } else {
        raw.to_string()
    }
}

fn context_max_for_model(raw: &str) -> u64 {
    if raw.contains("haiku") {
        200_000
    } else {
        1_000_000
    }
}

pub(crate) fn find_jsonl_path(session_id: &str, raw_cwd: &str) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let encoded = raw_cwd.replace('/', "-").replace(' ', "-");
    let project_dir = home.join(".claude").join("projects").join(&encoded);
    let path = project_dir.join(format!("{}.jsonl", session_id));

    // If the exact session JSONL exists and is recently modified, use it
    if path.exists() {
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let age = mtime.and_then(|m| m.elapsed().ok()).map(|d| d.as_secs());
        // If modified within last 5 minutes, trust it
        if age.map_or(false, |a| a < 300) {
            return Some(path);
        }
    }

    // Otherwise, find the most recently modified JSONL in the project dir.
    // This handles /clear creating a new session without updating the session file.
    let newest = std::fs::read_dir(&project_dir)
        .ok()?
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                == Some("jsonl")
        })
        .max_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        })?;

    let newest_path = newest.path();
    // Only prefer the newest if it's more recent than the original
    if newest_path != path {
        let newest_mtime = std::fs::metadata(&newest_path)
            .and_then(|m| m.modified())
            .ok();
        let original_mtime = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok();
        if newest_mtime > original_mtime {
            return Some(newest_path);
        }
    }

    path.exists().then_some(path)
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
