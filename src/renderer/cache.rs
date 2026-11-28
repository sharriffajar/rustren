//! Cross-thread render caches, frame buffer pools, and cache hit/miss telemetry.

use anyhow::{Result, anyhow};
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

/// Cache kill-switches for bisecting hit-vs-miss divergence.
///
/// `RUSTREN_DISABLE_CACHES=1` (or `all`) turns every cross-frame pixmap cache into a
/// pure pass-through; `RUSTREN_DISABLE_CACHES=blur,warp,shadow,text,svg` disables only
/// the named ones. With a cache disabled every frame takes the fresh path, so if two
/// thread counts then render identically, that cache's hit path is not pixel-exact.
pub(super) fn cache_disabled(name: &str) -> bool {
    use std::sync::OnceLock;
    static SPEC: OnceLock<(bool, Vec<String>)> = OnceLock::new();
    let (all, names) = SPEC.get_or_init(|| {
        let raw = std::env::var("RUSTREN_DISABLE_CACHES").unwrap_or_default();
        if raw.is_empty() || raw == "0" {
            return (false, Vec::new());
        }
        if raw == "1" || raw.eq_ignore_ascii_case("all") {
            return (true, Vec::new());
        }
        (
            false,
            raw.split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect(),
        )
    });
    *all || names.iter().any(|n| n == name)
}

#[allow(dead_code)]
pub(super) fn caches_disabled() -> bool {
    cache_disabled("any")
}

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
    RwLock<HashMap<(u64, u32, u32, u32, u32), Option<(Arc<Pixmap>, i32, i32)>>>,
> = LazyLock::new(|| RwLock::new(HashMap::new()));

pub(super) static GLOBAL_SHADOW_CACHE: LazyLock<
    RwLock<HashMap<(u32, u32, u32, u64, u32, u64), Arc<Pixmap>>>,
> = LazyLock::new(|| RwLock::new(HashMap::new()));

pub(super) static GLOBAL_FRAME_DEDUP_CACHE: LazyLock<RwLock<HashMap<u64, Arc<Vec<u8>>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

#[cfg(feature = "pro")]
static GLOBAL_FRAME_POOL: LazyLock<std::sync::Mutex<Vec<Vec<u8>>>> =
    LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

const MAX_THREAD_RGBA_POOL_ENTRIES: usize = 32;
const MAX_POOLED_RGBA_BYTES: usize = 32 * 1024 * 1024;

thread_local! {
    static THREAD_FONT_SYS: RefCell<Option<(usize, FontSystem, SwashCache)>> = const { RefCell::new(None) };
    pub(super) static THREAD_SVG_CACHE: RefCell<HashMap<(u64, usize), resvg::usvg::Tree>> = RefCell::new(HashMap::new());
    pub(super) static THREAD_SVG_RASTER_CACHE: RefCell<HashMap<(u64, usize, u32, u32), Pixmap>> = RefCell::new(HashMap::new());
    static THREAD_FX_SCRATCH: RefCell<Option<Pixmap>> = const { RefCell::new(None) };
    static THREAD_RGBA_POOL: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    pub(super) static THREAD_SIG_BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

pub(super) fn take_scratch_rgba(needed: usize, zeroed: bool) -> Vec<u8> {
    let reused = THREAD_RGBA_POOL.with(|cell| {
        let mut pool = cell.borrow_mut();
        pool.iter()
            .position(|b| b.capacity() >= needed)
            .map(|pos| pool.swap_remove(pos))
    });
    match reused {
        Some(mut buf) => {
            buf.resize(needed, 0);
            if zeroed {
                buf.fill(0);
            }
            buf
        }
        None => vec![0u8; needed],
    }
}

pub(super) fn recycle_scratch_rgba(buf: Vec<u8>) {
    if buf.capacity() == 0 || buf.capacity() > MAX_POOLED_RGBA_BYTES {
        return;
    }
    THREAD_RGBA_POOL.with(|cell| {
        let mut pool = cell.borrow_mut();
        if pool.len() < MAX_THREAD_RGBA_POOL_ENTRIES {
            pool.push(buf);
        }
    });
}

/// RAII scratch `Pixmap` backed by the thread-local RGBA pool (lock-free).
pub(super) struct ScratchPixmap {
    pm: Option<Pixmap>,
}

impl ScratchPixmap {
    pub(super) fn new(w: u32, h: u32) -> Option<Self> {
        let needed = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let size = tiny_skia::IntSize::from_wh(w, h)?;
        let buf = take_scratch_rgba(needed, true);
        let pm = Pixmap::from_vec(buf, size)?;
        Some(Self { pm: Some(pm) })
    }

    #[allow(dead_code)]
    pub(super) fn into_pixmap(mut self) -> Pixmap {
        self.pm.take().expect("ScratchPixmap already consumed")
    }
}

impl std::ops::Deref for ScratchPixmap {
    type Target = Pixmap;
    fn deref(&self) -> &Self::Target {
        self.pm.as_ref().expect("ScratchPixmap already consumed")
    }
}

impl std::ops::DerefMut for ScratchPixmap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.pm.as_mut().expect("ScratchPixmap already consumed")
    }
}

