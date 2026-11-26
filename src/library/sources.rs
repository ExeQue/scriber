//! Subscriptions to machine and material libraries published in GitHub repositories
//! (`docs/library-repositories.md`). A repository has `.scriber.json` at its root and the files in
//! `.scriber/machines/*.json` and `.scriber/materials/*.json`. The list lives in
//! `<config>/scriber/sources.json`; each source's files in `<config>/scriber/sources/<id>/<commit>/`.
//!
//! Fetching uses plain HTTPS (no git): the source's track (branch, tag, latest release or commit)
//! is resolved to a commit with one small API call; only when that commit differs from the pinned
//! one is the repository's zip downloaded, and only `.scriber.json` and `.scriber/**/*.json` are
//! extracted (size and count capped, unsafe paths refused). Nothing from a repository is ever run.

use super::{Base, Library, Machine, Material, Origin, exchange};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MANIFEST: &str = ".scriber.json";
pub const FOLDER: &str = ".scriber";
pub const VERSION: u32 = 1;
const MAX_ZIP: u64 = 50 << 20;
const MAX_JSON: u64 = 1 << 20;
const MAX_FILES: usize = 5000;
/// Automatic syncs at start check a source at most this often (seconds).
pub const AUTO_SYNC_EVERY: u64 = 24 * 3600;

/// `.scriber.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_version: Option<String>,
}

/// What a source follows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Track {
    /// New commits on this branch ("" = the repository's default branch).
    Branch(String),
    /// Locked to this tag's commit (re-checked on sync, in case the tag moved).
    Tag(String),
    /// GitHub's latest release ("latest"); a new release is an update.
    Release(String),
    /// Locked to this commit; sync does nothing.
    Commit(String),
}

impl Default for Track {
    fn default() -> Self {
        Track::Branch(String::new())
    }
}

impl Track {
    pub fn label(&self) -> String {
        match self {
            Track::Branch(b) if b.is_empty() => "default branch".into(),
            Track::Branch(b) => format!("branch {b}"),
            Track::Tag(t) => format!("tag {t}"),
            Track::Release(_) => "latest release".into(),
            Track::Commit(c) => format!("commit {}", short(c)),
        }
    }
}

fn yes() -> bool {
    true
}

/// One subscription in `sources.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Slug of owner/repo, e.g. "acme-laser-presets".
    pub id: String,
    /// https://github.com/owner/repo
    pub url: String,
    #[serde(default)]
    pub track: Track,
    /// The commit whose files are in use ("" = none yet).
    #[serde(default)]
    pub pinned_commit: String,
    /// The manifest's name.
    #[serde(default)]
    pub name: String,
    /// The release in use (track release), e.g. "v1.2 (Spring set)".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub release: String,
    /// When the pinned files were downloaded (Unix seconds).
    #[serde(default)]
    pub fetched_at: u64,
    /// When the track was last resolved successfully (Unix seconds).
    #[serde(default)]
    pub last_checked: u64,
    /// GitHub's ETag of the last resolve, so an unchanged answer (304) does not count against the rate limit.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub etag: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_error: String,
    /// What the last sync that changed something did, e.g. "+2 materials, 1 changed, 0 removed".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_sync: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl Source {
    pub fn folder(&self, dir: &Path) -> PathBuf {
        dir.join("sources").join(&self.id).join(&self.pinned_commit)
    }

    pub fn display_name(&self) -> &str {
        if self.name.is_empty() { &self.id } else { &self.name }
    }

    fn owner_repo(&self) -> Result<(String, String), String> {
        parse_url(&self.url).map(|(o, r, _)| (o, r))
    }
}

pub fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `$SCRIBER_CONFIG_DIR`, else `<config>/scriber`; tests get a temp folder.
pub fn config_dir() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("scriber-test-config-{}", std::process::id()));
    }
    match std::env::var_os("SCRIBER_CONFIG_DIR") {
        Some(d) => d.into(),
        None => dirs::config_dir().unwrap_or_default().join("scriber"),
    }
}

// ---------- the list ----------

static LOCK: Mutex<()> = Mutex::new(());

/// The subscriptions; a missing file is none, a broken one is reported and read as none.
pub fn list(dir: &Path) -> Vec<Source> {
    let path = dir.join("sources.json");
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
            eprintln!("scriber: could not read {}: {e}", path.display());
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

/// Change the list and save it (write-then-rename), one change at a time.
fn edit<T>(dir: &Path, f: impl FnOnce(&mut Vec<Source>) -> Result<T, String>) -> Result<T, String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut list = list(dir);
    let out = f(&mut list)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (path, tmp) = (dir.join("sources.json"), dir.join("sources.json.tmp"));
    std::fs::write(&tmp, serde_json::to_string_pretty(&list).unwrap()).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(out)
}

fn find<'a>(list: &'a mut [Source], id: &str) -> Result<&'a mut Source, String> {
    list.iter_mut().find(|s| s.id == id).ok_or(format!("no library source with id {id}"))
}

// ---------- addresses ----------

fn safe_name(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

fn safe_ref(s: &str) -> bool {
    !s.is_empty() && !s.contains("..") && !s.starts_with('/') && !s.ends_with('/') && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
}

/// `owner/repo`, `owner/repo@ref` or `https://github.com/owner/repo[/tree/ref]` → owner, repo, ref.
pub fn parse_url(input: &str) -> Result<(String, String, Option<String>), String> {
    let bad = || format!("\"{input}\" is not a GitHub repository; use owner/repo, owner/repo@ref or https://github.com/owner/repo");
    let s = input.trim();
    let (path, url) = match ["https://github.com/", "http://github.com/", "github.com/"].iter().find_map(|p| s.strip_prefix(p)) {
        Some(rest) => (rest.trim_end_matches('/'), true),
        None => (s, false),
    };
    let (owner, rest) = path.split_once('/').ok_or_else(bad)?;
    let (repo, git_ref) = if url {
        match rest.split_once('/') {
            None => (rest, None),
            Some((repo, tail)) => (repo, Some(tail.strip_prefix("tree/").ok_or_else(bad)?)),
        }
    } else {
        match rest.split_once('@') {
            None => (rest, None),
            Some((repo, r)) => (repo, Some(r)),
        }
    };
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if !safe_name(owner) || !safe_name(repo) || git_ref.is_some_and(|r| !safe_ref(r)) {
        return Err(bad());
    }
    Ok((owner.into(), repo.into(), git_ref.map(String::from)))
}

/// The id of a repository: "owner-repo", lowercase.
pub fn slug(owner: &str, repo: &str) -> String {
    format!("{owner}-{repo}").to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "-")
}

// ---------- HTTP ----------

pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub etag: String,
}

/// A GET over HTTPS; tests use a fake.
pub trait Http: Sync {
    fn get(&self, url: &str, accept: &str, etag: &str, limit: u64) -> Result<Reply, String>;
}

/// The real one (ureq with rustls). `GITHUB_TOKEN` from the environment goes to api.github.com only and is never stored.
pub struct Net;

