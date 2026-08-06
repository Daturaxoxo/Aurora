use image::imageops::FilterType;
use std::fs;
use std::path::Path;

#[cfg(target_os = "windows")]
const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#;

fn main() {
    let pairs = [("./production/icons", "./production/icons/processed")];

    for (source, processed) in pairs.map(|(s, p)| (Path::new(s), Path::new(p))) {
        if source.exists() {
            let _ = fs::remove_dir_all(processed);
            fs::create_dir_all(processed).unwrap();
            process_directory(source, source, processed);
        }
    }

    // Each entry point lands in $OUT_DIR/<file stem>.rs. The binaries include
    // their own file by name instead of `slint::include_modules!()`, which can
    // only ever point at whichever file was compiled last.
    slint_build::compile("./frontend/main.slint").unwrap();
    slint_build::compile("./frontend/uninstall.slint").unwrap();

    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../production/icons/logo.ico");
        res.set_manifest(MANIFEST);
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
                    fs::copy(&path, &dest_path).unwrap();
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
