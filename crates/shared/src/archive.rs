use std::path::{Component, Path};

use anyhow::{anyhow, Context, Result};
use archive::{ArchiveExtractor, ArchiveFormat};
use unrar::Archive as RarArchive;
use zip::ZipArchive;

pub const ARCHIVE_EXTENSIONS: [&str; 9] =
    ["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "lz4"];

pub fn extract_archive<P: AsRef<Path>>(src: P, dest: P) -> Result<()> {
    let src = src.as_ref();
    let Some(extension) = src.extension() else {
        return Err(anyhow!("Couldn't get file extension"));
    };

    match extension.to_str().unwrap_or("") {
        "" => return Err(anyhow!("Couldn't get file extension")),
        "rar" => {
            let archive = RarArchive::new(&src).open_for_processing()?;
            archive.extract_all(dest)?;
        }
        ext if ARCHIVE_EXTENSIONS.contains(&ext) => {
            let data = std::fs::read(src)?;
            if ext == "zip" {
                let reader = std::io::Cursor::new(&data);
                let mut zip_extractor = ZipArchive::new(reader)
                    .with_context(|| "Failed to initialize zip extractor")?;
                zip_extractor.extract(dest)?;
                return Ok(());
            }
            let extractor = ArchiveExtractor::new()
                .with_max_file_size(20 * 1024 * 1024 * 1024)
                .with_max_total_size(30 * 1024 * 1024 * 1024);
            let files = match ext {
                "7z" => extractor.extract(&data, ArchiveFormat::SevenZ)?,
                "tar" => extractor.extract(&data, ArchiveFormat::Tar)?,
                "gz" => extractor.extract(&data, ArchiveFormat::Gz)?,
                "bz2" => extractor.extract(&data, ArchiveFormat::Bz2)?,
                "xz" => extractor.extract(&data, ArchiveFormat::Xz)?,
                "zst" => extractor.extract(&data, ArchiveFormat::Zst)?,
                "lz4" => extractor.extract(&data, ArchiveFormat::Lz4)?,
                _ => unreachable!(),
            };

            let dest = dest.as_ref();
            for file in files {
                let rel = Path::new(&file.path);
                if rel
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
                {
                    return Err(anyhow!("unsafe path '{}' in archive", file.path));
                }
                let target = dest.join(rel);
                if file.is_directory {
                    std::fs::create_dir_all(&target)?;
                } else {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&target, file.data)?;
                }
            }
        }

        _ => {
            return Err(anyhow!("Unsupported archive type"));
        }
    }

    Ok(())
}
