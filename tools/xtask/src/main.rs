mod measure;
mod strip_wasm;

use std::{env, fs, process::Command};

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    let command = env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    match command.as_str() {
        "generate-contracts" => generate_contracts(),
        "build-ui" => {
            generate_contracts()?;
            run("pnpm", &["--dir", "ui", "build:client"])
        }
        "build-renderer" => {
            generate_contracts()?;
            run("pnpm", &["--dir", "ui", "build:renderer"])?;
            run("pnpm", &["--dir", "ui", "coverage:renderer"])?;
            run("pnpm", &["--dir", "ui", "compile:renderer"])?;
            strip_wasm::run()
        }
        "verify-renderer" => {
            generate_contracts()?;
            run("pnpm", &["--dir", "ui", "build:renderer"])?;
            run("pnpm", &["--dir", "ui", "coverage:renderer"])?;
            run("pnpm", &["--dir", "ui", "compile:renderer"])?;
            strip_wasm::run()?;
            run("cargo", &["test", "-p", "rill-renderer-host"])
        }
        "build-release" => {
            generate_contracts()?;
            run("pnpm", &["--dir", "ui", "build"])?;
            strip_wasm::run()?;
            run("cargo", &["build", "--release", "-p", "rill"])
        }
        "test-e2e" => run("pnpm", &["--dir", "ui", "test:e2e"]),
        "measure" => measure::run(),
        "help" => {
            println!(
                "cargo xtask <generate-contracts|build-ui|build-renderer|verify-renderer|build-release|test-e2e|measure>"
            );
            Ok(())
        }
        other => bail!("unknown xtask command: {other}"),
    }
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
