use std::io::{stdout, Stdout};

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use crate::ui::state::{AppState, FileStage, FileStatus, FilterBy, SortBy};

/// Terminal state for TUI application
pub struct UIRenderer {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
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
    /// Returns true if we should continue
    #[allow(dead_code)]
    pub fn handle_event(&mut self, app: &mut AppState) -> bool {
        if event::poll(std::time::Duration::from_millis(16)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return false,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return false,
                    // Navigation
                    KeyCode::Down | KeyCode::Char('j') => {
                        app.scroll_down();
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        app.scroll_up();
                    }
                    KeyCode::PageDown | KeyCode::Char(' ') => {
                        app.scroll_page_down();
                    }
                    KeyCode::PageUp => {
                        app.scroll_page_up();
                    }
                    // Sorting
                    KeyCode::Char('s') => {
                        if key.modifiers.contains(KeyModifiers::SHIFT) {
                            app.sort_by = SortBy::Size;
                        }
                    }
                    KeyCode::Char('S') => {
                        app.sort_by = SortBy::Speed;
                    }
                    // Clear sort (default)
                    KeyCode::Char('d') => {
                        app.sort_by = SortBy::Default;
                    }
                    // Filter
                    KeyCode::Char('/') => {
                        app.filter = FilterBy::None; // For now, just clear filter
                    }
                    _ => {}
                }
            }
        }
        true
    }

    /// Wait for user to press ESC or Ctrl+C to exit
    /// Continues rendering the UI while waiting
    pub fn wait_for_exit(&mut self, app: &AppState) -> bool {
        use std::time::Duration;

        loop {
            // Draw the UI while waiting
            if let Err(e) = self.draw(app) {
                eprintln!("UI draw error: {}", e);
            }

            // Small delay to avoid busy-waiting
            std::thread::sleep(Duration::from_millis(16));

            if event::poll(Duration::from_millis(0)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return false,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return false,
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Render the main UI
pub fn render_ui(frame: &mut Frame, app: &AppState) {
    let area = frame.area();
    let total_height = area.height as usize;

    // Layout: header (3), summary (2), file list (rest), status bar (1)
    let chunks = Layout::vertical([
        Constraint::Length(3),  // Header
        Constraint::Length(2),  // Summary
        Constraint::Min(0),     // File list (will be calculated)
        Constraint::Length(1),  // Status bar
    ]).split(area);

    // Calculate file list height
    let file_list_height = total_height as u16 - 3 - 2 - 1; // header, summary, status bar

    // Render header
    render_header(frame, chunks[0], app);

    // Render summary
    render_summary(frame, chunks[1], app);

    // Render file list
    render_file_list(frame, chunks[2], app, file_list_height as usize);

    // Render status bar
    render_status_bar(frame, chunks[3], app);
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
    let file_count = app.visible_file_count();
    let total_size = format_size(app.total_size_bytes);
    let sorted_info = match app.sort_by {
        SortBy::Default => "",
        SortBy::Size => " [Size]",
        SortBy::Speed => " [Speed]",
    };

    let summary = format!("Files: {}{} | Total: {}", file_count, sorted_info, total_size);

    let paragraph = Paragraph::new(summary)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}

/// Render the file list with progress indicators
fn render_file_list(frame: &mut Frame, area: Rect, app: &AppState, max_files: usize) {
    let files = app.filtered_files();
    let start = app.scroll_offset;
    let end = (start + max_files).min(files.len());

    if start >= files.len() {
        return; // Nothing to render
    }

    for (i, file) in files[start..end].iter().enumerate() {
        let line = area.y + i as u16;
        if line >= area.bottom() {
            break;
        }

        let file_area = Rect::new(area.x, line, area.width, 1);
        render_file_row(frame, file_area, file);
    }

    // Render scrollbar if needed
    if files.len() > max_files {
        render_scrollbar(frame, area, files.len(), max_files, start);
    }
}

/// Render a single file row with its current status
fn render_file_row(frame: &mut Frame, area: Rect, file: &FileStatus) {
    let max_width = area.width as usize;
    let filename_truncated = truncate_str(&file.filename, max_width - 30);

    let (status_text, status_style) = match &file.stage {
        FileStage::Found => {
            (format!("{} Found", filename_truncated), Style::default().fg(Color::Yellow))
        }
        FileStage::Mapped => {
            (format!("{} mmapped", filename_truncated), Style::default().fg(Color::Blue))
        }
        FileStage::Warming { progress, speed, .. } => {
            // Progress bar is 15 chars wide, stats before it
            let bar_width: usize = 15;
            let filled = (bar_width as f64 * progress / 100.0).round() as usize;
            let empty = bar_width.saturating_sub(filled);

            // Stats before progress bar: size, speed
            let size_str = format_size_bytes(file.size_bytes);
            let bar = format!(
                "[{}{}]",
                "=".repeat(filled),
                " ".repeat(empty),
            );

            let full_text = format!(
                "{:10} {:>7.1} MB/s {}",
                size_str,
                speed,
                bar
            );

            // Truncate if needed to fit area
            let full_text = if full_text.len() + filename_truncated.len() + 2 > max_width {
                format!("{} {}", filename_truncated, &full_text[..max_width - filename_truncated.len() - 2])
            } else {
                format!("{} {}", filename_truncated, full_text)
            };

            (full_text, Style::default().fg(Color::Cyan))
        }
        FileStage::Complete { speed, .. } => {
            let size_str = format_size_bytes(file.size_bytes);
            let full_text = format!(
                "{} {:>10} @ {:>7.1} MB/s",
                filename_truncated,
                size_str,
                speed
            );
            let full_text = if full_text.len() > max_width {
                format!("{}...", &full_text[..max_width - 3])
            } else {
                full_text
            };
            (full_text, Style::default().fg(Color::Green))
        }
        FileStage::Locked { speed, .. } => {
            let size_str = format_size_bytes(file.size_bytes);
            let full_text = format!(
                "{} {:>10} @ {:>7.1} MB/s locked",
                filename_truncated,
                size_str,
                speed
            );
            let full_text = if full_text.len() > max_width {
                format!("{}...", &full_text[..max_width - 3])
            } else {
                full_text
            };
            (full_text, Style::default().fg(Color::Green).bg(Color::DarkGray))
        }
    };

    let paragraph = Paragraph::new(status_text)
        .style(status_style)
        .alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}

/// Render scrollbar
fn render_scrollbar(frame: &mut Frame, area: Rect, total: usize, visible: usize, offset: usize) {
    if total == 0 || visible >= total {
        return;
    }

    let scrollbar_x = area.right() - 1;
    let scrollbar_height = ((visible as f64 / total as f64) * area.height as f64).round() as u16;
    let scrollbar_y = area.y + 3 + ((offset as f64 / total as f64) * (area.height - 3) as f64).round() as u16;

    let scrollbar_area = Rect::new(scrollbar_x, scrollbar_y, 1, scrollbar_height.max(1));
    frame.render_widget(
        Paragraph::new(vec![Line::from(Span::styled("█", Style::default().fg(Color::Gray)))]),
        scrollbar_area,
    );
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

/// Format bytes as human-readable string (alternative format)
fn format_size_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    if bytes as f64 >= GB {
        format!("{:.1}G", bytes as f64 / GB)
    } else if bytes as f64 >= MB {
        format!("{:.1}M", bytes as f64 / MB)
    } else if bytes as f64 >= KB {
        format!("{:.1}K", bytes as f64 / KB)
    } else {
        format!("{}B", bytes)
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

/// Status bar rendering
fn render_status_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let total_files = app.files.len();
    let complete_count = app.files.iter().filter(|f| f.warmup_complete).count();
    let locked_count = app.files.iter().filter(|f| matches!(f.stage, FileStage::Locked { .. })).count();

    let status = if app.all_locked {
        format!("All {} files locked in RAM. Press ESC or Ctrl+C to exit.", total_files)
    } else if complete_count > 0 {
        format!("{} / {} files complete ({} locked). Press ESC or Ctrl+C to exit.", complete_count, total_files, locked_count)
    } else {
        format!("Processing {} files... Press ESC or Ctrl+C to exit.", total_files)
    };

    let paragraph = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White))
        .alignment(Alignment::Center);

    frame.render_widget(paragraph, area);
}
