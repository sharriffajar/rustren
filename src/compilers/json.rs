//! JSON -> CompiledScene compiler.

use anyhow::{Context, Result};
use std::path::Path;

use super::Compiler;
use crate::ir::CompiledScene;

pub struct JsonCompiler;

impl Compiler for JsonCompiler {
    fn compile(&self, path: &Path) -> Result<CompiledScene> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Gagal baca file: {:?}", path))?;

        let scene: crate::schema::Scene = serde_json::from_str(&content)
            .with_context(|| format!("Gagal parse JSON di {:?}", path))?;

        Ok(CompiledScene::Static(scene))
    }

    fn format_name(&self) -> &'static str {
        "json"
    }
}
