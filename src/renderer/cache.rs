//! Cross-thread render caches, frame buffer pools, and cache hit/miss telemetry.

use anyhow::{anyhow, Result};
use cosmic_text::{FontSystem, SwashCache};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, RwLock};
use tiny_skia::Pixmap;

use crate::assets::AssetStore;

use super::fx::clear_pixmap_rect;
use super::text;

pub(super) const MAX_BLUR_CACHE_ENTRIES: usize = 256;
pub(super) const MAX_WARP_CACHE_ENTRIES: usize = 512;
pub(super) const MAX_SHADOW_CACHE_ENTRIES: usize = 256;
pub(super) const MAX_SVG_RASTER_CACHE_ENTRIES: usize = 512;
#[cfg(feature = "pro")]
pub(super) const MAX_FRAME_DEDUP_CACHE_ENTRIES: usize = 16;

pub(super) static SVG_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
pub(super) static SVG_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
pub(super) static BLUR_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
pub(super) static BLUR_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
pub(super) static FRAME_DEDUP_HITS: AtomicU64 = AtomicU64::new(0);
pub(super) static FRAME_DEDUP_MISSES: AtomicU64 = AtomicU64::new(0);

pub(super) type PreWarpEntry = Option<(Arc<Pixmap>, i32, i32, f32, f32)>;

pub(super) static GLOBAL_BLUR_CACHE: LazyLock<RwLock<HashMap<(u64, u32, u32), PreWarpEntry>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

pub(super) static GLOBAL_WARP_CACHE: LazyLock<
    RwLock<HashMap<(u64, i32, i32, i32, i32), Option<(Arc<Pixmap>, i32, i32)>>>,
> = LazyLock::new(|| RwLock::new(HashMap::new()));

pub(super) static GLOBAL_SHADOW_CACHE: LazyLock<
    RwLock<HashMap<(u32, u32, u32, u64, u32), Arc<Pixmap>>>,
> = LazyLock::new(|| RwLock::new(HashMap::new()));

pub(super) static GLOBAL_FRAME_DEDUP_CACHE: LazyLock<RwLock<HashMap<u64, Arc<Vec<u8>>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

#[cfg(feature = "pro")]
static GLOBAL_RGBA_POOL: rustren_pro::BufferPool = rustren_pro::BufferPool::new(32);

#[cfg(feature = "pro")]
static GLOBAL_FRAME_POOL: rustren_pro::BufferPool = rustren_pro::BufferPool::new(48);

