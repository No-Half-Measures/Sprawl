# Sprawl

A Windows disk-space visualizer built with Tauri 2, Rust and React.
Choose a drive or folder, then explore its largest contents in a canvas treemap.
Delete unwanted items after confirming in the app: choose the Recycle Bin or
confirm a second time to delete permanently. If Windows cannot recycle an item,
it asks before permanently deleting it. Drive roots, essential Windows folders
and the scan root are protected. Everything else is read-only.
See [DESIGN.md](DESIGN.md) for the product specification.

## Run

Run `npm install` once, then `npm run tauri dev` to start the desktop app.
`npm run build` builds the frontend and `npm run tauri build` packages the app.
A plain browser preview can't scan, because only the Tauri backend reads the disk.

The header's settings (cog) menu offers **Always run as administrator** and can
add **Scan with Sprawl** to Explorer's right-click menu for folders,
folder backgrounds and drives (under **Show more options** on Windows 11).
Picking it scans that path in the open window, or starts Sprawl if it isn't running.

## Checks

```text
npm run build
npm test
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
cargo build
cargo fmt --check
cargo test smoke_real_directory -- --ignored --nocapture
```

The manual smoke test scans `C:\Windows\System32`; set `SPRAWL_SMOKE_PATH`
to use another existing directory. Tests create temporary fixtures. The link test
tries a symlink, then a Windows junction, and reports a skip if neither is allowed.

`cargo test --release mft_matches_directory_scan -- --ignored --nocapture`, run
from an elevated terminal, scans `C:\` (or `SPRAWL_MFT_DRIVE`) through both
the MFT and the directory walk and prints timings and differences.

## Implementation notes

- Rust scans on a background thread with one Rayon task per directory. When the
  app runs as administrator, whole NTFS drive scans read the master file table
  directly instead, falling back to the directory walk on any failure. Home can
  relaunch the app elevated through UAC; the header's settings menu can enable
  elevation on every start. An atomic flag cancels scans and a worker mutex
  serializes replacements. Completed trees are immutable arena snapshots. Each
  node stores only its own name, and paths are rebuilt from parent indices.
  Folder counts exclude the folder itself.
- NTFS drive rescans replay the USN journal from the saved checkpoint. If the
  journal history or the changed records can't be read, Sprawl runs a full scan.
- The scanner counts and skips every filesystem error below the selected root.
  Symlinks and junctions are excluded; cloud placeholders and dedup reparse
  points remain included. Sizes are logical lengths;
  sparse/compressed allocation and hard-link deduplication are not measured.
- `get_tree` caps depth at 8 and children per folder at 500, and returns about
  8,000 nodes at most. Each group of omitted siblings becomes one synthetic
  `N smaller items` node with `id: null`, so byte totals still add up.
  The UI asks for 3 levels and 100 children per folder.
- `get_largest_files` returns at most 1,000 files (the UI requests 50).
  `get_type_breakdown` returns all eight categories, including empty ones.
- `scan-progress` contains `{ scanId, files, folders, bytes, errors, currentPath }`;
  `scan-complete` contains `{ scanId, root, errors }`; `scan-cancelled` contains
  `{ scanId }`; `scan-error` contains `{ scanId, message }`.
  `start_scan` returns the scan ID, which lets the UI ignore late events from an
  older scan.
- Beyond the commands in DESIGN.md, `get_node_path` backs Copy path and
  `open_folder` opens a scanned directory. Files can only be revealed, never run.
- `delete_item(nodeId, permanent)` returns `{ root, recycled, deleted }`, with
  separate byte totals for each mode in the current snapshot.
- [src/categories.json](src/categories.json) owns category IDs, labels, colors,
  and extensions. Rust embeds the same file that React imports.
- Backspace / Alt+Left goes back, Alt+Up opens the parent, and Escape clears
  highlights and menus. Tab navigates controls; Enter opens a folder row;
  Shift+F10 opens its context menu. Copy path uses the WebView clipboard API.
  Delete on a Contents, Largest files or Grown since row opens the same dialog as
  **Delete…** in the context menu. It shows the item's name, path, size and folder
  counts, with Cancel initially focused. Choose **Move to Recycle Bin** or
  **Delete permanently**; permanent deletion needs a second confirmation, with
  Back initially focused. Tab stays inside either step; Escape cancels unless running.

Deleting updates the map, panels and navigation without rescanning. Detached node
IDs stay stable in memory; saved trees omit deleted subtrees. The updated snapshot
keeps its scan date and previous comparison, and records recycled and permanently
deleted bytes separately. Recycling allows restoration; permanent deletion frees
space immediately and can't be undone. A partial failure leaves the snapshot
unchanged and asks you to rescan. The test that calls the real Windows shell for
both modes is `#[ignore]`d, because it deletes temporary files and puts items in
the Recycle Bin.

The tree is a snapshot: files can change or disappear after scanning. Rescan to
refresh it. Aggregates have no filesystem path, so you can't open or reveal them.
Zero-byte items remain available in Contents but occupy no treemap area.

Sprawl saves every completed scan under `scans/` in the app's local data
directory. Home can reopen the last scan of a drive or a recent folder without
rescanning. Each path keeps its latest snapshot plus one previous snapshot, for
up to ten paths. Previous snapshots use the same format under `scans/previous/`.
**Clear saved scans** removes this cache. Saved results show their date, and warn
once they are seven days old or drive free space has moved by at least 1 GiB or
1% of capacity. **Rescan** refreshes the snapshot.

When an earlier scan of the same path exists, results compare against it. Folder
totals and Contents show size changes, new items and removed direct children.
Switch **Largest files** to **Grown since** for the 50 files with the most growth.
`get_changes(nodeId)` computes the comparison lazily and caches it for the current
tree, separately from the map and other panels.

At a drive root, **Not found by scan** explains used space beyond the scanned
files when the gap reaches the larger of 1 GiB and 1% of capacity. Saved scans use
their recorded drive capacity. Unreadable items, system files, restore points and
file-system overhead can contribute; administrator access can include more.
Negative gaps from hard links or logical compressed-file sizes are not shown.
Recycled and permanently deleted bytes remain accounted for in this calculation
against the scan's drive usage. Only recycled bytes appear beside the scanned size;
that space is freed when you empty the Recycle Bin.
