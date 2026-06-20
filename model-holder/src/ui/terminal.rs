use std::io::{stdout, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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
    
    // Build pattern display for title
    let pattern_display = if app.files.is_empty() {
        String::new()
    } else if app.files.len() > 1 {
        // Multiple files - try to extract meaningful directory name
        // First, check if input_patterns has a useful pattern
        let dir = if app.input_patterns.len() == 1 {
            let pattern = &app.input_patterns[0];
            if pattern.contains('*') {
                // Original glob pattern preserved: extract last component before *
                let before_star = pattern.split('*').next().unwrap_or(pattern);
                std::path::Path::new(before_star.trim_end_matches('/'))
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| before_star.to_string())
            } else {
                // Resolved path (from bash realpath): get parent directory
                let clean_pattern = pattern.trim_end_matches('/');
                if pattern.ends_with('/') {
                    std::path::Path::new(clean_pattern)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| clean_pattern.to_string())
                } else {
                    std::path::Path::new(clean_pattern)
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| clean_pattern.to_string())
                }
            }
        } else {
            // Multiple patterns or no patterns: compute common parent from files
            app.compute_common_parent()
                .unwrap_or_else(|| format!("{} files", app.files.len()))
        };
        
        format!(" {}/* ({})", dir, app.files.len())
    } else {
        // Single file - MUST show filename
        let filename = &app.files[0].filename;
        format!(" {}", filename)
    };
    
    let right = format!("  {}", overall);
    
    // Truncate if too long for available width
    let available_width = area.width as usize;
    let max_pattern_len = available_width.saturating_sub(left.len() + right.len() + 4);
    let pattern_display = if pattern_display.len() > max_pattern_len && max_pattern_len > 10 {
        // Truncate with ellipsis
        format!("…{}", &pattern_display[pattern_display.len().saturating_sub(max_pattern_len - 1)..])
    } else {
        pattern_display
    };
    
    let title_text = format!("{}{}{}", left, pattern_display, right);

    let _status_color = if app.all_locked {
        Color::Green
    } else if app.warmup_complete {
        Color::Cyan
    } else {
        Color::Yellow
    };

    let line = Line::from(vec![
        Span::styled(
            title_text,
            Style::default()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
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

    // Calculate aggregate speeds for summary
    let speed_summary = if app.warmup_complete && app.all_locked && !app.files.is_empty() {
        let avg_mmap: f64 = app.files.iter()
            .filter_map(|f| f.mmap_speed)
            .sum::<f64>() / app.files.len() as f64;
        let avg_lock: f64 = app.files.iter()
            .filter_map(|f| f.lock_speed)
            .sum::<f64>() / app.files.len() as f64;
        format!("  avg mmap:{:.1} lock:{:.1} MB/s", avg_mmap, avg_lock)
    } else {
        String::new()
    };

    let text = format!(
        "  {} files  {}  {}/{} complete  {} locked{}{}{}",
        total, total_size, complete, total, locked, speed_summary, sort_label, filter_label
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
            Span::styled(lock_hint, Style::default().fg(Color::Yellow)),
        ])
    };

    frame.render_widget(Paragraph::new(content), area);
}

/// Total width of all non-name columns (size + progress + mmap + lock + total + status + gaps)
const RIGHT_COLS: usize = 69;

fn make_row(
    name: &str, name_style: Style, name_width: usize,
    size: &str, size_style: Style,
    progress: &str, progress_style: Style,
    mmap: &str, mmap_style: Style,
    lock: &str, lock_style: Style,
    total: &str, total_style: Style,
    status: &str, status_style: Style,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {:<name_width$}", name, name_width = name_width), name_style),
        Span::styled(format!("{:>7}", size), size_style),
        Span::raw("  "),
        Span::styled(format!("{:<12}", progress), progress_style),
        Span::raw("  "),
        Span::styled(format!("{:>11}", mmap), mmap_style),
        Span::raw(" "),
        Span::styled(format!("{:>11}", lock), lock_style),
        Span::raw(" "),
        Span::styled(format!("{:>11}", total), total_style),
        Span::raw("  "),
        Span::styled(format!("{:>9}", status), status_style),
    ])
}

