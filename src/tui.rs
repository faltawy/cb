use std::time::{Duration, Instant};

use chrono::{Duration as ChronoDuration, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use rusqlite::Connection;

use crate::clipboard::{write_image_to_clipboard, write_text_to_clipboard};
use crate::config::AppPaths;
use crate::daemon;
use crate::storage::models::{ClipFilter, ContentType};
use crate::storage::sqlite::SqliteStorage;
use crate::storage::ClipStorage;

#[derive(PartialEq)]
enum Mode {
    Normal,
    Search,
    Tag,
    RemoveTag,
    ConfirmDelete(i64),
}

struct App {
    clips: Vec<crate::storage::models::Clip>,
    list_state: ListState,
    mode: Mode,
    search_query: String,
    tag_input: String,
    status: String,
    status_time: Option<Instant>,
    preview_scroll: u16,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            clips: Vec::new(),
            list_state,
            mode: Mode::Normal,
            search_query: String::new(),
            tag_input: String::new(),
            status: String::new(),
            status_time: None,
            preview_scroll: 0,
            should_quit: false,
        }
    }

    fn set_status(&mut self, msg: String) {
        self.status = msg;
        self.status_time = Some(Instant::now());
    }

    fn selected_clip_id(&self) -> Option<i64> {
        self.list_state
            .selected()
            .and_then(|i| self.clips.get(i))
            .map(|c| c.id)
    }

    fn select_next(&mut self) {
        if self.clips.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1).min(self.clips.len() - 1),
            None => 0,
        };
        self.list_state.select(Some(i));
        self.preview_scroll = 0;
    }

    fn select_prev(&mut self) {
        if self.clips.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.list_state.select(Some(i));
        self.preview_scroll = 0;
    }

    fn select_by(&mut self, delta: isize) {
        if self.clips.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0) as isize;
        let new = (current + delta).clamp(0, self.clips.len() as isize - 1) as usize;
        self.list_state.select(Some(new));
        self.preview_scroll = 0;
    }

    fn select_first(&mut self) {
        if !self.clips.is_empty() {
            self.list_state.select(Some(0));
            self.preview_scroll = 0;
        }
    }

    fn select_last(&mut self) {
        if !self.clips.is_empty() {
            self.list_state.select(Some(self.clips.len() - 1));
            self.preview_scroll = 0;
        }
    }

    fn refresh(&mut self, storage: &SqliteStorage) {
        let result = if self.search_query.is_empty() {
            storage.list(ClipFilter {
                limit: 100,
                ..Default::default()
            })
        } else {
            storage.search(&self.search_query, 100)
        };

        match result {
            Ok(clips) => self.clips = clips,
            Err(e) => self.set_status(format!("Error: {e}")),
        }

        // Clamp selection
        if self.clips.is_empty() {
            self.list_state.select(None);
        } else if let Some(i) = self.list_state.selected() {
            if i >= self.clips.len() {
                self.list_state.select(Some(self.clips.len() - 1));
            }
        } else {
            self.list_state.select(Some(0));
        }
    }

    fn copy_selected(&mut self, storage: &SqliteStorage) {
        let Some(idx) = self.list_state.selected() else {
            return;
        };
        let Some(clip) = self.clips.get(idx) else {
            return;
        };

        match clip.content_type {
            ContentType::Text => {
                if let Some(ref text) = clip.text_content {
                    match write_text_to_clipboard(text) {
                        Ok(()) => {
                            let _ = storage.touch(clip.id);
                            self.set_status(format!("Copied #{}", clip.id));
                        }
                        Err(e) => self.set_status(format!("Copy failed: {e}")),
                    }
                }
            }
            ContentType::Image => {
                if let Some(ref path) = clip.image_path {
                    match write_image_to_clipboard(std::path::Path::new(path)) {
                        Ok(()) => {
                            let _ = storage.touch(clip.id);
                            self.set_status(format!("Copied image #{}", clip.id));
                        }
                        Err(e) => self.set_status(format!("Copy image failed: {e}")),
                    }
                } else {
                    self.set_status("Image clip has no path".to_string());
                }
            }
            ContentType::FileRef => {
                self.set_status(format!(
                    "File ref: {}",
                    clip.text_content.as_deref().unwrap_or("?")
                ));
            }
        }
    }

    fn request_delete(&mut self) {
        let Some(id) = self.selected_clip_id() else {
            return;
        };
        self.mode = Mode::ConfirmDelete(id);
        self.set_status(format!("Delete #{id}? [y/n]"));
    }

    fn confirm_delete(&mut self, storage: &SqliteStorage, id: i64) {
        match storage.delete(id) {
            Ok(true) => {
                self.set_status(format!("Deleted #{id}"));
                self.refresh(storage);
            }
            Ok(false) => self.set_status(format!("#{id} not found")),
            Err(e) => self.set_status(format!("Delete error: {e}")),
        }
    }

    fn toggle_pin(&mut self, storage: &SqliteStorage) {
        let Some(idx) = self.list_state.selected() else {
            return;
        };
        let Some(clip) = self.clips.get(idx) else {
            return;
        };
        let new_pinned = !clip.pinned;
        match storage.set_pinned(clip.id, new_pinned) {
            Ok(()) => {
                let verb = if new_pinned { "Pinned" } else { "Unpinned" };
                self.set_status(format!("{verb} #{}", clip.id));
                self.refresh(storage);
            }
            Err(e) => self.set_status(format!("Pin error: {e}")),
        }
    }

    fn add_tag(&mut self, storage: &SqliteStorage) {
        let tag = self.tag_input.trim().to_string();
        if tag.is_empty() {
            self.set_status("Empty tag".to_string());
            return;
        }
        let Some(id) = self.selected_clip_id() else {
            return;
        };
        match storage.add_tag(id, &tag) {
            Ok(()) => {
                self.set_status(format!("Tagged #{id} \"{tag}\""));
                self.refresh(storage);
            }
            Err(e) => self.set_status(format!("Tag error: {e}")),
        }
        self.tag_input.clear();
    }

    fn remove_tag(&mut self, storage: &SqliteStorage) {
        let tag = self.tag_input.trim().to_string();
        if tag.is_empty() {
            self.set_status("Empty tag".to_string());
            return;
        }
        let Some(id) = self.selected_clip_id() else {
            return;
        };
        match storage.remove_tag(id, &tag) {
            Ok(()) => {
                self.set_status(format!("Removed tag \"{tag}\" from #{id}"));
                self.refresh(storage);
            }
            Err(e) => self.set_status(format!("Remove tag error: {e}")),
        }
        self.tag_input.clear();
    }

    fn toggle_daemon(&mut self, paths: &AppPaths) {
        match daemon::daemon_status(paths) {
            Ok(Some(pid)) => {
                match daemon::stop_daemon(paths) {
                    Ok(true) => self.set_status(format!("Stopped daemon (was pid {pid})")),
                    Ok(false) => self.set_status("Daemon already stopped".to_string()),
                    Err(e) => self.set_status(format!("Stop error: {e}")),
                }
            }
            Ok(None) => {
                match start_daemon(paths) {
                    Ok(pid) => self.set_status(format!("Started daemon (pid {pid})")),
                    Err(e) => self.set_status(format!("Start error: {e}")),
                }
            }
            Err(e) => self.set_status(format!("Status error: {e}")),
        }
    }

    fn clear_old(&mut self, storage: &SqliteStorage) {
        let cutoff = Utc::now() - ChronoDuration::days(30);
        match storage.clear_older_than(cutoff) {
            Ok(n) => {
                self.set_status(format!("Cleared {n} clip(s) older than 30 days"));
                self.refresh(storage);
            }
            Err(e) => self.set_status(format!("Clear error: {e}")),
        }
    }
}

