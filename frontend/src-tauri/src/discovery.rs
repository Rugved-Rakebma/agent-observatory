//! Project discovery and session launching.
//!
//! Discovers all directories containing a `.claude/` config by walking the
//! user's home directory. Detects available custom agents and launches new
//! sessions via iTerm2 AppleScript.

use serde::Serialize;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 6;

/// Directories to skip during filesystem walk.
/// These are either large, irrelevant, or internal to build tools.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".svelte-kit",
    "build",
    "dist",
    "Library",
    "Applications",
    ".Trash",
    "Music",
    "Movies",
    "Photos",
    ".cache",
    ".npm",
    ".bun",
    ".cargo",
    ".rustup",
    ".local",
    ".docker",
    ".vscode",
    ".idea",
    ".rnd",
    "vendor",
    "__pycache__",
    ".venv",
    "venv",
];

#[derive(Debug, Clone, Serialize)]
pub struct DiscoverableProject {
    pub path: String,
    pub display_name: String,
    pub agents: Vec<String>,
    pub active_sessions: u32,
}

/// Walk a directory tree to find all directories containing a `.claude/` subdirectory.
/// Returns the parent paths (the project roots), not the `.claude/` dirs themselves.
fn find_claude_projects(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let skip: HashSet<&OsStr> = SKIP_DIRS.iter().map(|s| OsStr::new(s)).collect();
    walk_dir(root, 0, max_depth, &skip, &mut results);
    results
}

fn walk_dir(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    skip: &HashSet<&OsStr>,
    results: &mut Vec<PathBuf>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut subdirs = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_str().unwrap_or("");

        // Skip non-directories
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        if name_str == ".claude" {
            // Found a .claude dir — the parent is a project
            results.push(dir.to_path_buf());
            // Don't recurse into .claude itself, but keep scanning siblings
            continue;
        }

        // Skip excluded directories and hidden dirs (except .claude which is handled above)
        if skip.contains(name.as_os_str()) {
            continue;
        }

        // Collect subdirs for recursion (after we've checked all entries at this level)
        if depth < max_depth {
            subdirs.push(entry.path());
        }
    }

    // Recurse into subdirs
    for subdir in subdirs {
        walk_dir(&subdir, depth + 1, max_depth, skip, results);
    }
}

/// Find all custom agent .md files in a project's .claude/agents/ directory.
/// Returns agent names (filename without .md extension).
/// Validates names: only alphanumeric, hyphens, underscores allowed.
pub fn detect_agents(project_path: &str) -> Vec<String> {
    let agents_dir = Path::new(project_path).join(".claude").join("agents");
    let entries = match std::fs::read_dir(&agents_dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut agents = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if stem.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                agents.push(stem.to_string());
            }
        }
    }
    agents.sort();
    agents
}

/// Scan the filesystem for directories containing `.claude/` configs.
/// Cross-references with active session CWDs to populate active_sessions count.
pub fn scan_projects(active_cwds: &[String]) -> Vec<DiscoverableProject> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return vec![],
    };

    let project_paths = find_claude_projects(&home, MAX_DEPTH);

    let mut projects: Vec<DiscoverableProject> = Vec::new();

    for project_path in project_paths {
        let path_str = project_path.to_string_lossy().to_string();

        // Skip the user's global ~/.claude (not a project)
        if project_path == home {
            continue;
        }

        let display_name = project_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let agents = detect_agents(&path_str);

        // Count active sessions — exact match or parent/child relationship
        let active_sessions = active_cwds
            .iter()
            .filter(|cwd| {
                cwd.as_str() == path_str.as_str()
                    || path_str.starts_with(cwd.as_str())
                    || cwd.starts_with(path_str.as_str())
            })
            .count() as u32;

        projects.push(DiscoverableProject {
            path: path_str,
            display_name,
            agents,
            active_sessions,
        });
    }

    // Sort: active first, then alphabetical by display_name
    projects.sort_by(|a, b| {
        let a_active = a.active_sessions > 0;
        let b_active = b.active_sessions > 0;
        b_active
            .cmp(&a_active)
            .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
    });

    projects
}

/// Launch a new Claude Code session in iTerm2.
/// Opens a new tab, cd's to the project path, and runs `claude` (optionally with --agent).
pub fn launch_session(path: &str, agent: Option<&str>) -> Result<(), String> {
    if path.contains('\'') || path.contains('\\') || path.contains('"') {
        return Err("Path contains unsafe characters".to_string());
    }

    if !Path::new(path).is_dir() {
        return Err(format!("Directory does not exist: {}", path));
    }

    // Restrict to user's home directory
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let canonical =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve path: {}", e))?;
    if !canonical.starts_with(&home) {
        return Err("Path must be within home directory".to_string());
    }

    let cmd = match agent {
        Some(name) => {
            if !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                return Err("Invalid agent name".to_string());
            }
            format!("cd '{}' && claude --agent '{}'", path, name)
        }
        None => format!("cd '{}' && claude", path),
    };

    let script = format!(
        r#"tell application "iTerm2"
    if (count of windows) = 0 then
        create window with default profile
    else
        tell current window
            create tab with default profile
        end tell
    end if
    tell current session of current window
        write text "{}"
    end tell
    activate
end tell"#,
        cmd
    );

    std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("Failed to run AppleScript: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_claude_projects() {
        if !std::path::Path::new("/Users/rugvedambekar").exists() {
            eprintln!("Skipping: not on development machine");
            return;
        }
        let home = dirs::home_dir().unwrap();
        let projects = find_claude_projects(&home, MAX_DEPTH);

        // Should find at least the agent-observatory project itself
        assert!(
            projects.iter().any(|p| p.ends_with("agent-observatory")),
            "Should find agent-observatory in {:?}",
            projects
        );

        // Home dir will be in raw results (has ~/.claude), but scan_projects filters it
        assert!(
            projects.len() > 5,
            "Should find multiple projects, got {}",
            projects.len()
        );
    }

    #[test]
    fn test_scan_excludes_home() {
        if !std::path::Path::new("/Users/rugvedambekar").exists() {
            eprintln!("Skipping: not on development machine");
            return;
        }
        let projects = scan_projects(&[]);

        // Home dir (~/) has .claude/ but should be excluded
        let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
        assert!(
            !projects.iter().any(|p| p.path == home),
            "Should not include home dir as a project"
        );
    }
}
