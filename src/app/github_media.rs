//! GitHub body/comment cleanup and remote-media caching for the detail view.
//!
//! GitHub lets a body carry raw HTML: `<!-- -->` guidance comments,
//! `<details>`/`<summary>` folds, and pasted uploads as `<img>`/`<video>`
//! tags. [`clean`] rewrites that into the markdown subset the transcript
//! renderer speaks — image tags become `![](…)` blocks, video tags become
//! `[video](…)` links (nothing here plays video), comments vanish, and any
//! other tag degrades to its text.
//!
//! [`cache`] downloads the resulting image URLs to a bounded on-disk cache
//! so `img()` paints a local file. Bounds on what an attacker-controlled PR
//! body can make us fetch: HTTPS only — redirects too — a short redirect
//! chain, a per-file byte cap enforced by curl plus a pre-write length check
//! (curl buffers stdout before anything is persisted), and a whole-cache cap
//! enforced by oldest-first eviction.

use std::collections::HashMap;
use std::hash::{Hash, Hasher as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::md::parser::{self, Block};

/// Per-file cap: enforced by `--max-filesize` where the server reports a
/// length and by the post-download check where it does not.
const MAX_MEDIA_BYTES: usize = 20 * 1024 * 1024;
/// Whole-cache cap; the oldest files go first when it is exceeded.
const MAX_MEDIA_CACHE_BYTES: u64 = 256 * 1024 * 1024;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The media cache root: `~/Library/Caches/Goddard/pull-request-media` in
/// release, the checkout's gitignored `temp/` in debug — the same split the
/// daemon's model cache follows, so development never touches the installed
/// app's cache.
fn cache_dir() -> Option<PathBuf> {
    let root = if cfg!(debug_assertions) {
        crate::persistence::StateStore::default_path()
            .parent()?
            .to_path_buf()
    } else {
        dirs::cache_dir()?.join(waku_protocol::identity::DATA_DIRECTORY_NAME)
    };
    Some(root.join("pull-request-media"))
}

fn media_path(dir: &Path, url: &str) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    dir.join(format!("{:016x}", hasher.finish()))
}

/// Download each URL into the cache, returning URL → local file for the ones
/// that landed. Runs on a background executor — spawning curl and touching
/// the filesystem are both forbidden on the UI thread.
pub(super) fn cache(urls: Vec<String>) -> HashMap<String, PathBuf> {
    let Some(dir) = cache_dir() else {
        return HashMap::new();
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return HashMap::new();
    }
    prune(&dir);

    urls.into_iter()
        .filter(|url| url.starts_with("https://"))
        .filter_map(|url| {
            let path = media_path(&dir, &url);
            if path.is_file() || download(&url, &path).is_some() {
                Some((url, path))
            } else {
                None
            }
        })
        .collect()
}

