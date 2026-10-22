<p align="center">
  <img src="public/sprawl.svg" width="96" alt="Sprawl logo">
</p>

<h1 align="center">Sprawl</h1>

<p align="center">
  A disk-space visualizer for Windows that scans a 2-million-file system drive
  in about 3.5 seconds.
</p>

Sprawl scans a drive or folder and draws it as a treemap. Each rectangle's area
matches the bytes it uses, so the biggest space hogs are the biggest blocks. A
sorted list beside the map gives exact sizes. As administrator, Sprawl reads the
NTFS master file table directly instead of opening every folder, which is where
the speed comes from.

Sprawl is built with [Tauri 2](https://tauri.app), Rust and React.

## Features

- **Fast scans.** As administrator, Sprawl reads whole NTFS drives straight
  from the master file table. Without admin, it walks folders and drives in
  parallel. Drive rescans try to replay the NTFS change journal and only fall
  back to a full read when they can't.
- **Treemap.** The map shows the current folder three levels deep, coloured by
  file type, and stays sharp on high-DPI screens. Hover a block for its size and
  item counts. Click a folder to zoom in. Right-click for actions.
- **Sorted contents, largest files and type breakdown.** Each folder lists its
  children by size and percentage, the 50 largest files under it, and how much
  space goes to Video, Images, Audio, Documents, Archives, Code, Apps & system
  and Other. Hover a category to highlight its files in the map.
- **Changes since last scan.** Sprawl saves each completed scan. When you scan
  the same path again, it shows how much each folder grew or shrank, which items
  are new, what disappeared, and the 50 files that grew the most.
- **Saved scans.** The home screen reopens the last scan of any drive or recent
  folder without rescanning. Saved results show their age and warn you once
  they look stale.
- **Missing space.** At a drive root, Sprawl reports how much used space the
  scan couldn't account for. System files, restore points and unreadable
  folders usually explain the gap.
- **Safe deletion.** You can send an item to the Recycle Bin, or delete it
  permanently after a second confirmation. Sprawl refuses to delete drive
  roots, the scan root, the Windows and Program Files folders, ProgramData and
  your user profile folder. It never opens or runs files. It can only reveal
  them in Explorer.
- **Explorer integration.** Add **Scan with Sprawl** to the right-click menu
  for folders, folder backgrounds and drives. This doesn't need admin.
- **Keyboard shortcuts.** Backspace or Alt+Left goes back, Alt+Up goes to the
  parent, Delete opens the delete dialog, Shift+F10 opens a context menu, and
  Escape clears highlights.
- **Light and dark themes.** Sprawl follows the Windows setting.

## Performance

I ran each tool three times on the same machine against the same drive, with
default settings. Lower is better.

Full drive scan of `C:\` as administrator. The drive holds about 2.08 million
files and 549,000 folders, 980 GB in total.

| Tool | Median | Runs |
| --- | ---: | --- |
| **Sprawl 0.1.0**, MFT read | **3.5 s** | 3.4, 3.5, 4.4 s |
| WizTree 4.14 | 10.5 s | 9.5, 10.5, 11.7 s |
| WinDirStat 2.8.0 | 13.3 s | 13.2, 13.3, 16.6 s |
| Sprawl 0.1.0, directory walk without MFT | 15.2 s | 6 runs, 11.0 to 28.6 s |

Folder scan of `C:\Windows`, about 199,000 files and 141,000 folders.

| Tool | Median | Runs |
| --- | ---: | --- |
| **Sprawl 0.1.0** | **2.7 s** | 2.6, 2.7, 2.7 s |
| WinDirStat 2.8.0 | 8.4 s | 8.3, 8.4, 8.5 s |
| WizTree 4.14 | 31.1 s | 31.1, 31.1, 34.7 s |

WizTree is fast on a whole drive but took three times longer on a single
folder than on all of `C:\`. If you mostly scan folders, that matters.

The test machine has an AMD Ryzen 9 7900X3D, 64 GB of RAM and a 1 TB Sabrent
NVMe SSD, running Windows 11 Home build 26200. Every tool ran elevated, one at a
time, after a warm-up scan.

### How these were measured

The three tools weren't timed the same way, so treat the tables as a rough
comparison.

- **Sprawl.** The time is what Sprawl's own timing tests print for the scan
  itself. App startup and drawing the first treemap aren't included.
- **WizTree.** The time is the wall clock for
  `WizTree64.exe "C:\" /export=out.csv /admin=1 /exportmaxdepth=1`. That
  includes process startup and writing a small CSV.
- **WinDirStat.** The time is the wall clock for
  `WinDirStat.exe /saveto out.csv C:\`. That includes process startup and
  writing every item to the CSV, which came to 518 MB for `C:\`. Some of its
  time goes to that file.

Disk, CPU, file count and the filesystem cache all move these numbers. On this
machine, the first directory walk after the other tools ran took 28.6 s. Later
walks took 11 to 19 s.

To reproduce the Sprawl numbers, run these from an elevated terminal in
`src-tauri`:

```text
cargo test --release mft_matches_directory_scan -- --ignored --nocapture
cargo test --release smoke_real_directory -- --ignored --nocapture
```

The first command scans `C:\` through the MFT and then through the directory
walk, and prints both times. Set `SPRAWL_MFT_DRIVE` to scan another drive. The
second command scans `C:\Windows\System32` by default. Set
`SPRAWL_SMOKE_PATH=C:\Windows` to match the folder benchmark.

### Why it's fast

- **Direct MFT reads.** For a whole NTFS drive, Sprawl reads the master file
  table with overlapping unbuffered reads and builds the tree from its records,
  without opening a single directory. If anything goes wrong, it falls back to
  the directory walk.
- **Parallel walk.** Folder scans and non-admin scans run one
  [Rayon](https://github.com/rayon-rs/rayon) task per directory.
- **Compact tree.** Nodes sit in one arena and store only their own name, not
  the full path. Sprawl rebuilds paths from parent links when it needs one.
- **Small IPC payloads.** The UI asks the backend for only what it draws, three
  levels and 100 children per folder. Sprawl merges the rest of each folder into
  one "N smaller items" entry, so totals still add up.
- **Fast reopen.** Sprawl stores saved scans as zstd-compressed bincode and
  reopens them without rescanning the disk.

## Getting started

Sprawl is a Windows app. There are no prebuilt releases yet, so you'll need to
build it from source.

You need:

- [Node.js](https://nodejs.org) 20.19+ or 22.12+
- stable [Rust](https://rustup.rs)
- the [Tauri prerequisites for Windows](https://tauri.app/start/prerequisites/),
  which are the Microsoft C++ Build Tools and WebView2

```text
npm install
npm run tauri dev      # run in development
npm run tauri build    # build an installer under src-tauri/target/release/bundle
```

`npm run dev` opens the UI in a plain browser, but it can't scan. Only the
Tauri backend reads the disk.

## Usage

1. Pick a drive on the home screen, or choose **Scan a folder…**.
2. Click folders in the treemap to zoom in. Use the breadcrumb or Backspace to
   go back.
3. Right-click an item to open it, reveal it in Explorer, copy its path or
   delete it.

**Admin mode.** Without admin rights, Sprawl skips folders it can't read and
uses the slower directory walk. Choose **Restart as administrator** on the home
screen, or turn on **Always run as administrator** in the settings menu behind
the cog icon.

**Explorer menu.** Turn on **Scan with Sprawl** in the settings menu to add it to
Explorer's right-click menu. On Windows 11 it sits under **Show more options**.
If Sprawl is already open, the scan runs in that window.

**Saved scans.** Sprawl keeps them in its local app data folder. Each path keeps
its latest and previous scan, for up to ten paths. **Clear saved scans** removes
them all.

## Limitations

- Windows only.
- Sprawl reports logical file sizes. It ignores compression, sparse files and
  hard links, so totals can differ from the space actually allocated on disk.
- Sprawl doesn't follow symlinks or junctions. It counts cloud placeholders,
  such as OneDrive files, at their logical size.
- Results are a snapshot. Rescan to pick up files that changed afterwards.

## Development

```text
npm run build
npm test
cd src-tauri
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Tests create temporary fixtures. The link test tries a symlink, then a junction,
and reports a skip if Windows allows neither. Tests that scan real drives or
call the Windows shell to delete files carry `#[ignore]`. Run them with
`--ignored`.

[DESIGN.md](DESIGN.md) has the full product specification, IPC commands and
saved-scan format. A few details live only here:

- `get_tree` caps depth at 8 and children per folder at 500, and returns about
  8,000 nodes at most. Omitted siblings become one synthetic node with
  `id: null`.
- `get_largest_files` returns at most 1,000 files. `get_type_breakdown` returns
  all eight categories, including empty ones.
- `start_scan` returns a scan ID, and every scan event carries it as `scanId`,
  so the UI can ignore late events from an older scan. The events are
  `scan-progress` with `{ scanId, files, folders, bytes, errors, currentPath }`,
  `scan-complete` with `{ scanId, root, errors }`, `scan-cancelled` with
  `{ scanId }` and `scan-error` with `{ scanId, message }`.
- `get_node_path` backs Copy path, and `open_folder` opens a scanned directory.
- [src/categories.json](src/categories.json) defines category IDs, labels,
  colours and extensions. The Rust backend and the React frontend share it.

## License

[MIT](LICENSE)
