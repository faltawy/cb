# cb

A fast clipboard manager for macOS with history, search, tags, and a TUI.

## Features

- Automatic clipboard watching via background daemon
- Full-text search across clipboard history
- Pin important clips, tag and organize them
- Image clipboard support (PNG)
- Interactive TUI browser
- JSON output (`--json`) for scripting and AI agents
- SQLite-backed storage

## Installation

### Homebrew

```bash
brew tap faltawy/cb
brew install cb
```

### Cargo

```bash
cargo install cbhist
```

### Binary download

Grab the latest release from [GitHub Releases](https://github.com/faltawy/cb/releases) and extract it to your `$PATH`.

## Quick start

```bash
# Start the clipboard watcher
cb daemon start

# List recent clips
cb list

# Search history
cb search "some text"

# Copy a clip back to clipboard
cb copy 42

# Open interactive TUI
cb tui
```

## Usage

```
cb                        List recent clips (default: 10)
cb list [--limit N]       List clips with pagination
cb search <query>         Search clipboard history
cb get <id>               Show full clip details
cb copy <id>              Copy a clip back to clipboard
cb delete <id>            Delete a clip
cb pin <id>               Pin a clip (--unpin to remove)
cb tag <id> <tag>         Add a tag (--remove to delete)
cb clear [--days N]       Remove clips older than N days
cb stats                  Show storage statistics
cb tui                    Interactive TUI
cb daemon start|stop|status   Manage the watcher daemon
```

Add `--json` (or `-j`) to any command for structured JSON output:

```bash
cb --json list --limit 5
cb --json stats
cb --json get 42
```

## License

MIT
