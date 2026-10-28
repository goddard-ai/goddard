//! Paths for a durable worker attempt; construction performs validation only.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveLocations {
    pub source: PathBuf,
    pub archive: PathBuf,
    pub partial: PathBuf,
}

impl ArchiveLocations {
    pub fn new(root: &Path, source: &Path, attempt: Uuid) -> io::Result<Self> {
        if !root.is_absolute()
            || !source.is_absolute()
            || root
                .components()
                .chain(source.components())
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid projectless archive path",
            ));
        }
        validate_workspace_path_in(root, source)?;
        let archive = archive_path_in(root, source).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "projectless archive destination is unavailable",
            )
        })?;
        Ok(Self {
            source: source.to_owned(),
            partial: archive.with_extension(format!("zip.{attempt}.partial")),
            archive,
        })
    }

    pub fn prepare_archive_parent(&self) -> io::Result<()> {
        ensure_real_directory(
            self.archive.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "archive has no parent")
            })?,
        )
    }

    /// Worker has already verified every payload and synced the partial file.
    pub fn publish_verified(&self) -> io::Result<()> {
        fs::rename(&self.partial, &self.archive)?;
        sync_directory(
            self.archive.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "archive has no parent")
            })?,
        )
    }
}

/// Publication must survive restart before later stages may rely on it.
fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
