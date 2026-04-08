//! Application state for the TUI.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A tracked jail instance.
#[derive(Debug, Clone)]
pub struct JailInfo {
    pub id: u32,
    pub pid: u32,
    pub command: String,
    pub preset: String,
    pub status: JailStatus,
    pub started_at: Instant,
    pub memory_bytes: u64,
    pub cpu_percent: f32,
    pub stdout_lines: Vec<String>,
    pub stderr_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JailStatus {
    Running,
    Completed(i32),
    TimedOut,
    Killed,
}

impl JailStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Completed(0) => "Success",
            Self::Completed(_) => "Failed",
            Self::TimedOut => "Timeout",
            Self::Killed => "Killed",
        }
    }
}

/// TUI view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Detail,
}

/// Application state.
pub struct App {
    pub jails: HashMap<u32, JailInfo>,
    pub selected: Option<u32>,
    pub view: View,
    pub should_quit: bool,
    pub scroll_offset: usize,
    next_id: u32,
}

impl App {
    pub fn new() -> Self {
        Self {
            jails: HashMap::new(),
            selected: None,
            view: View::List,
            should_quit: false,
            scroll_offset: 0,
            next_id: 1,
        }
    }

    /// Add a jail for tracking.
    pub fn add_jail(&mut self, pid: u32, command: String, preset: String) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        let info = JailInfo {
            id,
            pid,
            command,
            preset,
            status: JailStatus::Running,
            started_at: Instant::now(),
            memory_bytes: 0,
            cpu_percent: 0.0,
            stdout_lines: Vec::new(),
            stderr_lines: Vec::new(),
        };

        self.jails.insert(id, info);

        if self.selected.is_none() {
            self.selected = Some(id);
        }

        id
    }

    /// Update jail status.
    pub fn update_status(&mut self, id: u32, status: JailStatus) {
        if let Some(jail) = self.jails.get_mut(&id) {
            jail.status = status;
        }
    }

    /// Update jail resource usage.
    pub fn update_resources(&mut self, id: u32, memory_bytes: u64, cpu_percent: f32) {
        if let Some(jail) = self.jails.get_mut(&id) {
            jail.memory_bytes = memory_bytes;
            jail.cpu_percent = cpu_percent;
        }
    }

    /// Append stdout line.
    pub fn append_stdout(&mut self, id: u32, line: String) {
        if let Some(jail) = self.jails.get_mut(&id) {
            jail.stdout_lines.push(line);
            // Keep last 1000 lines
            if jail.stdout_lines.len() > 1000 {
                jail.stdout_lines.remove(0);
            }
        }
    }

    /// Append stderr line.
    pub fn append_stderr(&mut self, id: u32, line: String) {
        if let Some(jail) = self.jails.get_mut(&id) {
            jail.stderr_lines.push(line);
            if jail.stderr_lines.len() > 1000 {
                jail.stderr_lines.remove(0);
            }
        }
    }

    /// Get sorted list of jail IDs.
    pub fn sorted_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.jails.keys().copied().collect();
        ids.sort();
        ids
    }

    /// Navigate to next jail.
    pub fn next(&mut self) {
        let ids = self.sorted_ids();
        if ids.is_empty() {
            return;
        }

        let idx = self
            .selected
            .and_then(|s| ids.iter().position(|&id| id == s))
            .map(|i| (i + 1) % ids.len())
            .unwrap_or(0);

        self.selected = Some(ids[idx]);
    }

    /// Navigate to previous jail.
    pub fn prev(&mut self) {
        let ids = self.sorted_ids();
        if ids.is_empty() {
            return;
        }

        let idx = self
            .selected
            .and_then(|s| ids.iter().position(|&id| id == s))
            .map(|i| if i == 0 { ids.len() - 1 } else { i - 1 })
            .unwrap_or(0);

        self.selected = Some(ids[idx]);
    }

    /// Toggle view.
    pub fn toggle_view(&mut self) {
        self.view = match self.view {
            View::List => View::Detail,
            View::Detail => View::List,
        };
    }

    /// Get selected jail.
    pub fn selected_jail(&self) -> Option<&JailInfo> {
        self.selected.and_then(|id| self.jails.get(&id))
    }

    /// Kill selected jail.
    pub fn kill_selected(&mut self) {
        if let Some(jail) = self.selected.and_then(|id| self.jails.get(&id)) {
            unsafe {
                libc::kill(jail.pid as i32, libc::SIGKILL);
            }
        }
    }

    /// Remove completed jails.
    pub fn clear_completed(&mut self) {
        self.jails
            .retain(|_, j| matches!(j.status, JailStatus::Running));
        if self.selected.is_some_and(|id| !self.jails.contains_key(&id)) {
            self.selected = self.jails.keys().next().copied();
        }
    }
}

/// Format duration as human readable.
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Format bytes as human readable.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
