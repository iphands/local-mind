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

/// Status of a file being processed
#[derive(Debug, Clone)]
pub struct FileStatus {
    pub path: String,
    pub filename: String,
    pub size_bytes: u64,
    pub size_mb: f64,
    pub stage: FileStage,
    pub warmup_complete: bool,
}

impl FileStatus {
    pub fn new(path: String) -> Self {
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

    pub fn is_complete(&self) -> bool {
        matches!(self.stage, FileStage::Complete { .. } | FileStage::Locked { .. })
    }
}

/// Application state for the TUI
#[derive(Debug)]
pub struct AppState {
    pub files: Vec<FileStatus>,
    pub total_size_bytes: u64,
    pub warmup_complete: bool,
    pub all_locked: bool,
    pub start_time: std::time::Instant,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            total_size_bytes: 0,
            warmup_complete: false,
            all_locked: false,
            start_time: std::time::Instant::now(),
        }
    }

    pub fn total_size_mb(&self) -> f64 {
        self.total_size_bytes as f64 / BYTES_PER_MB
    }

    pub fn total_size_gb(&self) -> f64 {
        self.total_size_bytes as f64 / (BYTES_PER_MB * 1024.0)
    }

    pub fn add_file(&mut self, path: String) {
        let mut file = FileStatus::new(path);
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

    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Message sent from worker threads to UI thread
#[derive(Debug, Clone)]
pub enum UpdateMessage {
    /// New file found
    FileFound(String),
    /// File size set
    FileSizeSet(String, u64),
    /// File has been mmapped
    FileMapped(String),
    /// Warming progress update
    WarmingProgress {
        path: String,
        progress: f64,
        speed: f64,
        elapsed: Duration,
    },
    /// File warming complete
    FileComplete {
        path: String,
        speed: f64,
        elapsed: Duration,
    },
    /// File locked in RAM
    FileLocked {
        path: String,
        speed: f64,
        elapsed: Duration,
    },
    /// All files complete
    WarmupComplete,
    /// All files locked
    AllLocked,
    /// Update file size for display
    UpdateTotalSize,
}
