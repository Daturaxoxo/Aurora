use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result};
use jwalk::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[cfg(feature = "extract")]
use {
    anyhow::anyhow,
    archive::{ArchiveExtractor, ArchiveFormat},
    std::path::Component,
    unrar::{Archive as RarArchive, ExtractEvent},
    zip::ZipArchive,
};

#[cfg(feature = "extract")]
pub const ARCHIVE_EXTENSIONS: [&str; 9] =
    ["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "lz4"];

#[cfg(feature = "extract")]
const EXTRACT_CHUNK: usize = 1024 * 1024;

#[cfg(feature = "extract")]
pub fn extract_archive<P: AsRef<Path>>(src: P, dest: P) -> Result<()> {
    extract_archive_with_progress(src, dest, &mut |_, _| {})
}

#[cfg(feature = "extract")]
pub fn extract_archive_with_progress<P: AsRef<Path>>(
    src: P,
    dest: P,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let src = src.as_ref();
    let dest = dest.as_ref();
    let Some(extension) = src.extension().and_then(|os| os.to_str()) else {
        return Err(anyhow!("Couldn't get file extension"));
    };
    let extension = extension.to_lowercase();

    let tarball = src
        .file_stem()
        .map(Path::new)
        .and_then(Path::extension)
        .is_some_and(|e| e.eq_ignore_ascii_case("tar"));

    match extension.as_str() {
        "" => return Err(anyhow!("Couldn't get file extension")),
        "rar" => {
            let total: u64 = RarArchive::new(&src)
                .open_for_listing()?
                .flatten()
                .map(|entry| entry.unpacked_size)
                .sum();
            progress(0, total);

            let mut done: u64 = 0;
            let archive = RarArchive::new(&src).open_for_processing()?;
            archive.extract_all_with_callback(dest, |event| {
                if let ExtractEvent::Ok { size, .. } = event {
                    done += size;
                    progress(done, total);
                }
                true
            })?;
        }
        "zip" => {
            let data = std::fs::read(src)?;
            let reader = std::io::Cursor::new(&data);
            let mut zip =
                ZipArchive::new(reader).with_context(|| "Failed to initialize zip extractor")?;

            let mut total: u64 = 0;
            for i in 0..zip.len() {
                total += zip.by_index(i)?.size();
            }
            progress(0, total);

            let mut done: u64 = 0;
            let mut buffer = vec![0u8; EXTRACT_CHUNK];
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i)?;
                let Some(rel) = entry.enclosed_name() else {
                    return Err(anyhow!(
                        "unsafe path '{}' in archive",
                        entry.name().unwrap_or_default()
                    ));
                };
                let target = dest.join(rel);

                if entry.is_dir() {
                    std::fs::create_dir_all(&target)?;
                    continue;
                }
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                let mut out = std::io::BufWriter::new(std::fs::File::create(&target)?);
                loop {
                    let read = entry.read(&mut buffer)?;
                    if read == 0 {
                        break;
                    }
                    out.write_all(&buffer[..read])?;
                    done += read as u64;
                    progress(done, total);
                }
                out.flush()?;
            }
        }
        ext if ARCHIVE_EXTENSIONS.contains(&ext) => {
            let data = std::fs::read(src)?;
            let extractor = ArchiveExtractor::new()
                .with_max_file_size(20 * 1024 * 1024 * 1024)
                .with_max_total_size(30 * 1024 * 1024 * 1024);
            let files = match ext {
                "gz" if tarball => extractor.extract(&data, ArchiveFormat::TarGz)?,
                "bz2" if tarball => extractor.extract(&data, ArchiveFormat::TarBz2)?,
                "xz" if tarball => extractor.extract(&data, ArchiveFormat::TarXz)?,
                "zst" if tarball => extractor.extract(&data, ArchiveFormat::TarZst)?,
                "lz4" if tarball => extractor.extract(&data, ArchiveFormat::TarLz4)?,

                "7z" => extractor.extract(&data, ArchiveFormat::SevenZ)?,
                "tar" => extractor.extract(&data, ArchiveFormat::Tar)?,
                "gz" => extractor.extract(&data, ArchiveFormat::Gz)?,
                "bz2" => extractor.extract(&data, ArchiveFormat::Bz2)?,
                "xz" => extractor.extract(&data, ArchiveFormat::Xz)?,
                "zst" => extractor.extract(&data, ArchiveFormat::Zst)?,
                "lz4" => extractor.extract(&data, ArchiveFormat::Lz4)?,
                _ => unreachable!(),
            };

            let total: u64 = files.iter().map(|f| f.data.len() as u64).sum();
            progress(0, total);

            let mut done: u64 = 0;
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
                    done += file.data.len() as u64;
                    std::fs::write(&target, file.data)?;
                    progress(done, total);
                }
            }
        }

        _ => {
            return Err(anyhow!("Unsupported archive type"));
        }
    }

    Ok(())
}

