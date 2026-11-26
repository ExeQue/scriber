//! Detect AI agents on this machine and add or remove Scriber's MCP entry in their config.

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs, io};

const NAME: &str = "scriber";

#[derive(Clone, Copy, Debug)]
enum Base {
    Home,
    Config,
}

/// Shape of the entry inside a JSON config.
#[derive(Clone, Copy, Debug)]
enum Shape {
    Plain,
    Stdio,
    OpenCode,
    Copilot,
    Zed,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    /// `claude mcp add/remove`; status read from `mcpServers` in the file.
    ClaudeCli,
    Json(&'static str, Shape),
    Toml,
}

#[derive(Debug)]
struct Agent {
    id: &'static str,
    name: &'static str,
    base: Base,
    file: &'static str,
    kind: Kind,
}

const fn agent(id: &'static str, name: &'static str, base: Base, file: &'static str, kind: Kind) -> Agent {
    Agent { id, name, base, file, kind }
}

use {Base::*, Kind::*, Shape::*};
const AGENTS: &[Agent] = &[
    agent("claude-code", "Claude Code", Home, ".claude.json", ClaudeCli),
    agent("claude-desktop", "Claude Desktop", Config, "Claude/claude_desktop_config.json", Json("mcpServers", Plain)),
    agent("cursor", "Cursor", Home, ".cursor/mcp.json", Json("mcpServers", Plain)),
    agent("vscode", "VS Code", Config, "Code/User/mcp.json", Json("servers", Stdio)),
    agent("windsurf", "Windsurf", Home, ".codeium/windsurf/mcp_config.json", Json("mcpServers", Plain)),
    agent("gemini", "Gemini CLI", Home, ".gemini/settings.json", Json("mcpServers", Plain)),
    agent("codex", "Codex", Home, ".codex/config.toml", Toml),
    agent("opencode", "opencode", Home, ".config/opencode/opencode.json", Json("mcp", OpenCode)),
    agent("copilot", "GitHub Copilot CLI", Home, ".copilot/mcp-config.json", Json("mcpServers", Copilot)),
    agent("zed", "Zed", Config, "zed/settings.json", Json("context_servers", Zed)),
    agent("lmstudio", "LM Studio", Home, ".lmstudio/mcp.json", Json("mcpServers", Plain)),
];

/// Where one host (native or a WSL distro) keeps its configs, and the command to write.
#[derive(Clone, Debug)]
struct Env {
    home: PathBuf,
    config: PathBuf,
    exe: String,
    distro: Option<String>,
    claude: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub host: String,
    pub config: String,
    pub status: Status,
    agent: &'static Agent,
    env: Env,
    path: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Installed,
    Outdated,
    NotInstalled,
    Error(String),
}

/// Agents present on this machine (and, on Windows, in WSL distros). May be slow; run it on a thread.
pub fn detect() -> Vec<Target> {
    let Ok(exe) = exe() else { return Vec::new() };
    let mut out = Vec::new();
    if let (Some(home), Some(config)) = (dirs::home_dir(), dirs::config_dir()) {
        let claude = which("claude");
        out.extend(detect_in(&Env { home, config, exe: exe.clone(), distro: None, claude }));
    }
    #[cfg(windows)]
    for e in wsl_envs(&exe) {
        out.extend(detect_in(&e));
    }
    out
}

pub fn install(t: &Target) -> Result<()> {
    match t.agent.kind {
        ClaudeCli => {
            if current(t.agent.kind, &t.path)?.is_some() {
                claude(t, &["mcp", "remove", "-s", "user", NAME])?;
            }
            claude(t, &["mcp", "add", "-s", "user", NAME, "--", &t.env.exe, "mcp"])
        }
        _ => edit(t, true),
    }
}

pub fn remove(t: &Target) -> Result<()> {
    match t.agent.kind {
        ClaudeCli if current(t.agent.kind, &t.path)?.is_none() => Ok(()),
        ClaudeCli => claude(t, &["mcp", "remove", "-s", "user", NAME]),
        _ => edit(t, false),
    }
}

/// `scriber agents [list | add [ids..] | remove [ids..]]`
pub fn cli(args: &[String]) -> Result<()> {
    let targets = detect();
    match args.first().map(String::as_str) {
        None | Some("list") => {
            let rows: Vec<[&str; 5]> = targets
                .iter()
                .map(|t| [&t.id, &t.name, &t.host, label(&t.status), &t.config][..].try_into().unwrap())
                .collect();
            let head = ["ID", "AGENT", "HOST", "STATUS", "CONFIG"];
            let w: Vec<usize> = (0..5).map(|i| rows.iter().map(|r| r[i].len()).chain([head[i].len()]).max().unwrap()).collect();
            for r in std::iter::once(&head).chain(&rows) {
                println!("{:<a$}  {:<b$}  {:<c$}  {:<d$}  {}", r[0], r[1], r[2], r[3], r[4], a = w[0], b = w[1], c = w[2], d = w[3]);
            }
            for t in &targets {
                if let Status::Error(e) = &t.status {
                    println!("\n{}: {e}", t.id);
                }
            }
            if targets.is_empty() {
                println!("No agents found.");
            }
            Ok(())
        }
        Some(op @ ("add" | "remove")) => {
            let ids = &args[1..];
            if let Some(id) = ids.iter().find(|id| !targets.iter().any(|t| &t.id == *id)) {
                bail!("agent not found: {id} (see `scriber agents list`)");
            }
            let mut failed = 0;
            for t in targets.iter().filter(|t| ids.is_empty() || ids.contains(&t.id)) {
                match if op == "add" { install(t) } else { remove(t) } {
                    Ok(()) => println!("{}: {}", t.id, if op == "add" { "added" } else { "removed" }),
                    Err(e) => {
                        failed += 1;
                        eprintln!("{}: {e:#}", t.id);
                    }
                }
            }
            if failed > 0 {
                bail!("{failed} agent(s) failed");
            }
            Ok(())
        }
        Some(other) => bail!("unknown command `{other}`; usage: scriber agents [list | add [ids..] | remove [ids..]]"),
    }
}

fn label(s: &Status) -> &'static str {
    match s {
        Status::Installed => "installed",
        Status::Outdated => "outdated",
        Status::NotInstalled => "not installed",
        Status::Error(_) => "error",
    }
}

