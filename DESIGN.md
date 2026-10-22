# Sprawl — Design

Sprawl shows where disk space goes. You pick a drive or folder, it scans, and you see what's using the space at a glance. The main view is a treemap, with a sorted list beside it for exact numbers.

## Goals

- Answer "what is filling my drive?" within seconds of a scan finishing.
- Make sizes easy to compare: the area of a rectangle matches the bytes it uses, and the list gives exact numbers and percentages.
- Stay responsive on a full system drive (1M+ files).
- Read-only except for deleting selected items after in-app confirmation. Users choose the Windows Recycle Bin or permanent deletion, which requires a second in-app confirmation. Windows must ask before permanently deleting anything that cannot be recycled in Recycle Bin mode.

## Stack

- Tauri 2 desktop app (Windows first).
- Backend: Rust. Parallel scanning uses `rayon`, and drive info comes from `sysinfo`.
- Frontend: React 19 + TypeScript + Vite. The treemap layout comes from `d3-hierarchy` (squarified) and is drawn on a `<canvas>`. Styling is plain CSS with custom properties; there is no UI kit.
- Plugins: `opener` (reveal in Explorer), `dialog` (folder picker), `single-instance` (forward launches to the existing window).

## Backend

### Scanner
- Walks the tree in parallel and does not follow symlinks or junctions (reparse points), so there are no loops and nothing is counted twice.
- When the app runs as administrator, scanning a whole NTFS drive reads the master file table directly, which is several times faster. Folder scans, other filesystems, and any MFT read failure use the directory walk.
- Drive rescans load the latest saved tree and replay the existing NTFS USN journal, fetching changed MFT records and resident attribute-list extensions before using the same tree builder. Full MFT scans capture the journal checkpoint before reading; replay is idempotent. Missing or reset/truncated history, unsupported USN versions, more than 200,000 changed records, non-resident attribute lists, unknown reachable directories without a create event, incomplete prior scans, and any read or parsing uncertainty fall back to a full MFT scan, then the directory walk. The journal is never created or modified.
- Files that can't be read (access denied and similar) are skipped and counted in `errors`. They never cause the scan to fail.
- Each node stores its logical size (`len`), file count, and folder count. Folder totals are rolled up after the walk.
- Progress goes out as a `scan-progress` event about every 100 ms: `{ files, folders, bytes, errors, currentPath }`. When the scan ends, a `scan-complete` event carries the root summary, or `scan-cancelled` if it was stopped.
- The scan can be cancelled through a shared atomic flag. Only one scan runs at a time, and starting a new one replaces the previous result.

### In-memory tree
- Nodes live in an arena (`Vec<Node>`) and point to each other by index. Each node stores only its own name, and full paths are rebuilt by following parent links. Children are sorted by size, largest first.
- Deleting detaches a subtree, updates ancestor totals and ordering, and preserves arena IDs. Detached nodes and descendants report "This item was deleted." Updates clone the immutable tree outside the session lock and swap only into the same session tree; replacing the tree resets panel caches. Deletes are serialized and shell operations run on a blocking thread with the main window as owner.

### Commands (IPC)
| Command | Returns |
| --- | --- |
| `list_drives()` | `[{ mount, label, total, free }]` |
| `list_saved_scans()` | Saved scan metadata, newest first |
| `load_saved_scan(path, scannedAt)` | Opens a saved tree and returns its root, skipped count, and metadata |
| `clear_saved_scans()` | Removes all saved scans |
| `start_scan(path)` | starts a background scan, returns immediately |
| `cancel_scan()` | — |
| `get_tree(nodeId?, depth, maxChildren)` | Subtree rooted at the node (the root if omitted), trimmed to `depth` levels. Each folder includes at most `maxChildren` children, and anything beyond that is merged into one synthetic `"N smaller items"` entry, so treemap totals still add up. |
| `get_nodes(ids)` | Shallow node views in request order, with `null` for invalid or detached IDs |
| `delete_item(nodeId, permanent)` | `{ root, recycled, deleted }`: updated shallow root and separate byte totals for Recycle Bin and permanent deletion for this snapshot |
| `get_largest_files(nodeId, limit)` | Largest files anywhere under the node, with relative paths |
| `get_type_breakdown(nodeId)` | Bytes and file count for each category under the node |
| `get_changes(nodeId)` | `null` without an older snapshot, otherwise `{ since, previousSize, children: [{ id, previousSize }], removed: { items, bytes }, grown: [{ id, name, relativePath, size, previousSize }] }`. Times are milliseconds; sizes are bytes; `previousSize: null` means new. All real direct children are included; grown files are the top 50 positive size increases under the node, descending, with new files counting fully. |
| `reveal_in_explorer(nodeId)` | Opens Explorer with the item selected |

