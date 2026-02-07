use std::process;

use chrono::{Duration, Utc};
use clap::{Parser, Subcommand};
use rusqlite::Connection;

use cb::clipboard::write_text_to_clipboard;
use cb::config::AppPaths;
use cb::daemon;
use cb::storage::models::{ClipFilter, ContentType};
use cb::storage::sqlite::SqliteStorage;
use cb::storage::ClipStorage;

#[derive(Parser)]
#[command(name = "cb", version, about = "A clipboard manager for macOS")]
struct Cli {
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

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("error: {}", e);
        process::exit(1);
    }
}

fn run(cli: Cli) -> cb::errors::Result<()> {
    let paths = AppPaths::new();

    match cli.command {
        None => cmd_list(
            &paths,
            ClipFilter {
                limit: 10,
                ..Default::default()
            },
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
            )
        }
        Some(Commands::Search { query, limit }) => cmd_search(&paths, &query, limit),
        Some(Commands::Get { id }) => cmd_get(&paths, id),
        Some(Commands::Copy { id }) => cmd_copy(&paths, id),
        Some(Commands::Delete { id }) => cmd_delete(&paths, id),
        Some(Commands::Pin { id, unpin }) => cmd_pin(&paths, id, !unpin),
        Some(Commands::Tag { id, tag, remove }) => cmd_tag(&paths, id, &tag, remove),
        Some(Commands::Clear { days }) => cmd_clear(&paths, days),
        Some(Commands::Stats) => cmd_stats(&paths),
        Some(Commands::Daemon { action }) => cmd_daemon(&paths, action),
    }
}

fn open_storage(paths: &AppPaths) -> cb::errors::Result<SqliteStorage> {
    std::fs::create_dir_all(&paths.base_dir)
        .map_err(|e| cb::errors::CbError::Storage(rusqlite::Error::ToSqlConversionFailure(e.into())))?;
    let conn = Connection::open(&paths.db_path)?;
    SqliteStorage::new(conn)
}

fn cmd_list(paths: &AppPaths, filter: ClipFilter) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let clips = storage.list(filter)?;

    if clips.is_empty() {
        println!("No clips found.");
        return Ok(());
    }

    for clip in &clips {
        print_clip_row(clip);
    }
    Ok(())
}

fn cmd_search(paths: &AppPaths, query: &str, limit: i64) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let clips = storage.search(query, limit)?;

    if clips.is_empty() {
        println!("No results for \"{}\".", query);
        return Ok(());
    }

    for clip in &clips {
        print_clip_row(clip);
    }
    Ok(())
}

fn cmd_get(paths: &AppPaths, id: i64) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let clip = storage.get_by_id(id)?;
    print_clip_detail(&clip);
    Ok(())
}

fn cmd_copy(paths: &AppPaths, id: i64) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let clip = storage.get_by_id(id)?;

    match clip.content_type {
        ContentType::Text => {
            if let Some(ref text) = clip.text_content {
                write_text_to_clipboard(text)?;
                println!("Copied clip #{} to clipboard.", id);
            }
        }
        ContentType::Image => {
            println!("Image clips cannot be copied back yet. Path: {}",
                clip.image_path.as_deref().unwrap_or("unknown"));
        }
        ContentType::FileRef => {
            println!("File reference: {}",
                clip.text_content.as_deref().unwrap_or("unknown"));
        }
    }

    storage.touch(id)?;
    Ok(())
}

fn cmd_delete(paths: &AppPaths, id: i64) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    if storage.delete(id)? {
        println!("Deleted clip #{}.", id);
    } else {
        println!("Clip #{} not found.", id);
    }
    Ok(())
}

fn cmd_pin(paths: &AppPaths, id: i64, pinned: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    storage.set_pinned(id, pinned)?;
    if pinned {
        println!("Pinned clip #{}.", id);
    } else {
        println!("Unpinned clip #{}.", id);
    }
    Ok(())
}

fn cmd_tag(paths: &AppPaths, id: i64, tag: &str, remove: bool) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    if remove {
        storage.remove_tag(id, tag)?;
        println!("Removed tag \"{}\" from clip #{}.", tag, id);
    } else {
        storage.add_tag(id, tag)?;
        println!("Added tag \"{}\" to clip #{}.", tag, id);
    }
    Ok(())
}

fn cmd_clear(paths: &AppPaths, days: i64) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let cutoff = Utc::now() - Duration::days(days);
    let removed = storage.clear_older_than(cutoff)?;
    println!("Removed {} clip(s) older than {} days.", removed, days);
    Ok(())
}

fn cmd_stats(paths: &AppPaths) -> cb::errors::Result<()> {
    let storage = open_storage(paths)?;
    let stats = storage.stats()?;

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

fn cmd_daemon(paths: &AppPaths, action: DaemonAction) -> cb::errors::Result<()> {
    match action {
        DaemonAction::Start => {
            if let Ok(Some(pid)) = daemon::daemon_status(paths) {
                println!("Daemon already running (pid {}).", pid);
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

            println!("Started clipboard watcher (pid {}).", child.id());
            Ok(())
        }
        DaemonAction::Stop => {
            if daemon::stop_daemon(paths)? {
                println!("Stopped clipboard watcher.");
            } else {
                println!("Daemon is not running.");
            }
            Ok(())
        }
        DaemonAction::Status => {
            match daemon::daemon_status(paths)? {
                Some(pid) => println!("Daemon running (pid {}).", pid),
                None => println!("Daemon is not running."),
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
