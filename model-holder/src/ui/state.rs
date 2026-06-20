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
        mmap_speed: f64,
        lock_speed: f64,
        total_speed: f64,
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
    pub mmap_duration: Option<Duration>,
    pub lock_duration: Option<Duration>,
    pub mmap_speed: Option<f64>,
    pub lock_speed: Option<f64>,
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
            mmap_duration: None,
            lock_duration: None,
            mmap_speed: None,
            lock_speed: None,
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

    pub fn mark_locked(&mut self, mmap_speed: f64, lock_speed: f64, elapsed: Duration) {
        let total_speed = if elapsed.as_secs_f64() > 0.0 {
            self.size_bytes as f64 / elapsed.as_secs_f64() / BYTES_PER_MB
        } else {
            0.0
        };
        self.stage = FileStage::Locked {
            size_mb: self.size_mb,
            mmap_speed,
            lock_speed,
            total_speed,
            elapsed,
        };
        self.mmap_speed = Some(mmap_speed);
        self.lock_speed = Some(lock_speed);
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
            FileStage::Locked { total_speed, .. } => *total_speed,
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
    /// Original input patterns (for display in title bar)
    pub input_patterns: Vec<String>,
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
            input_patterns: Vec::new(),
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

    /// Set the input patterns for display
    pub fn set_input_patterns(&mut self, patterns: Vec<String>) {
        self.input_patterns = patterns;
    }

    /// Compute the common parent directory name from all files
    /// Used when original patterns are lost (e.g., shell expansion)
    pub fn compute_common_parent(&self) -> Option<String> {
        if self.files.is_empty() {
            return None;
        }

        // Get all parent directories
        let parents: Vec<_> = self.files
            .iter()
            .filter_map(|f| std::path::Path::new(&f.path).parent())
            .collect();

        if parents.is_empty() {
            return None;
        }

        // Find common prefix
        let first = &parents[0];
        let mut common = first.to_path_buf();

        for parent in &parents[1..] {
            // Find common prefix between common and parent
            let common_components: Vec<_> = common.components().collect();
            let parent_components: Vec<_> = parent.components().collect();
            
            let min_len = common_components.len().min(parent_components.len());
            let mut prefix_len = 0;
            
            for i in 0..min_len {
                if common_components[i] == parent_components[i] {
                    prefix_len = i + 1;
                } else {
                    break;
                }
            }
            
            // Rebuild common path up to prefix_len
            common = parent_components[..prefix_len].iter().collect();
        }

        // Get the last component (the directory name we want)
        common.file_name()
            .map(|n| n.to_string_lossy().to_string())
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
