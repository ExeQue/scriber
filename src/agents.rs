//! Detect AI agents on this machine and add or remove Scriber's MCP entry in their config.

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs, io};

const NAME: &str = "scriber";

/// The Agent Skill installed next to the MCP entry, so agents know how to work with Scriber.
pub const SKILL: &str = include_str!("../skills/scriber/SKILL.md");

/// Where agents that load Agent Skills keep the user's skills, relative to the home directory.
/// The skill goes to `<dir>/scriber/SKILL.md`. Agents not listed have no skill support.
const SKILL_DIRS: &[(&str, &str)] = &[("claude-code", ".claude/skills"), ("codex", ".codex/skills")];

#[derive(Clone, Copy, Debug)]
enum Base {
    Home,
    Config,
}

/// Shape of the entry inside a JSON config.
#[derive(Clone, Copy, Debug)]
enum Shape {
    /// Claude Desktop: its config file only takes stdio servers (remote ones go through Settings > Connectors).
    Desktop,
    Plain,
    VsCode,
    Windsurf,
    Gemini,
    OpenCode,
    Copilot,
    Zed,
}

/// What an entry points at: the editor's URL, or the `scriber mcp` relay.
#[derive(Clone, Debug, PartialEq)]
enum Conn {
    Url(String),
    Exe(String),
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
    agent("claude-desktop", "Claude Desktop", Config, "Claude/claude_desktop_config.json", Json("mcpServers", Desktop)),
    agent("cursor", "Cursor", Home, ".cursor/mcp.json", Json("mcpServers", Plain)),
    agent("vscode", "VS Code", Config, "Code/User/mcp.json", Json("servers", VsCode)),
    agent("windsurf", "Windsurf", Home, ".codeium/windsurf/mcp_config.json", Json("mcpServers", Windsurf)),
    agent("gemini", "Gemini CLI", Home, ".gemini/settings.json", Json("mcpServers", Gemini)),
    agent("codex", "Codex", Home, ".codex/config.toml", Toml),
    agent("opencode", "opencode", Home, ".config/opencode/opencode.json", Json("mcp", OpenCode)),
    agent("copilot", "GitHub Copilot CLI", Home, ".copilot/mcp-config.json", Json("mcpServers", Copilot)),
    agent("zed", "Zed", Config, "zed/settings.json", Json("context_servers", Zed)),
    agent("lmstudio", "LM Studio", Home, ".lmstudio/mcp.json", Json("mcpServers", Plain)),
];

/// Where one host (native or a WSL distro) keeps its configs, and what to write.
#[derive(Clone, Debug)]
struct Env {
    home: PathBuf,
    config: PathBuf,
    exe: String,
    port: u16,
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
    /// The skill's state; `None` when the agent does not load skills.
    pub skill: Option<Status>,
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
        out.extend(detect_in(&Env { home, config, exe: exe.clone(), port: crate::mcp::port(), distro: None, claude }));
    }
    #[cfg(windows)]
    for e in wsl_envs(&exe) {
        out.extend(detect_in(&e));
    }
    out
}

/// Add or update the entry; says how the agent connects ("over HTTP" or "through the stdio relay").
pub fn install(t: &Target) -> Result<&'static str> {
    let conn = conn(t);
    match (t.agent.kind, &conn) {
        (ClaudeCli, conn) => {
            if current(t.agent.kind, &t.path)?.is_some() {
                claude(t, &["mcp", "remove", "-s", "user", NAME])?;
            }
            match conn {
                Conn::Url(u) => claude(t, &["mcp", "add", "--transport", "http", "-s", "user", NAME, u])?,
                Conn::Exe(e) => claude(t, &["mcp", "add", "-s", "user", NAME, "--", e, "mcp"])?,
            }
        }
        (_, conn) => edit(t, Some(conn))?,
    }
    install_skill(t)?;
    Ok(if matches!(conn, Conn::Url(_)) { "over HTTP" } else { "through the stdio relay" })
}