The IPC node shape is `{ id, name, kind: "dir" | "file" | "other", size, files, folders, category?, children? }`.

Both delete modes use `SHFileOperationW` with `FO_DELETE`, with COM initialized apartment-threaded on the blocking thread. Recycling uses `FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING | FOF_NOERRORUI`; permanent deletion uses only `FOF_NOCONFIRMATION | FOF_NOERRORUI`, without undo or a Windows nuke warning. Absolute, double-NUL terminated paths have no extended prefix; paths of 260 UTF-16 units or more are refused. Synthetic IDs, scan roots, detached nodes, changed kinds, symlinks and junctions are refused. Link checks also cover ancestors and reuse the scanner classification, allowing cloud placeholders and dedup reparse points. Drive roots, SystemRoot, ProgramFiles, ProgramFiles(x86), ProgramData, USERPROFILE, its parent and their ancestors are protected; their contents are allowed. A missing path after the operation counts as success even after an error or abort. Cancellation leaves the tree unchanged. Partial folder deletion reports an error and requires a rescan.

### File categories
Files are grouped by extension into Video, Images, Audio, Documents, Archives, Code, Apps & system (exe/dll/sys/msi…), and Other. The mapping lives in one place, and the frontend uses the same category list for colors.

### Saved scans
- Completed scans are saved in the app's local data directory under `scans/`, using streaming bincode + zstd and a JSON metadata sidecar written last. Cancelled and failed scans are never saved.
- Deleting resaves in the background under the cache mutex with the same `scannedAt`, so the previous snapshot is preserved. Only root-reachable nodes are written, compacted in original arena order with remapped indices and aligned MFT record numbers. Metadata includes separate `recycled` and `deleted` byte totals, both defaulting to zero for older JSON. Older background saves cannot overwrite either newer deletion total for the same timestamp, including zero-byte items.
- Tree format v3 is `DISKMAP\0` + little-endian u32 version + zstd (level 3, checksum). The bincode payload contains node count, root path and errors, then an aligned `Vec<u64>` of MFT record numbers (root = 5, empty for directory walks) and an optional journal checkpoint `{ journal_id: u64, next_usn: i64 }`, followed by node records (name, parent, ordered children, kind, size, files, folders, optional u8 category index into the shared category list). Deleting preserves arena IDs and record alignment; saving compacts both together. Loading decompresses before decoding and validates structure, categories, record alignment and checkpoint bounds. Version 2 uses the same header and node records without MFT metadata; it still loads, with incremental rescanning unavailable until a full MFT scan is saved. Other versions are discarded.
- The session retains snapshot metadata and reopens matching path keys and timestamps without disk access. Root largest-file and category panels are cached for that tree; replacing the snapshot clears them and releases the old tree in the background.
- Each path keeps its latest snapshot and one previous snapshot under `scans/previous/`, with the same `{key}.tree` and `{key}.json` format. Saving a newer scan moves the main pair to replace the previous pair before publishing the new snapshot. The ten newest paths are retained; eviction removes both snapshots, and previous entries without a main entry are removed. Listing the main directory ignores the previous subdirectory. Paths match without case or trailing separators. Listing checks metadata and tree file headers and discards invalid entries and orphan files. Loading checks compressed stream integrity and validates the decoded tree against its metadata, deleting corrupt entries.
- Changes are computed lazily in a blocking worker, without holding the session lock, and cached alongside the panels for that immutable tree. Under the cache mutex, the baseline is the main snapshot if older than the session's snapshot (before its background save finishes), otherwise the previous snapshot if older. Matching walks both trees by exact child name and kind; kind swaps are removed plus new. The cache retains previous sizes per current node and removed direct-child counts/bytes per directory, then releases the previous tree. Replacing the tree replaces its comparison cache, so late computations cannot populate a different tree's cache. Clearing saved scans also clears the session panel cache.
- Home shows each drive's last scan and recent folders. Saved results show a snapshot banner with Rescan, warning after seven days or when drive free space changes by at least 1 GiB or 1% of drive capacity, whichever is larger.

## UI

A calm, clean, dark-first look that also respects the system light theme. There is one accent color, and the category colors do the visual work. Labels are short, numbers are tabular, and sizes are formatted in binary units shown as KB/MB/GB/TB.