thread_local! {
    static THREAD_FONT_SYS: RefCell<Option<(usize, FontSystem, SwashCache)>> = const { RefCell::new(None) };
    pub(super) static THREAD_SVG_CACHE: RefCell<HashMap<(u64, usize), resvg::usvg::Tree>> = RefCell::new(HashMap::new());
    pub(super) static THREAD_SVG_RASTER_CACHE: RefCell<HashMap<(u64, usize, u32, u32), Pixmap>> = RefCell::new(HashMap::new());
    static THREAD_FX_SCRATCH: RefCell<Option<Pixmap>> = const { RefCell::new(None) };
    pub(super) static THREAD_SIG_BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

#[cfg(feature = "pro")]
pub fn recycle_frame_buffer(buf: Vec<u8>) {
    GLOBAL_FRAME_POOL.recycle(buf);
}

#[cfg(not(feature = "pro"))]
pub fn recycle_frame_buffer(_buf: Vec<u8>) {}

#[cfg(feature = "pro")]
pub(super) fn recycle_rgba_buffer(buf: Vec<u8>) {
    GLOBAL_RGBA_POOL.recycle(buf);
}

#[cfg(feature = "pro")]
pub(super) fn take_pooled_yuv_buffer(needed: usize) -> Vec<u8> {
    GLOBAL_FRAME_POOL
        .take_matching(needed)
        .unwrap_or_else(|| Vec::with_capacity(needed))
}

#[cfg(feature = "pro")]
pub(super) fn take_pooled_pixmap(base: &Pixmap) -> Option<Pixmap> {
    let needed = base.data().len();
    let mut buf = GLOBAL_RGBA_POOL.take_matching(needed)?;
    buf.copy_from_slice(base.data());
    let size = tiny_skia::IntSize::from_wh(base.width(), base.height())?;
    Pixmap::from_vec(buf, size)
}

#[cfg(not(feature = "pro"))]
pub(super) fn take_pooled_pixmap(_base: &Pixmap) -> Option<Pixmap> {
    None
}

pub fn record_frame_dedup_hits(count: u64) {
    if count > 0 {
        FRAME_DEDUP_HITS.fetch_add(count, Ordering::Relaxed);
    }
}

pub fn clear_render_caches() {
    GLOBAL_BLUR_CACHE.write().unwrap().clear();
    GLOBAL_WARP_CACHE.write().unwrap().clear();
    GLOBAL_SHADOW_CACHE.write().unwrap().clear();
    GLOBAL_FRAME_DEDUP_CACHE.write().unwrap().clear();
    #[cfg(feature = "pro")]
    {
        GLOBAL_RGBA_POOL.clear();
        GLOBAL_FRAME_POOL.clear();
    }
    text::clear_text_raster_cache();
}

pub fn reset_cache_stats() {
    SVG_CACHE_HITS.store(0, Ordering::Relaxed);
    SVG_CACHE_MISSES.store(0, Ordering::Relaxed);
    BLUR_CACHE_HITS.store(0, Ordering::Relaxed);
    BLUR_CACHE_MISSES.store(0, Ordering::Relaxed);
    FRAME_DEDUP_HITS.store(0, Ordering::Relaxed);
    FRAME_DEDUP_MISSES.store(0, Ordering::Relaxed);
    text::reset_text_cache_stats();
    clear_render_caches();
}

pub fn get_cache_stats() -> (u64, u64, u64, u64) {
    (
        SVG_CACHE_HITS.load(Ordering::Relaxed),
        SVG_CACHE_MISSES.load(Ordering::Relaxed),
        BLUR_CACHE_HITS.load(Ordering::Relaxed),
        BLUR_CACHE_MISSES.load(Ordering::Relaxed),
    )
}

pub fn get_text_cache_stats() -> (u64, u64) {
    text::get_text_cache_stats()
}

pub fn get_frame_dedup_stats() -> (u64, u64) {
    (
        FRAME_DEDUP_HITS.load(Ordering::Relaxed),
        FRAME_DEDUP_MISSES.load(Ordering::Relaxed),
    )
}

pub(super) fn take_fx_scratch_pixmap(pw: u32, ph: u32) -> Result<Pixmap> {
    if let Some(pm) = THREAD_FX_SCRATCH.with(|c| c.borrow_mut().take()) {
        if pm.width() == pw && pm.height() == ph {
            return Ok(pm);
        }
    }
    Pixmap::new(pw, ph)
        .ok_or_else(|| anyhow!("Failed to allocate layer fx pixmap {}x{}", pw, ph))
}

pub(super) fn return_fx_scratch_pixmap(mut pm: Pixmap, dirty: Option<(u32, u32, u32, u32)>) {
    if let Some((x0, y0, x1, y1)) = dirty {
        clear_pixmap_rect(&mut pm, x0, y0, x1, y1);
    }
    THREAD_FX_SCRATCH.with(|c| {
        let mut borrow = c.borrow_mut();
        if borrow.is_none() {
            *borrow = Some(pm);
        }
    });
}

pub(super) fn with_thread_font_system<T, F>(assets: &AssetStore, f: F) -> Result<T>
where
    F: FnOnce(&mut FontSystem, &mut SwashCache) -> Result<T>,
{
    let font_count = assets.fonts.len();
    THREAD_FONT_SYS.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let needs_init = match &*borrow {
            Some((count, _, _)) => *count != font_count,
            None => true,
        };
        if needs_init {
            let mut fs = FontSystem::new();
            for font_bytes in &assets.fonts {
                fs.db_mut().load_font_data(font_bytes.clone());
            }
            *borrow = Some((font_count, fs, SwashCache::new()));
        }
        let (_, font_system, swash_cache) = borrow.as_mut().unwrap();
        f(font_system, swash_cache)
    })
}
