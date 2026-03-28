use axum::{extract::State as AxumState, http::StatusCode, routing::{get, post}, Json, Router};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize)]
pub struct HookEvent {
    pub session_id: Option<String>,
    pub hook_event_name: Option<String>,
    pub notification_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HookStatus {
    pub status: String,
    pub activity: Option<String>,
    pub timestamp: std::time::Instant,
}

pub type HookState = Arc<Mutex<HashMap<String, HookStatus>>>;

pub fn new_hook_state() -> HookState {
    Arc::new(Mutex::new(HashMap::new()))
}

pub async fn start_hook_server(state: HookState) {
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
    AxumState(state): AxumState<HookState>,
    Json(event): Json<HookEvent>,
) -> StatusCode {
    let session_id = match &event.session_id {
        Some(id) => id.clone(),
        None => return StatusCode::BAD_REQUEST,
    };

    let hook_name = event.hook_event_name.as_deref().unwrap_or("");

    let hook_status = match hook_name {
        "PermissionRequest" => HookStatus {
            status: "WaitingInput".to_string(),
            activity: Some("Permission requested".to_string()),
            timestamp: std::time::Instant::now(),
        },
        "Notification" => {
            let notif_type = event.notification_type.as_deref().unwrap_or("");
            match notif_type {
                "permission_prompt" => HookStatus {
                    status: "WaitingInput".to_string(),
                    activity: Some("Permission prompt".to_string()),
                    timestamp: std::time::Instant::now(),
                },
                "idle_prompt" => HookStatus {
                    status: "Idle".to_string(),
                    activity: Some("Waiting for prompt".to_string()),
                    timestamp: std::time::Instant::now(),
                },
                _ => return StatusCode::OK,
            }
        }
        "UserPromptSubmit" => HookStatus {
            status: "Working".to_string(),
            activity: Some("Processing prompt".to_string()),
            timestamp: std::time::Instant::now(),
        },
        "Stop" => HookStatus {
            status: "Idle".to_string(),
            activity: Some("Finished responding".to_string()),
            timestamp: std::time::Instant::now(),
        },
        "PostToolUseFailure" => HookStatus {
            status: "Working".to_string(),
            activity: Some("Tool failed — retrying".to_string()),
            timestamp: std::time::Instant::now(),
        },
        "SessionStart" => HookStatus {
            status: "Idle".to_string(),
            activity: Some("Session started".to_string()),
            timestamp: std::time::Instant::now(),
        },
        _ => return StatusCode::OK,
    };

    if let Ok(mut map) = state.lock() {
        map.insert(session_id, hook_status);
    }

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