pub fn remove(t: &Target) -> Result<()> {
    match t.agent.kind {
        ClaudeCli if current(t.agent.kind, &t.path)?.is_none() => {}
        ClaudeCli => claude(t, &["mcp", "remove", "-s", "user", NAME])?,
        _ => edit(t, None)?,
    }
    remove_skill(t)
}

fn skill_path(agent: &Agent, home: &Path) -> Option<PathBuf> {
    let (_, dir) = SKILL_DIRS.iter().find(|(id, _)| *id == agent.id)?;
    Some(dir.split('/').fold(home.to_path_buf(), |p, c| p.join(c)).join(NAME).join("SKILL.md"))
}

/// Write (or overwrite, so updates ship) the skill for agents that load skills.
fn install_skill(t: &Target) -> Result<()> {
    let Some(path) = skill_path(t.agent, &t.env.home) else { return Ok(()) };
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, SKILL).with_context(|| format!("cannot write {}", path.display()))
}

fn remove_skill(t: &Target) -> Result<()> {
    let Some(path) = skill_path(t.agent, &t.env.home) else { return Ok(()) };
    match fs::remove_file(&path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e).with_context(|| format!("cannot remove {}", path.display())),
        _ => {}
    }
    // Only when empty: the person may keep their own files there.
    let _ = fs::remove_dir(path.parent().unwrap());
    Ok(())
}

fn skill_status(agent: &Agent, home: &Path) -> Option<Status> {
    let path = skill_path(agent, home)?;
    Some(match read(&path) {
        Ok(None) => Status::NotInstalled,
        Ok(Some(s)) if s == SKILL => Status::Installed,
        Ok(Some(_)) => Status::Outdated,
        Err(e) => Status::Error(format!("{e:#}")),
    })
}

/// HTTP when the agent takes a URL and (in WSL) reaches the editor's port; the stdio relay otherwise.
fn conn(t: &Target) -> Conn {
    let reachable = || t.env.distro.as_deref().is_none_or(|d| wsl_reaches(d, t.env.port));
    if http(t.agent.kind) && reachable() { Conn::Url(crate::mcp::url(t.env.port)) } else { Conn::Exe(t.env.exe.clone()) }
}

fn http(kind: Kind) -> bool {
    !matches!(kind, Json(_, Desktop))
}

/// Whether the distro reaches 127.0.0.1:port of this machine (WSL mirrored networking does, NAT does not).
fn wsl_reaches(distro: &str, port: u16) -> bool {
    // When the editor is not running, stand in for it so the probe still has something to reach.
    let _stand_in = std::net::TcpListener::bind(("127.0.0.1", port));
    let probe = format!("exec 3<>/dev/tcp/127.0.0.1/{port}");
    wsl(distro, &["timeout", "5", "bash", "-c", &probe]).status().is_ok_and(|s| s.success())
}

