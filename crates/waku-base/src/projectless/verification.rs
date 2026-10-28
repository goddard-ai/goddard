//! Payload verification can pause without retaining a borrowed ZipFile outside
//! its archive. The pinned future owns both across cooperative boundaries.
use super::*;
use std::future::{Future, poll_fn};
use std::io::Read as _;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

pub(super) const CHUNK_BYTES: usize = 64 * 1024;

/// Worker-only verification, with no I/O at construction. Each advance reads
/// at most one 64 KiB payload chunk or visits an empty/finished entry. Opening
/// the central directory is a separate first-step metadata cost; use heavy
/// admission, not an unknown-telemetry fallback, before advancing this object.
/// Drop cancels without draining the current entry or publishing an archive.
pub struct ArchiveVerification {
    work: Pin<Box<dyn Future<Output = io::Result<()>> + Send>>,
}

impl ArchiveVerification {
    pub fn new(file: fs::File) -> Self {
        Self {
            work: Box::pin(async move {
                let mut archive = zip::ZipArchive::new(file).map_err(zip_io_error)?;
                let mut buffer = [0u8; CHUNK_BYTES];
                for index in 0..archive.len() {
                    let mut entry = archive.by_index(index).map_err(zip_io_error)?;
                    // CRC validation occurs at EOF. The central directory and
                    // all symlink/directory entry payloads must be checked too.
                    loop {
                        let read = entry.read(&mut buffer)?;
                        if read == 0 {
                            break;
                        }
                        boundary().await;
                    }
                    // Empty entries must not turn one advance into a walk of
                    // the entire archive.
                    boundary().await;
                }
                Ok(())
            }),
        }
    }

    /// A pending result retains progress for the next admitted worker unit.
    /// Stop calling after Ready, as with Future::poll.
    pub fn advance(&mut self) -> Poll<io::Result<()>> {
        self.work
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }
}

pub(super) async fn boundary() {
    let mut first = true;
    poll_fn(move |cx| {
        if std::mem::replace(&mut first, false) {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek as _, Write as _};

    #[test]
    fn verification_yields_between_chunks_and_cancellation_does_not_drain() {
        let root = std::env::temp_dir().join(format!("zip-verification-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("capture.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        writer
            .start_file(
                "payload",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(&vec![b'x'; CHUNK_BYTES * 3]).unwrap();
        writer.finish().unwrap();
        let offset = zip::ZipArchive::new(fs::File::open(&path).unwrap())
            .unwrap()
            .by_index(0)
            .unwrap()
            .data_start()
            .expect("stored entry has a data offset");
        let file = fs::File::open(&path).unwrap();
        let mut observer = file.try_clone().unwrap();
        let before = observer.stream_position().unwrap();
        let mut verifier = ArchiveVerification::new(file);
        assert_eq!(
            observer.stream_position().unwrap(),
            before,
            "construction read the archive"
        );
        assert!(verifier.advance().is_pending());
        let paused = observer.stream_position().unwrap();
        assert!(paused > offset && paused < offset + (CHUNK_BYTES * 3) as u64);
        drop(verifier);
        assert_eq!(
            observer.stream_position().unwrap(),
            paused,
            "cancellation drained the entry"
        );
        drop(observer);

        let mut verifier = ArchiveVerification::new(fs::File::open(&path).unwrap());
        let mut advances = 0;
        loop {
            advances += 1;
            match verifier.advance() {
                Poll::Pending => {}
                Poll::Ready(result) => {
                    result.unwrap();
                    break;
                }
            }
        }
        assert!(advances > 3);
        drop(verifier);
        fs::remove_dir_all(root).unwrap();
    }
}
