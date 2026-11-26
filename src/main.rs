mod agents;
mod export;
mod gui;
mod library;
mod mcp;
mod model;

use std::path::PathBuf;

const USAGE: &str = "Scriber: 2D CAD for laser cutters, drivable by AI agents over MCP.

Usage:
  scriber [FILE.scrbr]            open the editor
  scriber mcp                     MCP server on stdio for AI agents (connects to the editor, starting it if needed)
  scriber mcp --headless          MCP server on stdio without a window
  scriber agents [list]           show detected AI agents and whether Scriber is connected
  scriber agents add [ID...]      connect Scriber to agents (default: all detected)
  scriber agents remove [ID...]   disconnect Scriber from agents

Environment: SCRIBER_PORT (default 47821) is the editor's local MCP port.";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("mcp") => mcp::relay(args.iter().any(|a| a == "--headless")),
        Some("agents") => agents::cli(&args[1..]),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("-V" | "--version") => {
            println!("scriber {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(a) if a.starts_with('-') => anyhow::bail!("unknown option {a}\n\n{USAGE}"),
        file => gui::run(file.map(PathBuf::from)),
    }
}