impl Http for Net {
    fn get(&self, url: &str, accept: &str, etag: &str, limit: u64) -> Result<Reply, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(120)))
            .user_agent(concat!("scriber/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let mut req = agent.get(url).header("Accept", accept);
        if !etag.is_empty() {
            req = req.header("If-None-Match", etag);
        }
        if url.starts_with("https://api.github.com/")
            && let Ok(token) = std::env::var("GITHUB_TOKEN")
            && !token.trim().is_empty()
        {
            req = req.header("Authorization", format!("Bearer {}", token.trim()));
        }
        let mut resp = req.call().map_err(|e| format!("could not reach GitHub: {e}"))?;
        let status = resp.status().as_u16();
        let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        let body = resp.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| format!("download failed (at most {} MB): {e}", limit >> 20))?;
        Ok(Reply { status, body, etag })
    }
}

/// A GitHub API call; 304 passes through, other non-200 answers become errors.
fn api(http: &dyn Http, path: &str, accept: &str, etag: &str, what: &str) -> Result<Reply, String> {
    let r = http.get(&format!("https://api.github.com/repos/{path}"), accept, etag, 4 << 20)?;
    match r.status {
        200 | 304 => Ok(r),
        404 | 422 => Err(format!("{what} not found on GitHub (private repositories are not supported)")),
        403 | 429 => Err("GitHub's rate limit is reached; try again later (or set GITHUB_TOKEN)".into()),
        s => Err(format!("GitHub answered {s} for {what}")),
    }
}

fn json<T: serde::de::DeserializeOwned>(r: &Reply) -> Result<T, String> {
    serde_json::from_slice(&r.body).map_err(|e| format!("unexpected answer from GitHub: {e}"))
}

fn sha(r: &Reply) -> Result<String, String> {
    let s = String::from_utf8_lossy(&r.body).trim().to_string();
    if s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()) { Ok(s.to_lowercase()) } else { Err("unexpected answer from GitHub (no commit)".into()) }
}

struct Resolved {
    commit: String,
    etag: String,
    release: String,
}

/// The commit the track points at now; None = unchanged since `etag` (or a locked commit already in use).
fn resolve(http: &dyn Http, owner: &str, repo: &str, track: &Track, pinned: &str, etag: &str) -> Result<Option<Resolved>, String> {
    let commit_of = |r: &str, etag: &str| api(http, &format!("{owner}/{repo}/commits/{r}"), "application/vnd.github.sha", etag, &format!("{owner}/{repo} {r}"));
    let (git_ref, release) = match track {
        Track::Commit(c) if !pinned.is_empty() && pinned.starts_with(&c.to_lowercase()) => return Ok(None),
        Track::Release(_) => {
            #[derive(Deserialize)]
            struct Release {
                tag_name: String,
                #[serde(default)]
                name: Option<String>,
            }
            let r = api(http, &format!("{owner}/{repo}/releases/latest"), "application/vnd.github+json", etag, &format!("{owner}/{repo}'s latest release"))?;
            if r.status == 304 {
                return Ok(None);
            }
            let rel: Release = json(&r)?;
            let label = match rel.name.filter(|n| !n.is_empty() && *n != rel.tag_name) {
                Some(n) => format!("{} ({n})", rel.tag_name),
                None => rel.tag_name.clone(),
            };
            let c = commit_of(&rel.tag_name, "")?;
            return Ok(Some(Resolved { commit: sha(&c)?, etag: r.etag, release: label }));
        }
        Track::Branch(b) if b.is_empty() => ("HEAD".to_string(), ""),
        Track::Branch(r) | Track::Tag(r) | Track::Commit(r) => (r.clone(), ""),
    };
    if !safe_ref(&git_ref) {
        return Err(format!("\"{git_ref}\" is not a valid branch, tag or commit"));
    }
    let r = commit_of(&git_ref, etag)?;
    if r.status == 304 {
        return Ok(None);
    }
    Ok(Some(Resolved { commit: sha(&r)?, etag: r.etag, release: release.into() }))
}

/// Whether the repository has this tag (for `owner/repo@v1.2`: a tag when there is one, else a branch).
fn has_tag(http: &dyn Http, owner: &str, repo: &str, tag: &str) -> Result<bool, String> {
    let r = http.get(&format!("https://api.github.com/repos/{owner}/{repo}/git/ref/tags/{tag}"), "application/vnd.github+json", "", 1 << 20)?;
    match r.status {
        200 => Ok(true),
        404 => Ok(false),
        403 | 429 => Err("GitHub's rate limit is reached; try again later (or set GITHUB_TOKEN)".into()),
        s => Err(format!("GitHub answered {s}")),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Choices {
    Branches,
    Tags,
    Releases,
}

/// What a source can follow, for the picker: (value, label). Releases give their tag as the value.
pub fn choices(http: &dyn Http, url: &str, kind: Choices) -> Result<Vec<(String, String)>, String> {
    #[derive(Deserialize)]
    struct Named {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        tag_name: Option<String>,
    }
    let (owner, repo, _) = parse_url(url)?;
    let path = match kind {
        Choices::Branches => "branches?per_page=100",
        Choices::Tags => "tags?per_page=100",
        Choices::Releases => "releases?per_page=30",
    };
    let list: Vec<Named> = json(&api(http, &format!("{owner}/{repo}/{path}"), "application/vnd.github+json", "", &format!("{owner}/{repo}"))?)?;
    Ok(list
        .into_iter()
        .filter_map(|n| match (n.tag_name, n.name) {
            (Some(t), Some(name)) if !name.is_empty() && name != t => Some((t.clone(), format!("{name} ({t})"))),
            (Some(t), _) => Some((t.clone(), t)),
            (None, Some(name)) => Some((name.clone(), name)),
            _ => None,
        })
        .collect())
}

// ---------- the files ----------

/// Extract `.scriber.json` and `.scriber/**/*.json` from a zip: GitHub's zip of a commit (everything
/// in one top folder) or one with them at its root. Returns what was skipped (unsafe paths, too large).
pub fn extract(zip: &[u8], into: &Path) -> Result<Vec<String>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| format!("not a zip archive: {e}"))?;
    let (mut written, mut skipped) = (0, Vec::new());
    // GitHub puts everything in one top folder; a zip made by hand may have the files at its root.
    let top = !archive.file_names().any(|n| n.is_ok_and(|n| n == MANIFEST));
    for i in 0..archive.len() {
        let mut f = match archive.by_index(i) {
            Ok(f) => f,
            Err(e) => {
                skipped.push(format!("entry {i}: {e}"));
                continue;
            }
        };
        let Ok(name) = f.name().map(|n| n.to_string()) else {
            skipped.push(format!("entry {i}: unreadable name"));
            continue;
        };
        let rest = if top { name.split_once('/').map_or("", |(_, r)| r) } else { name.as_str() };
        if f.is_dir() || (rest != MANIFEST && !(rest.starts_with(".scriber/") && rest.ends_with(".json"))) {
            continue;
        }
        let parts: Vec<&str> = rest.split('/').collect();
        if f.is_symlink() || !parts.iter().all(|p| safe_name(p)) {
            skipped.push(format!("{rest}: unsafe path"));
            continue;
        }
        let mut buf = Vec::new();
        if f.size() > MAX_JSON || f.by_ref().take(MAX_JSON + 1).read_to_end(&mut buf).is_err() || buf.len() as u64 > MAX_JSON {
            skipped.push(format!("{rest}: larger than 1 MB or unreadable"));
            continue;
        }
        written += 1;
        if written > MAX_FILES {
            return Err(format!("more than {MAX_FILES} files"));
        }
        let path = parts.iter().fold(into.to_path_buf(), |p, c| p.join(c));
        std::fs::create_dir_all(path.parent().unwrap()).and_then(|_| std::fs::write(&path, buf)).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(skipped)
}

/// A repository's library, read from a folder that has `.scriber.json`.
#[derive(Debug, Default)]
pub struct Repo {
    pub manifest: Manifest,
    pub machines: Vec<Machine>,
    pub materials: Vec<Material>,
    /// Files that were skipped, and why.
    pub problems: Vec<String>,
}

pub fn read_repo(dir: &Path) -> Result<Repo, String> {
    let manifest = read_manifest(dir)?;
    let mut problems = Vec::new();
    let machines = items(&dir.join(FOLDER).join("machines"), Machine::check, |m| &m.id, &mut problems);
    let materials = items(&dir.join(FOLDER).join("materials"), Material::check, |m| &m.id, &mut problems);
    Ok(Repo { manifest, machines, materials, problems })
}

pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let text = std::fs::read_to_string(dir.join(MANIFEST)).map_err(|_| format!("no {MANIFEST}: not a Scriber library repository"))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("{MANIFEST}: {e}"))?;
    if manifest.format != exchange::FORMAT {
        return Err(format!("{MANIFEST}: format must be \"{}\"", exchange::FORMAT));
    }
    if manifest.version > VERSION {
        return Err(format!("{MANIFEST}: made for a newer Scriber (version {}); please update", manifest.version));
    }
    if manifest.name.trim().is_empty() {
        return Err(format!("{MANIFEST}: the name is missing"));
    }
    Ok(manifest)
}

