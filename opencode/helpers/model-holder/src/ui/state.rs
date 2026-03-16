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
        progress: f64,      // 0.0 to 100.0
        speed: f64,         // MB/s
        elapsed: Duration,  // time spent warming
    },
    /// File warming is complete
    Complete {
        size_mb: f64,
        speed: f64,         // MB/s average
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
    /// Get the elapsed time from the stage
    #[allow(dead_code)]
    pub fn elapsed(&self) -> Duration {
        match self {
            FileStage::Found => Duration::new(0, 0),
            FileStage::Mapped => Duration::new(0, 0),
            FileStage::Warming { elapsed, .. } => *elapsed,
            FileStage::Complete { elapsed, .. } => *elapsed,
            FileStage::Locked { elapsed, .. } => *elapsed,
        }
    }
}

/// Sort criteria for file list
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    /// Default order (order processed/found)
    Default,
    /// Sort by size (descending)
    Size,
    /// Sort by speed (MB/s, descending)
    Speed,
}

/// Filter criteria for file list
#[derive(Debug, Clone)]
pub enum FilterBy {
    /// Show all files
    None,
    /// Filter by name pattern (glob)
    #[allow(dead_code)]
    Name(String),
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
    /// Index when files were processed (for default sort)
    pub process_index: usize,
}

impl FileStatus {
    pub fn new(path: String, process_index: usize) -> Self {
        let filename = std::path::Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let size_bytes = 0;
        let size_mb = 0.0;

        Self {
            path,
            filename,
            size_bytes,
            size_mb,
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
        self.stage = FileStage::Warming { progress, speed, elapsed };
    }

    pub fn mark_complete(&mut self, speed: f64, elapsed: Duration) {
        self.stage = FileStage::Complete {
            size_mb: self.size_mb,
            speed,
            elapsed,
        };
        self.warmup_complete = true;
    }

    pub fn mark_locked(&mut self, speed: f64, elapsed: Duration) {
        self.stage = FileStage::Locked {
            size_mb: self.size_mb,
            speed,
            elapsed,
        };
    }

    #[allow(dead_code)]
    pub fn is_complete(&self) -> bool {
        matches!(self.stage, FileStage::Complete { .. } | FileStage::Locked { .. })
    }

    /// Get current MB/s (from stage or 0)
    #[allow(dead_code)]
    pub fn current_speed(&self) -> f64 {
        match &self.stage {
            FileStage::Warming { speed, .. } => *speed,
            FileStage::Complete { speed, .. } => *speed,
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
    pub filter: FilterBy,
    pub scroll_offset: usize,
    #[allow(dead_code)]
    pub selected_index: Option<usize>,
}

impl AppState {
    #[allow(dead_code)]
    pub fn scroll_down(&mut self) {
        let max_offset = self.visible_file_count().saturating_sub(1);
        if self.scroll_offset < max_offset {
            self.scroll_offset += 1;
        }
    }

    #[allow(dead_code)]
    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    #[allow(dead_code)]
    pub fn scroll_page_down(&mut self) {
        // Scroll down by approximately 20 lines
        self.scroll_offset += 20;
        let max_offset = self.visible_file_count().saturating_sub(1);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    #[allow(dead_code)]
    pub fn scroll_page_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(20);
    }

    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            total_size_bytes: 0,
            warmup_complete: false,
            all_locked: false,
            sort_by: SortBy::Default,
            filter: FilterBy::None,
            scroll_offset: 0,
            selected_index: None,
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

    pub fn add_file(&mut self, path: String) {
        let process_index = self.files.len();
        let mut file = FileStatus::new(path, process_index);
        // For display purposes, we'll set a placeholder size
        // The actual size will be set when we open the file
        file.set_size(100); // placeholder
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

    /// Get filtered and sorted file list
    pub fn filtered_files(&self) -> Vec<&FileStatus> {
        let mut files: Vec<&FileStatus> = self.files.iter().filter(|f| {
            match &self.filter {
                FilterBy::None => true,
                FilterBy::Name(pattern) => {
                    // Simple glob matching - check if filename contains pattern
                    f.filename.contains(pattern) || f.path.contains(pattern)
                }
            }
        }).collect();

        // Sort based on criteria
        match self.sort_by {
            SortBy::Default => {
                // Sort by process index
                files.sort_by_key(|f| f.process_index);
            }
            SortBy::Size => {
                // Sort by size descending
                files.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
            }
            SortBy::Speed => {
                // Sort by current speed descending
                files.sort_by(|a, b| b.current_speed().partial_cmp(&a.current_speed()).unwrap_or(std::cmp::Ordering::Equal));
            }
        }

        files
    }

    /// Get number of visible files
    pub fn visible_file_count(&self) -> usize {
        self.filtered_files().len()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
