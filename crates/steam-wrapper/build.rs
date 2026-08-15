use image::imageops::FilterType;
use std::fs;
use std::path::Path;

fn main() {
    let pairs = [("./production/icons", "./production/icons/processed")];

    for (source, processed) in pairs.map(|(s, p)| (Path::new(s), Path::new(p))) {
        if source.exists() {
            fs::create_dir_all(processed).unwrap();
            process_directory(source, source, processed);
            prune_orphans(source, processed, processed);
        }
    }

    slint_build::compile("./frontend/main.slint").unwrap();

    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("./production/icons/logo.ico");
        res.compile().unwrap();
    }
}

fn process_directory(root_source: &Path, current_source: &Path, target_base: &Path) {
    for entry in fs::read_dir(current_source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path == target_base {
            continue;
        }

        let relative = path.strip_prefix(root_source).unwrap();
        let dest_path = target_base.join(relative);

        if path.is_dir() {
            fs::create_dir_all(&dest_path).unwrap();
            process_directory(root_source, &path, target_base);
        } else if path.is_file()
            && let Some(extension) = path.extension().and_then(|os| os.to_str())
        {
            let ext_lower = extension.to_lowercase();
            if ext_lower == "png" || ext_lower == "jpg" || ext_lower == "jpeg" {
                let file_name = path.file_name().and_then(|os| os.to_str()).unwrap_or("");
                if file_name.contains("background") {
                    if !is_up_to_date(&path, &dest_path) {
                        fs::copy(&path, &dest_path).unwrap();
                    }
                    continue;
                }

                if is_up_to_date(&path, &dest_path) {
                    continue;
                }

                if let Ok(img) = image::open(&path) {
                    let scaled = img.resize(64, 64, FilterType::Lanczos3);
                    scaled
                        .save(&dest_path)
                        .expect("Failed to save downsampled asset");
                } else {
                    fs::copy(&path, &dest_path).unwrap();
                }
            }
        }
    }
}

fn is_up_to_date(source: &Path, dest: &Path) -> bool {
    let (Ok(source), Ok(dest)) = (fs::metadata(source), fs::metadata(dest)) else {
        return false;
    };

    match (source.modified(), dest.modified()) {
        (Ok(source), Ok(dest)) => dest >= source,
        _ => false,
    }
}

fn prune_orphans(root_source: &Path, target_base: &Path, current: &Path) {
    for entry in fs::read_dir(current).unwrap() {
        let path = entry.unwrap().path();
        let relative = path.strip_prefix(target_base).unwrap();

        if path.is_dir() {
            prune_orphans(root_source, target_base, &path);
        } else if !root_source.join(relative).exists() {
            fs::remove_file(&path).unwrap();
        }
    }
}