/// Every valid `<id>.json` in the folder, in name order; the others go to `problems`.
fn items<T: serde::de::DeserializeOwned>(dir: &Path, check: fn(&T) -> Result<(), String>, id: fn(&T) -> &String, problems: &mut Vec<String>) -> Vec<T> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        let file = format!("{}/{}", dir.file_name().unwrap_or_default().to_string_lossy(), p.file_name().unwrap_or_default().to_string_lossy());
        let item = std::fs::metadata(&p)
            .map_err(|e| e.to_string())
            .and_then(|m| if m.len() > MAX_JSON { Err("larger than 1 MB".into()) } else { std::fs::read_to_string(&p).map_err(|e| e.to_string()) })
            .and_then(|t| serde_json::from_str::<T>(&t).map_err(|e| e.to_string()))
            .and_then(|x| check(&x).map(|_| x))
            .and_then(|x| if p.file_stem().is_some_and(|s| *s == **id(&x)) { Ok(x) } else { Err(format!("the file name must be the id ({}.json)", id(&x))) });
        match item {
            Ok(x) => out.push(x),
            Err(e) => problems.push(format!("{file} skipped: {e}")),
        }
    }
    out
}

/// Built-ins with the enabled sources on top, in list order: a source wins over a built-in with the
/// same id; between sources the first one wins and the clash is reported.
pub fn base_from(dir: &Path) -> Base {
    fn add<T>(list: &mut Vec<(Origin, T)>, items: Vec<T>, origin: &Origin, id: fn(&T) -> &String, problems: &mut Vec<String>) {
        for item in items {
            match list.iter().position(|(_, b)| id(b) == id(&item)) {
                Some(i) if list[i].0.is_source() => problems.push(format!("\"{}\" {} is not used: the one {} has the same id", id(&item), origin.label(), list[i].0.label())),
                Some(i) => list[i] = (origin.clone(), item),
                None => list.push((origin.clone(), item)),
            }
        }
    }
    let mut base = Base::builtin();
    for s in list(dir).into_iter().filter(|s| s.enabled && !s.pinned_commit.is_empty()) {
        match read_repo(&s.folder(dir)) {
            Ok(repo) => {
                let origin = Origin::Source { id: s.id.clone(), name: s.display_name().to_string() };
                base.problems.extend(repo.problems.into_iter().map(|p| format!("{}: {p}", s.display_name())));
                add(&mut base.machines, repo.machines, &origin, |m| &m.id, &mut base.problems);
                add(&mut base.materials, repo.materials, &origin, |m| &m.id, &mut base.problems);
            }
            Err(e) => base.problems.push(format!("{}: {e}", s.display_name())),
        }
    }
    base
}

pub fn base() -> Base {
    base_from(&config_dir())
}

/// The machines a source's materials have settings for that nobody has (the settings are kept).
pub fn missing_machines(lib: &Library, source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (o, m) in &lib.base.materials {
        if matches!(o, Origin::Source { id, .. } if id == source) {
            for s in &m.settings {
                if lib.machine(&s.machine).is_none() && !out.contains(&s.machine) {
                    out.push(s.machine.clone());
                }
            }
        }
    }
    out
}

// ---------- what changed ----------

#[derive(Clone, Debug, Default, Serialize)]
pub struct Diff {
    pub added_machines: Vec<String>,
    pub changed_machines: Vec<String>,
    pub removed_machines: Vec<String>,
    pub added_materials: Vec<String>,
    pub changed_materials: Vec<String>,
    pub removed_materials: Vec<String>,
    /// Files that were skipped, and why.
    pub problems: Vec<String>,
}

fn compare<T: PartialEq>(old: &[T], new: &[T], id: fn(&T) -> &String) -> [Vec<String>; 3] {
    let by = |list: &[T], k: &String| list.iter().position(|x| id(x) == k);
    let added = new.iter().filter(|n| by(old, id(n)).is_none()).map(|n| id(n).clone()).collect();
    let changed = new.iter().filter(|n| by(old, id(n)).is_some_and(|i| old[i] != **n)).map(|n| id(n).clone()).collect();
    let removed = old.iter().filter(|o| by(new, id(o)).is_none()).map(|o| id(o).clone()).collect();
    [added, changed, removed]
}

impl Diff {
    pub fn between(old: Option<&Repo>, new: &Repo) -> Diff {
        let empty = Repo::default();
        let old = old.unwrap_or(&empty);
        let [added_machines, changed_machines, removed_machines] = compare(&old.machines, &new.machines, |m| &m.id);
        let [added_materials, changed_materials, removed_materials] = compare(&old.materials, &new.materials, |m| &m.id);
        Diff { added_machines, changed_machines, removed_machines, added_materials, changed_materials, removed_materials, problems: new.problems.clone() }
    }

    /// "+1 machines, +2 materials, 1 changed, 0 removed".
    pub fn summary(&self) -> String {
        let mut added: Vec<String> = [(self.added_machines.len(), "machines"), (self.added_materials.len(), "materials")].iter().filter(|(n, _)| *n > 0).map(|(n, what)| format!("+{n} {what}")).collect();
        if added.is_empty() {
            added.push("+0".into());
        }
        format!("{}, {} changed, {} removed", added.join(", "), self.changed_machines.len() + self.changed_materials.len(), self.removed_machines.len() + self.removed_materials.len())
    }
}