fn detect_in(env: &Env) -> Vec<Target> {
    AGENTS
        .iter()
        .filter_map(|a| {
            let base = match a.base {
                Home => &env.home,
                Config => &env.config,
            };
            let path = a.file.split('/').fold(base.clone(), |p, c| p.join(c));
            let present = match a.kind {
                ClaudeCli => env.claude.is_some() || path.exists(),
                _ => path.parent().is_some_and(Path::is_dir),
            };
            present.then(|| Target {
                id: env.distro.as_ref().map_or(a.id.to_owned(), |d| format!("wsl:{d}:{}", a.id)),
                name: a.name.to_owned(),
                host: env.distro.as_ref().map_or("native".to_owned(), |d| format!("WSL: {d}")),
                config: path.display().to_string(),
                status: match current(a.kind, &path) {
                    Ok(None) => Status::NotInstalled,
                    Ok(Some(c)) if c == env.exe => Status::Installed,
                    Ok(Some(_)) => Status::Outdated,
                    Err(e) => Status::Error(format!("{e:#}")),
                },
                agent: a,
                env: env.clone(),
                path,
            })
        })
        .collect()
}

/// The command of the existing `scriber` entry, if any.
fn current(kind: Kind, path: &Path) -> Result<Option<String>> {
    let Some(text) = read(path)? else { return Ok(None) };
    let found = match kind {
        Toml => {
            let doc: toml_edit::DocumentMut = text.parse().with_context(|| format!("{} is not valid TOML", path.display()))?;
            let entry = doc.get("mcp_servers").and_then(|s| s.get(NAME));
            entry.map(|e| e.get("command").and_then(|c| c.as_str()).unwrap_or_default().to_owned())
        }
        Json(..) | ClaudeCli if !text.trim().is_empty() => {
            let key = if let Json(key, _) = kind { key } else { "mcpServers" };
            let v: Value = serde_json::from_str(&text).with_context(|| format!("{} is not plain JSON", path.display()))?;
            v.get(key).and_then(|s| s.get(NAME)).map(|e| {
                let c = e.get("command");
                // opencode keeps the command and its args in one array.
                c.and_then(|c| c.as_str().or_else(|| c.get(0)?.as_str())).unwrap_or_default().to_owned()
            })
        }
        _ => None,
    };
    Ok(found)
}

fn read(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn entry(shape: Shape, exe: &str) -> Value {
    match shape {
        Plain => json!({ "command": exe, "args": ["mcp"] }),
        Stdio => json!({ "type": "stdio", "command": exe, "args": ["mcp"] }),
        OpenCode => json!({ "type": "local", "command": [exe, "mcp"], "enabled": true }),
        Copilot => json!({ "type": "local", "command": exe, "args": ["mcp"], "tools": ["*"] }),
        Zed => json!({ "source": "custom", "command": exe, "args": ["mcp"] }),
    }
}

/// What to paste by hand when we cannot edit the file.
fn snippet(kind: Kind, exe: &str) -> String {
    match kind {
        Json(key, shape) => {
            let v = json!({ key: { NAME: entry(shape, exe) } });
            serde_json::to_string_pretty(&v).unwrap()
        }
        _ => format!("[mcp_servers.{NAME}]\ncommand = {}\nargs = [\"mcp\"]", toml_edit::Value::from(exe)),
    }
}

fn edit(t: &Target, add: bool) -> Result<()> {
    let (kind, path, exe) = (t.agent.kind, &t.path, t.env.exe.as_str());
    let new = (|| {
        let text = read(path)?;
        match kind {
            Json(key, shape) => edit_json(text.as_deref(), key, add.then(|| entry(shape, exe))),
            _ => edit_toml(text.as_deref(), add.then_some(exe)),
        }
    })()
    .with_context(|| format!("{} was left unchanged; add this by hand:\n{}", path.display(), snippet(kind, exe)))?;
    let Some(new) = new else { return Ok(()) };
    if path.exists() {
        let bak = PathBuf::from(format!("{}.{NAME}.bak", path.display()));
        if !bak.exists() {
            fs::copy(path, &bak).with_context(|| format!("cannot back up {}", path.display()))?;
        }
    } else if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, new).with_context(|| format!("cannot write {}", path.display()))
}

