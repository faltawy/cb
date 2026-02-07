use std::process;

use chrono::{Duration, Utc};
use clap::{Parser, Subcommand};
use rusqlite::Connection;
use serde::Serialize;

use cb::clipboard::{write_image_to_clipboard, write_text_to_clipboard};
use cb::config::AppPaths;
use cb::daemon;
use cb::storage::models::{ClipFilter, ContentType};
use cb::storage::sqlite::SqliteStorage;
use cb::storage::ClipStorage;

#[derive(Parser)]
#[command(name = "cb", version, about = "A clipboard manager for macOS")]
struct Cli {
    /// Output results as JSON
    #[arg(short = 'j', long = "json", global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List recent clipboard entries
    List {
        /// Maximum number of entries to show
        #[arg(short, long, default_value = "10")]
        limit: i64,

        /// Offset for pagination
        #[arg(short, long, default_value = "0")]
        offset: i64,

        /// Filter by type: text, image, fileref
        #[arg(short = 't', long)]
        r#type: Option<String>,

        /// Show only pinned entries
        #[arg(short, long)]
        pinned: bool,

        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
    },

    /// Search clipboard history
    Search {
        /// Search query
        query: String,

        /// Maximum results
        #[arg(short, long, default_value = "10")]
        limit: i64,
    },

    /// Get a specific clip by ID
    Get {
        /// Clip ID
        id: i64,
    },

    /// Copy a clip back to the clipboard
    Copy {
        /// Clip ID
        id: i64,
    },

    /// Delete a clip
    Delete {
        /// Clip ID
        id: i64,
    },

    /// Pin or unpin a clip
    Pin {
        /// Clip ID
        id: i64,

        /// Unpin instead of pin
        #[arg(short, long)]
        unpin: bool,
    },

    /// Add or remove tags
    Tag {
        /// Clip ID
        id: i64,

        /// Tag name
        tag: String,

        /// Remove the tag instead of adding
        #[arg(short, long)]
        remove: bool,
    },

    /// Clear old entries
    Clear {
        /// Clear entries older than N days
        #[arg(short, long, default_value = "30")]
        days: i64,
    },

    /// Show storage statistics
    Stats,

    /// Interactive TUI
    Tui,

    /// Manage the clipboard watcher daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the clipboard watcher
    Start,
    /// Stop the clipboard watcher
    Stop,
    /// Check daemon status
    Status,
    /// Run watcher in foreground (used internally)
    #[command(hide = true)]
    Run,
}

#[derive(Serialize)]
struct StatusResponse {
    success: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    removed: Option<i64>,
}

fn main() {
    let cli = Cli::parse();
    let json = cli.json;

    if let Err(e) = run(cli) {
        if json {
            eprintln!("{}", serde_json::json!({"error": e.to_string()}));
        } else {
            eprintln!("error: {}", e);
        }
        process::exit(1);
    }
}

fn run(cli: Cli) -> cb::errors::Result<()> {
    let paths = AppPaths::new();
    let json = cli.json;

    match cli.command {
        None => cmd_list(
            &paths,
            ClipFilter {
                limit: 10,
                ..Default::default()
            },
            json,
        ),
        Some(Commands::List {
            limit,
            offset,
            r#type,
            pinned,
            tag,
        }) => {
            let content_type = r#type.as_deref().and_then(ContentType::parse);
            cmd_list(
                &paths,
                ClipFilter {
                    content_type,
                    pinned: if pinned { Some(true) } else { None },
                    tag,
                    limit,
                    offset,
                },
                json,
            )
        }
        Some(Commands::Search { query, limit }) => cmd_search(&paths, &query, limit, json),
        Some(Commands::Get { id }) => cmd_get(&paths, id, json),
        Some(Commands::Copy { id }) => cmd_copy(&paths, id, json),
        Some(Commands::Delete { id }) => cmd_delete(&paths, id, json),
        Some(Commands::Pin { id, unpin }) => cmd_pin(&paths, id, !unpin, json),
        Some(Commands::Tag { id, tag, remove }) => cmd_tag(&paths, id, &tag, remove, json),
        Some(Commands::Clear { days }) => cmd_clear(&paths, days, json),
        Some(Commands::Stats) => cmd_stats(&paths, json),
        Some(Commands::Tui) => cb::tui::run(&paths),
        Some(Commands::Daemon { action }) => cmd_daemon(&paths, action, json),
    }
}

fn open_storage(paths: &AppPaths) -> cb::errors::Result<SqliteStorage> {
    std::fs::create_dir_all(&paths.base_dir)
        .map_err(|e| cb::errors::CbError::Storage(rusqlite::Error::ToSqlConversionFailure(e.into())))?;
    let conn = Connection::open(&paths.db_path)?;
    SqliteStorage::new(conn)
}

fn cmd_list(paths: &AppPaths, filter: ClipFilter, json: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let clips = storage.list(filter)?;

    if json {
        println!("{}", serde_json::to_string(&clips).unwrap());
        return Ok(());
    }

    if clips.is_empty() {
        println!("No clips found.");
        return Ok(());
    }

    for clip in &clips {
        print_clip_row(clip);
    }
    Ok(())
}