fn start_daemon(paths: &AppPaths) -> crate::errors::Result<u32> {
    let exe = std::env::current_exe()
        .map_err(|e| crate::errors::CbError::Daemon(e.to_string()))?;
    std::fs::create_dir_all(&paths.base_dir)
        .map_err(|e| crate::errors::CbError::Daemon(e.to_string()))?;
    let log_file = std::fs::File::create(&paths.log_file)
        .map_err(|e| crate::errors::CbError::Daemon(e.to_string()))?;

    let child = std::process::Command::new(exe)
        .args(["daemon", "run"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(log_file))
        .spawn()
        .map_err(|e| crate::errors::CbError::Daemon(e.to_string()))?;

    Ok(child.id())
}

fn format_age(dt: chrono::DateTime<Utc>) -> String {
    let dur = Utc::now() - dt;
    if dur.num_seconds() < 60 {
        "now".to_string()
    } else if dur.num_minutes() < 60 {
        format!("{}m", dur.num_minutes())
    } else if dur.num_hours() < 24 {
        format!("{}h", dur.num_hours())
    } else {
        format!("{}d", dur.num_days())
    }
}

fn format_bytes(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut chars = s.chars();
    let truncated: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

// ── UI rendering ───────────────────────────────────────────────────

fn draw(frame: &mut Frame, app: &mut App, paths: &AppPaths) {
    let [title_area, body_area, help_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    // Title bar
    let daemon_info = match daemon::daemon_status(paths) {
        Ok(Some(pid)) => format!("daemon: running (pid {pid})"),
        _ => "daemon: not running".to_string(),
    };
    let clip_count = app.clips.len();
    let total_size: i64 = app.clips.iter().map(|c| c.size_bytes).sum();
    let title = format!(
        " CB — {clip_count} clips — {} — {daemon_info} ",
        format_bytes(total_size)
    );
    frame.render_widget(
        Paragraph::new(title).style(Style::new().fg(Color::Black).bg(Color::Cyan)),
        title_area,
    );

    // Body: two-pane split
    let [list_area, preview_area] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .areas(body_area);

    // Left pane: clip list
    let items: Vec<ListItem> = app
        .clips
        .iter()
        .map(|clip| {
            let type_ch = match clip.content_type {
                ContentType::Text => "T",
                ContentType::Image => "I",
                ContentType::FileRef => "F",
            };
            let pin = if clip.pinned { "*" } else { " " };
            let age = format_age(clip.updated_at);
            let preview = match clip.content_type {
                ContentType::Text => {
                    let text = clip.text_content.as_deref().unwrap_or("");
                    let oneline = text.replace('\n', "↵");
                    truncate_chars(&oneline, 30)
                }
                ContentType::Image => format!(
                    "{}x{} img",
                    clip.image_width.unwrap_or(0),
                    clip.image_height.unwrap_or(0)
                ),
                ContentType::FileRef => "file ref".to_string(),
            };
            ListItem::new(format!("{:>4} {}{} {:>4}  {}", clip.id, type_ch, pin, age, preview))
        })
        .collect();

    let list_title = if app.mode == Mode::Search {
        format!("Search: {}_", app.search_query)
    } else {
        "Clips".to_string()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(list_title))
        .highlight_style(
            Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, list_area, &mut app.list_state);

    // Right pane: preview
    let preview_content = if let Some(idx) = app.list_state.selected() {
        if let Some(clip) = app.clips.get(idx) {
            let tags = if clip.tags.is_empty() {
                "—".to_string()
            } else {
                clip.tags.join(", ")
            };

            let mut lines = vec![
                Line::from(vec![
                    Span::styled("ID:      ", Style::new().fg(Color::DarkGray)),
                    Span::raw(clip.id.to_string()),
                ]),
                Line::from(vec![
                    Span::styled("Type:    ", Style::new().fg(Color::DarkGray)),
                    Span::raw(clip.content_type.as_str()),
                ]),
                Line::from(vec![
                    Span::styled("Pinned:  ", Style::new().fg(Color::DarkGray)),
                    Span::raw(clip.pinned.to_string()),
                ]),
                Line::from(vec![
                    Span::styled("Tags:    ", Style::new().fg(Color::DarkGray)),
                    Span::raw(tags),
                ]),
                Line::from(vec![
                    Span::styled("Size:    ", Style::new().fg(Color::DarkGray)),
                    Span::raw(format_bytes(clip.size_bytes)),
                ]),
                Line::from(vec![
                    Span::styled("Created: ", Style::new().fg(Color::DarkGray)),
                    Span::raw(clip.created_at.format("%Y-%m-%d %H:%M").to_string()),
                ]),
                Line::raw("─────────────────────────"),
            ];

            match clip.content_type {
                ContentType::Text => {
                    if let Some(ref text) = clip.text_content {
                        for line in text.lines() {
                            lines.push(Line::raw(line.to_string()));
                        }
                    }
                }
                ContentType::Image => {
                    lines.push(Line::from(vec![
                        Span::styled("Path:    ", Style::new().fg(Color::DarkGray)),
                        Span::raw(clip.image_path.as_deref().unwrap_or("?")),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("Dims:    ", Style::new().fg(Color::DarkGray)),
                        Span::raw(format!(
                            "{}x{}",
                            clip.image_width.unwrap_or(0),
                            clip.image_height.unwrap_or(0)
                        )),
                    ]));
                }
                ContentType::FileRef => {
                    lines.push(Line::from(vec![
                        Span::styled("File:    ", Style::new().fg(Color::DarkGray)),
                        Span::raw(clip.text_content.as_deref().unwrap_or("?")),
                    ]));
                }
            }

            lines
        } else {
            vec![Line::raw("No clip selected")]
        }
    } else {
        vec![Line::raw("No clips")]
    };

    let preview_title = match app.mode {
        Mode::Tag => format!("Tag: {}_", app.tag_input),
        Mode::RemoveTag => format!("Remove tag: {}_", app.tag_input),
        _ => {
            if app.preview_scroll > 0 {
                format!("Preview [scroll: {}]", app.preview_scroll)
            } else {
                "Preview".to_string()
            }
        }
    };

    let preview = Paragraph::new(preview_content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(preview_title),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.preview_scroll, 0));

    frame.render_widget(preview, preview_area);

    // Auto-clear status after 3 seconds
    if let Some(t) = app.status_time
        && t.elapsed() > Duration::from_secs(3)
    {
        app.status.clear();
        app.status_time = None;
    }

    // Help bar
    let help_text = match app.mode {
        Mode::Normal | Mode::ConfirmDelete(_) => {
            if app.status.is_empty() {
                " [q]uit [/]search [Enter]copy [d]el [p]in [t]ag [T]untag [r]efresh [D]aemon [c]lear [J/K]scroll"
                    .to_string()
            } else {
                format!(" {} ", app.status)
            }
        }
        Mode::Search => " Type to search (live) · [Enter] done · [Esc] cancel".to_string(),
        Mode::Tag => " Type tag name · [Enter] add · [Esc] cancel".to_string(),
        Mode::RemoveTag => " Type tag name · [Enter] remove · [Esc] cancel".to_string(),
    };

    frame.render_widget(
        Paragraph::new(help_text).style(Style::new().fg(Color::Black).bg(Color::White)),
        help_area,
    );
}

// ── Event handling ─────────────────────────────────────────────────

fn handle_event(app: &mut App, storage: &SqliteStorage, paths: &AppPaths) -> std::io::Result<()> {
    if !event::poll(Duration::from_millis(250))? {
        return Ok(());
    }

    let Event::Key(key) = event::read()? else {
        return Ok(());
    };
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match app.mode {
        Mode::Normal => {
            // Check for Shift+J / Shift+K (preview scroll)
            let shifted = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                KeyCode::Char('J') if shifted => {
                    app.preview_scroll = app.preview_scroll.saturating_add(1);
                }
                KeyCode::Char('K') if shifted => {
                    app.preview_scroll = app.preview_scroll.saturating_sub(1);
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    app.select_next();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.select_prev();
                }
                KeyCode::PageDown => app.select_by(10),
                KeyCode::PageUp => app.select_by(-10),
                KeyCode::Char('g') | KeyCode::Home => app.select_first(),
                KeyCode::Char('G') | KeyCode::End => app.select_last(),
                KeyCode::Enter => app.copy_selected(storage),
                KeyCode::Char('d') => app.request_delete(),
                KeyCode::Char('p') => app.toggle_pin(storage),
                KeyCode::Char('t') => {
                    app.mode = Mode::Tag;
                    app.tag_input.clear();
                    app.status.clear();
                    app.status_time = None;
                }
                KeyCode::Char('T') => {
                    app.mode = Mode::RemoveTag;
                    app.tag_input.clear();
                    app.status.clear();
                    app.status_time = None;
                }
                KeyCode::Char('/') => {
                    app.mode = Mode::Search;
                    app.search_query.clear();
                    app.status.clear();
                    app.status_time = None;
                }
                KeyCode::Char('r') => {
                    app.refresh(storage);
                    app.set_status("Refreshed".to_string());
                }
                KeyCode::Char('D') => app.toggle_daemon(paths),
                KeyCode::Char('c') => app.clear_old(storage),
                _ => {}
            }
        }
        Mode::ConfirmDelete(id) => match key.code {
            KeyCode::Char('y') => {
                app.mode = Mode::Normal;
                app.confirm_delete(storage, id);
            }
            _ => {
                app.mode = Mode::Normal;
                app.set_status("Delete cancelled".to_string());
            }
        },
        Mode::Search => match key.code {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.search_query.clear();
                app.refresh(storage);
            }
            KeyCode::Enter => {
                app.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                app.search_query.pop();
                app.refresh(storage);
            }
            KeyCode::Char(c) => {
                app.search_query.push(c);
                app.refresh(storage);
            }
            _ => {}
        },
        Mode::Tag => match key.code {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.tag_input.clear();
            }
            KeyCode::Enter => {
                app.add_tag(storage);
                app.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                app.tag_input.pop();
            }
            KeyCode::Char(c) => {
                app.tag_input.push(c);
            }
            _ => {}
        },
        Mode::RemoveTag => match key.code {
            KeyCode::Esc => {
                app.mode = Mode::Normal;
                app.tag_input.clear();
            }
            KeyCode::Enter => {
                app.remove_tag(storage);
                app.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                app.tag_input.pop();
            }
            KeyCode::Char(c) => {
                app.tag_input.push(c);
            }
            _ => {}
        },
    }

    Ok(())
}

// ── Entry point ────────────────────────────────────────────────────

pub fn run(paths: &AppPaths) -> crate::errors::Result<()> {
    std::fs::create_dir_all(&paths.base_dir)
        .map_err(|e| crate::errors::CbError::Daemon(e.to_string()))?;
    let conn = Connection::open(&paths.db_path)?;
    let storage = SqliteStorage::new(conn)?;

    let mut app = App::new();
    app.refresh(&storage);

    let mut terminal = ratatui::init();

    let result = (|| {
        loop {
            terminal.draw(|frame| draw(frame, &mut app, paths))?;
            handle_event(&mut app, &storage, paths)?;
            if app.should_quit {
                break;
            }
        }
        Ok::<(), std::io::Error>(())
    })();

    ratatui::restore();

    result.map_err(|e| crate::errors::CbError::Daemon(e.to_string()))
}