/// `scriber agents [list | add [ids..] | remove [ids..]]`
pub fn cli(args: &[String]) -> Result<()> {
    let targets = detect();
    match args.first().map(String::as_str) {
        None | Some("list") => {
            let rows: Vec<[&str; 6]> = targets
                .iter()
                .map(|t| [t.id.as_str(), &t.name, &t.host, label(&t.status), t.skill.as_ref().map_or("not supported", label), &t.config][..].try_into().unwrap())
                .collect();
            let head = ["ID", "AGENT", "HOST", "STATUS", "SKILL", "CONFIG"];
            let w: Vec<usize> = (0..6).map(|i| rows.iter().map(|r| r[i].len()).chain([head[i].len()]).max().unwrap()).collect();
            for r in std::iter::once(&head).chain(&rows) {
                println!("{:<a$}  {:<b$}  {:<c$}  {:<d$}  {:<e$}  {}", r[0], r[1], r[2], r[3], r[4], r[5], a = w[0], b = w[1], c = w[2], d = w[3], e = w[4]);
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
                let done = if op == "add" { install(t).map(|how| format!("added {how}")) } else { remove(t).map(|()| "removed".to_owned()) };
                match done {
                    Ok(done) => println!("{}: {done}", t.id),
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
            let skill = skill_status(a, &env.home);
            present.then(|| Target {
                id: env.distro.as_ref().map_or(a.id.to_owned(), |d| format!("wsl:{d}:{}", a.id)),
                name: a.name.to_owned(),
                host: env.distro.as_ref().map_or("native".to_owned(), |d| format!("WSL: {d}")),
                config: path.display().to_string(),
                status: match current(a.kind, &path) {
                    Ok(None) => Status::NotInstalled,
                    // A missing or older skill makes the agent outdated, so Connect installs the current one.
                    Ok(Some(c)) if ours(&c, a.kind, env) && skill.as_ref().is_none_or(|s| *s == Status::Installed) => Status::Installed,
                    Ok(Some(c)) if ours(&c, a.kind, env) => Status::Outdated,
                    Ok(Some(_)) => Status::Outdated,
                    Err(e) => Status::Error(format!("{e:#}")),
                },
                skill,
                agent: a,
                env: env.clone(),
                path,
            })
        })
        .collect()
}

/// An entry is up to date when it points at this Scriber in the form `install` writes: the URL, or the
/// relay for stdio-only agents. In WSL either form counts, since the relay is the fallback there.
fn ours(c: &Conn, kind: Kind, env: &Env) -> bool {
    match c {
        Conn::Url(u) => http(kind) && *u == crate::mcp::url(env.port),
        Conn::Exe(e) => *e == env.exe && (!http(kind) || env.distro.is_some()),
    }
}

/// What the existing `scriber` entry points at, if there is one.
fn current(kind: Kind, path: &Path) -> Result<Option<Conn>> {
    let Some(text) = read(path)? else { return Ok(None) };
    let found = match kind {
        Toml => {
            let doc: toml_edit::DocumentMut = text.parse().with_context(|| format!("{} is not valid TOML", path.display()))?;
            let entry = doc.get("mcp_servers").and_then(|s| s.get(NAME));
            entry.map(|e| match e.get("url").and_then(|u| u.as_str()) {
                Some(u) => Conn::Url(u.to_owned()),
                None => Conn::Exe(e.get("command").and_then(|c| c.as_str()).unwrap_or_default().to_owned()),
            })
        }
        Json(..) | ClaudeCli if !text.trim().is_empty() => {
            let key = if let Json(key, _) = kind { key } else { "mcpServers" };
            let v: Value = serde_json::from_str(&text).with_context(|| format!("{} is not plain JSON", path.display()))?;
            v.get(key).and_then(|s| s.get(NAME)).map(|e| {
                if let Some(u) = ["url", "serverUrl", "httpUrl"].iter().find_map(|k| e.get(k)?.as_str()) {
                    return Conn::Url(u.to_owned());
                }
                let c = e.get("command");
                // opencode keeps the command and its args in one array.
                Conn::Exe(c.and_then(|c| c.as_str().or_else(|| c.get(0)?.as_str())).unwrap_or_default().to_owned())
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

fn entry(shape: Shape, conn: &Conn) -> Value {
    use Conn::{Exe, Url};
    match (shape, conn) {
        (Desktop | Plain | Windsurf | Gemini, Exe(exe)) => json!({ "command": exe, "args": ["mcp"] }),
        (Desktop, Url(_)) => unreachable!("Claude Desktop only takes stdio servers"),
        (Plain, Url(u)) => json!({ "url": u }),
        (Windsurf, Url(u)) => json!({ "serverUrl": u }),
        (Gemini, Url(u)) => json!({ "httpUrl": u }),
        (VsCode, Exe(exe)) => json!({ "type": "stdio", "command": exe, "args": ["mcp"] }),
        (VsCode, Url(u)) => json!({ "type": "http", "url": u }),
        (OpenCode, Exe(exe)) => json!({ "type": "local", "command": [exe, "mcp"], "enabled": true }),
        (OpenCode, Url(u)) => json!({ "type": "remote", "url": u, "enabled": true }),
        (Copilot, Exe(exe)) => json!({ "type": "local", "command": exe, "args": ["mcp"], "tools": ["*"] }),
        (Copilot, Url(u)) => json!({ "type": "http", "url": u, "tools": ["*"] }),
        (Zed, Exe(exe)) => json!({ "source": "custom", "command": exe, "args": ["mcp"] }),
        // Remote servers in Zed are `{ "url": … }` (https://zed.dev/docs/ai/mcp).
        (Zed, Url(u)) => json!({ "url": u }),
    }
}

/// What to paste by hand when we cannot edit the file.
fn snippet(kind: Kind, conn: &Conn) -> String {
    match (kind, conn) {
        (Json(key, shape), _) => {
            let v = json!({ key: { NAME: entry(shape, conn) } });
            serde_json::to_string_pretty(&v).unwrap()
        }
        (_, Conn::Url(u)) => format!("[mcp_servers.{NAME}]\nurl = {}", toml_edit::Value::from(u.as_str())),
        (_, Conn::Exe(e)) => format!("[mcp_servers.{NAME}]\ncommand = {}\nargs = [\"mcp\"]", toml_edit::Value::from(e.as_str())),
    }
}

fn edit(t: &Target, conn: Option<&Conn>) -> Result<()> {
    let (kind, path) = (t.agent.kind, &t.path);
    let new = (|| {
        let text = read(path)?;
        match kind {
            Json(key, shape) => edit_json(text.as_deref(), key, conn.map(|c| entry(shape, c))),
            _ => edit_toml(text.as_deref(), conn),
        }
    })()
    .with_context(|| {
        let paste = snippet(kind, conn.unwrap_or(&Conn::Exe(t.env.exe.clone())));
        format!("{} was left unchanged; add this by hand:\n{paste}", path.display())
    })?;
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

fn edit_toml(text: Option<&str>, conn: Option<&Conn>) -> Result<Option<String>> {
    use toml_edit::{Array, Item, Table, value};
    let mut doc: toml_edit::DocumentMut = text.unwrap_or_default().parse().context("not valid TOML")?;
    match conn {
        Some(conn) => {
            let servers = doc.entry("mcp_servers").or_insert_with(|| {
                let mut t = Table::new();
                t.set_implicit(true);
                Item::Table(t)
            });
            let servers = servers.as_table_mut().context("`mcp_servers` is not a table")?;
            let ours = servers.entry(NAME).or_insert_with(|| Item::Table(Table::new()));
            let ours = ours.as_table_mut().context("`mcp_servers.scriber` is not a table")?;
            let get = |k| ours.get(k).and_then(Item::as_str);
            match conn {
                Conn::Url(u) if get("url") == Some(u) && get("command").is_none() => return Ok(None),
                Conn::Exe(e) if get("command") == Some(e) && get("url").is_none() => return Ok(None),
                Conn::Url(u) => {
                    ours.remove("command");
                    ours.remove("args");
                    ours["url"] = value(u);
                }
                Conn::Exe(e) => {
                    ours.remove("url");
                    ours["command"] = value(e);
                    ours["args"] = value(Array::from_iter(["mcp"]));
                }
            }
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
            Some(Env { config: home.join(".config"), home, exe, port: crate::mcp::port(), distro: Some(d.to_owned()), claude: None })
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
        let url = "http://127.0.0.1:47999/mcp";
        put(".cursor/mcp.json", "{\n    \"mcpServers\": {\n        \"lerd\": {\"command\": \"lerd\"}\n    },\n    \"z\": 1\n}\n");
        put(".codex/config.toml", "# keep me\nmodel = \"x\"\n\n[mcp_servers.lerd]\ncommand = \"lerd\"\n\n[mcp_servers.scriber]\ncommand = \"/opt/scriber\"\nargs = [\"mcp\"]\n");
        put(".config/zed/settings.json", "// comment\n{}\n");
        put(".config/Claude/claude_desktop_config.json", "{}");
        put(".claude.json", r#"{"mcpServers":{"scriber":{"type":"http","url":"http://127.0.0.1:47999/mcp"}}}"#);
        put(".gemini/settings.json", r#"{"mcpServers":{"scriber":{"command":"/opt/scriber"}}}"#);
        fs::create_dir_all(home.join(".config/opencode")).unwrap();

        let env = Env { config: home.join(".config"), home: home.clone(), exe: "/opt/scriber".into(), port: 47999, distro: None, claude: None };
        let status = |env: &Env, id: &str| detect_in(env).into_iter().find(|t| t.id == id).map(|t| t.status);
        let ids: Vec<String> = detect_in(&env).into_iter().map(|t| t.id).collect();
        assert_eq!(ids, ["claude-code", "claude-desktop", "cursor", "gemini", "codex", "opencode", "zed"]);
        // The MCP entry is current, but the skill is missing.
        assert_eq!(status(&env, "claude-code"), Some(Status::Outdated));
        // Stdio entries of HTTP-capable agents are upgraded by Connect.
        assert_eq!(status(&env, "gemini"), Some(Status::Outdated));
        assert_eq!(status(&env, "codex"), Some(Status::Outdated));
        assert!(matches!(status(&env, "zed"), Some(Status::Error(_))));

        for t in detect_in(&env) {
            match t.id.as_str() {
                "zed" => assert!(format!("{:#}", install(&t).unwrap_err()).contains(&format!("\"url\": \"{url}\""))),
                "claude-code" => {}
                "claude-desktop" => assert_eq!(install(&t).unwrap(), "through the stdio relay"),
                _ => assert_eq!(install(&t).unwrap(), "over HTTP"),
            }
        }
        for id in ["claude-desktop", "cursor", "gemini", "codex", "opencode"] {
            assert_eq!(status(&env, id), Some(Status::Installed), "{id}");
        }
        let cursor = fs::read_to_string(home.join(".cursor/mcp.json")).unwrap();
        assert!(cursor.starts_with("{\n    \"mcpServers\": {\n        \"lerd\""), "{cursor}");
        assert!(cursor.contains(&format!("\"scriber\": {{\n            \"url\": \"{url}\"")), "{cursor}");
        assert!(cursor.find("scriber").unwrap() < cursor.find("\"z\"").unwrap());
        assert!(home.join(".cursor/mcp.json.scriber.bak").exists());
        assert!(!home.join(".config/opencode/opencode.json.scriber.bak").exists());
        let codex = fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(codex.starts_with("# keep me") && codex.ends_with(&format!("[mcp_servers.scriber]\nurl = \"{url}\"\n")), "{codex}");
        let gemini = fs::read_to_string(home.join(".gemini/settings.json")).unwrap();
        assert!(gemini.contains("\"httpUrl\"") && !gemini.contains("command"), "{gemini}");
        let oc = fs::read_to_string(home.join(".config/opencode/opencode.json")).unwrap();
        assert!(oc.contains("\"type\": \"remote\"") && oc.contains(url), "{oc}");
        let desktop = fs::read_to_string(home.join(".config/Claude/claude_desktop_config.json")).unwrap();
        assert!(desktop.contains("\"command\": \"/opt/scriber\""), "{desktop}");

        for t in detect_in(&env).iter().filter(|t| !matches!(t.id.as_str(), "zed" | "claude-code")) {
            remove(t).unwrap();
        }
        for id in ["claude-desktop", "cursor", "codex", "opencode", "gemini"] {
            assert_eq!(status(&env, id), Some(Status::NotInstalled), "{id}");
        }
        assert!(fs::read_to_string(home.join(".cursor/mcp.json")).unwrap().contains("lerd"));
        assert!(fs::read_to_string(home.join(".codex/config.toml")).unwrap().contains("[mcp_servers.lerd]"));

        // A WSL distro that cannot reach the port falls back to the relay, which counts as connected there.
        let wsl = Env { distro: Some("scriber-test-no-such-distro".into()), ..env.clone() };
        let cursor = detect_in(&wsl).into_iter().find(|t| t.id.ends_with(":cursor")).unwrap();
        assert_eq!(install(&cursor).unwrap(), "through the stdio relay");
        assert_eq!(status(&wsl, "wsl:scriber-test-no-such-distro:cursor"), Some(Status::Installed));
        assert_eq!(status(&env, "cursor"), Some(Status::Outdated));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn skill_installs_and_removes_with_the_entry() {
        let home = env::temp_dir().join(format!("scriber-skill-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::create_dir_all(home.join(".cursor")).unwrap();
        fs::write(home.join(".claude.json"), r#"{"mcpServers":{"scriber":{"type":"http","url":"http://127.0.0.1:47999/mcp"}}}"#).unwrap();
        fs::create_dir_all(home.join(".claude/skills/scriber")).unwrap();
        fs::write(home.join(".claude/skills/scriber/SKILL.md"), "old").unwrap();
        let env = Env { config: home.join(".config"), home: home.clone(), exe: "/opt/scriber".into(), port: 47999, distro: None, claude: None };
        let find = |id: &str| detect_in(&env).into_iter().find(|t| t.id == id).unwrap();
        let claude_skill = home.join(".claude/skills/scriber/SKILL.md");
        let codex_skill = home.join(".codex/skills/scriber/SKILL.md");

        assert_eq!(find("cursor").skill, None);
        let cc = find("claude-code");
        assert_eq!((cc.status, cc.skill), (Status::Outdated, Some(Status::Outdated)));
        // Claude Code's entry goes through the `claude` CLI, so try the skill part on its own.
        install_skill(&find("claude-code")).unwrap();
        assert_eq!(fs::read_to_string(&claude_skill).unwrap(), SKILL);
        let cc = find("claude-code");
        assert_eq!((cc.status.clone(), cc.skill.clone()), (Status::Installed, Some(Status::Installed)));
        fs::write(home.join(".claude/skills/scriber/notes.txt"), "mine").unwrap();
        remove_skill(&cc).unwrap();
        assert!(!claude_skill.exists() && home.join(".claude/skills/scriber/notes.txt").exists());

        assert_eq!(find("codex").skill, Some(Status::NotInstalled));
        install(&find("codex")).unwrap();
        assert_eq!(fs::read_to_string(&codex_skill).unwrap(), SKILL);
        assert_eq!(find("codex").status, Status::Installed);
        remove(&find("codex")).unwrap();
        assert!(!codex_skill.exists() && !codex_skill.parent().unwrap().exists());
        assert_eq!(find("codex").skill, Some(Status::NotInstalled));
        fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn skill_has_front_matter_and_only_real_tool_names() {
        use rmcp::ServerHandler;
        let body = SKILL.strip_prefix("---\n").expect("front matter");
        let (front, text) = body.split_once("\n---\n").expect("front matter end");
        let field = |k: &str| front.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix(':')).map(str::trim).unwrap_or_default();
        assert_eq!(field("name"), NAME);
        assert!(field("description").len() > 40);

        let doc = std::sync::Arc::new(parking_lot::Mutex::new(crate::model::Document::new(100.0, 100.0)));
        let server = crate::mcp::Server::new(crate::model::workspace::Workspace::from_shared(doc), std::sync::Arc::new(|| {}));
        // Backticked snake_case words are tool names, apart from these arguments and values.
        let not_tools = ["no_output", "use_work_area", "line_interval", "lines_per_cm", "timed_out", "preview_opacity", "burn_colour", "image_mode", "image_modes"];
        let mut tools = 0;
        for word in text.replace("```", "").split('`').skip(1).step_by(2) {
            if word.contains('_') && word.chars().all(|c| c.is_ascii_lowercase() || c == '_') && !not_tools.contains(&word) {
                assert!(server.get_tool(word).is_some(), "SKILL.md names `{word}`, which is not a Scriber tool");
                tools += 1;
            }
        }
        assert!(tools > 20, "{tools}");
    }
}

