use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// macOS-specific process inspection (not in libc crate)
extern "C" {
    fn proc_pidpath(pid: libc::c_int, buf: *mut libc::c_void, bufsize: u32) -> libc::c_int;
}

/// Raw session file from ~/.claude/sessions/{pid}.json
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionFile {
    pid: u32,
    session_id: String,
    cwd: String,
    started_at: u64,
}

/// Session as exposed to the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub pid: u32,
    pub session_id: String,
    pub cwd: String,
    pub started_at: u64,
    pub status: String,
    pub activity: Option<String>,
    pub source: String,
}

/// Group of sessions sharing the same project directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectGroup {
    pub cwd: String,
    pub display_name: String,
    pub sessions: Vec<Session>,
}

/// Scan ~/.claude/sessions/ and return grouped sessions
pub fn scan_sessions() -> Vec<ProjectGroup> {
    let sessions_dir = match dirs::home_dir() {
        Some(home) => home.join(".claude").join("sessions"),
        None => return vec![],
    };

    if !sessions_dir.exists() {
        return vec![];
    }

    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut sessions: Vec<Session> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        if let Some(session) = read_session_file(&path) {
            sessions.push(session);
        }
    }

    group_by_project(sessions)
}

fn read_session_file(path: &Path) -> Option<Session> {
    let content = std::fs::read_to_string(path).ok()?;
    let file: SessionFile = serde_json::from_str(&content).ok()?;

    // Validate PID is alive
    if !is_pid_alive(file.pid) {
        return None;
    }

    // Validate it's actually a claude process
    if !is_claude_process(file.pid) {
        return None;
    }

    // Resolve cwd to git root for consistent grouping
    let cwd = resolve_git_root(&file.cwd).unwrap_or_else(|| file.cwd.clone());

    let source = detect_terminal_source(file.pid);

    Some(Session {
        pid: file.pid,
        session_id: file.session_id,
        cwd,
        started_at: file.started_at,
        status: "Unknown".to_string(),
        activity: None,
        source,
    })
}

fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn get_exe_path(pid: u32) -> Option<String> {
    let mut buf = [0u8; 4096];
    let len = unsafe {
        proc_pidpath(
            pid as i32,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };
    if len <= 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&buf[..len as usize]).to_string())
}

fn is_claude_process(pid: u32) -> bool {
    // If proc_pidpath works, verify it's claude
    // If it fails (common on macOS due to security restrictions),
    // trust the session file — its existence in ~/.claude/sessions/ is proof enough
    match get_exe_path(pid) {
        Some(path) => path.contains("claude") || path.contains("Claude") || path.contains("node"),
        None => true,
    }
}

fn resolve_git_root(cwd: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", cwd, "rev-parse", "--show-toplevel"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|s| s.trim().to_string())
}

fn detect_terminal_source(pid: u32) -> String {
    // Read TERM_PROGRAM from the process environment
    if let Some(term) = read_process_env(pid, "TERM_PROGRAM") {
        return match term.as_str() {
            "iTerm.app" => "iTerm2".to_string(),
            "Apple_Terminal" => "Terminal".to_string(),
            "ghostty" => "Ghostty".to_string(),
            "WarpTerminal" => "Warp".to_string(),
            "vscode" => "VS Code".to_string(),
            other => other.to_string(),
        };
    }

    // Fallback: check executable path for VS Code extension
    if let Some(path) = get_exe_path(pid) {
        if path.contains(".vscode") {
            return "VS Code".to_string();
        }
    }

    // Fallback: check VSCODE env vars that survive into terminals
    if read_process_env(pid, "VSCODE_GIT_IPC_HANDLE").is_some()
        || read_process_env(pid, "TERM_PROGRAM_VERSION").as_deref() == Some("vscode")
    {
        return "VS Code".to_string();
    }

    "Unknown".to_string()
}

/// Read an environment variable from another process using sysctl KERN_PROCARGS2
pub fn read_process_env(pid: u32, var_name: &str) -> Option<String> {
    let mut mib: [libc::c_int; 3] = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size: libc::size_t = 0;

    // First call: get buffer size
    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 || size == 0 {
        return None;
    }

    // Second call: get the data
    let mut buf: Vec<u8> = vec![0; size];
    let ret = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 {
        return None;
    }

    buf.truncate(size);

    // Parse KERN_PROCARGS2 format:
    // [argc: i32] [exec_path\0] [padding\0...] [argv strings\0...] [env strings\0...]
    if buf.len() < 4 {
        return None;
    }

    let argc = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let mut pos = 4;

    // Skip exec path
    while pos < buf.len() && buf[pos] != 0 {
        pos += 1;
    }
    // Skip null padding after exec path
    while pos < buf.len() && buf[pos] == 0 {
        pos += 1;
    }

    // Skip argc argv strings
    let mut args_skipped = 0;
    while pos < buf.len() && args_skipped < argc {
        while pos < buf.len() && buf[pos] != 0 {
            pos += 1;
        }
        pos += 1;
        args_skipped += 1;
    }

    // Scan environment section for the target variable
    let prefix = format!("{}=", var_name);
    while pos < buf.len() {
        let start = pos;
        while pos < buf.len() && buf[pos] != 0 {
            pos += 1;
        }
        if pos == start {
            break;
        }
        if let Ok(entry) = std::str::from_utf8(&buf[start..pos]) {
            if entry.starts_with(&prefix) {
                return Some(entry[prefix.len()..].to_string());
            }
        }
        pos += 1;
    }

    None
}

fn group_by_project(sessions: Vec<Session>) -> Vec<ProjectGroup> {
    let mut groups: BTreeMap<String, Vec<Session>> = BTreeMap::new();

    for session in sessions {
        groups
            .entry(session.cwd.clone())
            .or_default()
            .push(session);
    }

    groups
        .into_iter()
        .map(|(cwd, mut sessions)| {
            sessions.sort_by_key(|s| s.started_at);

            let display_name = PathBuf::from(&cwd)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            ProjectGroup {
                cwd,
                display_name,
                sessions,
            }
        })
        .collect()
}