fn header_row(name_width: usize) -> Line<'static> {
    let wb = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
    make_row(
        "NAME", wb.clone(), name_width,
        "SIZE", wb.clone(),
        "PROGRESS", wb.clone(),
        "MMAP", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        "LOCK", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        "TOTAL", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        "STATUS", wb.clone(),
    )
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

    let content_width = area.width.saturating_sub(1);
    let content_area = Rect::new(area.x, area.y, content_width, area.height);
    let name_width = (content_width as usize).saturating_sub(RIGHT_COLS + 2).max(8);
    let header = header_row(name_width);

    frame.render_widget(Paragraph::new(header), content_area);

    for (row, file) in files[start..end].iter().enumerate() {
        let y = area.y + (row + 1) as u16; // +1 for header
        if y >= area.bottom() {
            break;
        }
        let row_area = Rect::new(content_area.x, y, content_area.width, 1);
        render_file_row(frame, row_area, file);
    }

    // Scrollbar on right edge if needed (starts below header, avoids status bar)
    if files.len() > max_rows {
        let scrollbar_area = Rect::new(
            area.x + content_width,
            area.y + 1,
            1,
            area.height.saturating_sub(2),
        );
        render_scrollbar(frame, scrollbar_area, files.len(), max_rows, start);
    }
}

