use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use wasm_encoder::{Module, RawSection};
use wasmparser::{Encoding, Parser, Payload};

const RENDERER_PATH: &str = "artifacts/ui-renderer.wasm";

pub fn run() -> Result<()> {
    let path = Path::new(RENDERER_PATH);
    let input = fs::read(path)
        .with_context(|| format!("read renderer for stripping: {}", path.display()))?;
    let mut module = Module::new();

    for payload in Parser::new(0).parse_all(&input) {
        let payload = payload.context("parse renderer while stripping")?;
        if let Payload::Version { encoding, .. } = payload
            && encoding != Encoding::Module
        {
            bail!("renderer is not a core WebAssembly module");
        }
        if let Some((id, range)) = payload.as_section()
            && id != 0
        {
            module.section(&RawSection {
                id,
                data: &input[range],
            });
        }
    }

    let output = module.finish();
    let temporary = path.with_extension("wasm.tmp");
    fs::write(&temporary, &output)
        .with_context(|| format!("write stripped renderer: {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .with_context(|| format!("replace renderer: {}", path.display()))?;
    println!("renderer strip: {} -> {} bytes", input.len(), output.len());
    Ok(())
}