fn cmd_search(paths: &AppPaths, query: &str, limit: i64, json: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let clips = storage.search(query, limit)?;

    if json {
        println!("{}", serde_json::to_string(&clips).unwrap());
        return Ok(());
    }

    if clips.is_empty() {
        println!("No results for \"{}\".", query);
        return Ok(());
    }

    for clip in &clips {
        print_clip_row(clip);
    }
    Ok(())
}

fn cmd_get(paths: &AppPaths, id: i64, json: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let clip = storage.get_by_id(id)?;

    if json {
        println!("{}", serde_json::to_string(&clip).unwrap());
        return Ok(());
    }

    print_clip_detail(&clip);
    Ok(())
}

fn cmd_copy(paths: &AppPaths, id: i64, json: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let clip = storage.get_by_id(id)?;

    let message = match clip.content_type {
        ContentType::Text => {
            if let Some(ref text) = clip.text_content {
                write_text_to_clipboard(text)?;
                format!("Copied clip #{} to clipboard.", id)
            } else {
                format!("Text clip #{} has no content.", id)
            }
        }
        ContentType::Image => {
            if let Some(ref path) = clip.image_path {
                write_image_to_clipboard(std::path::Path::new(path))?;
                format!("Copied image clip #{} to clipboard.", id)
            } else {
                format!("Image clip #{} has no stored path.", id)
            }
        }
        ContentType::FileRef => {
            format!(
                "File reference: {}",
                clip.text_content.as_deref().unwrap_or("unknown")
            )
        }
    };

    storage.touch(id)?;

    if json {
        println!(
            "{}",
            serde_json::to_string(&StatusResponse {
                success: true,
                message,
                removed: None,
            })
            .unwrap()
        );
    } else {
        println!("{}", message);
    }
    Ok(())
}

fn cmd_delete(paths: &AppPaths, id: i64, json: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let found = storage.delete(id)?;
    let message = if found {
        format!("Deleted clip #{}.", id)
    } else {
        format!("Clip #{} not found.", id)
    };

    if json {
        println!(
            "{}",
            serde_json::to_string(&StatusResponse {
                success: found,
                message,
                removed: None,
            })
            .unwrap()
        );
    } else {
        println!("{}", message);
    }
    Ok(())
}

fn cmd_pin(paths: &AppPaths, id: i64, pinned: bool, json: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    storage.set_pinned(id, pinned)?;
    let message = if pinned {
        format!("Pinned clip #{}.", id)
    } else {
        format!("Unpinned clip #{}.", id)
    };

    if json {
        println!(
            "{}",
            serde_json::to_string(&StatusResponse {
                success: true,
                message,
                removed: None,
            })
            .unwrap()
        );
    } else {
        println!("{}", message);
    }
    Ok(())
}

fn cmd_tag(paths: &AppPaths, id: i64, tag: &str, remove: bool, json: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let message = if remove {
        storage.remove_tag(id, tag)?;
        format!("Removed tag \"{}\" from clip #{}.", tag, id)
    } else {
        storage.add_tag(id, tag)?;
        format!("Added tag \"{}\" to clip #{}.", tag, id)
    };

    if json {
        println!(
            "{}",
            serde_json::to_string(&StatusResponse {
                success: true,
                message,
                removed: None,
            })
            .unwrap()
        );
    } else {
        println!("{}", message);
    }
    Ok(())
}

fn cmd_clear(paths: &AppPaths, days: i64, json: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let cutoff = Utc::now() - Duration::days(days);
    let removed = storage.clear_older_than(cutoff)?;

    if json {
        println!(
            "{}",
            serde_json::to_string(&StatusResponse {
                success: true,
                message: format!("Removed {} clip(s) older than {} days.", removed, days),
                removed: Some(removed),
            })
            .unwrap()
        );
    } else {
        println!("Removed {} clip(s) older than {} days.", removed, days);
    }
    Ok(())
}

fn cmd_stats(paths: &AppPaths, json: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let stats = storage.stats()?;

    if json {
        let daemon_pid = daemon::daemon_status(paths).ok().flatten();
        let mut obj = serde_json::to_value(&stats).unwrap();
        let m = obj.as_object_mut().unwrap();
        m.insert("daemon_running".into(), serde_json::json!(daemon_pid.is_some()));
        m.insert("daemon_pid".into(), serde_json::json!(daemon_pid));
        println!("{}", serde_json::to_string(&obj).unwrap());
        return Ok(());
    }

    println!("Clipboard Statistics");
    println!("────────────────────");
    println!("Total clips:  {}", stats.total_clips);
    println!("  Text:       {}", stats.text_clips);
    println!("  Image:      {}", stats.image_clips);
    println!("  File refs:  {}", stats.fileref_clips);
    println!("Total size:   {}", format_bytes(stats.total_size));
    if let Some(oldest) = stats.oldest {
        println!("Oldest:       {}", oldest.format("%Y-%m-%d %H:%M"));
    }
    if let Some(newest) = stats.newest {
        println!("Newest:       {}", newest.format("%Y-%m-%d %H:%M"));
    }

    if let Ok(Some(pid)) = daemon::daemon_status(paths) {
        println!("Daemon:       running (pid {})", pid);
    } else {
        println!("Daemon:       not running");
    }

    Ok(())
}

