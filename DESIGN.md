# DiskMapper — Design

DiskMapper shows where disk space goes. You pick a drive or folder, it scans, and you see what's using the space at a glance. The main view is a treemap, with a sorted list beside it for exact numbers.

## Goals

- Answer "what is filling my drive?" within seconds of a scan finishing.
- Make sizes easy to compare: the area of a rectangle matches the bytes it uses, and the list gives exact numbers and percentages.
- Stay responsive on a full system drive (1M+ files).
- Read-only. v1 never deletes or modifies files.

## Stack

- Tauri 2 desktop app (Windows first).
- Backend: Rust. Parallel scanning uses `rayon`, and drive info comes from `sysinfo`.
- Frontend: React 19 + TypeScript + Vite. The treemap layout comes from `d3-hierarchy` (squarified) and is drawn on a `<canvas>`. Styling is plain CSS with custom properties; there is no UI kit.
- Plugins: `opener` (reveal in Explorer), `dialog` (folder picker).

## Backend

### Scanner
- Walks the tree in parallel and does not follow symlinks or junctions (reparse points), so there are no loops and nothing is counted twice.
- Files that can't be read (access denied and similar) are skipped and counted in `errors`. They never cause the scan to fail.
- Each node stores its logical size (`len`), file count, and folder count. Folder totals are rolled up after the walk.
- Progress goes out as a `scan-progress` event about every 100 ms: `{ files, folders, bytes, errors, currentPath }`. When the scan ends, a `scan-complete` event carries the root summary, or `scan-cancelled` if it was stopped.
- The scan can be cancelled through a shared atomic flag. Only one scan runs at a time, and starting a new one replaces the previous result.

### In-memory tree
- Nodes live in an arena (`Vec<Node>`) and point to each other by index. Each node stores only its own name, and full paths are rebuilt by following parent links. Children are sorted by size, largest first.

### Commands (IPC)
| Command | Returns |
| --- | --- |
| `list_drives()` | `[{ mount, label, total, free }]` |
| `list_saved_scans()` | Saved scan metadata, newest first |
| `load_saved_scan(path)` | Opens a saved tree and returns its root, skipped count, and metadata |
| `clear_saved_scans()` | Removes all saved scans |
| `start_scan(path)` | starts a background scan, returns immediately |
| `cancel_scan()` | — |
| `get_tree(nodeId?, depth, maxChildren)` | Subtree rooted at the node (the root if omitted), trimmed to `depth` levels. Each folder includes at most `maxChildren` children, and anything beyond that is merged into one synthetic `"N smaller items"` entry, so treemap totals still add up. |
| `get_largest_files(nodeId, limit)` | Largest files anywhere under the node, with relative paths |
| `get_type_breakdown(nodeId)` | Bytes and file count for each category under the node |
| `reveal_in_explorer(nodeId)` | Opens Explorer with the item selected |

The IPC node shape is `{ id, name, kind: "dir" | "file" | "other", size, files, folders, category?, children? }`.

### File categories
Files are grouped by extension into Video, Images, Audio, Documents, Archives, Code, Apps & system (exe/dll/sys/msi…), and Other. The mapping lives in one place, and the frontend uses the same category list for colors.

### Saved scans
- Completed scans are saved in the app's local data directory under `scans/`, using streaming bincode + zstd and a JSON metadata sidecar written last. Cancelled and failed scans are never saved.
- Each path keeps one snapshot; the ten newest snapshots are retained. Paths match without case or trailing separators. Listing checks metadata and tree file headers and discards invalid entries and orphan files. Loading checks compressed stream integrity and validates the decoded tree, deleting corrupt entries.
- Home shows each drive's last scan and recent folders. Saved results show a snapshot banner with Rescan, warning after seven days or when drive free space changes by at least 1 GiB or 1% of drive capacity, whichever is larger.

## UI

A calm, clean, dark-first look that also respects the system light theme. There is one accent color, and the category colors do the visual work. Labels are short, numbers are tabular, and sizes are formatted in binary units shown as KB/MB/GB/TB.

### 1. Home
- Each drive gets a card with its letter and label, a used/free bar, the text "412 GB used of 1 TB", and a **Scan** button.
- A **Scan a folder…** button opens the folder picker.

### 2. Scanning
- While scanning, the screen shows big live counters for files found and size found, a thin current-path line that is truncated in the middle, an indeterminate progress bar (the total isn't known ahead of time), and a **Cancel** button.

### 3. Results (main screen)
```
┌──────────────────────────────────────────────────────────────────┐
│ ◀ ▲  C: › Users › andy › Videos              [Rescan] [Home]     │
│ Videos — 182 GB · 4,210 files · 18% of drive                    │
├───────────────────────────────────────────┬──────────────────────┤
│                                           │ Contents (sorted)    │
│              TREEMAP (canvas)             │ ▇▇▇▇▇▇▇ Movies 120GB │
│                                           │ ▇▇▇     Clips   40GB │
│                                           │ ▇       ...          │
│                                           ├──────────────────────┤
│                                           │ Largest files        │
├───────────────────────────────────────────┴──────────────────────┤
│ ■ Video 64%  ■ Images 12%  ■ Apps 9% ...  (stacked bar + legend) │
└──────────────────────────────────────────────────────────────────┘
```
- **Header:** back and up buttons, then a clickable breadcrumb and a summary line with size, file count, and share of the parent or drive.
- **Treemap:** shows the current folder about 3 levels deep. Files are filled with their category color, and folders get a subtle frame with a name strip when there's room. Each rectangle's label shows the name and size when it fits. Hovering highlights a rectangle and shows a tooltip with name, size, % of current folder, and item counts. Clicking a folder zooms into it, and right-clicking opens a menu with Open folder / Reveal in Explorer / Copy path. Rectangles smaller than about 3 px aren't drawn individually. The canvas is DPI-aware and redraws when resized.
- **Contents list:** lists the direct children with a size bar, size, %, and item count. Hovering a row highlights the matching rectangle and the reverse also works. Clicking a folder row zooms into it.
- **Largest files:** the top 50 files under the current folder.
- **Type breakdown bar:** a stacked bar plus a legend for the current folder. It also explains the treemap's colors. Hovering a category dims every other category in the treemap.
- **Keyboard:** Backspace/Alt+← goes back, Alt+↑ goes up, and Esc clears the hover.
- **Empty and error states:** an empty folder, a scan with skipped items ("312 items couldn't be read — run as administrator to include them"), and no drives found.

## Testing
- Rust: unit tests run the scanner against a temp directory tree to check size rollup, counts, sorting, and `get_tree` trimming and aggregation. They also check that a symlink or junction isn't followed where one can be created.
- Frontend: Vitest covers size formatting, category mapping, and treemap layout (areas proportional, nothing overlapping, small items culled).

## Out of scope (v1)
- Deleting or moving files, NTFS MFT fast-scan, duplicate detection, exporting scans.
