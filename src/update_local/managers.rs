use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// A package manager that can hold a local install of a released package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Cargo,
    Uv,
    Npm,
    Brew,
}

impl Manager {
    pub const ALL: [Manager; 4] = [Manager::Cargo, Manager::Uv, Manager::Npm, Manager::Brew];

    pub fn name(self) -> &'static str {
        match self {
            Manager::Cargo => "cargo",
            Manager::Uv => "uv",
            Manager::Npm => "npm",
            Manager::Brew => "brew",
        }
    }
}

/// Where an install came from, which decides whether and how it can be
/// refreshed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The manager's default registry: crates.io, PyPI, the npm registry, a tap.
    Registry,
    /// A local directory, as with `cargo install --path`.
    Path(PathBuf),
    /// Somewhere we will not reinstall from, such as a git URL or an alternate
    /// registry. Reinstalling would fetch different code than was released.
    Foreign(String),
}

/// One install of this project's package found on this machine.
#[derive(Debug, Clone)]
pub struct Install {
    pub manager: Manager,
    pub package: String,
    pub version: String,
    pub source: Source,
    /// Executable names this install provides.
    pub bins: Vec<String>,
    /// Canonical paths of those executables, where the manager reports them.
    pub bin_paths: Vec<PathBuf>,
    /// Canonical directory every file of this install lives under, where known.
    pub root: Option<PathBuf>,
}

/// Run a command and hand back whether it succeeded together with its stdout.
/// `None` means the program is not installed, which is never an error here: a
/// machine without npm simply has no npm installs.
fn capture(program: &str, args: &[&str]) -> Option<(bool, String)> {
    let output = Command::new(program).args(args).output().ok()?;
    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

// --- cargo ---

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct CargoEntry {
    pub name: String,
    pub version: String,
    pub source: Source,
    pub bins: Vec<String>,
}

/// Parse `cargo install --list`. Entries are a header line ending in `:`,
/// followed by indented executable names:
///
/// ```text
/// rumdl v0.2.29:
///     rumdl
/// husker v0.4.39 (/Users/ruben/Projects/husker):
///     husker
/// bwbackup v0.1.0 (https://github.com/snoyberg/bwbackup#9e3bc12f):
///     bwbackup
/// ```
pub(crate) fn parse_cargo_install_list(text: &str) -> Vec<CargoEntry> {
    let mut entries: Vec<CargoEntry> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(entry) = entries.last_mut() {
                entry.bins.push(line.trim().to_string());
            }
            continue;
        }
        if let Some(entry) = parse_cargo_header(line) {
            entries.push(entry);
        }
    }
    entries
}

fn parse_cargo_header(line: &str) -> Option<CargoEntry> {
    let head = line.trim_end().strip_suffix(':')?;
    let (head, source) = match head.strip_suffix(')').and_then(|h| h.rsplit_once(" (")) {
        Some((rest, source)) => (rest, classify_cargo_source(source)),
        None => (head, Source::Registry),
    };
    let (name, version) = head.trim().rsplit_once(' ')?;
    let version = version.strip_prefix('v')?;
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some(CargoEntry {
        name: name.to_string(),
        version: version.to_string(),
        source,
        bins: Vec::new(),
    })
}

/// An absolute directory is a `--path` install. Everything else names a source
/// we cannot reproduce with a plain `cargo install`: a git URL, an alternate
/// registry.
fn classify_cargo_source(source: &str) -> Source {
    if source.starts_with('/') {
        Source::Path(PathBuf::from(source))
    } else {
        Source::Foreign(source.to_string())
    }
}

/// The directory `cargo install` puts executables in, resolved the way cargo
/// resolves it: `CARGO_INSTALL_ROOT`, then `install.root` from the cargo
/// config, then `CARGO_HOME`, then `~/.cargo`.
pub(crate) fn cargo_bin_dir(
    env: impl Fn(&str) -> Option<String>,
    read: impl Fn(&Path) -> Option<String>,
) -> Option<PathBuf> {
    if let Some(root) = env("CARGO_INSTALL_ROOT") {
        return Some(PathBuf::from(root).join("bin"));
    }
    let cargo_home = env("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    for name in ["config.toml", "config"] {
        let Some(content) = read(&cargo_home.join(name)) else {
            continue;
        };
        if let Some(root) = parse_cargo_install_root(&content) {
            return Some(PathBuf::from(root).join("bin"));
        }
    }
    Some(cargo_home.join("bin"))
}

