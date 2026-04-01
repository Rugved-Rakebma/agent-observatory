mod conversation;
mod enrichment;
mod hooks;
mod scanner;

use enrichment::EnrichmentCache;
use hooks::HookState;
use scanner::{scan_sessions, ProjectGroup};
use tauri::{AppHandle, Emitter};

struct AppState {
    hook_state: HookState,
    cache: EnrichmentCache,
}

#[tauri::command]
fn get_session_groups(state: tauri::State<AppState>) -> Vec<ProjectGroup> {
    let groups = scan_sessions(&state.hook_state, &state.cache);
    prune_cache(&state.cache, &groups);
    groups
}

#[tauri::command]
fn get_conversation(session_id: String, cwd: String) -> Result<conversation::ConversationData, String> {
    conversation::parse_conversation(&session_id, &cwd)
        .ok_or_else(|| "Could not load conversation".to_string())
}

#[tauri::command]
fn focus_session(pid: u32, state: tauri::State<AppState>) -> Result<(), String> {
    let groups = scan_sessions(&state.hook_state, &state.cache);
    let session = groups
        .iter()
        .flat_map(|g| &g.sessions)
        .find(|s| s.pid == pid);

    let session = match session {
        Some(s) => s,
        None => return Err(format!("Session with PID {} not found", pid)),
    };

    match session.source.as_str() {
        "iTerm2" => focus_iterm2(pid),
        "VS Code" => run_osascript(r#"tell application id "com.microsoft.VSCode" to activate"#),
        "Ghostty" => run_osascript(r#"tell application id "com.mitchellh.ghostty" to activate"#),
        "Terminal" => run_osascript(r#"tell application id "com.apple.Terminal" to activate"#),
        "Warp" => run_osascript(r#"tell application id "dev.warp.Warp-Stable" to activate"#),
        other => run_osascript(&format!(r#"tell application "{}" to activate"#, other)),
    }
}

fn focus_iterm2(pid: u32) -> Result<(), String> {
    let iterm_id = scanner::read_process_env(pid, "ITERM_SESSION_ID");

    let script = if let Some(ref id) = iterm_id {
        let uuid = id.split(':').last().unwrap_or(id);
        if !uuid.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err("Invalid iTerm session ID".to_string());
        }

        format!(
            r#"tell application "iTerm2"
    repeat with aWindow in windows
        repeat with aTab in tabs of aWindow
            repeat with aSession in sessions of aTab
                if unique ID of aSession is "{uuid}" then
                    select aTab
                    tell aWindow to select
                    activate
                    return
                end if
            end repeat
        end repeat
    end repeat
    activate
end tell"#
        )
    } else {
        r#"tell application "iTerm2" to activate"#.to_string()
    };

    run_osascript(&script)
}

fn run_osascript(script: &str) -> Result<(), String> {
    std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|e| format!("Failed to run AppleScript: {}", e))?;
    Ok(())
}

fn prune_cache(cache: &EnrichmentCache, groups: &[ProjectGroup]) {
    let active_ids: Vec<String> = groups
        .iter()
        .flat_map(|g| &g.sessions)
        .map(|s| s.session_id.clone())
        .collect();
    enrichment::prune_cache(cache, &active_ids);
}

fn start_poll_timer(app: AppHandle, hook_state: HookState, cache: EnrichmentCache) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            let groups = scan_sessions(&hook_state, &cache);
            prune_cache(&cache, &groups);
            let _ = app.emit("sessions-changed", &groups);
        }
    });
}

fn start_hook_server(hook_state: HookState, cache: EnrichmentCache, app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        hooks::start_hook_server(hook_state, cache, app_handle).await;
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let hook_state = hooks::new_hook_state();
    let cache = enrichment::new_enrichment_cache();

    tauri::Builder::default()
        .manage(AppState {
            hook_state: hook_state.clone(),
            cache: cache.clone(),
        })
        .setup(move |app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            start_hook_server(hook_state.clone(), cache.clone(), app.handle().clone());
            start_poll_timer(app.handle().clone(), hook_state, cache);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_session_groups, get_conversation, focus_session])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