const ZIP_COMPRESSION_LEVEL: i64 = 1;
const ZIP_CHUNK: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZippedFile {
    pub name: String,
    pub size: u64,
}

pub fn zip_directories(
    sources: &[(&str, &Path)],
    dest: &Path,
    mut progress: impl FnMut(&Path, u64, u64),
) -> Result<Vec<ZippedFile>> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    let walked: Vec<(&str, &Path, Vec<_>)> = sources
        .iter()
        .map(|(prefix, src)| {
            (
                *prefix,
                *src,
                WalkDir::new(src).into_iter().flatten().collect(),
            )
        })
        .collect();
    let total: u64 = walked
        .iter()
        .flat_map(|(_, _, entries)| entries)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum();

    let file = std::fs::File::create(dest)
        .with_context(|| format!("Failed to create {}", dest.display()))?;
    let mut writer = ZipWriter::new(std::io::BufWriter::new(file));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(ZIP_COMPRESSION_LEVEL))
        .large_file(true);

    let mut done: u64 = 0;
    let mut buffer = vec![0u8; ZIP_CHUNK];
    let mut written = Vec::new();

    for (prefix, src, entries) in walked {
        if !prefix.is_empty() {
            writer.add_directory(prefix, options)?;
        }

        for entry in entries {
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(src) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let name = entry_name(prefix, rel);

            if entry.file_type().is_dir() {
                writer.add_directory(name, options)?;
                continue;
            }
            if !entry.file_type().is_file() {
                continue;
            }

            progress(rel, done, total);
            writer.start_file(name.as_str(), options)?;

            let mut source = std::fs::File::open(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            let mut size: u64 = 0;
            loop {
                let read = source
                    .read(&mut buffer)
                    .with_context(|| format!("Failed to read {}", path.display()))?;
                if read == 0 {
                    break;
                }
                writer.write_all(&buffer[..read])?;
                size += read as u64;
                done += read as u64;
                progress(rel, done, total);
            }
            written.push(ZippedFile { name, size });
        }
    }

    writer
        .finish()?
        .into_inner()
        .map_err(std::io::IntoInnerError::into_error)?
        .sync_all()?;
    Ok(written)
}

pub fn verify_zip(archive: &Path, expected: &[ZippedFile]) -> Result<()> {
    let file = std::fs::File::open(archive)
        .with_context(|| format!("Failed to open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .with_context(|| format!("{} is not a readable zip", archive.display()))?;

    let mut found = Vec::new();
    let mut sink = vec![0u8; ZIP_CHUNK];
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name()?.into_owned();
        let mut size: u64 = 0;
        loop {
            let read = entry
                .read(&mut sink)
                .with_context(|| format!("{name} in {} is damaged", archive.display()))?;
            if read == 0 {
                break;
            }
            size += read as u64;
        }
        found.push(ZippedFile { name, size });
    }

    let mut expected = expected.to_vec();
    expected.sort();
    found.sort();
    if found != expected {
        anyhow::bail!(
            "{} does not match what was archived ({} files found, {} expected)",
            archive.display(),
            found.len(),
            expected.len()
        );
    }
    Ok(())
}

fn entry_name(prefix: &str, rel: &Path) -> String {
    let rel = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if prefix.is_empty() {
        rel
    } else {
        format!("{prefix}/{rel}")
    }
}
