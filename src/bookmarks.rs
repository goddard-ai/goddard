//! Finder bookmarks: opaque data macOS resolves by inode, so a folder
//! rename or same-volume move still finds the same directory where a stored
//! path cannot. Bookmark creation and resolution touch the filesystem, so
//! callers keep both off the UI thread.

use std::path::{Path, PathBuf};

/// Bookmark data for `path`, persisted alongside it. `None` when the
/// platform or the folder itself cannot provide one.
#[cfg(target_os = "macos")]
pub fn create(path: &Path) -> Option<Vec<u8>> {
    use objc2_foundation::{NSString, NSURL, NSURLBookmarkCreationOptions};

    let path = NSString::from_str(&path.to_string_lossy());
    let url = NSURL::fileURLWithPath_isDirectory(&path, true);
    url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
        NSURLBookmarkCreationOptions::empty(),
        None,
        None,
    )
    .ok()
    .map(|data| data.to_vec())
}

/// The bookmark's current target plus whether the data went stale resolving
/// it — a stale answer should be re-created and stored. `None` when the
/// folder is gone or moved across volumes.
#[cfg(target_os = "macos")]
pub fn resolve(data: &[u8]) -> Option<(PathBuf, bool)> {
    use objc2::runtime::Bool;
    use objc2_foundation::{NSData, NSURL, NSURLBookmarkResolutionOptions};

    let data = NSData::from_vec(data.to_vec());
    let mut stale = Bool::NO;
    let url = unsafe {
        NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
            &data,
            NSURLBookmarkResolutionOptions::WithoutUI
                | NSURLBookmarkResolutionOptions::WithoutMounting,
            None,
            &mut stale,
        )
    }
    .ok()?;
    let path = url.path()?;
    Some((PathBuf::from(path.to_string()), stale.as_bool()))
}

#[cfg(not(target_os = "macos"))]
pub fn create(_path: &Path) -> Option<Vec<u8>> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn resolve(_data: &[u8]) -> Option<(PathBuf, bool)> {
    None
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use uuid::Uuid;

    #[test]
    fn a_renamed_folder_still_resolves() {
        let base = std::env::temp_dir().join(format!("waku-bookmark-{}", Uuid::new_v4()));
        let renamed = base.with_file_name(format!(
            "{}-moved",
            base.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let data = super::create(&base).expect("bookmark");
        std::fs::rename(&base, &renamed).unwrap();
        let (resolved, _stale) = super::resolve(&data).expect("resolve after rename");
        assert_eq!(
            resolved,
            renamed.canonicalize().unwrap_or_else(|_| renamed.clone())
        );
        let _ = std::fs::remove_dir_all(&renamed);
    }

    #[test]
    fn a_deleted_folder_no_longer_resolves() {
        let base = std::env::temp_dir().join(format!("waku-bookmark-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let data = super::create(&base).expect("bookmark");
        std::fs::remove_dir_all(&base).unwrap();
        let resolved = super::resolve(&data);
        assert!(
            resolved.is_none() || !resolved.unwrap().0.is_dir(),
            "resolution must not point at a folder that is gone"
        );
    }
}
