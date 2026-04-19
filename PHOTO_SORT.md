# `photos` Subcommand — Implementation Plan

## Overview

The `photos` subcommand automates organizing camera photos into a date-based directory hierarchy. It has two modes depending on whether an input (`-i`) is supplied:

- **Sort mode** (no input): reorganize photos already sitting in the root of the output directory.
- **Import mode** (with input): recursively move photos from an input directory into the output, merging with any existing structure.

---

## CLI Shape

```
file-sort photos [OUTPUT] [-o OUTPUT] [-i INPUT] [--dry-run]
```

| Argument | Description |
|---|---|
| `OUTPUT` (positional) | Destination directory. Defaults to current directory. |
| `-o` / `--output` | Alternative way to specify destination. Conflicts with positional arg. |
| `-i` / `--input` | Source directory. When absent the tool runs in sort mode. |
| `--dry-run` | Print intended actions; do not move or create anything. |

The existing top-level flags (`-e`, `--ignore`, `-r`) are untouched — they belong to the implicit default command (the current extension-based sorter). With subcommands introduced, the existing behaviour becomes reachable as `file-sort sort <directory>` while `file-sort <directory>` will still work as a shorthand via a default subcommand alias.

---

## Output Directory Structure

```
<output>/
  2026 - 04 April/          ← month dir  (YYYY - MM MMMM)
    2026-04-15/             ← day dir (single day)
      IMG_001.jpg
      IMG_002.heic
      RAW/
        IMG_003.raf
    2026-04-20 - 2026-04-21/  ← day-range dir (consecutive days)
      IMG_010.jpg
      RAW/
        IMG_011.raf
```

### Month directory format

`YYYY - MM MMMM` — zero-padded two-digit month number followed by the full English month name.  
Example: `2026 - 04 April`, `2025 - 12 December`.

### Day subdirectory format

- **Single day**: `YYYY-MM-DD`
- **Date range** (consecutive days, same month): `YYYY-MM-DD - YYYY-MM-DD`

A "consecutive" group is a maximal run of calendar days with no gap. Days are grouped per month — a sequence spanning a month boundary produces a separate dir in each month.

### File placement

| Extension | Destination within the day dir |
|---|---|
| `.jpg`, `.jpeg`, `.heic` | Day dir root |
| Everything else (raw formats: `.raf`, `.cr2`, `.nef`, `.arw`, `.dng`, `.rw2`, …) | `RAW/` sub-subdirectory |

---

## Date Extraction

1. Parse **EXIF `DateTimeOriginal`** (tag 0x9003) — most reliable for camera photos.
2. Fall back to **EXIF `DateTime`** (tag 0x0132).
3. Fall back to **file system modification time** (useful for RAW files whose EXIF is embedded differently).
4. If no date can be determined, print a warning and skip the file.

### Dependencies to add

```toml
exif = "0.5"      # kamadak-exif — pure-Rust EXIF reader, handles JPEG/HEIF/TIFF-based RAW
chrono = "0.4"    # date arithmetic and formatting
```

---

## Algorithm Details

### Sort mode (no `-i`)

1. Scan files in the **root** of the output directory only (non-recursive); ignore existing subdirectories.
2. Extract the date for each file.
3. Group files by month.
4. Within each month, sort files by date and compute consecutive-day clusters.
5. Determine the day-dir name (single date or range) for each cluster.
6. In dry-run: print each planned move. Otherwise:
   - Create `<output>/<month-dir>/<day-dir>/` (and `RAW/` as needed).
   - Move each file to its destination.

### Import mode (with `-i`)

1. Walk the input directory **recursively**, collecting all photo files.
2. Extract the date for each file.
3. For each file, determine its target month dir.
4. **Check for an existing day dir** in `<output>/<month-dir>/` that contains at least one photo sharing the same calendar date. If found, use that dir regardless of its name (it may be `Hiking Trip` instead of `2026-04-15`).
5. If no matching existing dir is found, batch newly-imported files by consecutive-day clusters (same logic as sort mode) to derive a new day-dir name.
6. **Duplicate detection**: if a file with the **same name** already exists in the target dir and its EXIF date matches, skip it and leave it in the input directory. Print a notice.
7. In dry-run: print each planned move/skip. Otherwise move files.

#### Finding the matching existing day dir

For each candidate subdirectory under `<output>/<month-dir>/`:
- Scan its root for HEIC/JPG files and its `RAW/` subdir for raw files.
- Collect their EXIF dates.
- If any date in that dir matches the incoming file's date → use this dir.

This scan is done lazily per month, building a `date → existing_dir` map once per month, then reused for all files targeting that month.

---

## Dry-Run Output Format

```
[DRY RUN] Would move:
  /Volumes/Camera/DCIM/IMG_001.jpg
    → /Photos/2026 - 04 April/2026-04-15/IMG_001.jpg

  /Volumes/Camera/DCIM/IMG_002.raf
    → /Photos/2026 - 04 April/2026-04-15/RAW/IMG_002.raf

[DRY RUN] Would skip (already exists):
  /Volumes/Camera/DCIM/IMG_003.jpg  (same name and date found in Hiking Trip/)
```

---

## Example Scenarios

### Scenario 1 — Sort mode, single day

**Before** (`/Photos/` root):
```
IMG_001.jpg   (EXIF: 2026-04-15)
IMG_002.heic  (EXIF: 2026-04-15)
IMG_003.raf   (EXIF: 2026-04-15)
```