// ---------- subscribe, sync, unsubscribe ----------

#[derive(Debug, Serialize)]
pub struct SyncReport {
    pub id: String,
    /// The files in use changed (a new commit).
    pub changed: bool,
    /// Not checked: an automatic sync that ran less than a day ago, or a disabled source.
    pub skipped: bool,
    pub commit: String,
    pub diff: Option<Diff>,
    pub summary: String,
}

/// Resolve the source's track and, when the commit is new, download and validate it into its own
/// folder. The pinned files stay until the new ones read fine. Returns the diff and the old commit.
fn update(dir: &Path, http: &dyn Http, s: &mut Source) -> Result<Option<(Diff, String)>, String> {
    let (owner, repo) = s.owner_repo()?;
    let have = !s.pinned_commit.is_empty() && s.folder(dir).join(MANIFEST).is_file();
    let Some(res) = resolve(http, &owner, &repo, &s.track, if have { &s.pinned_commit } else { "" }, if have { &s.etag } else { "" })? else { return Ok(None) };
    (s.etag, s.release) = (res.etag, res.release);
    if have && res.commit == s.pinned_commit {
        return Ok(None);
    }
    let new_dir = dir.join("sources").join(&s.id).join(&res.commit);
    let mut skipped = Vec::new();
    if !new_dir.join(MANIFEST).is_file() {
        let zip = http.get(&format!("https://codeload.github.com/{owner}/{repo}/zip/{}", res.commit), "application/zip", "", MAX_ZIP)?;
        if zip.status != 200 {
            return Err(format!("could not download {owner}/{repo} (GitHub answered {})", zip.status));
        }
        let tmp = dir.join("sources").join(&s.id).join(format!("{}.tmp{}", res.commit, std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let checked = extract(&zip.body, &tmp).and_then(|sk| read_repo(&tmp).map(|_| sk));
        match checked {
            Ok(sk) => skipped = sk,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(if have { format!("the new version is not usable, keeping the one in use: {e}") } else { e });
            }
        }
        let _ = std::fs::remove_dir_all(&new_dir);
        std::fs::rename(&tmp, &new_dir).map_err(|e| format!("{}: {e}", new_dir.display()))?;
    }
    let new = read_repo(&new_dir)?;
    let old = if have { read_repo(&s.folder(dir)).ok() } else { None };
    let mut diff = Diff::between(old.as_ref(), &new);
    diff.problems.extend(skipped);
    let old_commit = std::mem::replace(&mut s.pinned_commit, res.commit);
    s.name = new.manifest.name;
    s.fetched_at = now();
    Ok(Some((diff, old_commit)))
}

/// Subscribe to a repository and download its library. `track` missing: from the address
/// (`owner/repo@x`: tag x when the repository has that tag, else branch x), else the default branch.
pub fn subscribe(dir: &Path, http: &dyn Http, input: &str, track: Option<Track>) -> Result<(Source, Diff), String> {
    let (owner, repo, at) = parse_url(input)?;
    let id = slug(&owner, &repo);
    if list(dir).iter().any(|s| s.id == id) {
        return Err(format!("already subscribed to {owner}/{repo} (id {id})"));
    }
    let track = match (track, at) {
        (Some(t), _) => t,
        (None, None) => Track::default(),
        (None, Some(x)) if has_tag(http, &owner, &repo, &x)? => Track::Tag(x),
        (None, Some(x)) => Track::Branch(x),
    };
    let mut s = Source { id: id.clone(), url: format!("https://github.com/{owner}/{repo}"), track, pinned_commit: String::new(), name: String::new(), release: String::new(), fetched_at: 0, last_checked: now(), etag: String::new(), last_error: String::new(), last_sync: String::new(), enabled: true };
    let (diff, _) = update(dir, http, &mut s)?.ok_or("GitHub gave no commit")?;
    s.last_sync = diff.summary();
    edit(dir, |l| {
        if l.iter().any(|o| o.id == id) {
            return Err(format!("already subscribed to {owner}/{repo}"));
        }
        l.push(s.clone());
        Ok(())
    })?;
    Ok((s, diff))
}

/// Bring a source up to date with its track. `force` false (the automatic sync at start): skip
/// disabled sources and ones checked less than a day ago. Errors are recorded on the source.
pub fn sync(dir: &Path, http: &dyn Http, id: &str, force: bool) -> Result<SyncReport, String> {
    let mut s = list(dir).into_iter().find(|s| s.id == id).ok_or(format!("no library source with id {id}"))?;
    let report = |s: &Source, changed, skipped, diff: Option<Diff>, summary: String| SyncReport { id: s.id.clone(), changed, skipped, commit: s.pinned_commit.clone(), diff, summary };
    if !force && (!s.enabled || now().saturating_sub(s.last_checked) < AUTO_SYNC_EVERY) {
        return Ok(report(&s, false, true, None, "checked less than a day ago".into()));
    }
    match update(dir, http, &mut s) {
        Err(e) => {
            edit(dir, |l| Ok(find(l, id).map(|o| o.last_error = e.clone()).ok()))?;
            Err(e)
        }
        Ok(result) => {
            let summary = result.as_ref().map_or("up to date".to_string(), |(d, _)| d.summary());
            edit(dir, |l| {
                if let Ok(o) = find(l, id) {
                    (o.pinned_commit, o.name, o.release, o.etag, o.fetched_at) = (s.pinned_commit.clone(), s.name.clone(), s.release.clone(), s.etag.clone(), s.fetched_at);
                    (o.last_checked, o.last_error) = (now(), String::new());
                    if result.is_some() {
                        o.last_sync = summary.clone();
                    }
                }
                Ok(())
            })?;
            let changed = result.is_some();
            let diff = result.map(|(d, old)| {
                if !old.is_empty() && old != s.pinned_commit {
                    let _ = std::fs::remove_dir_all(dir.join("sources").join(&s.id).join(old));
                }
                d
            });
            Ok(report(&s, changed, false, diff, summary))
        }
    }
}

/// The automatic sync at start: every enabled source not checked for a day.
pub fn auto_sync(dir: &Path, http: &dyn Http) -> Vec<Result<SyncReport, String>> {
    list(dir).iter().map(|s| sync(dir, http, &s.id, false)).filter(|r| !r.as_ref().is_ok_and(|r| r.skipped)).collect()
}

/// Follow something else (a sync follows right away).
pub fn set_track(dir: &Path, http: &dyn Http, id: &str, track: Track) -> Result<SyncReport, String> {
    edit(dir, |l| {
        let s = find(l, id)?;
        (s.track, s.etag) = (track, String::new());
        Ok(())
    })?;
    sync(dir, http, id, true)
}

pub fn set_enabled(dir: &Path, id: &str, enabled: bool) -> Result<(), String> {
    edit(dir, |l| find(l, id).map(|s| s.enabled = enabled))
}

/// Remove the subscription and its downloaded files.
pub fn unsubscribe(dir: &Path, id: &str) -> Result<(), String> {
    edit(dir, |l| {
        find(l, id)?;
        l.retain(|s| s.id != id);
        Ok(())
    })?;
    let _ = std::fs::remove_dir_all(dir.join("sources").join(id));
    Ok(())
}

// ---------- publishing ----------

/// What `export_repository` did, file paths relative to the repository.
#[derive(Debug, Default, PartialEq, Serialize)]
pub struct Exported {
    /// A new repository (no `.scriber.json` before); else an existing one was updated.
    pub created: bool,
    pub added: Vec<String>,
    pub changed: Vec<String>,
    /// Files in `.scriber/` removed because they are no longer the user's items.
    pub removed: Vec<String>,
    /// The manifest's new `library_version`, when it had one and something changed.
    pub library_version: Option<String>,
}

impl Exported {
    pub fn summary(&self) -> String {
        let what = if self.created { "Created" } else { "Updated" };
        let mut s = format!("{what}: {} added, {} changed, {} removed", self.added.len(), self.changed.len(), self.removed.len());
        if let Some(v) = &self.library_version {
            s += &format!(", library version {v}");
        }
        s
    }
}

/// The ids the user made themselves: not a built-in's or a source's id (so not their edits of those either).
pub fn own_ids(lib: &Library) -> (Vec<String>, Vec<String>) {
    let machines = lib.machines.iter().filter(|m| !lib.base.machines.iter().any(|(_, b)| b.id == m.id)).map(|m| m.id.clone()).collect();
    let materials = lib.materials.iter().filter(|m| !lib.base.materials.iter().any(|(_, b)| b.id == m.id)).map(|m| m.id.clone()).collect();
    (machines, materials)
}

/// The files of the user's own machines and materials (both lists missing = all of them; other ids
/// are refused), as (path in the repository, JSON). Settings for the user's own machines bring those
/// machines along; built-in machines are only referred to by id.
fn own_files(lib: &Library, machines: Option<&[String]>, materials: Option<&[String]>) -> Result<Vec<(String, String)>, String> {
    let (own_machines, own_materials) = own_ids(lib);
    let all = machines.is_none() && materials.is_none();
    let pick = |given: Option<&[String]>, own: &[String], what: &str| -> Result<Vec<String>, String> {
        if all {
            return Ok(own.to_vec());
        }
        given.unwrap_or_default().iter().map(|id| if own.contains(id) { Ok(id.clone()) } else { Err(format!("\"{id}\" is not a {what} you made (built-ins and sources' items are not exported; duplicate it first)")) }).collect()
    };
    let mut machine_ids = pick(machines, &own_machines, "machine")?;
    let material_ids = pick(materials, &own_materials, "material")?;
    if machine_ids.is_empty() && material_ids.is_empty() {
        return Err("nothing to export: only machines and materials you made yourself are exported, and there are none".into());
    }
    let mats: Vec<&Material> = material_ids.iter().filter_map(|id| lib.material(id)).collect();
    for s in mats.iter().flat_map(|m| &m.settings) {
        if own_machines.contains(&s.machine) && !machine_ids.contains(&s.machine) {
            machine_ids.push(s.machine.clone());
        }
    }
    let ms: Vec<&Machine> = machine_ids.iter().filter_map(|id| lib.machine(id)).collect();
    let mut files = Vec::new();
    for (sub, id, json) in ms.iter().map(|m| ("machines", &m.id, serde_json::to_string_pretty(m))).chain(mats.iter().map(|m| ("materials", &m.id, serde_json::to_string_pretty(m)))) {
        if !safe_name(&format!("{id}.json")) {
            return Err(format!("the id \"{id}\" cannot be a file name (use letters, digits, - and _)"));
        }
        files.push((format!("{FOLDER}/{sub}/{id}.json"), json.unwrap() + "\n"));
    }
    Ok(files)
}

fn new_manifest(name: Option<&str>, fallback: &str, description: Option<&str>) -> String {
    let name = name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(if fallback.is_empty() { "My Scriber library" } else { fallback });
    let m = Manifest { format: exchange::FORMAT.into(), version: VERSION, name: name.into(), description: description.unwrap_or_default().trim().into(), ..Manifest::default() };
    serde_json::to_string_pretty(&m).unwrap() + "\n"
}

/// "1.4.2" → "1.4.3", "7" → "8"; anything else gets ".1".
fn bump(v: &str) -> String {
    let (head, last) = v.rsplit_once('.').map_or(("", v), |(a, b)| (a, b));
    match last.parse::<u64>() {
        Ok(n) if head.is_empty() => format!("{}", n + 1),
        Ok(n) => format!("{head}.{}", n + 1),
        Err(_) => format!("{v}.1"),
    }
}

/// Write the user's own machines and materials as a library repository into `dir`: a new folder,
/// or an existing (git) repository updated in place. Only `.scriber.json` and
/// `.scriber/machines|materials/*.json` are touched; files there that are not the user's items any
/// more are removed, everything else (`.git`, README, …) stays. An existing manifest keeps its
/// metadata (its `library_version`, when it has one, is bumped when something changed); a new one
/// gets `name` (default: the folder name) and `description`. No git: the person commits and pushes.
pub fn export_repository(lib: &Library, dir: &Path, machines: Option<&[String]>, materials: Option<&[String]>, name: Option<&str>, description: Option<&str>) -> Result<Exported, String> {
    let files = own_files(lib, machines, materials)?;
    let (own_machines, own_materials) = own_ids(lib);
    let io = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    let mut out = Exported { created: !dir.join(MANIFEST).is_file(), ..Exported::default() };
    for (rel, text) in &files {
        let path = rel.split('/').fold(dir.to_path_buf(), |p, c| p.join(c));
        match std::fs::read_to_string(&path) {
            Ok(old) if old == *text => continue,
            Ok(_) => out.changed.push(rel.clone()),
            Err(_) => out.added.push(rel.clone()),
        }
        std::fs::create_dir_all(path.parent().unwrap()).and_then(|_| std::fs::write(&path, text)).map_err(|e| io(&path, e))?;
    }
    for (sub, keep) in [("machines", &own_machines), ("materials", &own_materials)] {
        let Ok(entries) = std::fs::read_dir(dir.join(FOLDER).join(sub)) else { continue };
        for p in entries.flatten().map(|e| e.path()) {
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            if p.extension().is_some_and(|e| e == "json") && !keep.contains(&stem) {
                std::fs::remove_file(&p).map_err(|e| io(&p, e))?;
                out.removed.push(format!("{FOLDER}/{sub}/{stem}.json"));
            }
        }
    }
    let path = dir.join(MANIFEST);
    if out.created {
        let folder = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        std::fs::write(&path, new_manifest(name, &folder, description)).map_err(|e| io(&path, e))?;
        out.added.insert(0, MANIFEST.into());
    } else if !(out.added.is_empty() && out.changed.is_empty() && out.removed.is_empty()) {
        let mut manifest: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| io(&path, e))?).map_err(|e| format!("{MANIFEST}: {e}"))?;
        if let Some(serde_json::Value::String(v)) = manifest.get("library_version") {
            let v = bump(v);
            manifest.insert("library_version".into(), v.clone().into());
            std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap() + "\n").map_err(|e| io(&path, e))?;
            out.changed.push(MANIFEST.into());
            out.library_version = Some(v);
        }
    }
    Ok(out)
}

