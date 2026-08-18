use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn run() -> Result<()> {
    let status = Command::new("node")
        .arg("tools/measure.mjs")
        .status()
        .context("start Node measurement harness")?;
    if !status.success() {
        bail!("measurement harness exited with {status}");
    }
    Ok(())
}
