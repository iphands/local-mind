use std::io::{stdout, Stdout};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::state::{AppState, FileStage, FileStatus, SortBy};

/// Terminal state for TUI application
pub struct UIRenderer {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl UIRenderer {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        terminal::enable_raw_mode()?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(UIRenderer { terminal })
    }

    pub fn cleanup(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        terminal::disable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, LeaveAlternateScreen)?;
        Ok(())
    }

    pub fn draw(&mut self, app: &AppState) -> Result<(), Box<dyn std::error::Error>> {
        self.terminal.draw(|frame| render_ui(frame, app))?;
        Ok(())
    }

    /// Runs the UI loop on the calling thread until `should_exit` is set.
    /// Draws at ~30 fps and handles keyboard input. Cleans up terminal on exit.
    pub fn run(mut self, state: Arc<Mutex<AppState>>, should_exit: Arc<AtomicBool>) {
        loop {
            if should_exit.load(Ordering::Relaxed) {
                break;
            }

            if let Ok(app) = state.lock() {
                let _ = self.draw(&*app);
            }

            if event::poll(Duration::from_millis(33)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if let Ok(mut app) = state.lock() {
                        handle_key(&mut app, key, &should_exit);
                    }
                }
            }
        }
        let _ = self.cleanup();
    }
}

fn handle_key(app: &mut AppState, key: KeyEvent, should_exit: &Arc<AtomicBool>) {
    if app.filter_mode {
        match key.code {
            KeyCode::Enter => {
                app.filter_mode = false;
                app.scroll_offset = 0;
            }
            KeyCode::Esc => {
                app.filter_mode = false;
            }
            KeyCode::Backspace => {
                app.filter_input.pop();
                app.scroll_offset = 0;
            }
            KeyCode::Char(c) => {
                app.filter_input.push(c);
                app.scroll_offset = 0;
            }
            _ => {}
        }
    } else if app.show_help {
        match key.code {
            KeyCode::Char('h') | KeyCode::Esc | KeyCode::Char('q') => {
                app.show_help = false;
            }
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                should_exit.store(true, Ordering::Relaxed);
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                should_exit.store(true, Ordering::Relaxed);
            }
            KeyCode::Down | KeyCode::Char('j') => app.scroll_down(),
            KeyCode::Up | KeyCode::Char('k') => app.scroll_up(),
            KeyCode::PageDown => app.scroll_page_down(10),
            KeyCode::Char(' ') => app.scroll_page_down(10),
            KeyCode::PageUp => app.scroll_page_up(10),
            KeyCode::Char('s') => app.sort_by = SortBy::Size,
            KeyCode::Char('p') => app.sort_by = SortBy::Speed,
            KeyCode::Char('d') => app.sort_by = SortBy::Default,
            KeyCode::Char('/') => {
                app.filter_mode = true;
            }
            KeyCode::Char('x') => {
                app.filter_input.clear();
                app.scroll_offset = 0;
            }
            KeyCode::Char('h') => {
                app.show_help = true;
            }
            _ => {}
        }
    }
}

// ─── Rendering ───────────────────────────────────────────────────────────────

fn render_ui(frame: &mut Frame, app: &AppState) {
    let area = frame.area();

    // Layout: title (1) | file list (fill) | summary (1) | help/filter bar (1)
    let chunks = Layout::vertical([
        Constraint::Length(1), // title bar
        Constraint::Min(1),    // file list
        Constraint::Length(1), // summary
        Constraint::Length(1), // status / filter bar
    ])
    .split(area);

    render_title(frame, chunks[0], app);
    render_file_list(frame, chunks[1], app);
    render_summary(frame, chunks[2], app);
    render_statusbar(frame, chunks[3], app);

    if app.show_help {
        render_help_overlay(frame, area, app);
    }
}

fn render_title(frame: &mut Frame, area: Rect, app: &AppState) {
    let overall = if app.all_locked {
        "● locked"
    } else if app.warmup_complete {
        "● complete"
    } else {
        "◌ warming…"
    };

    let left = " model-holder";
    let right = format!("{}  ", overall);
    let gap = (area.width as usize)
        .saturating_sub(left.len() + right.len());
    let title_text = format!("{}{}{}", left, " ".repeat(gap), right);

    let status_color = if app.all_locked {
        Color::Green
    } else if app.warmup_complete {
        Color::Cyan
    } else {
        Color::Yellow
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" model-holder{}", " ".repeat(gap)),
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            right,
            Style::default()
                .fg(status_color)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let _ = title_text; // used for length calc only
    frame.render_widget(Paragraph::new(line), area);
}

