# DiskMapper

A read-only Windows disk-space visualizer built with Tauri 2, Rust and React.
Choose a drive or folder, then explore its largest contents in a canvas treemap.
See [DESIGN.md](DESIGN.md) for the product specification.

## Run

Dependencies are already installed. Start the desktop app with `npm run tauri dev`.
`npm run build` builds the frontend; `npm run tauri build` packages the app.
A standalone browser preview cannot scan: disk access is provided by the Tauri backend.

## Checks

```text
npm run build
npm test
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
cargo build
cargo test smoke_real_directory -- --ignored --nocapture
```

The manual smoke test scans `C:\Windows\System32`; set `DISKMAPPER_SMOKE_PATH`
to use another existing directory. Tests create temporary fixtures. The link test
tries a symlink, then a Windows junction, and reports a skip if neither is allowed.

## Implementation notes

- Rust scans directory batches with Rayon on a background thread. An atomic flag
  cancels scans; a worker mutex serializes replacements. Completed trees are
  immutable arena snapshots. Files store their own names, with parent indices
  used to reconstruct paths. Folder counts exclude the folder itself.
- All filesystem errors below the selected root are counted and skipped.
  Symlinks and junctions are excluded; cloud placeholders and dedup reparse
  points remain included. Sizes are logical lengths;
  sparse/compressed allocation and hard-link deduplication are not measured.
- `get_tree` bounds depth to 8 and individual children to 500, with a response
  budget of approximately 8,000 nodes. Every omitted sibling group becomes a
  synthetic `N smaller items` node with `id: null`, preserving byte totals.
  The UI asks for 3 levels and 100 children per folder.
- `get_largest_files` returns at most 1,000 files (the UI requests 50).
  `get_type_breakdown` returns all eight categories, including empty ones.
- `scan-progress` contains `{ scanId, files, folders, bytes, errors, currentPath }`;
  `scan-complete` contains `{ scanId, root, errors }`; `scan-cancelled` contains
  `{ scanId }`; `scan-error` contains `{ scanId, message }`.
  `start_scan` returns the scan ID. IDs prevent late events changing a newer scan.
- In addition to the commands in the spec, `get_node_path` supports Copy path and
  `open_folder` opens a scanned directory. Files can only be revealed, never run.
- [src/categories.json](src/categories.json) owns category IDs, labels, colors,
  and extensions. Rust embeds the same file that React imports.
- Backspace / Alt+Left goes back, Alt+Up opens the parent, and Escape clears
  highlights and menus. Tab navigates controls; Enter opens a folder row;
  Shift+F10 opens its context menu. Copy path uses the WebView clipboard API.

The tree is a snapshot: files can change or disappear after scanning. Rescan to
refresh it. Aggregates have no filesystem path, so cannot be opened or revealed.
Zero-byte items remain available in Contents but occupy no treemap area.

Completed scans are saved automatically in the app's local data directory under
`scans/`. Home can reopen the last scan of a drive or a recent folder without
rescanning. One snapshot per path is kept, up to ten paths; **Clear saved scans**
removes this cache. Saved results show their date and warn after seven days or a
significant change in drive free space. **Rescan** refreshes the snapshot.
