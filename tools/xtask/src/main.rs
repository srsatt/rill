mod measure;
mod strip_wasm;

use std::{env, fs, process::Command};

use anyhow::{Context, Result, anyhow, bail};
use wasmtime::{Config, Engine};

const RENDERER_WASM: &str = "artifacts/ui-renderer.wasm";
const RENDERER_AOT: &str = "artifacts/ui-renderer.cwasm";

fn main() -> Result<()> {
    let command = env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    match command.as_str() {
        "generate-contracts" => generate_contracts(),
        "build-ui" => {
            generate_contracts()?;
            run("pnpm", &["--dir", "ui", "build:client"])
        }
        "build-renderer" => build_renderer(),
        "build-assets" => build_assets(),
        "verify-renderer" => {
            build_renderer()?;
            run("cargo", &["test", "-p", "rill-renderer-host"])
        }
        "build-release" => {
            build_assets()?;
            run("cargo", &["build", "--release", "-p", "rill"])
        }
        "test-e2e" => {
            build_assets()?;
            run("pnpm", &["--dir", "ui", "exec", "playwright", "test"])
        }
        "measure" => measure::run(),
        "help" => {
            println!(
                "cargo xtask <generate-contracts|build-ui|build-renderer|build-assets|verify-renderer|build-release|test-e2e|measure>"
            );
            Ok(())
        }
        other => bail!("unknown xtask command: {other}"),
    }
}

fn build_renderer() -> Result<()> {
    generate_contracts()?;
    run("pnpm", &["--dir", "ui", "build:renderer"])?;
    run("pnpm", &["--dir", "ui", "coverage:renderer"])?;
    run("pnpm", &["--dir", "ui", "compile:renderer"])?;
    strip_wasm::run()?;
    precompile_renderer()
}

fn build_assets() -> Result<()> {
    generate_contracts()?;
    run("pnpm", &["--dir", "ui", "build"])?;
    strip_wasm::run()?;
    precompile_renderer()
}

fn precompile_renderer() -> Result<()> {
    let wasm = fs::read(RENDERER_WASM).context("read stripped renderer WASM")?;
    let mut config = Config::new();
    config.consume_fuel(true);
    config.epoch_interruption(true);
    let engine =
        Engine::new(&config).map_err(|error| anyhow!("create renderer compiler: {error}"))?;
    let compiled = engine
        .precompile_module(&wasm)
        .map_err(|error| anyhow!("AOT-compile renderer: {error}"))?;
    let temporary = format!("{RENDERER_AOT}.next");
    fs::write(&temporary, &compiled).context("write renderer AOT artifact")?;
    fs::rename(&temporary, RENDERER_AOT).context("install renderer AOT artifact")?;
    println!("renderer AOT: {} -> {} bytes", wasm.len(), compiled.len());
    Ok(())
}

fn generate_contracts() -> Result<()> {
    fs::create_dir_all("ui/generated").context("create generated contract directory")?;
    fs::write(
        "ui/generated/render-contract.ts",
        rill_contracts::typescript_bindings(),
    )
    .context("write generated renderer contract")?;
    Ok(())
}

fn run(program: &str, arguments: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(arguments)
        .status()
        .with_context(|| format!("failed to start {program}"))?;
    if !status.success() {
        bail!("{program} exited with {status}");
    }
    Ok(())
}
