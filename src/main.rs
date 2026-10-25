mod convert;
mod nbx;

#[cfg(test)]
mod e2e;

use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    input: PathBuf,

    #[arg(short, long)]
    output: Option<PathBuf>,

    #[arg(long, default_value = "Workspace")]
    from: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let output = args.output.clone().unwrap_or_else(|| args.input.with_extension("nbxl"));

    let bytes = fs::read(&args.input).with_context(|| format!("failed to read {}", args.input.display()))?;

    let dom = if bytes.starts_with(b"<roblox!") {
        rbx_binary::from_reader(bytes.as_slice()).context("error parsing rbxl")?
    } else {
        rbx_xml::from_reader_default(bytes.as_slice()).context("error parsing rbxlx")?
    };

    let (root, found) = convert::find_root(&dom, &args.from);
    if !found {
        eprintln!("warning: class {} not found", args.from);
    }

    let (nodes, stats) = convert::convert(&dom, root);
    let encoded = nbx::encode(&nodes)?;
    fs::write(&output, &encoded).with_context(|| format!("failed to write {}", output.display()))?;

    println!("{} -> {} ({} instances, {} bytes)", args.input.display(), output.display(), nodes.len(), encoded.len());
    print_map("converted", &stats.converted);
    print_map("skipped", &stats.skipped);
    print_map("replaced", &stats.fallbacks);
    Ok(())
}

fn print_map(title: &str, map: &std::collections::BTreeMap<String, usize>) {
    if map.is_empty() {
        return;
    }
    let items: Vec<String> = map.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    println!("  {title}: {}", items.join(", "));
}