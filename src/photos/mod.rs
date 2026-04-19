use chrono::{Datelike, NaiveDate};
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

mod date;
mod group;
mod layout;
mod ops;
mod scan;

use group::{cluster_by_date, PhotoFile};
use layout::{day_dir_name, file_subdir, is_primary, month_dir_name};
use scan::{find_matching_dir, scan_month_dir};

pub fn subcommand() -> Command {
    Command::new("photos")
        .about("Organize photos into a date-based directory hierarchy")
        .arg(
            Arg::new("output")
                .help("Output directory (defaults to current directory)")
                .value_name("OUTPUT"),
        )
        .arg(
            Arg::new("output_flag")
                .short('o')
                .long("output")
                .help("Output directory")
                .value_name("DIR")
                .conflicts_with("output"),
        )
        .arg(
            Arg::new("input")
                .short('i')
                .long("input")
                .help("Input directory to import from")
                .value_name("DIR"),
        )
        .arg(
            Arg::new("recursive")
                .short('r')
                .long("recursive")
                .help("Recurse into subdirectories of the input directory")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("dry_run")
                .long("dry-run")
                .help("Print planned actions without making any changes")
                .action(ArgAction::SetTrue),
        )
}

pub fn run(matches: &ArgMatches) {
    let output = matches
        .get_one::<String>("output")
        .or_else(|| matches.get_one::<String>("output_flag"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("Cannot determine current directory"));

    let input = matches.get_one::<String>("input").map(PathBuf::from);
    let recursive = matches.get_flag("recursive");
    let dry_run = matches.get_flag("dry_run");

    if let Some(input_dir) = input {
        import_mode(&input_dir, &output, recursive, dry_run);
    } else {
        sort_mode(&output, dry_run);
    }
}

// ---------------------------------------------------------------------------
// Sort mode
// ---------------------------------------------------------------------------

fn sort_mode(output: &Path, dry_run: bool) {
    let Ok(entries) = std::fs::read_dir(output) else {
        eprintln!("Error reading output directory: {}", output.display());
        return;
    };

    let mut files: Vec<PhotoFile> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        match date::extract_date(&path) {
            Some(d) => files.push(PhotoFile { path, date: d }),
            None => eprintln!("Warning: no date found for {}, skipping", path.display()),
        }
    }

    if files.is_empty() {
        println!("No files to sort.");
        return;
    }

    if dry_run {
        println!("[DRY RUN] Would move:");
    }

    for cluster in cluster_by_date(files) {
        let month_dir = output.join(month_dir_name(cluster.start));
        let day_dir = month_dir.join(day_dir_name(cluster.start, cluster.end));
        let has_primaries = any_primary(&cluster.files);

        for file in &cluster.files {
            let ext = ext_of(&file.path);
            let dest_dir = match file_subdir(ext, has_primaries) {
                None => day_dir.clone(),
                Some(sub) => day_dir.join(sub),
            };

            if dry_run {
                println!(
                    "  {}\n    → {}\n",
                    file.path.display(),
                    dest_dir.join(file.path.file_name().unwrap()).display()
                );
            } else {
                ops::move_file(&file.path, &dest_dir);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Import mode
// ---------------------------------------------------------------------------

fn import_mode(input: &Path, output: &Path, recursive: bool, dry_run: bool) {
    let raw_paths = collect_files(input, recursive);

    let mut files: Vec<PhotoFile> = Vec::new();
    for path in raw_paths {
        match date::extract_date(&path) {
            Some(d) => files.push(PhotoFile { path, date: d }),
            None => eprintln!("Warning: no date found for {}, skipping", path.display()),
        }
    }

    if files.is_empty() {
        println!("No files to import.");
        return;
    }

    if dry_run {
        println!("[DRY RUN] Would move:");
    }

    // Group by (year, month)
    let mut by_month: HashMap<(i32, u32), Vec<PhotoFile>> = HashMap::new();
    for file in files {
        by_month
            .entry((file.date.year(), file.date.month()))
            .or_default()
            .push(file);
    }

    for ((year, month), month_files) in by_month {
        let sample = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
        let month_dir = output.join(month_dir_name(sample));

        let existing = if month_dir.exists() {
            scan_month_dir(&month_dir)
        } else {
            Vec::new()
        };

        // Split files into those going to an existing dir vs. brand-new dirs
        let mut to_existing: HashMap<PathBuf, Vec<PhotoFile>> = HashMap::new();
        let mut new_files: Vec<PhotoFile> = Vec::new();

        for file in month_files {
            if let Some(dir) = find_matching_dir(&existing, file.date) {
                to_existing
                    .entry(dir.path.clone())
                    .or_default()
                    .push(file);
            } else {
                new_files.push(file);
            }
        }

        // --- Files merging into an existing day dir ---
        for (existing_path, files) in &to_existing {
            let has_primaries =
                dir_has_primaries(existing_path) || any_primary(files);

            for file in files {
                let ext = ext_of(&file.path);
                let dest_dir = match file_subdir(ext, has_primaries) {
                    None => existing_path.clone(),
                    Some(sub) => existing_path.join(sub),
                };

                let file_name = file.path.file_name().unwrap();
                let dest_path = dest_dir.join(file_name);

                if dest_path.exists() {
                    if let Some(existing_date) = date::extract_date(&dest_path) {
                        if existing_date == file.date {
                            println!(
                                "[SKIP] {}  (same name and date found in {})\n",
                                file.path.display(),
                                existing_path.file_name().unwrap().to_string_lossy()
                            );
                            continue;
                        }
                    }
                }

                if dry_run {
                    println!(
                        "  {}\n    → {}\n",
                        file.path.display(),
                        dest_path.display()
                    );
                } else {
                    ops::move_file(&file.path, &dest_dir);
                }
            }
        }

        // --- Files going into newly created day dirs ---
        for cluster in cluster_by_date(new_files) {
            let day_dir = month_dir.join(day_dir_name(cluster.start, cluster.end));
            let has_primaries = any_primary(&cluster.files);

            for file in &cluster.files {
                let ext = ext_of(&file.path);
                let dest_dir = match file_subdir(ext, has_primaries) {
                    None => day_dir.clone(),
                    Some(sub) => day_dir.join(sub),
                };

                if dry_run {
                    println!(
                        "  {}\n    → {}\n",
                        file.path.display(),
                        dest_dir.join(file.path.file_name().unwrap()).display()
                    );
                } else {
                    ops::move_file(&file.path, &dest_dir);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collect_files(dir: &Path, recursive: bool) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if recursive {
        for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
            let path = entry.path().to_path_buf();
            if path.is_file() {
                result.push(path);
            }
        }
    } else if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                result.push(path);
            }
        }
    }
    result
}

fn any_primary(files: &[PhotoFile]) -> bool {
    files
        .iter()
        .any(|f| is_primary(ext_of(&f.path)))
}

fn dir_has_primaries(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        p.is_file() && is_primary(ext_of(&p))
    })
}

fn ext_of(path: &Path) -> &str {
    path.extension().and_then(|e| e.to_str()).unwrap_or("")
}
