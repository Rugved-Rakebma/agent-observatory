mod scanner;

use scanner::{scan_sessions, ProjectGroup};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

struct AppState {
    groups: Mutex<Vec<ProjectGroup>>,
}

#[tauri::command]
fn get_session_groups(state: tauri::State<AppState>) -> Vec<ProjectGroup> {
    let mut groups = state.groups.lock().unwrap();
    *groups = scan_sessions();
    groups.clone()
}

#[tauri::command]
fn focus_session(pid: u32) -> Result<(), String> {
    // For now, find the terminal source and activate it
    let groups = scan_sessions();
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
        "VS Code" => activate_app("com.microsoft.VSCode"),
        "Ghostty" => activate_app("com.mitchellh.ghostty"),
        "Terminal" => activate_app("com.apple.Terminal"),
        "Warp" => activate_app("dev.warp.Warp-Stable"),
        _ => activate_app_by_name(&session.source),
    }
}

fn focus_iterm2(pid: u32) -> Result<(), String> {
    // Read ITERM_SESSION_ID from the process environment
    let iterm_id = scanner::read_process_env(pid, "ITERM_SESSION_ID");

    let script = if let Some(ref id) = iterm_id {
        // Extract the UUID portion (format: w0t0p0:UUID)
        let uuid = id.split(':').last().unwrap_or(id);

        // Validate to prevent AppleScript injection
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
        // Fallback: just activate iTerm2
        r#"tell application "iTerm2" to activate"#.to_string()
    };

    std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("Failed to run AppleScript: {}", e))?;

    Ok(())
}

fn activate_app(bundle_id: &str) -> Result<(), String> {
    let script = format!(
        r#"tell application id "{}" to activate"#,
        bundle_id
    );
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("Failed to activate app: {}", e))?;
    Ok(())
}

fn activate_app_by_name(name: &str) -> Result<(), String> {
    let script = format!(
        r#"tell application "{}" to activate"#,
        name
    );
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("Failed to activate app: {}", e))?;
    Ok(())
}

fn start_poll_timer(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            let groups = scan_sessions();

            if let Some(state) = app.try_state::<AppState>() {
                let mut current = state.groups.lock().unwrap();
                *current = groups.clone();
            }

            let _ = app.emit("sessions-changed", &groups);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            groups: Mutex::new(vec![]),
        })
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // Start the 3-second poll timer
            start_poll_timer(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_session_groups, focus_session])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