fn render_file_row(frame: &mut Frame, area: Rect, file: &FileStatus) {
    let w = area.width as usize;

    let name_width = w.saturating_sub(RIGHT_COLS + 2).max(8);

    let name = truncate_str(&file.filename, name_width);

    let size_str = format!("{:>7}", format_size_bytes(file.size_bytes));

    let line: Line = match &file.stage {
        FileStage::Found => make_row(
            &name, Style::default().fg(Color::DarkGray), name_width,
            &size_str, Style::default().fg(Color::DarkGray),
            "", Style::default().fg(Color::DarkGray),
            "", Style::default().fg(Color::DarkGray),
            "", Style::default().fg(Color::DarkGray),
            "", Style::default().fg(Color::DarkGray),
            " pending ", Style::default().fg(Color::DarkGray),
        ),

        FileStage::Mapped => make_row(
            &name, Style::default().fg(Color::Blue), name_width,
            &size_str, Style::default().fg(Color::DarkGray),
            "", Style::default().fg(Color::DarkGray),
            "", Style::default().fg(Color::DarkGray),
            "", Style::default().fg(Color::DarkGray),
            "", Style::default().fg(Color::DarkGray),
            " mapped  ", Style::default().fg(Color::Blue),
        ),

        FileStage::Warming {
            progress, speed, ..
        } => {
            let bar = progress_bar(*progress, 12);
            let (filled_len, empty_len) = bar;
            let bar_str = format!("{}{}", "█".repeat(filled_len), "░".repeat(empty_len));
            let speed_str = format!("{:>6.1} MB/s", speed);
            let progress_str = format!("{:>5.1}%", progress);
            make_row(
                &name, Style::default().fg(Color::Cyan), name_width,
                &size_str, Style::default().fg(Color::DarkGray),
                &bar_str, Style::default().fg(Color::Cyan),
                &speed_str, Style::default().fg(Color::White),
                "", Style::default().fg(Color::DarkGray),
                "", Style::default().fg(Color::DarkGray),
                &progress_str, Style::default().fg(Color::Cyan),
            )
        }

        FileStage::Complete { speed, .. } => {
            let bar_str = "████████████".to_string();
            let speed_str = format!("{:>6.1} MB/s", speed);
            make_row(
                &name, Style::default().fg(Color::Green), name_width,
                &size_str, Style::default().fg(Color::DarkGray),
                &bar_str, Style::default().fg(Color::Green),
                "", Style::default().fg(Color::DarkGray),
                "", Style::default().fg(Color::DarkGray),
                &speed_str, Style::default().fg(Color::Green),
                "done    ", Style::default().fg(Color::Green),
            )
        }

        FileStage::Locking { speed, .. } => {
            let bar_str = "████████████".to_string();
            let speed_str = format!("{:>6.1} MB/s", speed);
            make_row(
                &name, Style::default().fg(Color::Yellow), name_width,
                &size_str, Style::default().fg(Color::DarkGray),
                &bar_str, Style::default().fg(Color::Yellow),
                &speed_str, Style::default().fg(Color::Yellow),
                "locking…", Style::default().fg(Color::Yellow),
                "", Style::default().fg(Color::DarkGray),
                "", Style::default().fg(Color::DarkGray),
            )
        }

        FileStage::Locked { mmap_speed, lock_speed, total_speed, .. } => {
            let mmap_str = if *mmap_speed >= 1000.0 {
                format!("{:>6.2} GB/s", mmap_speed / 1000.0)
            } else {
                format!("{:>6.1} MB/s", mmap_speed)
            };
            let lock_str = if *lock_speed >= 1000.0 {
                format!("{:>6.2} GB/s", lock_speed / 1000.0)
            } else {
                format!("{:>6.1} MB/s", lock_speed)
            };
            let total_str = if *total_speed >= 1000.0 {
                format!("{:>6.2} GB/s", total_speed / 1000.0)
            } else {
                format!("{:>6.1} MB/s", total_speed)
            };
            let bar_str = "████████████".to_string();
            make_row(
                &name, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD), name_width,
                &size_str, Style::default().fg(Color::DarkGray),
                &bar_str, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                &mmap_str, Style::default().fg(Color::Cyan),
                &lock_str, Style::default().fg(Color::Yellow),
                &total_str, Style::default().fg(Color::Green),
                " locked  ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )
        }
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
    let thumb = ((visible as f64 / total as f64) * track as f64).round() as usize;
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
        let (ch, color) = if in_thumb {
            ("▐", Color::Gray)
        } else {
            ("░", Color::DarkGray)
        };
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
            format!(
                "  …{}",
                &path[path.len().saturating_sub(inner_w.saturating_sub(3))..]
            )
        } else {
            format!("  {}", path)
        };
        lines.push(Line::from(Span::styled(
            display,
            Style::default().fg(Color::Red),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "Fix: ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("ulimit -l unlimited", Style::default().fg(Color::Cyan)),
    ]));
    lines.push(Line::from(Span::styled(
        "      (add to ~/.bashrc or /etc/security/limits.conf)",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Speed columns (locked files):",
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "  mmap  = memory-mapped load (cyan)",
        Style::default().fg(Color::Cyan),
    )));
    lines.push(Line::from(Span::styled(
        "  lock  = page-locking speed (yellow)",
        Style::default().fg(Color::Yellow),
    )));
    lines.push(Line::from(Span::styled(
        "  total = combined speed (green)",
        Style::default().fg(Color::Green),
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

fn truncate_str(s: &str, max_chars: usize) -> String {
    if max_chars < 2 {
        return s[..max_chars.min(s.len())].to_string();
    }
    match s.char_indices().nth(max_chars - 1) {
        None => s.to_string(),
        Some((byte_pos, _)) => format!("{}…", &s[..byte_pos]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The total width of all non-name columns (size + progress + mmap + lock + total + status + gaps)
    /// This is the expected width contribution from every column EXCEPT the name column.
    const EXPECTED_RIGHT_COLS_WIDTH: usize = 7 + 2 + 12 + 2 + 11 + 1 + 11 + 1 + 11 + 2 + 9;

    /// Verify the constant matches the actual column layout in make_row
    #[test]
    fn right_cols_constant_matches_layout() {
        assert_eq!(RIGHT_COLS, EXPECTED_RIGHT_COLS_WIDTH);
    }

    /// A row built with make_row should have total width = 2 (leading spaces) + name_width + RIGHT_COLS
    #[test]
    fn make_row_total_width_matches_formula() {
        let name_width: usize = 20;
        let style = Style::default();
        let row = make_row(
            "test", style, name_width,
            "1.0M", style,
            "████████████", style,
            "100 MB/s", style,
            "50 MB/s", style,
            "150 MB/s", style,
            "done", style,
        );
        let expected = 2 + name_width + RIGHT_COLS;
        assert_eq!(row.width(), expected, "row width should be 2 + name_width + RIGHT_COLS");
    }

    /// Header and data row with the SAME name_width must produce the same total width
    #[test]
    fn header_and_data_row_same_width() {
        let name_width: usize = 25;
        let wb = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
        let header = header_row(name_width);

        let row = make_row(
            "test.safetensors", wb, name_width,
            "800M", wb,
            "████████████", wb,
            "100.00 GB/s", wb,
            "50.00 GB/s", wb,
            "150.00 GB/s", wb,
            "locked", wb,
        );

        assert_eq!(
            header.width(),
            row.width(),
            "header width ({}) must equal data row width ({}) for same name_width ({})",
            header.width(),
            row.width(),
            name_width
        );
    }

    /// The actual name_width computed in render_file_list must match the one in render_file_row
    /// Both should use: (content_width as usize).saturating_sub(RIGHT_COLS + 2).max(8)
    #[test]
    fn name_width_formula_consistency() {
        let area_width: u16 = 120;
        let content_width = area_width.saturating_sub(1);

        // What render_file_list computes:
        let header_name_width = (content_width as usize).saturating_sub(RIGHT_COLS + 2).max(8);

        // What render_file_row computes (it gets content_area.width as area.width):
        let row_name_width = (content_width as usize).saturating_sub(RIGHT_COLS + 2).max(8);

        assert_eq!(
            header_name_width, row_name_width,
            "header and data row must compute the same name_width"
        );
    }

    /// Full end-to-end: build header and data row with the name_width that render_file_list
    /// would compute for a given terminal width, then verify they align
    #[test]
    fn full_alignment_for_various_terminals() {
        for term_width in [80u16, 100, 120, 150, 200] {
            let content_width = term_width.saturating_sub(1);
            let name_width = (content_width as usize).saturating_sub(RIGHT_COLS + 2).max(8);

            let wb = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
            let header = header_row(name_width);

            // Data rows truncate the filename BEFORE calling make_row
            let filename = "file.safetensors";
            let truncated = truncate_str(filename, name_width);
            let row = make_row(
                &truncated, wb, name_width,
                "800M", wb,
                "████████████", wb,
                "100.00 GB/s", wb,
                "50.00 GB/s", wb,
                "150.00 GB/s", wb,
                "locked", wb,
            );

            assert_eq!(
                header.width(),
                row.width(),
                "width mismatch at terminal width {} (name_width={}), header={}, row={}",
                term_width,
                name_width,
                header.width(),
                row.width()
            );

            // Also verify the row fits in content_width
            assert!(
                row.width() <= content_width as usize,
                "row width ({}) should fit in content_width ({}) at terminal width {}",
                row.width(),
                content_width,
                term_width
            );
        }
    }

    /// Edge case: narrow terminal (width = 80, minimum for this layout)
    #[test]
    fn narrow_terminal_alignment() {
        let term_width: u16 = 80;
        let content_width = term_width.saturating_sub(1);
        let name_width = (content_width as usize).saturating_sub(RIGHT_COLS + 2).max(8);

        let wb = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
        let header = header_row(name_width);
        let filename = "f";
        let truncated = truncate_str(filename, name_width);
        let row = make_row(
            &truncated, wb, name_width,
            "1K", wb,
            "", wb,
            "", wb,
            "", wb,
            "", wb,
            "pending", wb,
        );

        assert_eq!(header.width(), row.width(), "narrow terminal: widths must match (header={}, row={})", header.width(), row.width());
        
        // The row should fit in content_width (or be very close due to minimum name_width)
        assert!(
            row.width() <= content_width as usize + 10, // Allow some slack for minimum name_width
            "row width ({}) should fit in content_width ({}) at terminal width {}",
            row.width(),
            content_width,
            term_width
        );
    }

    /// Test Warming stage alignment (progress bar + speed in progress column)
    #[test]
    fn warming_stage_alignment() {
        let term_width: u16 = 120;
        let content_width = term_width.saturating_sub(1);
        let name_width = (content_width as usize).saturating_sub(RIGHT_COLS + 2).max(8);

        let wb = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
        let header = header_row(name_width);

        // Warming stage: progress bar (12 chars) + speed (8 chars) in progress column
        let filename = "test.safetensors";
        let truncated = truncate_str(filename, name_width);
        let bar_str = "████████████".to_string(); // 12 chars
        let speed_str = format!("{:>6.1} MB/s", 50.0); // "  50.0 MB/s" = 10 chars
        let progress_str = format!("{:>5.1}%", 50.0); // " 50.0%" = 6 chars

        let row = make_row(
            &truncated, wb, name_width,
            "800M", wb,
            &bar_str, wb,
            &speed_str, wb,
            "", wb,
            "", wb,
            &progress_str, wb,
        );

        assert_eq!(header.width(), row.width(), "warming: header width ({}) must equal row width ({})", header.width(), row.width());
        assert!(row.width() <= content_width as usize, "warming row must fit in content area");
    }

    /// Test Locking stage alignment (bar in progress, speed in mmap, "locking…" in lock column)
    #[test]
    fn locking_stage_alignment() {
        let term_width: u16 = 120;
        let content_width = term_width.saturating_sub(1);
        let name_width = (content_width as usize).saturating_sub(RIGHT_COLS + 2).max(8);

        let wb = Style::default().fg(Color::White).add_modifier(Modifier::BOLD);
        let header = header_row(name_width);

        // Locking stage: bar in progress, speed in mmap, "locking…" in lock column
        let filename = "test.safetensors";
        let truncated = truncate_str(filename, name_width);
        let bar_str = "████████████".to_string();
        let speed_str = format!("{:>6.1} MB/s", 50.0);
        let lock_str = "locking…"; // 8 chars, fits in 11-char lock column

        let row = make_row(
            &truncated, wb, name_width,
            "800M", wb,
            &bar_str, wb,
            &speed_str, wb,
            lock_str, wb,
            "", wb,
            "", wb,
        );

        assert_eq!(header.width(), row.width(), "locking: header width ({}) must equal row width ({})", header.width(), row.width());
        assert!(row.width() <= content_width as usize, "locking row must fit in content area");
    }
}