**After**:
```
2026 - 04 April/
  2026-04-15/
    IMG_001.jpg
    IMG_002.heic
    RAW/
      IMG_003.raf
```

---

### Scenario 2 — Sort mode, multi-day consecutive

**Before** (`/Photos/` root):
```
IMG_001.jpg   (EXIF: 2026-04-15)
IMG_002.jpg   (EXIF: 2026-04-16)
IMG_003.jpg   (EXIF: 2026-04-20)
IMG_004.jpg   (EXIF: 2026-04-21)
```

**After**:
```
2026 - 04 April/
  2026-04-15 - 2026-04-16/
    IMG_001.jpg
    IMG_002.jpg
  2026-04-20 - 2026-04-21/
    IMG_003.jpg
    IMG_004.jpg
```

---

### Scenario 3 — Sort mode, month boundary

Photos from March 31 and April 1 are **not** grouped into a range because they fall in different month directories.

**Before** (`/Photos/` root):
```
IMG_001.jpg  (EXIF: 2026-03-31)
IMG_002.jpg  (EXIF: 2026-04-01)
```

**After**:
```
2026 - 03 March/
  2026-03-31/
    IMG_001.jpg
2026 - 04 April/
  2026-04-01/
    IMG_002.jpg
```

---

### Scenario 4 — Import mode, merging into a named existing dir

**Existing output** (`/Photos/`):
```
2026 - 04 April/
  Hiking Trip/
    IMG_001.jpg  (EXIF: 2026-04-15)
    IMG_002.heic (EXIF: 2026-04-16)
    RAW/
      IMG_003.raf (EXIF: 2026-04-15)
```

**Input** (`/Volumes/Camera/`):
```
IMG_010.jpg  (EXIF: 2026-04-15)
IMG_011.raf  (EXIF: 2026-04-16)
```

**After** (files moved into the existing named dir):
```
2026 - 04 April/
  Hiking Trip/
    IMG_001.jpg
    IMG_002.heic
    IMG_010.jpg   ← imported
    RAW/
      IMG_003.raf
      IMG_011.raf  ← imported
```

`Hiking Trip/` is matched because it contains photos from both 2026-04-15 and 2026-04-16, which overlap with the incoming dates.

---

### Scenario 5 — Import mode, new photos alongside an existing dir

**Existing output** (`/Photos/`):
```
2026 - 04 April/
  Hiking Trip/
    IMG_001.jpg  (EXIF: 2026-04-15)
```

**Input**:
```
IMG_020.jpg  (EXIF: 2026-04-20)
IMG_021.jpg  (EXIF: 2026-04-21)
```

No existing dir covers the 20th–21st, so a new range dir is created:

**After**:
```
2026 - 04 April/
  Hiking Trip/
    IMG_001.jpg
  2026-04-20 - 2026-04-21/
    IMG_020.jpg
    IMG_021.jpg
```

---

### Scenario 6 — Import mode, duplicate skip

**Existing output** (`/Photos/`):
```
2026 - 04 April/
  2026-04-15/
    IMG_001.jpg  (EXIF: 2026-04-15)
```

**Input**:
```
IMG_001.jpg  (EXIF: 2026-04-15)   ← same name, same date
IMG_002.jpg  (EXIF: 2026-04-15)   ← same date, different name
```

**After**:
- `IMG_001.jpg` in input is **skipped** (left in place) — name and date already present.
- `IMG_002.jpg` is moved into `2026-04-15/` as normal.

---

### Scenario 7 — Import mode, cross-month batch

**Input**:
```
IMG_001.jpg  (EXIF: 2026-03-30)
IMG_002.jpg  (EXIF: 2026-03-31)
IMG_003.jpg  (EXIF: 2026-04-01)
IMG_004.jpg  (EXIF: 2026-04-02)
```

The March files and April files are in different months so they cannot share a single date-range dir:

**After**:
```
2026 - 03 March/
  2026-03-30 - 2026-03-31/
    IMG_001.jpg
    IMG_002.jpg
2026 - 04 April/
  2026-04-01 - 2026-04-02/
    IMG_003.jpg
    IMG_004.jpg
```

---

## Implementation Structure

The code will be split across modules:

```
src/
  main.rs          — CLI definition and dispatch
  sort.rs          — existing extension-sorter logic (extracted)
  photos/
    mod.rs         — entry point: parse args, call sort or import
    date.rs        — EXIF date extraction helpers
    group.rs       — consecutive-day cluster logic
    layout.rs      — month/day dir naming, RAW subdir routing
    scan.rs        — scanning output for existing date→dir mapping
    ops.rs         — file move / dry-run print operations
```

---

## Open Questions / Decisions Needed

1. **Default grouping threshold**: should photos only 1 day apart always be grouped, or should there be a gap tolerance (e.g. group if ≤ 2 days apart)? Current plan: strictly consecutive (no gaps).
2. **Mixed-format day dirs**: if a day dir only has RAW files (no JPG/HEIC), there will be only a `RAW/` subdir inside. Is that acceptable?
3. **Existing subcommand naming**: the current default behaviour (extension sort) — should it remain the default `file-sort <dir>` invocation, or be moved explicitly to `file-sort sort <dir>`? Current plan: keep backwards-compatible default.
4. **RAW format list**: the plan treats any non-JPG/JPEG/HEIC as RAW. Should there be an explicit allowlist of photo extensions, with non-photo files ignored?