/// New file contents, or `None` when nothing changes.
fn edit_json(text: Option<&str>, key: &str, entry: Option<Value>) -> Result<Option<String>> {
    let text = text.filter(|t| !t.trim().is_empty());
    if text.is_none() && entry.is_none() {
        return Ok(None);
    }
    let mut root: Value = match text {
        Some(t) => serde_json::from_str(t).context("not plain JSON (comments or a syntax error)")?,
        None => json!({}),
    };
    let obj = root.as_object_mut().context("top level is not a JSON object")?;
    match entry {
        Some(e) => {
            let servers = obj.entry(key).or_insert_with(|| Value::Object(Map::new()));
            let servers = servers.as_object_mut().with_context(|| format!("`{key}` is not an object"))?;
            if servers.get(NAME) == Some(&e) {
                return Ok(None);
            }
            servers.insert(NAME.to_owned(), e);
        }
        None => {
            if obj.get_mut(key).and_then(Value::as_object_mut).and_then(|s| s.shift_remove(NAME)).is_none() {
                return Ok(None);
            }
        }
    }
    // Keep the file's indentation.
    let indent = text
        .and_then(|t| t.lines().find_map(|l| Some(&l[..l.len() - l.trim_start().len()]).filter(|i| !i.is_empty())))
        .unwrap_or("  ");
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    serde::Serialize::serialize(&root, &mut serde_json::Serializer::with_formatter(&mut out, fmt))?;
    out.push(b'\n');
    Ok(Some(String::from_utf8(out)?))
}

fn edit_toml(text: Option<&str>, exe: Option<&str>) -> Result<Option<String>> {
    use toml_edit::{Array, Item, Table, value};
    let mut doc: toml_edit::DocumentMut = text.unwrap_or_default().parse().context("not valid TOML")?;
    match exe {
        Some(exe) => {
            let servers = doc.entry("mcp_servers").or_insert_with(|| {
                let mut t = Table::new();
                t.set_implicit(true);
                Item::Table(t)
            });
            let servers = servers.as_table_mut().context("`mcp_servers` is not a table")?;
            let ours = servers.entry(NAME).or_insert_with(|| Item::Table(Table::new()));
            let ours = ours.as_table_mut().context("`mcp_servers.scriber` is not a table")?;
            if ours.get("command").and_then(Item::as_str) == Some(exe) {
                return Ok(None);
            }
            ours["command"] = value(exe);
            ours["args"] = value(Array::from_iter(["mcp"]));
        }
        None => {
            if doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut).and_then(|s| s.remove(NAME)).is_none() {
                return Ok(None);
            }
        }
    }
    Ok(Some(doc.to_string()))
}

fn claude(t: &Target, args: &[&str]) -> Result<()> {
    let mut cmd = match &t.env.distro {
        // Login + interactive shell, so PATH from mise/nvm is set.
        Some(d) => {
            let line: Vec<String> = args.iter().map(|a| format!("'{}'", a.replace('\'', r"'\''"))).collect();
            wsl(d, &["bash", "-lic", &format!("claude {}", line.join(" "))])
        }
        None => {
            let mut c = command(t.env.claude.as_ref().context("the `claude` command was not found on PATH")?);
            c.args(args);
            c
        }
    };
    let out = cmd.output().context("cannot run claude")?;
    if !out.status.success() {
        bail!(
            "claude {} failed: {}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim(),
            String::from_utf8_lossy(&out.stdout).trim()
        );
    }
    Ok(())
}

/// A command that does not flash a console window on Windows.
fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000); // CREATE_NO_WINDOW
    c
}

fn wsl(distro: &str, args: &[&str]) -> Command {
    let mut c = command("wsl.exe");
    c.args(["-d", distro, "-e"]).args(args);
    c
}