fn cmd_daemon(paths: &AppPaths, action: DaemonAction, json: bool) -> cb::errors::Result<()> {
    match action {
        DaemonAction::Start => {
            if let Ok(Some(pid)) = daemon::daemon_status(paths) {
                let msg = format!("Daemon already running (pid {}).", pid);
                if json {
                    println!(
                        "{}",
                        serde_json::to_string(&StatusResponse {
                            success: true,
                            message: msg,
                            removed: None,
                        })
                        .unwrap()
                    );
                } else {
                    println!("{}", msg);
                }
                return Ok(());
            }

            let exe = std::env::current_exe()
                .map_err(|e| cb::errors::CbError::Daemon(e.to_string()))?;

            std::fs::create_dir_all(&paths.base_dir)
                .map_err(|e| cb::errors::CbError::Daemon(e.to_string()))?;
            let log_file = std::fs::File::create(&paths.log_file)
                .map_err(|e| cb::errors::CbError::Daemon(e.to_string()))?;

            let child = std::process::Command::new(exe)
                .args(["daemon", "run"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::from(log_file))
                .spawn()
                .map_err(|e| cb::errors::CbError::Daemon(e.to_string()))?;

            let msg = format!("Started clipboard watcher (pid {}).", child.id());
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&StatusResponse {
                        success: true,
                        message: msg,
                        removed: None,
                    })
                    .unwrap()
                );
            } else {
                println!("{}", msg);
            }
            Ok(())
        }
        DaemonAction::Stop => {
            let stopped = daemon::stop_daemon(paths)?;
            let msg = if stopped {
                "Stopped clipboard watcher."
            } else {
                "Daemon is not running."
            };
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&StatusResponse {
                        success: stopped,
                        message: msg.into(),
                        removed: None,
                    })
                    .unwrap()
                );
            } else {
                println!("{}", msg);
            }
            Ok(())
        }
        DaemonAction::Status => {
            if json {
                let pid = daemon::daemon_status(paths)?;
                println!(
                    "{}",
                    serde_json::json!({
                        "running": pid.is_some(),
                        "pid": pid,
                    })
                );
            } else {
                match daemon::daemon_status(paths)? {
                    Some(pid) => println!("Daemon running (pid {}).", pid),
                    None => println!("Daemon is not running."),
                }
            }
            Ok(())
        }
        DaemonAction::Run => {
            daemon::run_watcher(paths)
        }
    }
}

fn print_clip_row(clip: &cb::storage::models::Clip) {
    let type_icon = match clip.content_type {
        ContentType::Text => "T",
        ContentType::Image => "I",
        ContentType::FileRef => "F",
    };

    let pin = if clip.pinned { "*" } else { " " };

    let preview = match clip.content_type {
        ContentType::Text => {
            let text = clip.text_content.as_deref().unwrap_or("");
            let oneline = text.replace('\n', "\\n");
            if oneline.len() > 60 {
                format!("{}...", &oneline[..57])
            } else {
                oneline
            }
        }
        ContentType::Image => {
            format!("{}x{} image",
                clip.image_width.unwrap_or(0),
                clip.image_height.unwrap_or(0))
        }
        ContentType::FileRef => {
            clip.image_path.as_deref().unwrap_or("file").to_string()
        }
    };

    let age = format_age(clip.updated_at);
    let tags = if clip.tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", clip.tags.join(", "))
    };

    println!("{:>4} {}{} {:>6}  {}{}", clip.id, type_icon, pin, age, preview, tags);
}

fn print_clip_detail(clip: &cb::storage::models::Clip) {
    println!("ID:      {}", clip.id);
    println!("Type:    {}", clip.content_type.as_str());
    println!("Pinned:  {}", clip.pinned);
    println!("Created: {}", clip.created_at.format("%Y-%m-%d %H:%M:%S"));
    println!("Updated: {}", clip.updated_at.format("%Y-%m-%d %H:%M:%S"));
    println!("Hash:    {}", &clip.hash[..16]);
    println!("Size:    {}", format_bytes(clip.size_bytes));

    if !clip.tags.is_empty() {
        println!("Tags:    {}", clip.tags.join(", "));
    }

    match clip.content_type {
        ContentType::Text => {
            println!("─────────────────────────");
            println!("{}", clip.text_content.as_deref().unwrap_or(""));
        }
        ContentType::Image => {
            println!("Path:    {}", clip.image_path.as_deref().unwrap_or("unknown"));
            println!("Size:    {}x{}",
                clip.image_width.unwrap_or(0),
                clip.image_height.unwrap_or(0));
        }
        ContentType::FileRef => {
            println!("Path:    {}", clip.text_content.as_deref().unwrap_or("unknown"));
        }
    }
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
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
