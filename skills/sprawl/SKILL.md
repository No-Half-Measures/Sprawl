---
name: sprawl
description: Find what is using disk space with the sprawl-cli disk analyzer. Use when the user asks why a drive is full, what takes up space, which folders or files are largest, or what grew recently.
---

# Sprawl disk analysis

`sprawl-cli` scans drives and folders and prints JSON reports. Use it instead
of recursive `Get-ChildItem`, `du` or `dir /s`. It is much faster and reports
growth since the previous scan.

## Setup

Run `sprawl-cli --version`. If the command isn't found, download the binary for
your platform from https://github.com/No-Half-Measures/Sprawl/releases/latest
(`sprawl-cli-windows-x64.exe`, `sprawl-cli-linux-x64.tar.gz` or
`sprawl-cli-macos-universal.tar.gz`) and ask the user before putting it on PATH.

## Workflow

1. Run `sprawl-cli list` to see saved scans and their `ageSeconds`.
2. If a saved scan contains the path and is recent enough for the question, run
   `sprawl-cli show <path>`. It works for any folder inside a saved scan, so a
   drive scan answers questions about every folder on that drive.
3. Otherwise run `sprawl-cli scan <path>`. Prefer scanning the drive root, such
   as `C:\`, so later questions can use `show`. Without administrator rights, a
   full drive scan can take a minute or more.
4. Drill down with `show <subfolder>` rather than raising `--depth` on a large
   tree. Use `--limit` to see more items per folder.

Saved scans are shared with the Sprawl app, so scans the user ran there are
available too.

## Reading reports

- Sizes are bytes. Times are Unix milliseconds. Convert to GB or MB when you
  report to the user.
- `children` are sorted by size. An item named like `25 smaller items` with kind
  `other` groups the rest.
- `sizeChange` is growth in bytes since the previous scan, `new: true` marks
  items that didn't exist then, and `changes.grownFiles` lists the files that
  grew most. `changes` is null when there is no earlier scan.
- `unaccounted`, at drive roots only, is used space the scan couldn't see:
  system files, restore points and unreadable folders.
- `scan.elevated: false` or nonzero `scan.errors` means some folders were
  skipped. Running the scan as administrator includes them.
- Sizes are logical. Compressed, sparse and hard-linked files can make totals
  differ from what the operating system reports.

## Safety

- Sprawl only reads. Never delete, move or empty anything without the user's
  explicit approval for that specific item.
- Report the biggest items and what they are, then let the user decide.
- Some large files must not be deleted directly. Explain them and suggest the
  proper tool instead:
  - `pagefile.sys`, `hiberfil.sys` and `swapfile.sys`: Windows settings, or
    `powercfg /h off`
  - WSL `ext4.vhdx`: `wsl --manage <distro> --set-sparse true` or compacting
    the disk
  - Docker `docker_data.vhdx`: `docker system prune`
  - `C:\Windows\WinSxS` and `Installer`: Disk Cleanup or
    `DISM /Online /Cleanup-Image /StartComponentCleanup`
  - Caches of package managers and IDEs: their own clean commands