### 1. Home
- Each drive gets a card with its letter and label, a used/free bar, the text "412 GB used of 1 TB", and a **Scan** button.
- A **Scan a folder…** button opens the folder picker.
- The header's settings (cog) menu is available on every screen on Windows. **Add "Scan with Sprawl" to Explorer's right-click menu** registers folders, backgrounds and drives per user, without admin access. The checkbox reads the registry and repairs stale executable paths when enabled. Explorer launches scan in the existing window, ignoring requests during deletion; Windows 11 shows the item under **Show more options**.
- On Windows, an admin row offers **Restart as administrator** (UAC relaunch) when not elevated. **Always run as administrator** lives in the header's settings (cog) menu. The preference lives in `settings.json` in the app's local data directory; when set, startup relaunches elevated and continues unelevated if UAC is declined.

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
- **Treemap:** shows the current folder about 3 levels deep. Files are filled with their category color, and folders get a subtle frame with a name strip when there's room. Each rectangle's label shows the name and size when it fits. Hovering highlights a rectangle and shows a tooltip with name, size, % of current folder, and item counts. Clicking a folder zooms into it, and right-clicking opens a menu with Open folder / Reveal in Explorer / Copy path / Delete… (except the scan root). Rectangles smaller than about 3 px aren't drawn individually. The canvas is DPI-aware and redraws when resized.
- **Contents list:** lists the direct children with a size bar, size, %, and item count. Hovering a row highlights the matching rectangle and the reverse also works. Clicking a folder row zooms into it.
- **Delete…:** available in real-item context menus except the scan root, and through Delete on Contents, Largest files and Grown since rows. The alert dialog asks "Delete <name>?" and shows the name, full path, size and folder counts. Cancel is initially focused, followed by an outlined danger **Delete permanently** button and a primary **Move to Recycle Bin** button. The note says "Moving to the Recycle Bin lets you restore it later. Deleting permanently frees the space now and can't be undone." Choosing permanent deletion opens a second step titled "Delete permanently?", with "<size> will be deleted permanently. This can't be undone.", Back initially focused and a danger **Delete permanently** button. Back returns to the first step. Tab stays inside either step, Escape cancels the whole dialog unless running, and Enter activates the focused button. While running, all buttons are disabled and the active button shows Moving… or Deleting…. Success shows "Moved <name> to the Recycle Bin." or "Deleted <name> permanently." and refreshes all panels, trail and history sizes; detached history is dropped and navigation returns to the deepest attached ancestor. "Delete cancelled." is a notice; other errors use the error banner and close the dialog. Hover, menu and category filter state are cleared after success.
- **Changes:** when an older scan exists, the header shows the signed size change and baseline date, or New/No change. Contents shows New or signed deltas of at least 1 MiB beside sizes, excluding synthetic aggregates, and a non-interactive removed-items row for direct children. The comparison request is separate from map and panel requests, and stale navigation responses are ignored.
- **Largest files:** the top 50 files under the current folder.
- **Grown since:** a second tab, available with a comparison, ranks up to 50 files by positive growth, showing the increase and previous size or New. Rows reveal files and offer the same context menu as Largest files. Tab choice persists across folder navigation.
- **Not found by scan:** at the root of a drive scan, compare used space (`total - free`) with scanned logical bytes plus recycled and permanently deleted bytes for this snapshot, preferring snapshot drive totals/free space for saved results. Show a header value and non-interactive Contents row only when the gap is at least the larger of 1 GiB and 1% of total capacity. Recycled bytes appear beside the scanned size with a tooltip explaining when space is freed. Its neutral hatched meter shows its share of used space. The explanation includes skipped items when present, system files and file-system overhead, and an administrator hint when unelevated. Negative gaps (for example hard links counted by name or compressed files at logical size) are hidden.
- **Type breakdown bar:** a stacked bar plus a legend for the current folder. It also explains the treemap's colors. Hovering a category dims every other category in the treemap.
- **Keyboard:** Backspace/Alt+← goes back, Alt+↑ goes up, Delete opens the delete dialog on an item row, and Esc clears the hover.
- **Empty and error states:** an empty folder, a scan with skipped items ("312 items couldn't be read — run as administrator to include them"), and no drives found.

## Testing
- Rust: unit tests run the scanner against a temp directory tree to check size rollup, counts, sorting, and `get_tree` trimming and aggregation. They also check that a symlink or junction isn't followed where one can be created.
- Frontend: Vitest covers size formatting, category mapping, and treemap layout (areas proportional, nothing overlapping, small items culled).

## Out of scope (v1)
- Moving files to other folders, duplicate detection, exporting scans.