fn download(url: &str, path: &Path) -> Option<()> {
    let mut curl = std::process::Command::new("curl");
    if let Some(search_path) = crate::command_env::executable_search_path() {
        curl.env("PATH", search_path);
    }
    let output = curl
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "5",
            "--max-filesize",
            &MAX_MEDIA_BYTES.to_string(),
            "--max-time",
            "20",
            url,
        ])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() || output.stdout.len() > MAX_MEDIA_BYTES
    {
        return None;
    }
    // Write-then-rename so an interrupted write cannot leave a torn file the
    // URL's hash would keep hitting forever. The temp name stays unique
    // across the concurrent fetches different projects may run.
    let temp = path.with_file_name(format!(
        ".{}.{}.{}.part",
        path.file_name()?.to_string_lossy(),
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    std::fs::write(&temp, &output.stdout).ok()?;
    std::fs::rename(&temp, path).ok()?;
    Some(())
}

/// Drop abandoned `.part` temp files, then evict oldest-first until the
/// cache fits under `MAX_MEDIA_CACHE_BYTES`. Best-effort: a failed prune
/// only leaves the cache larger than intended.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("part") {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        files.push((
            path,
            metadata.len(),
            metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
        ));
    }
    let mut total: u64 = files.iter().map(|(_, size, _)| *size).sum();
    if total <= MAX_MEDIA_CACHE_BYTES {
        return;
    }
    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, size, _) in files {
        if total <= MAX_MEDIA_CACHE_BYTES {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

/// The remote image destinations in a cleaned body — what [`cache`] should
/// fetch. HTTPS only: `data:` URLs decode in place and anything else does
/// not get fetched at all.
pub(super) fn media_urls(text: &str) -> Vec<String> {
    fn visit<'a>(blocks: impl Iterator<Item = &'a Block>, urls: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Image { url, .. } => {
                    if url.starts_with("https://") && !urls.contains(url) {
                        urls.push(url.clone());
                    }
                }
                Block::BlockQuote { children } => visit(children.iter(), urls),
                Block::List { items, .. } => {
                    for item in items {
                        visit(item.blocks.iter(), urls);
                    }
                }
                _ => {}
            }
        }
    }
    let tree = parser::parse(text);
    let mut urls = Vec::new();
    visit(tree.blocks.iter().map(|top| &top.block), &mut urls);
    urls
}

