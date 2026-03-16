use std::time::Duration;

use crate::BYTES_PER_MB;

/// Current progress stage for a file
#[derive(Debug, Clone, PartialEq)]
pub enum FileStage {
    /// File was found and is being processed
    Found,
    /// File has been memory-mapped
    Mapped,
    /// File is currently being warmed up
    Warming {
        progress: f64,     // 0.0 to 100.0
        speed: f64,        // MB/s
        elapsed: Duration, // time spent warming
    },
    /// File warming is complete
    Complete {
        size_mb: f64,
        speed: f64, // MB/s average
        elapsed: Duration,
    },
    /// File is being mlocked (blocking syscall in progress)
    Locking {
        speed: f64, // preserved from Complete
        elapsed: Duration,
    },
    /// File is locked in RAM
    Locked {
        size_mb: f64,
        speed: f64,
        elapsed: Duration,
    },
}

impl FileStage {
    #[allow(dead_code)]
    pub fn elapsed(&self) -> Duration {
        match self {
            FileStage::Found => Duration::new(0, 0),
            FileStage::Mapped => Duration::new(0, 0),
            FileStage::Warming { elapsed, .. } => *elapsed,
            FileStage::Complete { elapsed, .. } => *elapsed,
            FileStage::Locking { elapsed, .. } => *elapsed,
            FileStage::Locked { elapsed, .. } => *elapsed,
        }
    }
}

/// Sort criteria for file list
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Default,
    Size,
    Speed,
}

/// Status of a file being processed
#[derive(Debug, Clone)]
pub struct FileStatus {
    pub path: String,
    pub filename: String,
    pub size_bytes: u64,
    pub size_mb: f64,
    pub stage: FileStage,
    pub warmup_complete: bool,
    pub process_index: usize,
}

impl FileStatus {
    pub fn new(path: String, process_index: usize) -> Self {
        let filename = std::path::Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        Self {
            path,
            filename,
            size_bytes: 0,
            size_mb: 0.0,
            stage: FileStage::Found,
            warmup_complete: false,
            process_index,
        }
    }

    pub fn set_size(&mut self, size: u64) {
        self.size_bytes = size;
        self.size_mb = size as f64 / BYTES_PER_MB;
    }

    pub fn mark_mapped(&mut self) {
        self.stage = FileStage::Mapped;
    }

    pub fn mark_warming(&mut self, progress: f64, speed: f64, elapsed: Duration) {
        self.stage = FileStage::Warming {
            progress,
            speed,
            elapsed,
        };
    }

    pub fn mark_complete(&mut self, speed: f64, elapsed: Duration) {
        self.stage = FileStage::Complete {
            size_mb: self.size_mb,
            speed,
            elapsed,
        };
        self.warmup_complete = true;
    }

    pub fn mark_locking(&mut self, elapsed: Duration) {
        let speed = self.current_speed();
        self.stage = FileStage::Locking { speed, elapsed };
    }

    pub fn mark_locked(&mut self, speed: f64, elapsed: Duration) {
        self.stage = FileStage::Locked {
            size_mb: self.size_mb,
            speed,
            elapsed,
        };
    }

    pub fn is_complete(&self) -> bool {
        matches!(
            self.stage,
            FileStage::Complete { .. } | FileStage::Locking { .. } | FileStage::Locked { .. }
        )
    }

    pub fn current_speed(&self) -> f64 {
        match &self.stage {
            FileStage::Warming { speed, .. } => *speed,
            FileStage::Complete { speed, .. } => *speed,
            FileStage::Locking { speed, .. } => *speed,
            FileStage::Locked { speed, .. } => *speed,
            _ => 0.0,
        }
    }
}

/// Application state for the TUI
#[derive(Debug)]
pub struct AppState {
    pub files: Vec<FileStatus>,
    pub total_size_bytes: u64,
    pub warmup_complete: bool,
    pub all_locked: bool,
    pub sort_by: SortBy,
    pub scroll_offset: usize,
    /// Text typed in filter mode
    pub filter_input: String,
    /// Whether we're currently accepting filter keystrokes
    pub filter_mode: bool,
    /// Files that could not be mlocked (e.g. ulimit too low)
    pub failed_lock_files: Vec<String>,
    /// Whether the lock-failure help overlay is visible
    pub show_help: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            total_size_bytes: 0,
            warmup_complete: false,
            all_locked: false,
            sort_by: SortBy::Default,
            scroll_offset: 0,
            filter_input: String::new(),
            filter_mode: false,
            failed_lock_files: Vec::new(),
            show_help: false,
        }
    }

    #[allow(dead_code)]
    pub fn total_size_mb(&self) -> f64 {
        self.total_size_bytes as f64 / BYTES_PER_MB
    }

    #[allow(dead_code)]
    pub fn total_size_gb(&self) -> f64 {
        self.total_size_bytes as f64 / (BYTES_PER_MB * 1024.0)
    }

    pub fn scroll_down(&mut self) {
        let max_offset = self.visible_file_count().saturating_sub(1);
        if self.scroll_offset < max_offset {
            self.scroll_offset += 1;
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn scroll_page_down(&mut self, page: usize) {
        self.scroll_offset += page;
        let max_offset = self.visible_file_count().saturating_sub(1);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    pub fn scroll_page_up(&mut self, page: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(page);
    }

    pub fn add_file(&mut self, path: String) {
        let process_index = self.files.len();
        let mut file = FileStatus::new(path, process_index);
        file.set_size(100); // placeholder until actual size known
        self.files.push(file);
    }

    pub fn find_file_mut(&mut self, path: &str) -> Option<&mut FileStatus> {
        self.files.iter_mut().find(|f| f.path == path)
    }

    pub fn update_total_size(&mut self) {
        self.total_size_bytes = self.files.iter().map(|f| f.size_bytes).sum();
    }

    pub fn warmup_complete(&mut self) {
        self.warmup_complete = true;
    }

    pub fn all_locked(&mut self) {
        self.all_locked = true;
    }

    /// Returns filtered and sorted files based on current state
    pub fn filtered_files(&self) -> Vec<&FileStatus> {
        let query = self.filter_input.to_lowercase();
        let mut files: Vec<&FileStatus> = self
            .files
            .iter()
            .filter(|f| {
                if query.is_empty() {
                    return true;
                }
                f.filename.to_lowercase().contains(&query) || f.path.to_lowercase().contains(&query)
            })
            .collect();

        match self.sort_by {
            SortBy::Default => files.sort_by_key(|f| f.process_index),
            SortBy::Size => files.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes)),
            SortBy::Speed => files.sort_by(|a, b| {
                b.current_speed()
                    .partial_cmp(&a.current_speed())
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
        }

        files
    }

    pub fn visible_file_count(&self) -> usize {
        self.filtered_files().len()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