#[cfg(windows)]
fn wsl_envs(exe: &str) -> Vec<Env> {
    let Ok(out) = command("wsl.exe").args(["-l", "-q"]).output() else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let wide: Vec<u16> = out.stdout.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let list = String::from_utf16_lossy(&wide).replace(['\0', '\u{feff}'], "");
    list.lines()
        .map(str::trim)
        .filter(|d| !d.is_empty() && !d.starts_with("docker-desktop"))
        .filter_map(|d| {
            let run = |args: &[&str]| {
                let o = wsl(d, args).output().ok().filter(|o| o.status.success())?;
                Some(String::from_utf8_lossy(&o.stdout).trim().to_owned()).filter(|s| !s.is_empty())
            };
            let home = run(&["sh", "-c", "echo $HOME"])?;
            let exe = run(&["wslpath", "-u", exe])?;
            let home = PathBuf::from(format!(r"\\wsl.localhost\{d}{}", home.replace('/', "\\")));
            Some(Env { config: home.join(".config"), home, exe, distro: Some(d.to_owned()), claude: None })
        })
        .collect()
}

fn exe() -> Result<String> {
    let s = env::current_exe()?.canonicalize()?.to_string_lossy().into_owned();
    // canonicalize gives `\\?\C:\...` on Windows; agents want a plain path.
    Ok(s.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(s))
}

fn which(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    env::split_paths(&env::var_os("PATH")?)
        .flat_map(|dir| exts.iter().map(move |e| dir.join(format!("{name}{e}"))))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agents_round_trip_in_temp_home() {
        let home = env::temp_dir().join(format!("scriber-agents-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let put = |rel: &str, s: &str| {
            let p = home.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, s).unwrap();
        };
        put(".cursor/mcp.json", "{\n    \"mcpServers\": {\n        \"lerd\": {\"command\": \"lerd\"}\n    },\n    \"z\": 1\n}\n");
        put(".codex/config.toml", "# keep me\nmodel = \"x\"\n\n[mcp_servers.lerd]\ncommand = \"lerd\"\n");
        put(".config/zed/settings.json", "// comment\n{}\n");
        put(".claude.json", r#"{"mcpServers":{"scriber":{"command":"/old"}}}"#);
        put(".gemini/settings.json", r#"{"mcpServers":{"scriber":{"command":"/opt/scriber"}}}"#);
        fs::create_dir_all(home.join(".config/opencode")).unwrap();

        let env = Env { config: home.join(".config"), home: home.clone(), exe: "/opt/scriber".into(), distro: None, claude: None };
        let status = |id: &str| detect_in(&env).into_iter().find(|t| t.id == id).map(|t| t.status);
        let ids: Vec<String> = detect_in(&env).into_iter().map(|t| t.id).collect();
        assert_eq!(ids, ["claude-code", "cursor", "gemini", "codex", "opencode", "zed"]);
        assert_eq!(status("claude-code"), Some(Status::Outdated));
        assert_eq!(status("gemini"), Some(Status::Installed));
        assert!(matches!(status("zed"), Some(Status::Error(_))));

        for t in detect_in(&env) {
            match t.id.as_str() {
                "zed" => assert!(format!("{:#}", install(&t).unwrap_err()).contains("\"context_servers\"")),
                "claude-code" => {}
                _ => install(&t).unwrap(),
            }
        }
        for id in ["cursor", "codex", "opencode"] {
            assert_eq!(status(id), Some(Status::Installed), "{id}");
        }
        let cursor = fs::read_to_string(home.join(".cursor/mcp.json")).unwrap();
        assert!(cursor.starts_with("{\n    \"mcpServers\": {\n        \"lerd\""), "{cursor}");
        assert!(cursor.find("scriber").unwrap() < cursor.find("\"z\"").unwrap());
        assert!(home.join(".cursor/mcp.json.scriber.bak").exists());
        assert!(!home.join(".config/opencode/opencode.json.scriber.bak").exists());
        let codex = fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.starts_with("# keep me") && codex.contains("[mcp_servers.scriber]\ncommand = \"/opt/scriber\""), "{codex}");
        let oc = fs::read_to_string(home.join(".config/opencode/opencode.json")).unwrap();
        assert!(oc.contains("\"command\": [\n        \"/opt/scriber\""), "{oc}");

        for t in detect_in(&env).iter().filter(|t| !matches!(t.id.as_str(), "zed" | "claude-code")) {
            remove(t).unwrap();
        }
        for id in ["cursor", "codex", "opencode", "gemini"] {
            assert_eq!(status(id), Some(Status::NotInstalled), "{id}");
        }
        assert!(fs::read_to_string(home.join(".cursor/mcp.json")).unwrap().contains("lerd"));
        assert!(fs::read_to_string(home.join(".codex/config.toml")).unwrap().contains("[mcp_servers.lerd]"));
        fs::remove_dir_all(&home).unwrap();
    }
}