fn parse_cargo_install_root(config: &str) -> Option<String> {
    let value: toml::Value = toml::from_str(config).ok()?;
    value
        .get("install")?
        .get("root")?
        .as_str()
        .map(str::to_string)
}

fn probe_cargo(name: &str) -> Option<Install> {
    probe_cargo_all(&[name.to_string()]).into_iter().next()
}

/// Every cargo install naming one of `names`, in the order given.
///
/// One `cargo install --list` answers for the whole set. A workspace has as
/// many candidate package names as it has members, and asking once per name
/// would run the same command a dozen times to read the same output.
fn probe_cargo_all(names: &[String]) -> Vec<Install> {
    let Some((ok, stdout)) = capture("cargo", &["install", "--list"]) else {
        return Vec::new();
    };
    if !ok {
        return Vec::new();
    }
    let entries = parse_cargo_install_list(&stdout);
    let bin_dir = cargo_bin_dir(
        |key| std::env::var(key).ok(),
        |path| std::fs::read_to_string(path).ok(),
    );
    names
        .iter()
        .filter_map(|name| entries.iter().find(|e| &e.name == name))
        .map(|entry| {
            let bin_paths = bin_dir
                .as_ref()
                .map(|dir| {
                    entry
                        .bins
                        .iter()
                        .filter_map(|bin| canonical(&dir.join(bin)))
                        .collect()
                })
                .unwrap_or_default();
            Install {
                manager: Manager::Cargo,
                package: entry.name.clone(),
                version: entry.version.clone(),
                source: entry.source.clone(),
                bins: entry.bins.clone(),
                bin_paths,
                root: None,
            }
        })
        .collect()
}

// --- uv ---

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct UvEntry {
    pub name: String,
    pub version: String,
    pub root: Option<PathBuf>,
    pub bins: Vec<(String, PathBuf)>,
}

/// Parse `uv tool list --show-paths`:
///
/// ```text
/// rumdl v0.2.48 (/Users/ruben/.local/share/uv/tools/rumdl)
/// - rumdl (/Users/ruben/.local/bin/rumdl)
/// ```
pub(crate) fn parse_uv_tool_list(text: &str) -> Vec<UvEntry> {
    let mut entries: Vec<UvEntry> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("- ") {
            let Some((name, path)) = split_trailing_path(rest) else {
                continue;
            };
            if let Some(entry) = entries.last_mut() {
                entry.bins.push((name.to_string(), PathBuf::from(path)));
            }
            continue;
        }
        let (head, root) = match split_trailing_path(line) {
            Some((head, path)) => (head, Some(PathBuf::from(path))),
            None => (line, None),
        };
        let Some((name, version)) = head.trim().rsplit_once(' ') else {
            continue;
        };
        let Some(version) = version.strip_prefix('v') else {
            continue;
        };
        entries.push(UvEntry {
            name: name.to_string(),
            version: version.to_string(),
            root,
            bins: Vec::new(),
        });
    }
    entries
}

/// Split `head (path)` into its head and the parenthesized path.
fn split_trailing_path(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_end().strip_suffix(')')?;
    let (head, path) = rest.rsplit_once(" (")?;
    Some((head, path))
}

