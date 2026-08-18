//! Capability-poor host for the AOT-compiled Solid renderer.

mod wasi;

use std::{path::Path, sync::Arc, time::Duration};

use rill_contracts::{RenderRequest, RenderResponse};
use thiserror::Error;

pub use wasi::WasiRenderer;

pub trait Renderer: Send + Sync {
    fn render(&self, request: &RenderRequest) -> Result<RenderResponse, RenderError>;
}

#[derive(Debug, Clone)]
pub struct RendererLimits {
    pub fuel: u64,
    pub memory_bytes: usize,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub timeout: Duration,
}

impl Default for RendererLimits {
    fn default() -> Self {
        Self {
            fuel: 200_000_000,
            memory_bytes: 64 * 1024 * 1024,
            input_bytes: 512 * 1024,
            output_bytes: 2 * 1024 * 1024,
            timeout: Duration::from_millis(250),
        }
    }
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("renderer request exceeds {limit} byte input limit")]
    InputTooLarge { limit: usize },
    #[error("renderer response exceeds {limit} byte output limit")]
    OutputTooLarge { limit: usize },
    #[error("renderer module could not be loaded: {0}")]
    Load(String),
    #[error("renderer trapped: {0}")]
    Trap(String),
    #[error("renderer returned invalid UTF-8")]
    InvalidUtf8,
    #[error("renderer returned invalid protocol JSON: {0}")]
    InvalidResponse(String),
    #[error("renderer protocol version {actual} is unsupported")]
    Version { actual: u16 },
}

pub fn load_renderer(
    path: impl AsRef<Path>,
    limits: RendererLimits,
) -> Result<Arc<dyn Renderer>, RenderError> {
    Ok(Arc::new(WasiRenderer::load(path, limits)?))
}
