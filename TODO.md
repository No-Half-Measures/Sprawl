# To do

Features that comparable tools (WizTree, WinDirStat, TreeSize, SpaceSniffer,
DaisyDisk) have and Sprawl lacks, most valuable first.

## High priority

- [x] **Search and filter.** Find items by name, wildcard (`*.iso`),
      extension, size or date, instead of only browsing the treemap or the
      top-50 list.
- [x] **Export from the app.** Save the scan or the current view as CSV or
      JSON from the GUI, like the CLI's JSON output.
- [x] **Size on disk.** Show allocated size next to logical size, accounting
      for compression, sparse files, cluster slack and hard links. Totals are
      far off for WSL and Docker `.vhdx` files, OneDrive placeholders and
      `WinSxS`. Changes the saved-scan format, so it needs a migration.

## Medium priority

- [x] **Breakdown by extension.** List every extension with its total size and
      file count, alongside the 8 categories.
- [x] **Dates.** Store last-modified times, then filter or colour by file age
      to find big files nobody has touched in a year. Changes the saved-scan
      format, so it needs a migration. Shares that work with size on disk.
- [x] **Flat file list.** A sorted, searchable list of every file in the scan.
      The largest-files list currently stops at 50 in the UI and 1,000 in the
      backend.

## Lower priority

- [ ] **Duplicate finder.** Find identical files by size, then hash.
- [ ] **Cleanup advice.** On right-click, suggest the proper tool for
      pagefile, hiberfil, WSL and Docker disks, WinSxS and package caches, as
      `skills/sprawl/SKILL.md` already does. Possibly one-click Temp or
      Recycle Bin cleanup.
- [ ] **Exclude paths.** Skip chosen folders during a scan, such as mounted
      VHDs or `node_modules`.
- [ ] **Scan history.** Keep more than the latest and previous scan per path,
      compare any two, and chart growth over time.
- [ ] **Treemap options.** Cushion shading, colouring by age or extension, and
      saving the treemap as an image.
- [ ] **More drive types.** Check how UNC paths and network drives behave
      today. Add scanning all drives at once and fast reads on ReFS or exFAT.
- [ ] **Portable build.** A release zip that runs from a USB stick without
      installing.
- [ ] **Owner and permissions columns.** Useful on shared servers.

## Not planned

- Opening or running files. Sprawl only reveals them in Explorer, on purpose.
