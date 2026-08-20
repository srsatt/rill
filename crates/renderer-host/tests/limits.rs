use std::{io::Write, time::Duration};

use anyhow::{Result, anyhow};
use rill_contracts::{RENDER_PROTOCOL_VERSION, RenderMode, RenderRequest};
use rill_renderer_host::{RenderError, Renderer, RendererLimits, WasiRenderer};
use serde_json::json;
use tempfile::NamedTempFile;
use wasmtime::{Config, Engine};

fn request() -> RenderRequest {
    RenderRequest {
        version: RENDER_PROTOCOL_VERSION,
        template: "test".to_owned(),
        mode: RenderMode::Modern,
        locale: "en".to_owned(),
        render_id: "limits-".to_owned(),
        props: json!({}),
        assets: Default::default(),
        csrf_token: String::new(),
    }
}

fn renderer_from_wat(source: &str, limits: RendererLimits) -> Result<WasiRenderer> {
    let wasm = wat::parse_str(source)?;
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    let engine = Engine::new(&config).map_err(|error| anyhow!(error.to_string()))?;
    let compiled = engine
        .precompile_module(&wasm)
        .map_err(|error| anyhow!(error.to_string()))?;
    let mut file = NamedTempFile::new()?;
    file.write_all(&compiled)?;
    Ok(WasiRenderer::load(file.path(), limits)?)
}

fn output_module(output: &[u8]) -> String {
    let bytes = output
        .iter()
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<String>();
    format!(
        r#"(module
            (import "wasi_snapshot_preview1" "fd_write"
                (func $fd_write (param i32 i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (data (i32.const 32) "{bytes}")
            (func (export "_start")
                i32.const 0
                i32.const 32
                i32.store
                i32.const 4
                i32.const {length}
                i32.store
                i32.const 1
                i32.const 0
                i32.const 1
                i32.const 16
                call $fd_write
                drop))"#,
        length = output.len()
    )
}

#[test]
fn rejects_oversized_input_before_guest_execution() -> Result<()> {
    let renderer = renderer_from_wat(
        &output_module(
            br#"{"version":1,"status":200,"headHtml":"","bodyHtml":"","hydrationState":null}"#,
        ),
        RendererLimits {
            input_bytes: 16,
            ..RendererLimits::default()
        },
    )?;

    assert!(matches!(
        renderer.render(&request()),
        Err(RenderError::InputTooLarge { limit: 16 })
    ));
    Ok(())
}

#[test]
fn rejects_invalid_json_response() -> Result<()> {
    let renderer = renderer_from_wat(&output_module(b"not json"), RendererLimits::default())?;

    assert!(matches!(
        renderer.render(&request()),
        Err(RenderError::InvalidResponse(_))
    ));
    Ok(())
}

#[test]
fn output_limit_stops_large_response() -> Result<()> {
    let renderer = renderer_from_wat(
        &output_module(&vec![b'x'; 1_024]),
        RendererLimits {
            output_bytes: 64,
            ..RendererLimits::default()
        },
    )?;

    assert!(matches!(
        renderer.render(&request()),
        Err(RenderError::OutputTooLarge { limit: 64 } | RenderError::Trap(_))
    ));
    Ok(())
}

#[test]
fn fuel_exhaustion_traps_without_killing_host() -> Result<()> {
    let renderer = renderer_from_wat(
        r#"(module (func (export "_start") (loop $forever (br $forever))))"#,
        RendererLimits {
            fuel: 1_000,
            timeout: Duration::from_secs(1),
            ..RendererLimits::default()
        },
    )?;

    assert!(matches!(
        renderer.render(&request()),
        Err(RenderError::Trap(_))
    ));
    Ok(())
}

#[test]
fn epoch_timeout_traps_without_killing_host() -> Result<()> {
    let renderer = renderer_from_wat(
        r#"(module (func (export "_start") (loop $forever (br $forever))))"#,
        RendererLimits {
            fuel: u64::MAX,
            timeout: Duration::from_millis(1),
            ..RendererLimits::default()
        },
    )?;

    assert!(matches!(
        renderer.render(&request()),
        Err(RenderError::Trap(_))
    ));
    Ok(())
}

#[test]
fn memory_minimum_above_limit_traps_without_killing_host() -> Result<()> {
    let renderer = renderer_from_wat(
        r#"(module (memory 1) (func (export "_start")))"#,
        RendererLimits {
            memory_bytes: 32 * 1024,
            ..RendererLimits::default()
        },
    )?;

    assert!(matches!(
        renderer.render(&request()),
        Err(RenderError::Trap(_))
    ));
    Ok(())
}