fn render_summary(frame: &mut Frame, area: Rect, app: &AppState) {
    let total = app.files.len();
    let complete = app.files.iter().filter(|f| f.is_complete()).count();
    let locked = app
        .files
        .iter()
        .filter(|f| matches!(f.stage, FileStage::Locked { .. }))
        .count();
    let total_size = format_size(app.total_size_bytes);

    let sort_label = match app.sort_by {
        SortBy::Default => "",
        SortBy::Size => "  sort:size",
        SortBy::Speed => "  sort:speed",
    };

    let filter_label = if !app.filter_input.is_empty() {
        format!("  filter:\"{}\"", app.filter_input)
    } else {
        String::new()
    };

    let text = format!(
        "  {} files  {}  {}/{} complete  {} locked{}{}",
        total, total_size, complete, total, locked, sort_label, filter_label
    );

    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_statusbar(frame: &mut Frame, area: Rect, app: &AppState) {
    let content = if app.filter_mode {
        Line::from(vec![
            Span::styled(
                " Filter: ",
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ),
            Span::styled(
                format!("{}_", app.filter_input),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  Enter/ESC to confirm ",
                Style::default().fg(Color::DarkGray).bg(Color::Yellow),
            ),
        ])
    } else {
        let failed = app.failed_lock_files.len();
        let lock_hint = if failed > 0 {
            format!("   ⚠ {} file(s) not locked  h info", failed)
        } else {
            String::new()
        };
        Line::from(vec![
            Span::styled(
                "  ↑↓/jk scroll   / filter   x clear   s size   p speed   d default   q quit",
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                lock_hint,
                Style::default().fg(Color::Yellow),
            ),
        ])
    };

    frame.render_widget(Paragraph::new(content), area);
}

fn render_file_list(frame: &mut Frame, area: Rect, app: &AppState) {
    let files = app.filtered_files();
    let max_rows = area.height as usize;
    let start = app.scroll_offset.min(files.len().saturating_sub(1));
    let end = (start + max_rows).min(files.len());

    if files.is_empty() {
        let msg = if app.filter_input.is_empty() {
            " No files"
        } else {
            " No files match filter"
        };
        frame.render_widget(
            Paragraph::new(msg).style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    for (row, file) in files[start..end].iter().enumerate() {
        let y = area.y + row as u16;
        if y >= area.bottom() {
            break;
        }
        let row_area = Rect::new(area.x, y, area.width, 1);
        render_file_row(frame, row_area, file);
    }

    // Scrollbar on right edge if needed
    if files.len() > max_rows {
        render_scrollbar(frame, area, files.len(), max_rows, start);
    }
}

fn render_file_row(frame: &mut Frame, area: Rect, file: &FileStatus) {
    let w = area.width as usize;

    // Right-side column widths: size(7) + gap(2) + bar(20) + gap(2) + speed(13) + gap(2) + status(9)
    // speed = "{:>8.1} MB/s" = 13 chars; status has a leading space for padding
    // = 55 chars. Filename gets the rest.
    let right_cols = 55usize;
    let name_width = w.saturating_sub(right_cols + 2).max(8); // +2 for leading spaces

    let name = truncate_str(&file.filename, name_width);
    let name_padded = format!("  {:<name_width$}", name, name_width = name_width);

    let size_str = format!("{:>7}", format_size_bytes(file.size_bytes));

    let line: Line = match &file.stage {
        FileStage::Found => Line::from(vec![
            Span::styled(name_padded, Style::default().fg(Color::DarkGray)),
            Span::styled(size_str, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(
                format!("{:<20}", ""),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{:>13}", ""),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            Span::styled(" pending ", Style::default().fg(Color::DarkGray)),
        ]),

        FileStage::Mapped => Line::from(vec![
            Span::styled(name_padded, Style::default().fg(Color::Blue)),
            Span::styled(size_str, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(
                format!("{:<20}", ""),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{:>13}", ""),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            Span::styled(" mapped  ", Style::default().fg(Color::Blue)),
        ]),

        FileStage::Warming { progress, speed, .. } => {
            let bar = progress_bar(*progress, 20);
            let (filled_len, empty_len) = bar;
            Line::from(vec![
                Span::styled(name_padded, Style::default().fg(Color::Cyan)),
                Span::styled(size_str, Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled(
                    "█".repeat(filled_len),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    "░".repeat(empty_len),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{:>8.1} MB/s", speed),
                    Style::default().fg(Color::White),
                ),
                Span::raw("  "),
                Span::styled(
                    format!(" {:>5.1}%  ", progress),
                    Style::default().fg(Color::Cyan),
                ),
            ])
        }

        FileStage::Complete { speed, .. } => Line::from(vec![
            Span::styled(name_padded, Style::default().fg(Color::Green)),
            Span::styled(size_str, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled("████████████████████", Style::default().fg(Color::Green)),
            Span::raw("  "),
            Span::styled(
                format!("{:>8.1} MB/s", speed),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            Span::styled(" done    ", Style::default().fg(Color::Green)),
        ]),

        FileStage::Locking { speed, .. } => Line::from(vec![
            Span::styled(name_padded, Style::default().fg(Color::Yellow)),
            Span::styled(size_str, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled("████████████████████", Style::default().fg(Color::Yellow)),
            Span::raw("  "),
            Span::styled(
                format!("{:>8.1} MB/s", speed),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            Span::styled(" locking…", Style::default().fg(Color::Yellow)),
        ]),

        FileStage::Locked { speed, .. } => Line::from(vec![
            Span::styled(
                name_padded,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(size_str, Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(
                "████████████████████",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{:>8.1} MB/s", speed),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw("  "),
            Span::styled(
                " locked  ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    };

    frame.render_widget(Paragraph::new(line), area);
}

/// Returns (filled_count, empty_count) for a progress bar of given width
fn progress_bar(progress: f64, width: usize) -> (usize, usize) {
    let filled = ((width as f64 * progress / 100.0).round() as usize).min(width);
    (filled, width - filled)
}

fn render_scrollbar(frame: &mut Frame, area: Rect, total: usize, visible: usize, offset: usize) {
    if total == 0 || visible >= total || area.height == 0 || area.width == 0 {
        return;
    }
    let track = area.height as usize;

    // Thumb size: proportional to visible/total, at least 1 row
    let thumb = ((visible as f64 / total as f64) * track as f64)
        .round() as usize;
    let thumb = thumb.max(1).min(track);

    // Thumb position: proportional to how far we've scrolled
    let scroll_range = total.saturating_sub(visible);
    let track_range = track.saturating_sub(thumb);
    let thumb_start = if scroll_range == 0 || track_range == 0 {
        0
    } else {
        ((offset as f64 / scroll_range as f64) * track_range as f64).round() as usize
    };
    let thumb_start = thumb_start.min(track_range);

    let x = area.right().saturating_sub(1);

    // Render cell by cell so we never go out of bounds
    for row in 0..track {
        let y = area.y + row as u16;
        if y >= area.bottom() {
            break;
        }
        let in_thumb = row >= thumb_start && row < thumb_start + thumb;
        let (ch, color) = if in_thumb { ("▐", Color::Gray) } else { ("░", Color::DarkGray) };
        frame.render_widget(
            Paragraph::new(ch).style(Style::default().fg(color)),
            Rect::new(x, y, 1, 1),
        );
    }
}

fn render_help_overlay(frame: &mut Frame, area: Rect, app: &AppState) {
    let failed = &app.failed_lock_files;

    // Box: up to 20 rows tall, 64 cols wide, centered
    let box_w = 64u16.min(area.width.saturating_sub(4));
    let content_lines = 5 + failed.len() as u16; // header + blank + files + blank + footer
    let box_h = (content_lines + 2).min(area.height.saturating_sub(4)); // +2 for border
    let x = area.x + area.width.saturating_sub(box_w) / 2;
    let y = area.y + area.height.saturating_sub(box_h) / 2;
    let popup_area = Rect::new(x, y, box_w, box_h);

    // Clear the area beneath the popup first
    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .title(" Lock Failures ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let inner_w = inner.width as usize;

    // Build lines
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        format!("{} file(s) could not be locked in RAM:", failed.len()),
        Style::default().fg(Color::Yellow),
    )));
    lines.push(Line::from(""));

    for path in failed {
        let display = if path.len() > inner_w.saturating_sub(2) {
            format!("  …{}", &path[path.len().saturating_sub(inner_w.saturating_sub(3))..])
        } else {
            format!("  {}", path)
        };
        lines.push(Line::from(Span::styled(display, Style::default().fg(Color::Red))));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Fix: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("ulimit -l unlimited", Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(Span::styled(
        "      (add to ~/.bashrc or /etc/security/limits.conf)",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press h or ESC to close",
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}

// ─── Formatting helpers ───────────────────────────────────────────────────────

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

fn truncate_str(s: &str, max_len: usize) -> String {
    if max_len < 3 {
        return s[..max_len.min(s.len())].to_string();
    }
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}
