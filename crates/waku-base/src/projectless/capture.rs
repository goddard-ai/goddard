//! Cooperative worker-side capture. The caller owns the staging pathname,
//! admission, publication and cleanup; this object only writes its supplied file.
use super::verification::{CHUNK_BYTES, boundary};
use super::*;
use std::future::Future;
use std::io::{Read as _, Seek, SeekFrom, Write};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

/// Retains traversal and compression progress between worker admissions.
/// Construction performs no I/O. Advances yield after each 64 KiB source chunk,
/// directory entry and completed directory. ZIP finalization still writes all
/// central-directory metadata in one step: use heavy admission before advancing,
/// never the unknown-telemetry fallback. Drop stops output without finalizing,
/// publishing an archive or changing the source directory.
pub struct ArchiveCapture {
    canceled: Arc<AtomicBool>,
    work: Pin<Box<dyn Future<Output = io::Result<fs::File>> + Send>>,
}

impl ArchiveCapture {
    pub fn new(root: PathBuf, destination: fs::File) -> Self {
        let canceled = Arc::new(AtomicBool::new(false));
        let output_canceled = Arc::clone(&canceled);
        Self {
            canceled,
            work: Box::pin(async move {
                let mut writer = zip::ZipWriter::new(AbortableFile {
                    file: destination,
                    canceled: Arc::clone(&output_canceled),
                });
                // Locals drop in reverse declaration order. On source errors,
                // disable output before ZipWriter::drop tries to finalize it.
                let _cancel_on_drop = CancelOnDrop(output_canceled);
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated);
                let mut pending = vec![PathBuf::new()];
                let mut buffer = [0u8; CHUNK_BYTES];
                while let Some(directory) = pending.pop() {
                    for entry in fs::read_dir(root.join(&directory))? {
                        let entry = entry?;
                        let relative = directory.join(entry.file_name());
                        let name = relative.to_string_lossy().into_owned();
                        let file_type = entry.file_type()?;
                        if file_type.is_dir() {
                            writer.add_directory(name, options).map_err(zip_io_error)?;
                            pending.push(relative);
                        } else if file_type.is_symlink() {
                            let target = fs::read_link(entry.path())?;
                            writer
                                .add_symlink(name, target.to_string_lossy().into_owned(), options)
                                .map_err(zip_io_error)?;
                        } else if file_type.is_file() {
                            writer.start_file(name, options).map_err(zip_io_error)?;
                            let mut source = fs::File::open(entry.path())?;
                            loop {
                                let read = source.read(&mut buffer)?;
                                if read == 0 {
                                    break;
                                }
                                writer.write_all(&buffer[..read])?;
                                boundary().await;
                            }
                        }
                        boundary().await;
                    }
                    boundary().await;
                }
                Ok(writer.finish().map_err(zip_io_error)?.file)
            }),
        }
    }

    /// Pending retains progress; stop calling after Ready, as with Future::poll.
    pub fn advance(&mut self) -> Poll<io::Result<fs::File>> {
        self.work
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }
}

impl Drop for ArchiveCapture {
    fn drop(&mut self) {
        self.canceled.store(true, Ordering::Release);
    }
}

struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

// ZipWriter::drop finalizes unfinished archives. Refusing further output keeps
// cancellation/error handling from writing an unbounded central directory.
struct AbortableFile {
    file: fs::File,
    canceled: Arc<AtomicBool>,
}

impl AbortableFile {
    fn perform<T>(
        &mut self,
        operation: impl FnOnce(&mut fs::File) -> io::Result<T>,
    ) -> io::Result<T> {
        if self.canceled.load(Ordering::Acquire) {
            // Interrupted is retried by write_all, so cancellation must use a
            // non-retryable error kind.
            return Err(io::Error::other("archive capture canceled"));
        }
        operation(&mut self.file).inspect_err(|_| self.canceled.store(true, Ordering::Release))
    }
}

impl Write for AbortableFile {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.perform(|file| file.write(buffer))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.perform(Write::flush)
    }
}

impl Seek for AbortableFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.perform(|file| file.seek(position))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_resumes_and_cancellation_does_not_finalize_the_partial_archive() {
        let root = std::env::temp_dir().join(format!("zip-capture-{}", Uuid::new_v4()));
        let source = root.join("workspace");
        fs::create_dir_all(&source).unwrap();
        let payload = vec![b'x'; CHUNK_BYTES * 3];
        fs::write(source.join("payload"), &payload).unwrap();
        let partial = root.join("canceled.partial");
        let file = fs::File::create(&partial).unwrap();
        let mut observer = file.try_clone().unwrap();
        let before = observer.stream_position().unwrap();
        let mut capture = ArchiveCapture::new(source.clone(), file);
        assert_eq!(observer.stream_position().unwrap(), before);
        assert!(capture.advance().is_pending());
        let paused = observer.stream_position().unwrap();
        assert!(paused > before);
        drop(capture);
        assert_eq!(
            observer.stream_position().unwrap(),
            paused,
            "cancellation finalized the partial archive"
        );
        drop(observer);
        assert_eq!(fs::read(source.join("payload")).unwrap(), payload);
        assert!(!root.join("archive.zip").exists());

        fs::create_dir_all(source.join("nested/empty")).unwrap();
        fs::write(source.join("nested/child"), b"preserved").unwrap();
        let mut capture = ArchiveCapture::new(
            source.clone(),
            fs::File::create(root.join("completed.partial")).unwrap(),
        );
        let mut pending = 0;
        let file = loop {
            match capture.advance() {
                Poll::Pending => pending += 1,
                Poll::Ready(result) => break result.unwrap(),
            }
        };
        assert!(pending > 3);
        file.sync_all().unwrap();
        drop(file);
        drop(capture);
        let mut archive =
            zip::ZipArchive::new(fs::File::open(root.join("completed.partial")).unwrap()).unwrap();
        let mut restored = Vec::new();
        archive
            .by_name("payload")
            .unwrap()
            .read_to_end(&mut restored)
            .unwrap();
        assert_eq!(restored, payload);
        restored.clear();
        archive
            .by_name("nested/child")
            .unwrap()
            .read_to_end(&mut restored)
            .unwrap();
        assert_eq!(restored, b"preserved");
        assert!(archive.by_name("nested/empty/").unwrap().is_dir());
        drop(archive);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn source_failure_does_not_finalize_an_empty_archive() {
        let root = std::env::temp_dir().join(format!("zip-capture-failure-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let file = fs::File::create(root.join("capture.partial")).unwrap();
        let mut observer = file.try_clone().unwrap();
        let mut capture = ArchiveCapture::new(root.join("missing-source"), file);
        let Poll::Ready(result) = capture.advance() else {
            panic!("missing source should fail before any payload work");
        };
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(observer.stream_position().unwrap(), 0);
        drop(capture);
        assert_eq!(observer.stream_position().unwrap(), 0);
        drop(observer);
        fs::remove_dir_all(root).unwrap();
    }
}