/// Rewrite GitHub's raw-HTML body markup into the renderer's markdown
/// subset.
pub(super) fn clean(body: &str) -> String {
    let mut result = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("<!--") {
        result.push_str(&rest[..start]);
        let Some(end) = rest[start + 4..].find("-->") else {
            break;
        };
        rest = &rest[start + 4 + end + 3..];
    }
    result.push_str(rest);

    // Media tags go first so the generic strip below cannot erase them.
    let with_media = media_tags_to_markdown(&result);

    let mut summary_cleaned = String::with_capacity(with_media.len());
    let mut rest = with_media.as_str();
    while let Some(start) = rest.find("<summary>") {
        summary_cleaned.push_str(&rest[..start]);
        if let Some(end) = rest[start + 9..].find("</summary>") {
            let summary_text = &rest[start + 9..start + 9 + end];
            summary_cleaned.push_str(&format!("\n> **{}**\n\n", summary_text.trim()));
            rest = &rest[start + 9 + end + 10..];
        } else {
            summary_cleaned.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    summary_cleaned.push_str(rest);

    let mut plain = String::with_capacity(summary_cleaned.len());
    let mut rest = summary_cleaned.as_str();
    while let Some(start) = rest.find('<') {
        plain.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            plain.push_str(&rest[start..]);
            rest = "";
            break;
        };
        rest = &rest[start + end + 1..];
    }
    if !rest.is_empty() {
        plain.push_str(rest);
    }
    plain = plain.replace("&nbsp;", " ");
    plain.trim().to_owned()
}

/// Rewrite `<img>` tags to markdown images and `<video>`/`<source>` tags to
/// `[video](…)` links. Anything without a usable `src` is dropped (it
/// carried no content); unrelated tags pass through for the stripper above.
fn media_tags_to_markdown(body: &str) -> String {
    let mut output = String::with_capacity(body.len());
    let mut rest = body;
    let mut inside_video = false;
    while let Some(start) = rest.find('<') {
        output.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let tag = &rest[start..start + end + 1];
        let tag_lower = tag.to_ascii_lowercase();
        let is_video = tag_lower.starts_with("<video")
            || (tag_lower.starts_with("<source")
                && (inside_video
                    || tag_lower.contains("type=\"video/")
                    || tag_lower.contains("type='video/")));
        let is_media_tag = tag_lower.starts_with("<img")
            || tag_lower.starts_with("<video")
            || tag_lower.starts_with("<source");
        if is_media_tag {
            if let Some(src) = tag_src(tag) {
                let src = src.trim();
                if !src.is_empty() {
                    if is_video {
                        output.push_str("\n\n[video](");
                    } else {
                        output.push_str("\n\n![](");
                    }
                    output.push_str(src);
                    output.push_str(")\n\n");
                }
            }
            if tag_lower.starts_with("<video") {
                inside_video = true;
            } else if tag_lower.starts_with("</video") {
                inside_video = false;
            }
        } else {
            output.push_str(tag);
        }
        rest = &rest[start + end + 1..];
    }
    output.push_str(rest);
    output
}

/// Extract the `src` attribute value from a single HTML tag (double- or
/// single-quoted). Returns `None` when absent or unterminated.
fn tag_src(tag: &str) -> Option<&str> {
    let bytes = tag.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        // Find a case-insensitive `src` word boundary.
        let remaining = &tag[index..];
        let pos = remaining.to_ascii_lowercase().find("src")?;
        let key = index + pos;
        let before_ok =
            key == 0 || !(bytes[key - 1].is_ascii_alphanumeric() || bytes[key - 1] == b'-');
        let after = key + 3;
        let after_ok =
            after >= bytes.len() || !(bytes[after].is_ascii_alphanumeric() || bytes[after] == b'-');
        index = after;
        if !(before_ok && after_ok) {
            continue;
        }
        let mut cursor = after;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] != b'=' {
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return None;
        }
        let quote = bytes[cursor];
        if quote != b'"' && quote != b'\'' {
            // Unquoted attribute values are rare in pasted GitHub HTML; skip
            // rather than guess a terminator.
            continue;
        }
        let value_start = cursor + 1;
        let mut value_end = value_start;
        while value_end < bytes.len() && bytes[value_end] != quote {
            value_end += 1;
        }
        if value_end >= bytes.len() {
            return None;
        }
        return Some(&tag[value_start..value_end]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_media_becomes_markdown() {
        let cleaned = clean(
            "look at this\n\n<img src=\"https://github.com/user-attachments/assets/abc\">\n\n\
             <video src=\"https://github.com/user-attachments/assets/def\"></video>\n\n\
             <video><source src='https://example.com/clip.mp4' type=\"video/mp4\"></video>",
        );
        assert!(
            cleaned.contains("![](https://github.com/user-attachments/assets/abc)"),
            "{cleaned}"
        );
        assert!(
            cleaned.contains("[video](https://github.com/user-attachments/assets/def)"),
            "{cleaned}"
        );
        assert!(
            cleaned.contains("[video](https://example.com/clip.mp4)"),
            "{cleaned}"
        );
    }

    #[test]
    fn comments_and_other_tags_are_stripped() {
        let cleaned = clean(
            "real<!-- hidden --> text <b>bold</b> <details><summary>More</summary>inner</details>",
        );
        assert!(!cleaned.contains("hidden"), "{cleaned}");
        assert!(cleaned.contains("real"), "{cleaned}");
        assert!(cleaned.contains("bold"), "{cleaned}");
        assert!(cleaned.contains("> **More**"), "{cleaned}");
        assert!(cleaned.contains("inner"), "{cleaned}");
        assert!(!cleaned.contains('<'), "{cleaned}");
    }

    #[test]
    fn tag_src_handles_both_quote_styles() {
        assert_eq!(tag_src("<img src=\"a\">"), Some("a"));
        assert_eq!(tag_src("<img SRC='b'>"), Some("b"));
        assert_eq!(tag_src("<img data-src=\"x\">"), None);
        assert_eq!(tag_src("<img src>"), None);
    }

    #[test]
    fn media_urls_finds_nested_https_images() {
        let urls = media_urls(&clean(
            "text\n\n![](https://a.example/x.png)\n\n> ![](https://a.example/y.png)\n\n\
             ![](data:image/png;base64,aa)\n\n![](http://insecure.example/z.png)",
        ));
        assert_eq!(
            urls,
            vec![
                "https://a.example/x.png".to_owned(),
                "https://a.example/y.png".to_owned()
            ]
        );
    }
}