/// The same files as a repository, in a zip (`.scriber.json` at its root); named after the file.
/// Returns how many machines and materials it holds.
pub fn export_zip(lib: &Library, path: &Path, machines: Option<&[String]>, materials: Option<&[String]>) -> Result<(usize, usize), String> {
    use std::io::Write;
    let files = own_files(lib, machines, materials)?;
    let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let io = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let mut w = zip::ZipWriter::new(std::fs::File::create(path).map_err(|e| io(&e))?);
    let opts = zip::write::SimpleFileOptions::default();
    for (rel, text) in std::iter::once((MANIFEST.to_string(), new_manifest(None, &name, None))).chain(files.iter().cloned()) {
        w.start_file(rel, opts).map_err(|e| io(&e))?;
        w.write_all(text.as_bytes()).map_err(|e| io(&e))?;
    }
    w.finish().map_err(|e| io(&e))?;
    let count = |sub: &str| files.iter().filter(|(p, _)| p.starts_with(&format!("{FOLDER}/{sub}/"))).count();
    Ok((count("machines"), count("materials")))
}

/// A library zip (the repository layout, at the root or in one top folder as GitHub's zips have
/// it), with the same caps and checks as a source. Invalid files are skipped and reported.
pub fn read_zip(path: &Path) -> Result<Repo, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.len() > MAX_ZIP {
        return Err(format!("{} is larger than {} MB", path.display(), MAX_ZIP >> 20));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let tmp = std::env::temp_dir().join(format!("scriber-zip-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
    let out = extract(&bytes, &tmp).and_then(|skipped| {
        let mut repo = read_repo(&tmp)?;
        repo.problems.extend(skipped);
        Ok(repo)
    });
    let _ = std::fs::remove_dir_all(&tmp);
    out.map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::store::{LibraryFile, merged_with};
    use std::collections::HashMap;
    use std::io::Write;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scriber-sources-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    type Canned = (u16, Vec<u8>, String);

    /// Answers by URL; counts the calls.
    #[derive(Default)]
    struct Fake {
        /// URL → status, body, ETag.
        replies: Mutex<HashMap<String, Canned>>,
        calls: Mutex<Vec<String>>,
    }

    impl Fake {
        fn set(&self, url: &str, status: u16, body: impl Into<Vec<u8>>, etag: &str) {
            self.replies.lock().unwrap().insert(url.into(), (status, body.into(), etag.into()));
        }
        fn downloads(&self) -> usize {
            self.calls.lock().unwrap().iter().filter(|c| c.contains("codeload")).count()
        }
    }

    impl Http for Fake {
        fn get(&self, url: &str, _: &str, etag: &str, limit: u64) -> Result<Reply, String> {
            self.calls.lock().unwrap().push(url.into());
            let (status, body, tag) = self.replies.lock().unwrap().get(url).cloned().unwrap_or((404, Vec::new(), String::new()));
            if !etag.is_empty() && etag == tag {
                return Ok(Reply { status: 304, body: Vec::new(), etag: tag });
            }
            if body.len() as u64 > limit {
                return Err("too large".into());
            }
            Ok(Reply { status, body, etag: tag })
        }
    }

    fn machine(id: &str) -> Machine {
        serde_json::from_value(serde_json::json!({"id": id, "name": id.to_uppercase(), "width": 300, "height": 200,
            "lasers": [{"id": "co2", "name": "CO2", "kind": "co2", "power_w": 40, "ops": ["cut"]}]}))
        .unwrap()
    }

    fn material(id: &str, machine: &str) -> Material {
        serde_json::from_value(serde_json::json!({"id": id, "name": id, "thickness": 3,
            "settings": [{"machine": machine, "laser": "co2", "operation": "cut", "power": 80, "speed": 10}]}))
        .unwrap()
    }

    const MANIFEST_JSON: &str = r#"{"format": "scriber-library", "version": 1, "name": "Acme presets", "description": "d", "author": "a", "license": "CC0-1.0"}"#;

    /// GitHub's zip of a commit: everything in one top folder.
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in files {
            w.start_file(format!("acme-presets-abc/{name}"), opts).unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn repo_zip(machines: &[Machine], materials: &[Material]) -> Vec<u8> {
        let mut files: Vec<(String, Vec<u8>)> = vec![(MANIFEST.into(), MANIFEST_JSON.into())];
        files.extend(machines.iter().map(|m| (format!(".scriber/machines/{}.json", m.id), serde_json::to_vec(m).unwrap())));
        files.extend(materials.iter().map(|m| (format!(".scriber/materials/{}.json", m.id), serde_json::to_vec(m).unwrap())));
        zip(&files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>())
    }

    const C1: &str = "1111111111111111111111111111111111111111";
    const C2: &str = "2222222222222222222222222222222222222222";
    const API: &str = "https://api.github.com/repos/acme/presets";

    fn publish(fake: &Fake, git_ref: &str, commit: &str, etag: &str, zip: Vec<u8>) {
        fake.set(&format!("{API}/commits/{git_ref}"), 200, commit, etag);
        fake.set(&format!("https://codeload.github.com/acme/presets/zip/{commit}"), 200, zip, "");
    }

    #[test]
    fn addresses() {
        let p = |s: &str| parse_url(s).map(|(o, r, x)| format!("{o}/{r}@{}", x.unwrap_or_default()));
        assert_eq!(p("acme/presets").unwrap(), "acme/presets@");
        assert_eq!(p(" acme/presets@v1.2 ").unwrap(), "acme/presets@v1.2");
        assert_eq!(p("https://github.com/acme/presets").unwrap(), "acme/presets@");
        assert_eq!(p("https://github.com/acme/presets.git/").unwrap(), "acme/presets@");
        assert_eq!(p("github.com/acme/presets/tree/feature/x").unwrap(), "acme/presets@feature/x");
        for bad in ["acme", "https://gitlab.com/a/b", "a/b@../x", "../b", "a/b c", "https://github.com/a/b/blob/main/x", "a/b@"] {
            assert!(parse_url(bad).is_err(), "{bad}");
        }
        assert_eq!(slug("Acme", "Laser.Presets"), "acme-laser-presets");
    }

    #[test]
    fn extract_takes_only_library_json_and_refuses_unsafe_entries() {
        let dir = temp("extract");
        let big = vec![b' '; MAX_JSON as usize + 1];
        let z = zip(&[
            (MANIFEST, MANIFEST_JSON.as_bytes()),
            (".scriber/machines/k40.json", b"{}"),
            (".scriber/../../evil.json", b"{}"),
            (".scriber/materials/big.json", &big),
            ("README.md", b"hi"),
            ("build.rs", b"fn main() {}"),
            (".scriber/run.sh", b"rm -rf /"),
        ]);
        let skipped = extract(&z, &dir).unwrap();
        assert!(dir.join(MANIFEST).is_file() && dir.join(".scriber/machines/k40.json").is_file());
        assert!(!dir.join("README.md").exists() && !dir.join("build.rs").exists() && !dir.join(".scriber/run.sh").exists());
        assert!(!dir.parent().unwrap().join("evil.json").exists() && !dir.join("evil.json").exists());
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert!(extract(b"not a zip", &dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalid_files_are_skipped_and_reported() {
        let dir = temp("invalid");
        let mut bad_power = machine("bad");
        bad_power.lasers[0].power_w = 0.0;
        let z = zip(&[
            (MANIFEST, MANIFEST_JSON.as_bytes()),
            (".scriber/machines/k40.json", &serde_json::to_vec(&machine("k40")).unwrap()),
            (".scriber/machines/bad.json", &serde_json::to_vec(&bad_power).unwrap()),
            (".scriber/machines/wrong-name.json", &serde_json::to_vec(&machine("other")).unwrap()),
            (".scriber/materials/oak.json", b"{not json"),
        ]);
        extract(&z, &dir).unwrap();
        let repo = read_repo(&dir).unwrap();
        assert_eq!(repo.machines.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["k40"]);
        assert_eq!(repo.problems.len(), 3, "{:?}", repo.problems);
        std::fs::write(dir.join(MANIFEST), r#"{"format": "other", "version": 1, "name": "x"}"#).unwrap();
        assert!(read_repo(&dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn subscribe_sync_and_unsubscribe() {
        let dir = temp("sync");
        let fake = Fake::default();
        fake.set(&format!("{API}/git/ref/tags/main"), 404, "", "");
        publish(&fake, "main", C1, "\"e1\"", repo_zip(&[machine("k40")], &[material("oak", "k40")]));
        let (s, diff) = subscribe(&dir, &fake, "acme/presets@main", None).unwrap();
        assert_eq!((s.id.as_str(), &s.track, s.pinned_commit.as_str(), s.name.as_str()), ("acme-presets", &Track::Branch("main".into()), C1, "Acme presets"));
        assert_eq!(diff.summary(), "+1 machines, +1 materials, 0 changed, 0 removed");
        assert!(subscribe(&dir, &fake, "https://github.com/acme/presets", None).is_err(), "only once");

        // Same commit (ETag answers 304): no download.
        let r = sync(&dir, &fake, "acme-presets", true).unwrap();
        assert!(!r.changed && fake.downloads() == 1);
        // Automatic sync: skipped within a day.
        assert!(sync(&dir, &fake, "acme-presets", false).unwrap().skipped);

        // A new commit: downloaded, applied, the old folder removed.
        let mut oak = material("oak", "k40");
        oak.thickness = 4.0;
        publish(&fake, "main", C2, "\"e2\"", repo_zip(&[], &[oak, material("ash", "nobody")]));
        let r = sync(&dir, &fake, "acme-presets", true).unwrap();
        assert!(r.changed);
        assert_eq!(r.summary, "+1 materials, 1 changed, 1 removed");
        let s = &list(&dir)[0];
        assert_eq!((s.pinned_commit.as_str(), s.last_sync.as_str()), (C2, "+1 materials, 1 changed, 1 removed"));
        assert!(!dir.join("sources/acme-presets").join(C1).exists() && s.folder(&dir).is_dir());

        // A broken new version: the one in use stays.
        publish(&fake, "main", C1, "\"e3\"", zip(&[(".scriber/machines/x.json", b"{}")]));
        assert!(sync(&dir, &fake, "acme-presets", true).is_err());
        let s = &list(&dir)[0];
        assert_eq!(s.pinned_commit, C2);
        assert!(!s.last_error.is_empty() && s.folder(&dir).join(MANIFEST).is_file());
        // Offline: recorded, last_checked untouched, so the next start retries.
        fake.set(&format!("{API}/commits/main"), 500, "", "");
        let checked = list(&dir)[0].last_checked;
        assert!(sync(&dir, &fake, "acme-presets", true).is_err());
        assert_eq!(list(&dir)[0].last_checked, checked);

        // The library sees the materials, with the machine nobody has flagged.
        let lib = merged_with(LibraryFile::default(), base_from(&dir));
        assert!(lib.material("ash").is_some());
        assert_eq!(missing_machines(&lib, "acme-presets"), ["nobody", "k40"], "kept, flagged");

        unsubscribe(&dir, "acme-presets").unwrap();
        assert!(list(&dir).is_empty() && !dir.join("sources/acme-presets").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tracks_tag_release_and_commit() {
        let dir = temp("tracks");
        let fake = Fake::default();
        fake.set(&format!("{API}/git/ref/tags/v1"), 200, "{}", "");
        publish(&fake, "v1", C1, "", repo_zip(&[machine("k40")], &[]));
        let (s, _) = subscribe(&dir, &fake, "acme/presets@v1", None).unwrap();
        assert_eq!(s.track, Track::Tag("v1".into()), "an existing tag is a tag");

        fake.set(&format!("{API}/releases/latest"), 200, r#"{"tag_name": "v2", "name": "Spring set"}"#, "\"r2\"");
        publish(&fake, "v2", C2, "", repo_zip(&[machine("k40"), machine("k50")], &[]));
        let r = set_track(&dir, &fake, "acme-presets", Track::Release("latest".into())).unwrap();
        assert!(r.changed);
        assert_eq!(list(&dir)[0].release, "v2 (Spring set)");
        let calls = fake.calls.lock().unwrap().len();
        assert!(!sync(&dir, &fake, "acme-presets", true).unwrap().changed, "same release (304)");
        assert_eq!(fake.calls.lock().unwrap().len(), calls + 1, "one small call");

        // Locked to a commit: sync makes no call at all.
        fake.set(&format!("{API}/commits/{C2}"), 200, C2, "");
        set_track(&dir, &fake, "acme-presets", Track::Commit(C2.into())).unwrap();
        let calls = fake.calls.lock().unwrap().len();
        assert!(!sync(&dir, &fake, "acme-presets", true).unwrap().changed);
        assert_eq!(fake.calls.lock().unwrap().len(), calls);

        fake.set(&format!("{API}/tags?per_page=100"), 200, r#"[{"name": "v1"}, {"name": "v2"}]"#, "");
        fake.set(&format!("{API}/releases?per_page=30"), 200, r#"[{"tag_name": "v2", "name": "Spring set"}]"#, "");
        assert_eq!(choices(&fake, "acme/presets", Choices::Tags).unwrap().len(), 2);
        assert_eq!(choices(&fake, "acme/presets", Choices::Releases).unwrap(), [("v2".to_string(), "Spring set (v2)".to_string())]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn merge_order_origins_and_clashes() {
        let dir = temp("merge");
        for (id, ms) in [("first", vec![machine("k40"), machine("xtool-m2")]), ("second", vec![machine("k40"), machine("k50")])] {
            let s = Source { id: id.into(), url: format!("https://github.com/acme/{id}"), track: Track::default(), pinned_commit: C1.into(), name: format!("{id} name"), release: String::new(), fetched_at: 0, last_checked: 0, etag: String::new(), last_error: String::new(), last_sync: String::new(), enabled: true };
            extract(&repo_zip(&ms, &[]), &s.folder(&dir)).unwrap();
            edit(&dir, |l| {
                l.push(s);
                Ok(())
            })
            .unwrap();
        }
        let base = base_from(&dir);
        assert_eq!(base.problems.len(), 1, "k40 clashes: {:?}", base.problems);
        let mut user = machine("k50");
        user.notes = "mine".into();
        let lib = merged_with(LibraryFile { machines: vec![user], ..Default::default() }, base);
        let src = |id: &str, name: &str| Origin::Source { id: id.into(), name: name.into() };
        assert_eq!(lib.machine_origin("k40"), src("first", "first name"), "first in list order wins");
        assert_eq!(lib.machine_origin("xtool-m2"), src("first", "first name"), "a source wins over the built-in");
        assert_eq!(lib.machine_origin("k50"), Origin::User, "the user's edit wins");
        assert_eq!(lib.machine("k50").unwrap().notes, "mine");
        assert_eq!(lib.machine_origin("xtool-d1"), Origin::Builtin);
        // Saving keeps only the user's part; a disabled source drops out.
        assert_eq!(crate::library::store::user_part(&lib).machines.len(), 1);
        set_enabled(&dir, "first", false).unwrap();
        let lib = crate::library::store::rebase(&lib, base_from(&dir));
        assert_eq!(lib.machine_origin("k40"), src("second", "second name"));
        assert_eq!(lib.machine_origin("xtool-m2"), Origin::Builtin);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn exported_repository_reads_back_as_a_source() {
        let dir = temp("export");
        let mut lib = crate::library::store::merged(LibraryFile::default());
        lib.put_machine(machine("k40"));
        lib.put_material(material("oak", "k40"));
        lib.put_material(material("birch", "xtool-m2"));
        let mut m2 = lib.machine("xtool-m2").unwrap().clone();
        m2.notes = "my edit".into();
        lib.put_machine(m2);
        let e = export_repository(&lib, &dir, None, None, Some("Acme"), Some("Presets")).unwrap();
        assert!(e.created);
        assert_eq!(e.added.len(), 4, "manifest, k40, oak, birch; not the edited built-in: {:?}", e.added);
        let repo = read_repo(&dir).unwrap();
        assert!(repo.problems.is_empty(), "{:?}", repo.problems);
        assert_eq!(repo.machines, [machine("k40")]);
        assert_eq!(repo.materials.len(), 2);
        assert_eq!((repo.manifest.name.as_str(), repo.manifest.description.as_str()), ("Acme", "Presets"));
        // As a source, it loads like any other.
        let s = Source { id: "mine".into(), url: "https://github.com/me/mine".into(), track: Track::default(), pinned_commit: C1.into(), name: String::new(), release: String::new(), fetched_at: 0, last_checked: 0, etag: String::new(), last_error: String::new(), last_sync: String::new(), enabled: true };
        let config = temp("export-config");
        copy_dir(&dir, &s.folder(&config));
        edit(&config, |l| {
            l.push(s);
            Ok(())
        })
        .unwrap();
        let base = base_from(&config);
        assert!(base.problems.is_empty() && base.materials.iter().any(|(o, m)| m.id == "oak" && o.is_source()));
        // Only material "birch": selected items, a built-in id refused, nothing of one's own refused.
        assert!(export_repository(&crate::library::store::merged(LibraryFile::default()), &temp("export-none"), None, None, None, None).is_err(), "nothing of one's own");
        assert!(export_repository(&lib, &dir, Some(&["xtool-m2".into()]), None, None, None).is_err(), "a built-in id is refused");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&config).ok();
    }

    #[test]
    fn export_updates_an_existing_repository_and_keeps_other_files() {
        let dir = temp("export-existing");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("README.md"), "hello").unwrap();
        std::fs::write(dir.join(MANIFEST), r#"{"format": "scriber-library", "version": 1, "name": "Acme", "description": "d", "author": "me", "license": "CC0-1.0", "homepage": "https://x", "library_version": "1.4"}"#).unwrap();
        std::fs::create_dir_all(dir.join(".scriber/materials")).unwrap();
        std::fs::write(dir.join(".scriber/materials/gone.json"), "{}").unwrap();
        std::fs::write(dir.join(".scriber/notes.txt"), "keep").unwrap();
        let mut lib = crate::library::store::merged(LibraryFile::default());
        lib.put_material(material("oak", "xtool-m2"));
        let e = export_repository(&lib, &dir, None, None, Some("ignored"), None).unwrap();
        assert!(!e.created);
        assert_eq!((e.added.as_slice(), e.removed.as_slice(), e.library_version.as_deref()), (&[".scriber/materials/oak.json".to_string()][..], &[".scriber/materials/gone.json".to_string()][..], Some("1.5")));
        assert!(dir.join(".git").is_dir() && dir.join("README.md").is_file() && dir.join(".scriber/notes.txt").is_file());
        let m = read_manifest(&dir).unwrap();
        assert_eq!((m.name.as_str(), m.author.as_str(), m.homepage.as_deref(), m.library_version.as_deref()), ("Acme", "me", Some("https://x"), Some("1.5")));
        // Nothing changed: no new version. A changed material is reported as changed.
        assert_eq!(export_repository(&lib, &dir, None, None, None, None).unwrap(), Exported::default());
        lib.material_mut("oak").unwrap().notes = "new".into();
        let e = export_repository(&lib, &dir, None, None, None, None).unwrap();
        assert_eq!((e.changed.len(), e.library_version.as_deref()), (2, Some("1.6")));
        assert_eq!((bump("7"), bump("1.4.9"), bump("beta")), ("8".to_string(), "1.4.10".to_string(), "beta.1".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn library_zip_round_trip_github_folder_and_bad_entries() {
        let dir = temp("zip");
        let mut lib = crate::library::store::merged(LibraryFile::default());
        lib.put_machine(machine("k40"));
        lib.put_material(material("oak", "k40"));
        let path = dir.join("my-presets.zip");
        assert_eq!(export_zip(&lib, &path, None, None).unwrap(), (1, 1));
        let repo = read_zip(&path).unwrap();
        assert_eq!((repo.manifest.name.as_str(), repo.machines.len(), repo.materials.len()), ("my-presets", 1, 1));
        // GitHub's layout (one top folder) with bad entries.
        let gh = dir.join("gh.zip");
        std::fs::write(&gh, zip(&[(MANIFEST, MANIFEST_JSON.as_bytes()), (".scriber/materials/oak.json", &serde_json::to_vec(&material("oak", "k40")).unwrap()), (".scriber/../../evil.json", b"{}"), (".scriber/materials/bad.json", b"{")])).unwrap();
        let repo = read_zip(&gh).unwrap();
        assert_eq!(repo.materials.len(), 1);
        assert_eq!(repo.problems.len(), 2, "{:?}", repo.problems);
        assert!(!dir.join("evil.json").exists() && !std::env::temp_dir().join("evil.json").exists());
        std::fs::write(&gh, b"nope").unwrap();
        assert!(read_zip(&gh).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            if e.path().is_dir() {
                copy_dir(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }
}
