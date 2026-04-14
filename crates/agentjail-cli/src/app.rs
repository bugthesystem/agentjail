//! Application state for the TUI.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Output stream type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// A tracked jail instance.
#[derive(Debug, Clone)]
pub struct JailInfo {
    pub pid: u32,
    pub command: String,
    pub preset: String,
    pub status: JailStatus,
    pub started_at: Instant,
    pub memory_bytes: u64,
    pub network: String,
    pub seccomp: String,
    pub timeout_secs: u64,
    pub memory_limit_mb: u64,
    pub output: VecDeque<(Stream, String)>,
    pub stdout_count: usize,
    pub stderr_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JailStatus {
    Running,
    Completed(i32),
    TimedOut,
    Killed,
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
    pub auto_scroll: bool,
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
            auto_scroll: true,
            next_id: 1,
        }
    }

    pub fn add_jail(
        &mut self,
        pid: u32,
        command: String,
        preset: String,
        network: String,
        seccomp: String,
        timeout_secs: u64,
        memory_limit_mb: u64,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        self.jails.insert(id, JailInfo {
            pid,
            command,
            preset,
            status: JailStatus::Running,
            started_at: Instant::now(),
            memory_bytes: 0,
            network,
            seccomp,
            timeout_secs,
            memory_limit_mb,
            output: VecDeque::new(),
            stdout_count: 0,
            stderr_count: 0,
        });

        if self.selected.is_none() {
            self.selected = Some(id);
        }
        id
    }

    pub fn update_status(&mut self, id: u32, status: JailStatus) {
        if let Some(jail) = self.jails.get_mut(&id) {
            jail.status = status;
        }
    }

    #[allow(dead_code)] // API for future cgroup stats integration
    pub fn update_memory(&mut self, id: u32, bytes: u64) {
        if let Some(jail) = self.jails.get_mut(&id) {
            jail.memory_bytes = bytes;
        }
    }

    pub fn append_output(&mut self, id: u32, stream: Stream, line: String) {
        if let Some(jail) = self.jails.get_mut(&id) {
            match stream {
                Stream::Stdout => jail.stdout_count += 1,
                Stream::Stderr => jail.stderr_count += 1,
            }
            jail.output.push_back((stream, line));
            if jail.output.len() > 1000 {
                jail.output.pop_front();
            }
            // Auto-scroll follows new output
            if self.auto_scroll && self.selected == Some(id) {
                self.scroll_offset = jail.output.len().saturating_sub(1);
            }
        }
    }

    pub fn sorted_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.jails.keys().copied().collect();
        ids.sort();
        ids
    }

    pub fn next(&mut self) {
        let ids = self.sorted_ids();
        if ids.is_empty() { return; }
        let idx = self.selected
            .and_then(|s| ids.iter().position(|&id| id == s))
            .map(|i| (i + 1) % ids.len())
            .unwrap_or(0);
        self.selected = Some(ids[idx]);
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    pub fn prev(&mut self) {
        let ids = self.sorted_ids();
        if ids.is_empty() { return; }
        let idx = self.selected
            .and_then(|s| ids.iter().position(|&id| id == s))
            .map(|i| if i == 0 { ids.len() - 1 } else { i - 1 })
            .unwrap_or(0);
        self.selected = Some(ids[idx]);
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    pub fn selected_jail(&self) -> Option<&JailInfo> {
        self.selected.and_then(|id| self.jails.get(&id))
    }

    pub fn scroll_up(&mut self) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        if let Some(jail) = self.selected_jail() {
            let max = jail.output.len().saturating_sub(1);
            self.scroll_offset = (self.scroll_offset + 1).min(max);
            if self.scroll_offset >= max {
                self.auto_scroll = true;
            }
        }
    }

    pub fn scroll_end(&mut self) {
        if let Some(jail) = self.selected_jail() {
            self.scroll_offset = jail.output.len().saturating_sub(1);
            self.auto_scroll = true;
        }
    }

    pub fn kill_selected(&mut self) {
        if let Some(jail) = self.selected.and_then(|id| self.jails.get(&id)) {
            // SAFETY: Sending SIGKILL to a process we spawned.
            unsafe { libc::kill(jail.pid as i32, libc::SIGKILL) };
        }
    }

    pub fn clear_completed(&mut self) {
        self.jails.retain(|_, j| matches!(j.status, JailStatus::Running));
        if self.selected.is_some_and(|id| !self.jails.contains_key(&id)) {
            self.selected = self.jails.keys().next().copied();
        }
    }
}

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
