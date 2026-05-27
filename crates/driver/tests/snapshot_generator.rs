//! Utility for generating PNG snapshots from graphic_tests HTML files
//! Run with: SAVE_SNAPSHOTS=1 cargo test -p lumen-driver snapshot_generator

use lumen_driver::InProcessSession;
use std::fs;
use std::path::Path;

#[test]
fn snapshot_generator() {
    if std::env::var("SAVE_SNAPSHOTS").is_err() {
        eprintln!("Set SAVE_SNAPSHOTS=1 to generate snapshots");
        return;
    }

    let workspace_root = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(workspace_root)
        .parent()
        .and_then(|p| p.parent())
        .expect("Could not find workspace root");

    let tests_dir = root.join("graphic_tests");
    let snapshots_dir = tests_dir.join("snapshots");

    // Create snapshots directory if it doesn't exist
    fs::create_dir_all(&snapshots_dir).expect("Failed to create snapshots directory");

    // Find all HTML test files
    let entries = fs::read_dir(&tests_dir).expect("Failed to read graphic_tests directory");
    let mut html_files: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "html")
                .unwrap_or(false)
        })
        .filter(|e| {
            // Skip index.html and 1000000-final.html (final is manual review)
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            !name_str.starts_with("index") && !name_str.starts_with("1000000")
        })
        .collect();

    html_files.sort_by(|a, b| {
        let a_name = a.file_name();
        let b_name = b.file_name();
        a_name.cmp(&b_name)
    });

    eprintln!("Generating {} PNG snapshots...", html_files.len());

    for entry in html_files {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();

        // Strip .html extension and create .png filename
        let png_name = file_name_str.replace(".html", ".png");
        let snapshot_path = snapshots_dir.join(&png_name);

        // Skip if snapshot already exists
        if snapshot_path.exists() {
            eprintln!("  ✓ {}: snapshot exists", file_name_str);
            continue;
        }

        // Load and render HTML
        let url = format!("file://{}", path.display());
        let mut session = InProcessSession::new();

        match session.navigate(&url) {
            Ok(_) => {
                // Wait for render to complete
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Get screenshot
                match session.screenshot() {
                    Ok(png_bytes) => {
                        // Save PNG
                        match fs::write(&snapshot_path, &png_bytes) {
                            Ok(_) => {
                                eprintln!("  ✓ {}: saved {} bytes", file_name_str, png_bytes.len());
                            }
                            Err(e) => {
                                eprintln!("  ✗ {}: failed to write PNG: {}", file_name_str, e);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  ✗ {}: screenshot failed: {}", file_name_str, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("  ✗ {}: navigate failed: {}", file_name_str, e);
            }
        }
    }

    eprintln!("Snapshot generation complete!");
}