/// PEP 503 normalization: lowercase, with runs of `-`, `_` and `.` collapsed to
/// a single `-`. `uv tool list` reports the normalized name, so `foo_bar` in
/// pyproject.toml appears as `foo-bar`.
pub(crate) fn normalize_pypi(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if matches!(ch, '-' | '_' | '.') {
            if !out.ends_with('-') {
                out.push('-');
            }
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

fn probe_uv(name: &str) -> Option<Install> {
    let (ok, stdout) = capture("uv", &["tool", "list", "--show-paths"])?;
    if !ok {
        return None;
    }
    let wanted = normalize_pypi(name);
    let entry = parse_uv_tool_list(&stdout)
        .into_iter()
        .find(|e| normalize_pypi(&e.name) == wanted)?;
    Some(Install {
        manager: Manager::Uv,
        package: entry.name,
        version: entry.version,
        source: Source::Registry,
        bins: entry.bins.iter().map(|(n, _)| n.clone()).collect(),
        bin_paths: entry
            .bins
            .iter()
            .filter_map(|(_, p)| canonical(p))
            .collect(),
        root: entry.root.as_deref().and_then(canonical),
    })
}

// --- npm ---

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NpmEntry {
    pub version: String,
    pub path: Option<PathBuf>,
    pub bins: Vec<String>,
}

/// Read one package out of `npm ls -g --depth=0 --json --long`. The `--long`
/// form is required: the short form carries versions but no `bin` map, and an
/// npm package's executable routinely has a different name than the package
/// (`@doist/todoist-cli` installs `td`).
pub(crate) fn parse_npm_global(json: &str, package: &str) -> Option<NpmEntry> {
    let root: serde_json::Value = serde_json::from_str(json).ok()?;
    let entry = root.get("dependencies")?.get(package)?;
    let version = entry.get("version")?.as_str()?.to_string();
    let path = entry
        .get("path")
        .and_then(|p| p.as_str())
        .map(PathBuf::from);
    // `bin` is a map of executable name to script, or a bare string, which
    // names one executable after the package itself.
    let bins = match entry.get("bin") {
        Some(serde_json::Value::Object(map)) => map.keys().cloned().collect(),
        Some(serde_json::Value::String(_)) => {
            vec![package.rsplit('/').next().unwrap_or(package).to_string()]
        }
        _ => Vec::new(),
    };
    Some(NpmEntry {
        version,
        path,
        bins,
    })
}

fn probe_npm(name: &str) -> Option<Install> {
    // `npm ls` exits non-zero when any global package is extraneous or has an
    // unmet peer dependency, while still printing the full tree. The exit
    // status says nothing about our package, so only the JSON is read.
    let (_, stdout) = capture("npm", &["ls", "-g", "--depth=0", "--json", "--long"])?;
    let entry = parse_npm_global(&stdout, name)?;
    Some(Install {
        manager: Manager::Npm,
        package: name.to_string(),
        version: entry.version,
        source: Source::Registry,
        bins: entry.bins,
        bin_paths: Vec::new(),
        root: entry.path.as_deref().and_then(canonical),
    })
}

// --- brew ---

/// Read the versions of `formula` out of `brew list --versions <formula>`,
/// whose line is the formula name followed by every installed version.
pub(crate) fn parse_brew_versions(text: &str, formula: &str) -> Option<Vec<String>> {
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some(formula) {
            continue;
        }
        let versions: Vec<String> = fields.map(str::to_string).collect();
        if !versions.is_empty() {
            return Some(versions);
        }
    }
    None
}

/// The version component of a canonical keg path, `.../Cellar/<formula>/<version>`.
pub(crate) fn parse_cellar_version(path: &Path, formula: &str) -> Option<String> {
    let version = path.file_name()?.to_str()?;
    let parent = path.parent()?.file_name()?.to_str()?;
    if parent != formula {
        return None;
    }
    Some(version.to_string())
}

/// The version of the Homebrew keg a path lives in, from the layout Homebrew
/// itself creates: `<prefix>/Cellar/<formula>/<version>/...`.
///
/// This is the same fact the brew probe reads, so it gives a definite owner for
/// a file no probed manager claims. It says nothing about which formula the
/// project publishes, so a copy identified this way is reported, never upgraded.
pub(crate) fn brew_keg_version(path: &Path) -> Option<String> {
    let parts: Vec<&str> = path.iter().filter_map(|p| p.to_str()).collect();
    parts
        .windows(3)
        // A keg directory is always a version, so a path merely passing through
        // some other directory named Cellar is not mistaken for one.
        .find(|w| w[0] == "Cellar" && w[2].starts_with(|c: char| c.is_ascii_digit()))
        .map(|w| w[2].to_string())
}

fn probe_brew(formulas: &[String]) -> Option<Install> {
    for formula in formulas {
        let (ok, stdout) = capture("brew", &["list", "--versions", formula])?;
        if !ok {
            continue;
        }
        let Some(versions) = parse_brew_versions(&stdout, formula) else {
            continue;
        };
        // Several kegs can be installed at once; the linked one is what PATH
        // reaches, and `brew --prefix` points at exactly that keg.
        let keg = capture("brew", &["--prefix", formula])
            .filter(|(ok, _)| *ok)
            .and_then(|(_, out)| canonical(Path::new(out.trim())));
        let version = keg
            .as_deref()
            .and_then(|k| parse_cellar_version(k, formula))
            .unwrap_or_else(|| versions[0].clone());
        let bins = keg
            .as_ref()
            .map(|keg| read_bin_names(&keg.join("bin")))
            .unwrap_or_default();
        let bin_paths = keg
            .as_ref()
            .map(|keg| {
                bins.iter()
                    .filter_map(|bin| canonical(&keg.join("bin").join(bin)))
                    .collect()
            })
            .unwrap_or_default();
        return Some(Install {
            manager: Manager::Brew,
            package: formula.clone(),
            version,
            source: Source::Registry,
            bins,
            bin_paths,
            root: keg,
        });
    }
    None
}

fn read_bin_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// Probe one manager for an install of `package`.
pub fn probe(manager: Manager, package: &Package) -> Option<Install> {
    match (manager, package) {
        (Manager::Cargo, Package::Named(name)) => probe_cargo(name),
        (Manager::Uv, Package::Named(name)) => probe_uv(name),
        (Manager::Npm, Package::Named(name)) => probe_npm(name),
        (Manager::Brew, Package::Candidates(formulas)) => probe_brew(formulas),
        // First match, as with Homebrew's formulas: a candidate list is ordered
        // by preference, so the last one is nobody's idea of the answer.
        (Manager::Cargo, Package::Candidates(names)) => probe_cargo_all(names).into_iter().next(),
        _ => None,
    }
}

/// Every install one manager holds of this project's packages.
///
/// Only cargo can hold more than one: a workspace publishes several binaries
/// from a single repository, and each is its own install to keep current.
pub fn probe_all(manager: Manager, package: &Package) -> Vec<Install> {
    match (manager, package) {
        (Manager::Cargo, Package::Candidates(names)) => probe_cargo_all(names),
        _ => probe(manager, package).into_iter().collect(),
    }
}

/// Re-read what a manager holds after installing.
///
/// Asked by the name the manager itself reported rather than the candidate list
/// the run started from: a candidate list can name a dozen packages, and what
/// matters here is the state of the one just installed. Homebrew is probed by
/// formula candidates rather than by package name, so its single name is passed
/// back in the form its probe accepts.
pub fn reprobe(install: &Install) -> Option<Install> {
    let package = match install.manager {
        Manager::Brew => Package::Candidates(vec![install.package.clone()]),
        _ => Package::Named(install.package.clone()),
    };
    probe(install.manager, &package)
}

/// What a manager should be asked about: one exact name, or the ordered
/// formula candidates Homebrew is probed with.
pub enum Package {
    Named(String),
    Candidates(Vec<String>),
}

/// The commands that bring `install` to `version`, in order.
pub(crate) fn install_commands(install: &Install, version: &str) -> Vec<Vec<String>> {
    let package = &install.package;
    match (install.manager, &install.source) {
        (Manager::Cargo, Source::Path(path)) => vec![vec![
            "cargo".into(),
            "install".into(),
            "--path".into(),
            path.display().to_string(),
            "--force".into(),
        ]],
        (Manager::Cargo, _) => vec![vec![
            "cargo".into(),
            "install".into(),
            format!("{package}@{version}"),
            "--force".into(),
        ]],
        // A pinned reinstall with a fresh index is the only uv form that is
        // deterministic: `uv tool upgrade` has no `--refresh` and reports
        // nothing to upgrade against a cached index.
        (Manager::Uv, _) => vec![vec![
            "uv".into(),
            "tool".into(),
            "install".into(),
            format!("{package}=={version}"),
            "--reinstall".into(),
            "--refresh".into(),
        ]],
        (Manager::Npm, _) => vec![vec![
            "npm".into(),
            "install".into(),
            "-g".into(),
            format!("{package}@{version}"),
        ]],
        // Homebrew cannot install an arbitrary version, and `brew upgrade` sees
        // only what the local tap clone holds, so the tap is refreshed first.
        (Manager::Brew, _) => vec![
            vec!["brew".into(), "update".into()],
            vec!["brew".into(), "upgrade".into(), package.clone()],
        ],
    }
}

/// The same install with every local cache bypassed, for one retry after a
/// failure the index says should not have happened. None where the manager has
/// no such form, so nothing is ever retried blindly.
///
/// cargo updates its registry index on every `cargo install`, and brew's own
/// first command is `brew update`, so neither has a stale local view left to
/// bypass. uv and npm both cache index metadata with a lifetime, which is the
/// one explanation for "the index has this version but the install could not
/// resolve it" that a second attempt can settle.
pub(crate) fn retry_commands(install: &Install, version: &str) -> Option<Vec<Vec<String>>> {
    let bypass = match install.manager {
        Manager::Uv => "--no-cache",
        Manager::Npm => "--prefer-online",
        Manager::Cargo | Manager::Brew => return None,
    };
    // Derived from the planned install rather than written out again, so the
    // retry cannot drift into being a different command.
    let mut commands = install_commands(install, version);
    for argv in &mut commands {
        argv.push(bypass.to_string());
    }
    Some(commands)
}

/// Run one install command, letting it write to the terminal as it works.
/// Stdout is suppressed under JSON output so the structured report stays the
/// only thing on stdout.
pub(crate) fn execute(argv: &[String], quiet_stdout: bool) -> Result<()> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| Error::Other("empty install command".to_string()))?;
    let mut command = Command::new(program);
    command.args(args);
    if quiet_stdout {
        command.stdout(std::process::Stdio::null());
    }
    let status = command
        .status()
        .map_err(|e| Error::Other(format!("{program}: {e}")))?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Other(format!(
        "{} exited with {}",
        argv.join(" "),
        status.code().unwrap_or(-1)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_list_reads_registry_path_and_git_installs() {
        let text = "\
agentgauge v0.1.0 (/Users/ruben/Projects/agentgauge/host):
    agentgauge
apkeep v0.18.0:
    apkeep
bwbackup v0.1.0 (https://github.com/snoyberg/bwbackup#9e3bc12f):
    bwbackup
cargo-edit v0.13.2:
    cargo-add
    cargo-rm
";
        let entries = parse_cargo_install_list(text);
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries[0].source,
            Source::Path(PathBuf::from("/Users/ruben/Projects/agentgauge/host"))
        );
        assert_eq!(entries[1].name, "apkeep");
        assert_eq!(entries[1].version, "0.18.0");
        assert_eq!(entries[1].source, Source::Registry);
        assert!(matches!(entries[2].source, Source::Foreign(_)));
        assert_eq!(entries[3].bins, vec!["cargo-add", "cargo-rm"]);
    }

    #[test]
    fn cargo_list_does_not_match_a_name_prefix() {
        let text = "rumdl-core v1.0.0:\n    rumdl-core\n";
        let entries = parse_cargo_install_list(text);
        assert!(
            !entries.iter().any(|e| e.name == "rumdl"),
            "'rumdl' must not match the 'rumdl-core' entry"
        );
    }

    #[test]
    fn cargo_list_of_an_empty_machine_is_empty() {
        assert!(parse_cargo_install_list("").is_empty());
    }

    #[test]
    fn cargo_bin_dir_prefers_install_root_then_config_then_cargo_home() {
        let no_config = |_: &Path| None;
        let explicit = cargo_bin_dir(
            |k| match k {
                "CARGO_INSTALL_ROOT" => Some("/opt/installs".to_string()),
                "CARGO_HOME" => Some("/home/u/.cargo".to_string()),
                _ => None,
            },
            no_config,
        );
        assert_eq!(explicit, Some(PathBuf::from("/opt/installs/bin")));

        let from_config = cargo_bin_dir(
            |k| (k == "CARGO_HOME").then(|| "/home/u/.cargo".to_string()),
            |path| {
                (path == Path::new("/home/u/.cargo/config.toml"))
                    .then(|| "[install]\nroot = \"/opt/from-config\"\n".to_string())
            },
        );
        assert_eq!(from_config, Some(PathBuf::from("/opt/from-config/bin")));

        let default = cargo_bin_dir(|k| (k == "HOME").then(|| "/home/u".to_string()), no_config);
        assert_eq!(default, Some(PathBuf::from("/home/u/.cargo/bin")));
    }

    #[test]
    fn uv_tool_list_reads_versions_and_executable_paths() {
        let text = "\
rumdl v0.2.48 (/Users/ruben/.local/share/uv/tools/rumdl)
- rumdl (/Users/ruben/.local/bin/rumdl)
esptool v5.1.0 (/Users/ruben/.local/share/uv/tools/esptool)
- espefuse (/Users/ruben/.local/bin/espefuse)
- esptool (/Users/ruben/.local/bin/esptool)
";
        let entries = parse_uv_tool_list(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "rumdl");
        assert_eq!(entries[0].version, "0.2.48");
        assert_eq!(
            entries[0].root,
            Some(PathBuf::from("/Users/ruben/.local/share/uv/tools/rumdl"))
        );
        assert_eq!(
            entries[0].bins,
            vec![(
                "rumdl".to_string(),
                PathBuf::from("/Users/ruben/.local/bin/rumdl")
            )]
        );
        assert_eq!(entries[1].bins.len(), 2);
    }

    #[test]
    fn pypi_names_normalize_to_one_spelling() {
        assert_eq!(normalize_pypi("Foo_Bar"), "foo-bar");
        assert_eq!(normalize_pypi("foo.bar"), "foo-bar");
        assert_eq!(normalize_pypi("foo--bar"), "foo-bar");
        assert_eq!(normalize_pypi("rumdl"), "rumdl");
        assert_ne!(normalize_pypi("foo-bar"), normalize_pypi("foobar"));
    }

    #[test]
    fn npm_global_reads_bin_names_that_differ_from_the_package() {
        let json = r#"{
          "name": "lib",
          "dependencies": {
            "@doist/todoist-cli": {
              "version": "1.62.1",
              "path": "/opt/homebrew/lib/node_modules/@doist/todoist-cli",
              "bin": {"td": "dist/index.js"}
            },
            "eslint": {"version": "9.37.0", "extraneous": true, "bin": {"eslint": "bin/eslint.js"}}
          }
        }"#;
        let entry = parse_npm_global(json, "@doist/todoist-cli").expect("package present");
        assert_eq!(entry.version, "1.62.1");
        assert_eq!(entry.bins, vec!["td"]);
        assert_eq!(
            entry.path,
            Some(PathBuf::from(
                "/opt/homebrew/lib/node_modules/@doist/todoist-cli"
            ))
        );
        // An extraneous entry is what makes `npm ls` exit non-zero; the tree is
        // still complete and still readable.
        assert_eq!(parse_npm_global(json, "eslint").unwrap().version, "9.37.0");
        assert!(parse_npm_global(json, "not-installed").is_none());
    }

    #[test]
    fn npm_string_bin_names_the_executable_after_the_package() {
        let json = r#"{"dependencies": {"@scope/tool": {"version": "1.0.0", "bin": "cli.js"}}}"#;
        let entry = parse_npm_global(json, "@scope/tool").unwrap();
        assert_eq!(entry.bins, vec!["tool"]);
    }

    #[test]
    fn brew_versions_reads_the_named_formula_only() {
        assert_eq!(
            parse_brew_versions("rumdl 0.2.42\n", "rumdl"),
            Some(vec!["0.2.42".to_string()])
        );
        assert_eq!(
            parse_brew_versions("rumdl 0.2.42 0.2.41\n", "rumdl"),
            Some(vec!["0.2.42".to_string(), "0.2.41".to_string()])
        );
        assert_eq!(parse_brew_versions("rumdl-extra 1.0\n", "rumdl"), None);
        assert_eq!(parse_brew_versions("", "rumdl"), None);
    }

    #[test]
    fn cellar_version_comes_from_the_keg_path() {
        let keg = Path::new("/opt/homebrew/Cellar/rumdl/0.2.42");
        assert_eq!(parse_cellar_version(keg, "rumdl"), Some("0.2.42".into()));
        assert_eq!(parse_cellar_version(keg, "other"), None);
    }

    #[test]
    fn a_keg_path_names_its_version_wherever_the_prefix_sits() {
        for prefix in ["/opt/homebrew", "/usr/local", "/home/linuxbrew/.linuxbrew"] {
            let file = PathBuf::from(prefix).join("Cellar/rumdl/0.2.42/bin/rumdl");
            assert_eq!(
                brew_keg_version(&file),
                Some("0.2.42".into()),
                "{}",
                file.display()
            );
        }
        // A formula whose executable name differs is still the keg's.
        assert_eq!(
            brew_keg_version(Path::new("/opt/homebrew/Cellar/todoist-cli/1.62.1/bin/td")),
            Some("1.62.1".into())
        );
        // A date-style version, which Homebrew also uses.
        assert_eq!(
            brew_keg_version(Path::new("/opt/homebrew/Cellar/x/2026-07-31/bin/x")),
            Some("2026-07-31".into())
        );
    }

    #[test]
    fn a_path_outside_a_keg_names_no_version() {
        for path in [
            "/home/u/.cargo/bin/rumdl",
            // A directory named Cellar that is not Homebrew's: the component
            // where a version belongs is not one.
            "/home/u/Cellar/bin/rumdl",
            // Truncated below the version component.
            "/opt/homebrew/Cellar/rumdl",
        ] {
            assert_eq!(brew_keg_version(Path::new(path)), None, "{path}");
        }
    }

    fn install_of(manager: Manager, source: Source) -> Install {
        Install {
            manager,
            package: "rumdl".to_string(),
            version: "0.2.42".to_string(),
            source,
            bins: vec!["rumdl".to_string()],
            bin_paths: Vec::new(),
            root: None,
        }
    }

    #[test]
    fn install_commands_pin_the_version_per_manager() {
        assert_eq!(
            install_commands(&install_of(Manager::Cargo, Source::Registry), "0.2.48"),
            vec![vec!["cargo", "install", "rumdl@0.2.48", "--force"]]
        );
        assert_eq!(
            install_commands(&install_of(Manager::Uv, Source::Registry), "0.2.48"),
            vec![vec![
                "uv",
                "tool",
                "install",
                "rumdl==0.2.48",
                "--reinstall",
                "--refresh"
            ]]
        );
        assert_eq!(
            install_commands(&install_of(Manager::Npm, Source::Registry), "0.2.48"),
            vec![vec!["npm", "install", "-g", "rumdl@0.2.48"]]
        );
        assert_eq!(
            install_commands(&install_of(Manager::Brew, Source::Registry), "0.2.48"),
            vec![vec!["brew", "update"], vec!["brew", "upgrade", "rumdl"]]
        );
    }

    #[test]
    fn only_a_manager_with_a_cache_to_bypass_is_retried() {
        assert_eq!(
            retry_commands(&install_of(Manager::Uv, Source::Registry), "0.2.48").unwrap(),
            vec![vec![
                "uv",
                "tool",
                "install",
                "rumdl==0.2.48",
                "--reinstall",
                "--refresh",
                "--no-cache"
            ]]
        );
        assert_eq!(
            retry_commands(&install_of(Manager::Npm, Source::Registry), "0.2.48").unwrap(),
            vec![vec![
                "npm",
                "install",
                "-g",
                "rumdl@0.2.48",
                "--prefer-online"
            ]]
        );
        // cargo refreshes its index on every install and brew's own first
        // command is `brew update`, so a second attempt would repeat the first.
        // A cargo retry would also mean a second full build.
        assert!(retry_commands(&install_of(Manager::Cargo, Source::Registry), "0.2.48").is_none());
        assert!(
            retry_commands(
                &install_of(Manager::Cargo, Source::Path(PathBuf::from("/src/rumdl"))),
                "0.2.48"
            )
            .is_none()
        );
        assert!(retry_commands(&install_of(Manager::Brew, Source::Registry), "0.2.48").is_none());
    }

    #[test]
    fn a_retry_is_the_planned_install_and_not_a_second_spelling_of_it() {
        let install = install_of(Manager::Uv, Source::Registry);
        let planned = install_commands(&install, "0.2.48");
        let retry = retry_commands(&install, "0.2.48").expect("uv has a cache to bypass");
        assert_eq!(retry.len(), planned.len());
        for (retry, planned) in retry.iter().zip(&planned) {
            assert_eq!(
                &retry[..planned.len()],
                &planned[..],
                "the retry must install exactly what the plan did"
            );
            assert_eq!(retry.len(), planned.len() + 1, "adding only the bypass");
        }
    }

    #[test]
    fn a_path_install_is_refreshed_from_its_own_directory() {
        let install = install_of(Manager::Cargo, Source::Path(PathBuf::from("/src/rumdl")));
        assert_eq!(
            install_commands(&install, "0.2.48"),
            vec![vec!["cargo", "install", "--path", "/src/rumdl", "--force"]]
        );
    }
}
