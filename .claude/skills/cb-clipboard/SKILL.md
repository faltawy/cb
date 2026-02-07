---
name: cb-clipboard
description: Reference for using the cb clipboard manager CLI. Triggers on clipboard history, clip management, clipboard search, recent copies.
---

# cb — Clipboard Manager CLI

`cb` is a local clipboard manager daemon + CLI for macOS. It watches the system clipboard and stores history in SQLite.

## Setup

Ensure the daemon is running before querying:

```bash
cb daemon start
```

## Key Rule

**Always use `--json` for programmatic access.** Without it, output is human-formatted text that is fragile to parse.

```bash
cb --json list          # correct
cb list --json          # also correct (global flag)
cb list                 # wrong for programmatic use — human-formatted
```

## Reading Commands

### List recent clips

```bash
cb --json list [--limit N] [--offset N] [--type text|image|fileref] [--pinned] [--tag TAG]
```

Returns a JSON array of clip objects. Empty result is `[]`.

```json
[
  {
    "id": 42,
    "content_type": "text",
    "text_content": "copied text here",
    "image_path": null,
    "image_width": null,
    "image_height": null,
    "hash": "a1b2c3...",
    "size_bytes": 18,
    "pinned": false,
    "created_at": "2024-03-15T10:30:00Z",
    "updated_at": "2024-03-15T10:30:00Z",
    "tags": ["important"]
  }
]
```

### Search clips

```bash
cb --json search "QUERY" [--limit N]
```

Returns a JSON array (same shape as list). Empty result is `[]`.

### Get a single clip by ID

```bash
cb --json get ID
```

Returns a single clip JSON object (same shape as array element above).

## Action Commands

All action commands return:

```json
{"success": true, "message": "Copied clip #42 to clipboard."}
```

### Copy clip to system clipboard

```bash
cb --json copy ID
```

### Delete a clip

```bash
cb --json delete ID
```

Returns `"success": false` if clip not found.

### Pin/unpin a clip

```bash
cb --json pin ID          # pin
cb --json pin ID --unpin  # unpin
```

### Tag management

```bash
cb --json tag ID "tag-name"            # add tag
cb --json tag ID "tag-name" --remove   # remove tag
```

### Clear old entries

```bash
cb --json clear [--days N]   # default: 30 days
```

Returns an extra `"removed"` field:

```json
{"success": true, "message": "Removed 5 clip(s) older than 30 days.", "removed": 5}
```

## Stats

```bash
cb --json stats
```

```json
{
  "total_clips": 150,
  "text_clips": 120,
  "image_clips": 25,
  "fileref_clips": 5,
  "total_size": 524288,
  "oldest": "2024-01-01T00:00:00Z",
  "newest": "2024-03-15T10:30:00Z",
  "daemon_running": true,
  "daemon_pid": 12345
}
```

## Daemon Management

```bash
cb --json daemon start    # {"success": true, "message": "Started clipboard watcher (pid 12345)."}
cb --json daemon stop     # {"success": true, "message": "Stopped clipboard watcher."}
cb --json daemon status   # {"running": true, "pid": 12345}
```

## Error Handling

On error, `cb` exits with code 1 and prints JSON to **stderr**:

```json
{"error": "Clip not found"}
```

Always check exit code before parsing stdout.

## Common jq Patterns

```bash
# Get text of most recent clip
cb --json list --limit 1 | jq -r '.[0].text_content'

# List IDs of all pinned clips
cb --json list --pinned --limit 100 | jq '.[].id'

# Get clips tagged "work"
cb --json list --tag work | jq '.[].text_content'

# Count total clips
cb --json stats | jq '.total_clips'

# Check if daemon is running
cb --json daemon status | jq '.running'
```
