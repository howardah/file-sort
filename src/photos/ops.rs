use std::fs;
use std::path::Path;

pub fn move_file(src: &Path, dest_dir: &Path) {
    let Some(file_name) = src.file_name() else {
        eprintln!("Error: could not determine filename for {}", src.display());
        return;
    };

    if let Err(e) = fs::create_dir_all(dest_dir) {
        eprintln!("Error creating {}: {}", dest_dir.display(), e);
        return;
    }

    let dest = dest_dir.join(file_name);

    // Try rename first (same filesystem); fall back to copy+delete for cross-device moves.
    if fs::rename(src, &dest).is_err() {
        match fs::copy(src, &dest) {
            Ok(_) => {
                if let Err(e) = fs::remove_file(src) {
                    eprintln!(
                        "Warning: copied {} but could not remove original: {}",
                        src.display(),
                        e
                    );
                    return;
                }
            }
            Err(e) => {
                eprintln!("Error moving {}: {}", src.display(), e);
                return;
            }
        }
    }

    println!("Moved {} → {}", src.display(), dest.display());
}
