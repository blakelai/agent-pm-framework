use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

/// Build a native binary/starter ZIP from a clean committed framework checkout.
#[derive(Parser)]
struct Args {
    #[arg(long)]
    binary: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let report = pm_agent::distribution::create(&root, &args.binary, &args.output)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
