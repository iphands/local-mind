use std::io::{stdout, Stdout};

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use crate::ui::state::{AppState, FileStage, FileStatus};

/// Terminal state for TUI application
pub struct UIRenderer {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    pub running: bool,
}

impl UIRenderer {
    /// Initialize the terminal in alternate screen mode
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut stdout = stdout();

        // Enter alternate screen
        execute!(stdout, EnterAlternateScreen)?;

        // Enable raw mode
        terminal::enable_raw_mode()?;

        // Create terminal
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;

        Ok(UIRenderer {
            terminal,
            running: true,
        })
    }

    /// Clear the screen and restore normal terminal mode
    pub fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Disable raw mode
        terminal::disable_raw_mode()?;

        // Leave alternate screen
        let mut stdout = stdout();
        execute!(stdout, LeaveAlternateScreen)?;

        Ok(())
    }

    /// Draw the current state
    pub fn draw(&mut self, app: &AppState) -> Result<(), Box<dyn std::error::Error>> {
        self.terminal.draw(|frame| {
            render_ui(frame, app);
        })?;
        Ok(())
    }

    /// Handle events from the event loop
    /// Returns false if the application should exit
    pub fn handle_events(&mut self) -> bool {
        // Non-blocking check for events
        if event::poll(std::time::Duration::from_millis(16)).unwrap_or(false) {
            if let Event::Key(key) = event::read().unwrap_or(Event::Key(KeyCode::Enter.into())) {
                if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                    self.running = false;
                    return false;
                }
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.running = false;
                    return false;
                }
            }
        }
        true
    }

    /// Run the event loop
    pub fn run(&mut self, app: &AppState) -> Result<(), Box<dyn std::error::Error>> {
        while self.running {
            self.draw(app)?;

            if !self.handle_events() {
                break;
            }
        }
        Ok(())
    }
}

/// Render the main UI
pub fn render_ui(frame: &mut Frame, app: &AppState) {
    // Get the current size
    let area = frame.area();

    // Create the main layout
    let chunks = Layout::vertical([
        Constraint::Length(3),  // Header
        Constraint::Length(2),  // Summary
        Constraint::Min(1),     // File list
    ]).split(area);

    // Render header
    render_header(frame, chunks[0], app);

    // Render summary
    render_summary(frame, chunks[1], app);

    // Render file list
    render_file_list(frame, chunks[2], app);
}

/// Render the header
fn render_header(frame: &mut Frame, area: Rect, _app: &AppState) {
    let header = Paragraph::new(Line::from(vec![
        Span::from("model-holder - "),
        Span::styled("Keeping models in memory", Style::default().fg(Color::Cyan)),
    ]))
    .style(Style::default().bg(Color::DarkGray).fg(Color::White))
    .alignment(Alignment::Center);

    frame.render_widget(header, area);
}

/// Render the summary bar
fn render_summary(frame: &mut Frame, area: Rect, app: &AppState) {
    let file_count = app.files.len();
    let total_size = format_size(app.total_size_bytes);

    let summary = format!("Files: {} | Total: {}", file_count, total_size);

    let paragraph = Paragraph::new(summary)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}

/// Render the file list with progress indicators
fn render_file_list(frame: &mut Frame, area: Rect, app: &AppState) {
    // Calculate available lines for file list
    let max_files: usize = area.height as usize;

    for (i, file) in app.files.iter().enumerate() {
        if i >= max_files {
            break;
        }

        let line = area.y + 3 + i as u16;  // +3 for header + summary + blank line

        if line >= area.bottom() {
            break;
        }

        let file_area = Rect::new(area.x, line, area.width, 1);
        render_file_row(frame, file_area, file);
    }
}

/// Render a single file row with its current status
fn render_file_row(frame: &mut Frame, area: Rect, file: &FileStatus) {
    let filename_truncated = truncate_str(&file.filename, area.width as usize - 4);

    let (status_text, status_style) = match &file.stage {
        FileStage::Found => {
            (format!("{} Found", filename_truncated), Style::default().fg(Color::Yellow))
        }
        FileStage::Mapped => {
            (format!("{} mmapped", filename_truncated), Style::default().fg(Color::Blue))
        }
        FileStage::Warming { progress, speed, .. } => {
            let bar_width = area.width.saturating_sub(30) as usize;
            let filled = (bar_width as f64 * progress / 100.0).round() as usize;
            let empty = bar_width.saturating_sub(filled);

            let bar = format!(
                "{}{} {:.1}% {:7.1} MB/s",
                "=".repeat(filled),
                " ".repeat(empty),
                progress,
                speed
            );

            let full_text = format!("{} Warming up [{}] {:.1}%", filename_truncated, bar, progress);
            (full_text, Style::default().fg(Color::Cyan))
        }
        FileStage::Complete { speed, .. } => {
            let full_text = format!(
                "{} Read complete {:7.2} MB/s",
                filename_truncated,
                speed
            );
            (full_text, Style::default().fg(Color::Green))
        }
        FileStage::Locked { speed, .. } => {
            let full_text = format!(
                "{} Read complete {:7.2} MB/s locked",
                filename_truncated,
                speed
            );
            (full_text, Style::default().fg(Color::Green).bg(Color::DarkGray))
        }
    };

    let paragraph = Paragraph::new(status_text)
        .style(status_style)
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}

/// Format bytes as human-readable string
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    if bytes as f64 >= GB {
        format!("{:.2} GB", bytes as f64 / GB)
    } else if bytes as f64 >= MB {
        format!("{:.2} MB", bytes as f64 / MB)
    } else if bytes as f64 >= KB {
        format!("{:.2} KB", bytes as f64 / KB)
    } else {
        format!("{} B", bytes)
    }
}

/// Truncate string to max length with ellipsis
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
