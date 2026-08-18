use std::{path::Path, sync::Arc, thread, time::Duration};

use rill_contracts::{RENDER_PROTOCOL_VERSION, RenderRequest, RenderResponse};
use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{
    WasiCtxBuilder,
    p1::{self, WasiP1Ctx},
    p2::pipe::{MemoryInputPipe, MemoryOutputPipe},
};

use crate::{RenderError, Renderer, RendererLimits};

pub struct WasiRenderer {
    engine: Arc<Engine>,
    module: Module,
    limits: RendererLimits,
}

const EPOCH_TICK: Duration = Duration::from_millis(10);

struct HostState {
    wasi: WasiP1Ctx,
    limits: StoreLimits,
}

impl WasiRenderer {
    pub fn load(path: impl AsRef<Path>, limits: RendererLimits) -> Result<Self, RenderError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);

        let engine =
            Arc::new(Engine::new(&config).map_err(|error| RenderError::Load(error.to_string()))?);
        let module = Module::from_file(&engine, path)
            .map_err(|error| RenderError::Load(error.to_string()))?;
        let weak_engine = Arc::downgrade(&engine);
        thread::spawn(move || {
            loop {
                thread::sleep(EPOCH_TICK);
                let Some(engine) = weak_engine.upgrade() else {
                    break;
                };
                engine.increment_epoch();
            }
        });

        Ok(Self {
            engine,
            module,
            limits,
        })
    }
}

impl Renderer for WasiRenderer {
    fn render(&self, request: &RenderRequest) -> Result<RenderResponse, RenderError> {
        let input = serde_json::to_vec(request)
            .map_err(|error| RenderError::InvalidResponse(error.to_string()))?;
        if input.len() > self.limits.input_bytes {
            return Err(RenderError::InputTooLarge {
                limit: self.limits.input_bytes,
            });
        }

        let stdin = MemoryInputPipe::new(input);
        let stdout = MemoryOutputPipe::new(self.limits.output_bytes);
        let stderr = MemoryOutputPipe::new(64 * 1024);
        let wasi = WasiCtxBuilder::new()
            .stdin(stdin)
            .stdout(stdout.clone())
            .stderr(stderr.clone())
            .build_p1();
        let store_limits = StoreLimitsBuilder::new()
            .memory_size(self.limits.memory_bytes)
            .build();
        let mut store = Store::new(
            &self.engine,
            HostState {
                wasi,
                limits: store_limits,
            },
        );
        store.limiter(|state| &mut state.limits);
        store
            .set_fuel(self.limits.fuel)
            .map_err(|error| RenderError::Trap(error.to_string()))?;
        let timeout_ticks = self
            .limits
            .timeout
            .as_nanos()
            .div_ceil(EPOCH_TICK.as_nanos())
            .max(1);
        store.set_epoch_deadline(u64::try_from(timeout_ticks).unwrap_or(u64::MAX));

        let mut linker = Linker::new(&self.engine);
        p1::add_to_linker_sync(&mut linker, |state: &mut HostState| &mut state.wasi)
            .map_err(|error| RenderError::Load(error.to_string()))?;

        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|error| RenderError::Trap(error.to_string()))?;
        let start = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .map_err(|error| RenderError::Trap(error.to_string()))?;
        start.call(&mut store, ()).map_err(|error| {
            let diagnostics = stderr.contents();
            let diagnostics = String::from_utf8_lossy(&diagnostics);
            if diagnostics.trim().is_empty() {
                RenderError::Trap(format!("{error:#}"))
            } else {
                RenderError::Trap(format!(
                    "{error:#}; renderer stderr: {}",
                    diagnostics.trim()
                ))
            }
        })?;

        let output = stdout.contents();
        if output.len() >= self.limits.output_bytes {
            return Err(RenderError::OutputTooLarge {
                limit: self.limits.output_bytes,
            });
        }
        let output = std::str::from_utf8(&output).map_err(|_| RenderError::InvalidUtf8)?;
        let response: RenderResponse = serde_json::from_str(output.trim())
            .map_err(|error| RenderError::InvalidResponse(error.to_string()))?;
        if response.version != RENDER_PROTOCOL_VERSION {
            return Err(RenderError::Version {
                actual: response.version,
            });
        }
        Ok(response)
    }
}
