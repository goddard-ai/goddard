//! Bounded Mermaid workers, following the native math cache. Parsing, layout,
//! SVG generation and rasterization never run on the UI thread.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use gpui::{App, EntityId, Global, RenderImage, SvgRenderer};

// The measured text element uses the same atomic image metrics for both.
use super::math::{Metrics, Rendered};

const MAX_SOURCE_BYTES: usize = 8 * 1024;
const MAX_ENTRIES: usize = 256;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_PENDING: usize = 128;
const WORKERS: usize = 2;
const BATCH_SIZE: usize = 8;
const MAX_PIXELS: f64 = 2_000_000.0;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct Key {
    source: Arc<str>,
    font_size: u32,
    scale: u32,
    dark: bool,
}

impl Key {
    pub fn new(source: Arc<str>, font_size: f32, scale: f32, dark: bool) -> Self {
        Self {
            source,
            font_size: font_size.clamp(6.0, 96.0).to_bits(),
            scale: scale.clamp(1.0, 3.0).to_bits(),
            dark,
        }
    }
}

enum State {
    Pending(HashSet<EntityId>),
    // Failures are cached too, so malformed input does not retry every frame.
    Ready(Option<Arc<Rendered>>),
}

struct Entry {
    state: State,
    touched: u64,
    bytes: usize,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    queue: VecDeque<Key>,
    waiting: HashSet<EntityId>,
    clock: u64,
    bytes: usize,
    active: usize,
    pending: usize,
    retired: Vec<Arc<RenderImage>>,
}

// Interior mutability lets cache hits avoid GPUI's global-observer effects.
// This store belongs to the UI thread and never takes a blocking lock.
#[derive(Default)]
struct Store(RefCell<Cache>);
impl Global for Store {}

impl Cache {
    fn request(&mut self, key: &Key, view: EntityId) -> Option<Arc<Rendered>> {
        // Reject by length before retaining a key or queueing work. Oversized
        // transcript content must not bypass the cache's memory bound.
        if key.source.is_empty() || key.source.len() > MAX_SOURCE_BYTES {
            return None;
        }
        self.clock += 1;
        if let Some(entry) = self.entries.get_mut(key) {
            entry.touched = self.clock;
            return match &mut entry.state {
                State::Pending(views) => {
                    views.insert(view);
                    None
                }
                State::Ready(result) => result.clone(),
            };
        }
        if self.pending >= MAX_PENDING {
            self.waiting.insert(view);
            return None;
        }
        self.entries.insert(
            key.clone(),
            Entry {
                state: State::Pending(HashSet::from([view])),
                touched: self.clock,
                bytes: 0,
            },
        );
        self.queue.push_back(key.clone());
        self.pending += 1;
        self.trim();
        None
    }

    fn trim(&mut self) {
        while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_CACHE_BYTES {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, entry)| matches!(entry.state, State::Ready(_)))
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| key.clone());
            let Some(key) = oldest else { break };
            let entry = self.entries.remove(&key).unwrap();
            self.bytes -= entry.bytes;
            if let State::Ready(Some(rendered)) = entry.state {
                self.retired.push(rendered.image.clone());
            }
        }
    }

    fn complete(&mut self, results: Vec<(Key, Option<Arc<Rendered>>)>) -> HashSet<EntityId> {
        let mut notify = std::mem::take(&mut self.waiting);
        for (key, result) in results {
            // Pending entries are never evicted or replaced. Content, size,
            // theme and display scale are part of the immutable key, so an old
            // completion cannot overwrite a newer diagram/style.
            if let Some(entry) = self.entries.get_mut(&key) {
                let State::Pending(views) = &mut entry.state else {
                    continue;
                };
                notify.extend(views.drain());
                entry.bytes = result
                    .as_ref()
                    .map_or(0, |r| r.image.as_bytes(0).map_or(0, |b| b.len()));
                self.bytes += entry.bytes;
                entry.state = State::Ready(result);
                self.pending -= 1;
            }
        }
        self.trim();
        notify
    }
}

/// Queue visible diagrams, deduplicating across every
/// MarkdownView and notifying each observing pane once per completed batch.
pub(super) fn request(keys: &[Key], view: EntityId, cx: &mut App) -> Vec<Option<Arc<Rendered>>> {
    if !cx.has_global::<Store>() {
        cx.set_global(Store::default());
    }
    let results = {
        let mut cache = cx.global::<Store>().0.borrow_mut();
        keys.iter().map(|key| cache.request(key, view)).collect()
    };
    retire_images(cx);
    pump(cx);
    results
}

fn retire_images(cx: &mut App) {
    let images = std::mem::take(&mut cx.global::<Store>().0.borrow_mut().retired);
    if !images.is_empty() {
        // An Arc dropping does not remove GPUI's sprite-atlas entry. Do that
        // explicitly after the current frame, as GPUI's native image caches do.
        cx.defer(move |cx| {
            for image in images {
                cx.drop_image(image, None);
            }
        });
    }
}

