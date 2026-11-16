//! Intermediate Representation (IR) untuk rustren.
//!
//! IR adalah single source of truth yang dikonsumsi oleh renderer, validator,
//! snapshot, layout, diff, watch, dan studio. Dihasilkan oleh Compiler trait.
//!
//! Sprint 5: Hanya variant `Static` yang di-support.
//! Sprint 7 (future): Variant `Dynamic` akan menampung TSX AST + per-frame evaluator.

// Re-export penting structs biar path-nya clean: `ir::Scene`, `ir::Layer`, etc.
#[allow(unused_imports)]
pub use crate::schema::{Animation, Canvas, Layer, Scene};

/// Scene yang sudah dikompilasi dari input file (JSON/HTML/TSX).
/// Renderer dan tooling lain hanya mengkonsumsi enum ini.
pub enum CompiledScene {
    /// Scene statis — sudah fully materialized. Cocok untuk JSON & HTML.
    /// Renderer tinggal iterasi frames dari `Scene`.
    Static(Scene),

    /// Scene dinamis — butuh evaluasi per-frame. Untuk TSX dengan hooks.
    /// Field ini diisi oleh TsxCompiler di Sprint 7.
    #[allow(dead_code)] // tidak dipakai di Sprint 5, diaktifkan Sprint 7
    Dynamic {
        /// Source AST untuk caching (parsed once, evaluated N times)
        #[allow(dead_code)]
        source_hash: u64,
        /// Evaluator closure: (frame_idx, fps) -> Scene
        #[allow(dead_code)]
        evaluator: Box<dyn Fn(u32, u32) -> Scene + Send + Sync>,
    },
}

impl std::fmt::Debug for CompiledScene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompiledScene::Static(scene) => f.debug_tuple("Static").field(scene).finish(),
            CompiledScene::Dynamic { source_hash, .. } => f
                .debug_struct("Dynamic")
                .field("source_hash", source_hash)
                .finish(),
        }
    }
}

impl Clone for CompiledScene {
    fn clone(&self) -> Self {
        match self {
            CompiledScene::Static(scene) => CompiledScene::Static(scene.clone()),
            CompiledScene::Dynamic { .. } => {
                panic!("Dynamic scene cloning belum diimplementasi (Sprint 7)")
            }
        }
    }
}

impl CompiledScene {
    /// Helper: extract static Scene. Panic kalau Dynamic (Sprint 7 akan handle).
    pub fn as_static(&self) -> &Scene {
        match self {
            CompiledScene::Static(s) => s,
            CompiledScene::Dynamic { .. } => {
                panic!("Dynamic scene evaluation belum diimplementasi (Sprint 7)")
            }
        }
    }

    /// Helper: extract mutable static Scene.
    pub fn as_static_mut(&mut self) -> &mut Scene {
        match self {
            CompiledScene::Static(s) => s,
            CompiledScene::Dynamic { .. } => {
                panic!("Dynamic scene evaluation belum diimplementasi (Sprint 7)")
            }
        }
    }

    /// Helper: untuk Sprint 5 — semua scene di-anggap static.
    /// Render N frames dari Scene yang sama.
    #[allow(dead_code)]
    pub fn frame_count(&self, fps: u32) -> u32 {
        match self {
            CompiledScene::Static(s) => (s.canvas.duration_secs * fps as f32).ceil() as u32,
            CompiledScene::Dynamic { .. } => 0, // Sprint 7
        }
    }
}