impl Drop for ScratchPixmap {
    fn drop(&mut self) {
        if let Some(pm) = self.pm.take() {
            recycle_scratch_rgba(pm.take());
        }
    }
}

#[cfg(feature = "pro")]
pub fn recycle_frame_buffer(buf: Vec<u8>) {
    let mut pool = GLOBAL_FRAME_POOL.lock().unwrap_or_else(|e| e.into_inner());
    if pool.len() < 48 {
        pool.push(buf);
    }
}

#[cfg(not(feature = "pro"))]
pub fn recycle_frame_buffer(_buf: Vec<u8>) {}

#[cfg(feature = "pro")]
pub(super) fn recycle_rgba_buffer(buf: Vec<u8>) {
    recycle_scratch_rgba(buf);
}

#[cfg(feature = "pro")]
pub(super) fn take_pooled_yuv_buffer(needed: usize) -> Vec<u8> {
    let mut pool = GLOBAL_FRAME_POOL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pos) = pool.iter().position(|b| b.len() == needed) {
        return pool.swap_remove(pos);
    }
    Vec::with_capacity(needed)
}

#[cfg(feature = "pro")]
pub(super) fn take_pooled_pixmap(base: &Pixmap) -> Option<Pixmap> {
    let needed = base.data().len();
    let mut buf = take_scratch_rgba(needed, false);
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
    GLOBAL_BLUR_CACHE
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    GLOBAL_WARP_CACHE
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    GLOBAL_SHADOW_CACHE
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    GLOBAL_FRAME_DEDUP_CACHE
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    THREAD_RGBA_POOL.with(|cell| cell.borrow_mut().clear());
    #[cfg(feature = "pro")]
    {
        GLOBAL_FRAME_POOL
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
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
    if let Some(pm) = THREAD_FX_SCRATCH.with(|c| c.borrow_mut().take())
        && pm.width() == pw
        && pm.height() == ph
    {
        return Ok(pm);
    }
    Pixmap::new(pw, ph).ok_or_else(|| anyhow!("Failed to allocate layer fx pixmap {}x{}", pw, ph))
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
            const BUNDLED_ALLO_FONTS: &[&[u8]] = &[
                include_bytes!("../../assets/fonts/allo/Allo-Thin.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-ThinOblique.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-ExtraLight.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-ExtraLightOblique.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-Light.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-LightOblique.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-Regular.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-Oblique.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-Medium.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-MediumOblique.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-SemiBold.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-SemiBoldOblique.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-Bold.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-BoldOblique.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-ExtraBold.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-ExtraBoldOblique.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-Black.otf"),
                include_bytes!("../../assets/fonts/allo/Allo-BlackOblique.otf"),
            ];
            for font_bytes in BUNDLED_ALLO_FONTS {
                fs.db_mut().load_font_data(font_bytes.to_vec());
            }
            for font_bytes in &assets.fonts {
                fs.db_mut().load_font_data(font_bytes.clone());
            }
            *borrow = Some((font_count, fs, SwashCache::new()));
        }
        let (_, font_system, swash_cache) = borrow.as_mut().unwrap();
        f(font_system, swash_cache)
    })
}
