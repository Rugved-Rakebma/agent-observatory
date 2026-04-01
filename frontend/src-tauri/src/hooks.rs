use axum::{extract::State as AxumState, http::StatusCode, routing::{get, post}, Json, Router};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

use crate::enrichment::EnrichmentCache;
use crate::scanner::{scan_sessions, ProjectGroup};

#[derive(Debug, Deserialize)]
pub struct HookEvent {
    pub session_id: Option<String>,
    pub hook_event_name: Option<String>,
    pub notification_type: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct HookStatus {
    pub status: String,
    pub activity: Option<String>,
    pub tool_detail: Option<String>,
    pub timestamp: std::time::Instant,
}

pub type HookState = Arc<Mutex<HashMap<String, HookStatus>>>;

pub fn new_hook_state() -> HookState {
    Arc::new(Mutex::new(HashMap::new()))
}

#[derive(Clone)]
struct HookServerState {
    hook_state: HookState,
    cache: EnrichmentCache,
    app_handle: AppHandle,
}

pub async fn start_hook_server(hook_state: HookState, cache: EnrichmentCache, app_handle: AppHandle) {
    let state = HookServerState { hook_state, cache, app_handle };
    let app = Router::new()
        .route("/hook", post(handle_hook))
        .route("/health", get(health))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:7890").await {
        Ok(l) => l,
        Err(e) => {
            log::warn!("Failed to bind hook server on :7890: {}. Hooks disabled.", e);
            return;
        }
    };

    log::info!("Hook receiver listening on 127.0.0.1:7890");

    if let Err(e) = axum::serve(listener, app).await {
        log::error!("Hook server error: {}", e);
    }
}

async fn handle_hook(
    AxumState(state): AxumState<HookServerState>,
    Json(event): Json<HookEvent>,
) -> StatusCode {
    let session_id = match &event.session_id {
        Some(id) => id.clone(),
        None => return StatusCode::BAD_REQUEST,
    };

    let hook_name = event.hook_event_name.as_deref().unwrap_or("");

    let hook_status = match hook_name {
        "PermissionRequest" => {
            let detail = extract_tool_detail(&event);
            let activity = detail.clone().unwrap_or_else(|| "Permission requested".into());

            send_notification("Agent needs input", &activity);

            HookStatus {
                status: "WaitingInput".into(),
                activity: Some(activity),
                tool_detail: detail,
                timestamp: std::time::Instant::now(),
            }
        }
        "Notification" => {
            let notif_type = event.notification_type.as_deref().unwrap_or("");
            match notif_type {
                "permission_prompt" => {
                    send_notification("Agent needs input", "Permission prompt");

                    HookStatus {
                        status: "WaitingInput".into(),
                        activity: Some("Permission prompt".into()),
                        tool_detail: None,
                        timestamp: std::time::Instant::now(),
                    }
                }
                "idle_prompt" => HookStatus {
                    status: "Idle".into(),
                    activity: Some("Waiting for prompt".into()),
                    tool_detail: None,
                    timestamp: std::time::Instant::now(),
                },
                _ => return StatusCode::OK,
            }
        }
        "UserPromptSubmit" => HookStatus {
            status: "Working".into(),
            activity: Some("Processing prompt".into()),
            tool_detail: None,
            timestamp: std::time::Instant::now(),
        },
        "Stop" => HookStatus {
            status: "Idle".into(),
            activity: Some("Finished responding".into()),
            tool_detail: None,
            timestamp: std::time::Instant::now(),
        },
        "PostToolUseFailure" => HookStatus {
            status: "Working".into(),
            activity: Some("Tool failed — retrying".into()),
            tool_detail: None,
            timestamp: std::time::Instant::now(),
        },
        "SessionStart" => HookStatus {
            status: "Idle".into(),
            activity: Some("Session started".into()),
            tool_detail: None,
            timestamp: std::time::Instant::now(),
        },
        _ => return StatusCode::OK,
    };

    if let Ok(mut map) = state.hook_state.lock() {
        map.insert(session_id, hook_status);
    }

    // Immediate refresh: re-scan and push to frontend
    let groups = scan_sessions(&state.hook_state, &state.cache);
    let _ = state.app_handle.emit("sessions-changed", &groups);

    StatusCode::OK
}

async fn health() -> &'static str {
    "ok"
}

pub fn get_hook_status(state: &HookState, session_id: &str) -> Option<HookStatus> {
    let map = state.lock().ok()?;
    let status = map.get(session_id)?;

    if status.timestamp.elapsed() > std::time::Duration::from_secs(60) {
        None
    } else {
        Some(status.clone())
    }
}

fn extract_tool_detail(event: &HookEvent) -> Option<String> {
    let name = event.tool_name.as_deref()?;
    let input = event.tool_input.as_ref();

    let target = input.and_then(|v| {
        match name {
            "Bash" => v.get("command").and_then(|c| c.as_str()),
            "Edit" | "Write" | "Read" => v.get("file_path").and_then(|f| f.as_str()),
            "Grep" | "Glob" => v.get("pattern").and_then(|p| p.as_str()),
            "WebFetch" => v.get("url").and_then(|u| u.as_str()),
            "Agent" => v.get("subagent_type").and_then(|s| s.as_str()),
            _ => None,
        }
    });

    match target {
        Some(t) => {
            let truncated = if t.len() > 60 { &t[..60] } else { t };
            Some(format!("{}: {}", name, truncated))
        }
        None => Some(name.to_string()),
    }
}

fn send_notification(title: &str, body: &str) {
    let script = format!(
        r#"display notification "{}" with title "{}""#,
        body.replace('"', "'"),
        title.replace('"', "'"),
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output();
}