fn pump(cx: &mut App) {
    loop {
        let batch = {
            let mut cache = cx.global::<Store>().0.borrow_mut();
            if cache.active >= WORKERS || cache.queue.is_empty() {
                return;
            }
            cache.active += 1;
            let count = BATCH_SIZE.min(cache.queue.len());
            cache.queue.drain(..count).collect::<Vec<_>>()
        };
        let renderer = cx.svg_renderer();
        let task = cx.background_executor().spawn(async move {
            batch
                .into_iter()
                .map(|key| {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        render(&key, &renderer).ok().map(Arc::new)
                    }))
                    .ok()
                    .flatten();
                    (key, result)
                })
                .collect()
        });
        cx.spawn(async move |cx| {
            let results = task.await;
            let _ = cx.update(|cx| {
                let mut cache = cx.global::<Store>().0.borrow_mut();
                cache.active -= 1;
                let views = cache.complete(results);
                drop(cache);
                for view in views {
                    cx.notify(view);
                }
                retire_images(cx);
                pump(cx);
            });
        })
        .detach();
    }
}

fn render(key: &Key, renderer: &SvgRenderer) -> anyhow::Result<Rendered> {
    anyhow::ensure!(
        !key.source.trim().is_empty() && key.source.len() <= MAX_SOURCE_BYTES,
        "mermaid source limit"
    );
    let parsed = mermaid_rs_renderer::parse_mermaid_strict(&key.source)?;
    anyhow::ensure!(
        parsed.graph.nodes.len() <= 256 && parsed.graph.edges.len() <= 512,
        "mermaid graph limit"
    );
    let mut theme = if key.dark {
        mermaid_rs_renderer::Theme::dark()
    } else {
        mermaid_rs_renderer::Theme::modern()
    };
    theme.font_size = f32::from_bits(key.font_size);
    theme.background = "transparent".into();
    let config = mermaid_rs_renderer::LayoutConfig::default();
    let layout = mermaid_rs_renderer::compute_layout(&parsed.graph, &theme, &config);
    let dimensions = mermaid_rs_renderer::measure_svg_dimensions(&layout, &config, None);
    let width = dimensions.width as f64;
    let height = dimensions.height as f64;
    // GPUI supersamples SVGs at 2x in addition to the display's scale factor.
    let raster_scale = f32::from_bits(key.scale) as f64 * 2.0;
    anyhow::ensure!(
        width.is_finite()
            && height.is_finite()
            && width > 0.0
            && height > 0.0
            && width * raster_scale <= 4096.0
            && height * raster_scale <= 4096.0
            && width * height * raster_scale * raster_scale <= MAX_PIXELS,
        "mermaid image limit"
    );
    let svg = mermaid_rs_renderer::render_svg(&layout, &theme, &config);
    anyhow::ensure!(svg.len() <= 2 * 1024 * 1024, "mermaid svg limit");
    let image = renderer.render_single_frame(svg.as_bytes(), f32::from_bits(key.scale))?;
    Ok(Rendered {
        metrics: Metrics {
            width: width as f32,
            ascent: height as f32,
            descent: 0.0,
        },
        image,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_diagrams_rasterize_and_invalid_or_oversized_input_falls_back() {
        let renderer = SvgRenderer::new(Arc::new(()));
        for source in [
            "flowchart TD\n A[Start] --> B{Ready?}\n B -->|Yes| C[Done]",
            "sequenceDiagram\n Alice->>Bob: Hello\n Bob-->>Alice: Hi",
        ] {
            for dark in [false, true] {
                let rendered =
                    render(&Key::new(source.into(), 14.0, 2.0, dark), &renderer).unwrap();
                assert!(rendered.metrics.width > 0.0 && rendered.metrics.ascent > 0.0);
                let bytes = rendered.image.as_bytes(0).unwrap();
                assert!(bytes.len() <= MAX_PIXELS as usize * 4);
                assert!(bytes.chunks_exact(4).any(|pixel| pixel[3] > 0));
            }
        }
        for source in [
            String::new(),
            "not a diagram".into(),
            "x".repeat(MAX_SOURCE_BYTES + 1),
        ] {
            assert!(render(&Key::new(source.into(), 14.0, 2.0, false), &renderer).is_err());
        }
        let huge = format!("flowchart LR\n A[{}] --> B", "word ".repeat(1000));
        assert!(render(&Key::new(huge.into(), 14.0, 3.0, false), &renderer).is_err());
    }

    #[test]
    fn work_is_deduplicated_bounded_and_failures_are_cached() {
        let mut cache = Cache::default();
        let key = Key::new("invalid".into(), 14.0, 2.0, false);
        let view = EntityId::from(1);
        let oversized = Key::new("x".repeat(MAX_SOURCE_BYTES + 1).into(), 14.0, 2.0, false);
        assert!(cache.request(&oversized, view).is_none());
        assert!(cache.entries.is_empty() && cache.queue.is_empty());
        for _ in 0..1000 {
            assert!(cache.request(&key, view).is_none());
        }
        assert_eq!(cache.queue.len(), 1);
        cache.queue.clear();
        assert_eq!(
            cache.complete(vec![(key.clone(), None)]),
            HashSet::from([view])
        );
        for _ in 0..1000 {
            assert!(cache.request(&key, view).is_none());
        }
        assert!(cache.queue.is_empty());
        for i in 0..MAX_PENDING * 2 {
            cache.request(
                &Key {
                    source: format!("flowchart LR; A{i}-->B").into(),
                    ..key.clone()
                },
                view,
            );
        }
        assert_eq!(cache.pending, MAX_PENDING);
        assert_eq!(cache.queue.len(), MAX_PENDING);
    }
}
