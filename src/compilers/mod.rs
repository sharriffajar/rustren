//! Compiler trait + dispatcher.
//!
//! Setiap format input (JSON, HTML, TSX) punya implementasi Compiler sendiri.
//! Dispatcher memilih compiler berdasarkan file extension.

use anyhow::Result;
use std::path::Path;

use crate::ir::CompiledScene;

pub mod json;
// pub mod html;  // Sprint 6
// pub mod tsx;   // Sprint 7

/// Trait untuk semua format compiler.
/// Implementor bertanggung jawab parse file -> CompiledScene.
pub trait Compiler: Send + Sync {
    /// Parse file input menjadi CompiledScene.
    fn compile(&self, path: &Path) -> Result<CompiledScene>;

    /// Nama format (untuk error messages & --profile output).
    fn format_name(&self) -> &'static str;
}

/// Deteksi compiler yang sesuai berdasarkan file extension.
/// Return Box<dyn Compiler> — caller tinggal panggil `.compile(path)`.
pub fn detect(path: &Path) -> Result<Box<dyn Compiler>> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("File tidak punya extension: {:?}", path))?;

    match ext.to_lowercase().as_str() {
        "json" => Ok(Box::new(json::JsonCompiler)),
        // "html" | "htm" => Ok(Box::new(html::HtmlCompiler)),  // Sprint 6
        // "tsx" | "jsx" => Ok(Box::new(tsx::TsxCompiler::new()?)),  // Sprint 7
        other => Err(anyhow::anyhow!(
            "Format tidak didukung: .{}\nSupported: .json (Sprint 5), .html (Sprint 6), .tsx (Sprint 7)",
            other
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::CompiledScene;

    #[test]
    fn test_json_compiler_parses_showreel() {
        let path = std::path::Path::new("examples/showreel.json");
        let compiler = json::JsonCompiler;
        let compiled = compiler.compile(path).unwrap();
        assert!(matches!(compiled, CompiledScene::Static(_)));
        let scene = compiled.as_static();
        assert_eq!(scene.canvas.width, 1280);
        assert_eq!(scene.canvas.height, 720);
    }

    #[test]
    fn test_detect_returns_json_compiler() {
        let path = std::path::Path::new("examples/showreel.json");
        let compiler = detect(path).unwrap();
        assert_eq!(compiler.format_name(), "json");
    }

    #[test]
    fn test_detect_rejects_unknown_extension() {
        let path = std::path::Path::new("scene.unknown");
        assert!(detect(path).is_err());
    }

    #[test]
    fn test_detect_rejects_no_extension() {
        let path = std::path::Path::new("scene");
        assert!(detect(path).is_err());
    }

    #[test]
    fn test_compiled_scene_helpers() {
        let path = std::path::Path::new("examples/showreel.json");
        let compiler = detect(path).unwrap();
        let compiled = compiler.compile(path).unwrap();
        assert_eq!(compiled.frame_count(30), 510);
        assert_eq!(compiled.as_static().canvas.fps, 30);
    }
}
