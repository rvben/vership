use std::path::{Path, PathBuf};

use super::managers::{Install, Manager, brew_keg_version};

/// One executable of a given name reachable through `$PATH`.
#[derive(Debug, Clone)]
pub struct Copy {
    /// The path as `$PATH` yields it.
    pub path: PathBuf,
    /// The executable this copy hands off to when it is a version manager's
    /// shim rather than the program itself. The shim runs nothing of its own,
    /// so the manager and version below are the target's.
    pub dispatches_to: Option<PathBuf>,
    /// The manager that owns this file, by resolved file identity. `None` is an
    /// unmanaged copy: something we did not install and will not update.
    pub manager: Option<Manager>,
    /// The owning manager's reported version. Never guessed for an unmanaged
    /// copy, which is why it is optional rather than a placeholder.
    pub version: Option<String>,
}

impl Copy {
    /// Where this copy is, and where it leads when it is a shim.
    pub fn location(&self) -> String {
        match &self.dispatches_to {
            Some(target) => format!("{} -> {}", self.path.display(), target.display()),
            None => self.path.display().to_string(),
        }
    }
}

/// Every copy of one executable name, in `$PATH` order. The first entry is
/// what the shell runs.
#[derive(Debug, Clone)]
pub struct BinaryReport {
    pub name: String,
    pub copies: Vec<Copy>,
}

impl BinaryReport {
    pub fn winner(&self) -> Option<&Copy> {
        self.copies.first()
    }

    pub fn shadowed(&self) -> &[Copy] {
        self.copies.get(1..).unwrap_or_default()
    }
}

/// Find every copy of `names` on `dirs`, in order, attributed to the install
/// that owns it.
///
/// `is_exec`, `canonicalize` and `dispatch` are injected so the walk, the
/// deduplication and the attribution rules are testable without touching the
/// real filesystem. `dispatch` answers, for a resolved file and the name it was
/// reached under, which executable a shim of that name hands off to, and `None`
/// for a file that is the program itself.
pub(crate) fn scan(
    dirs: &[PathBuf],
    names: &[String],
    installs: &[Install],
    is_exec: impl Fn(&Path) -> bool,
    canonicalize: impl Fn(&Path) -> Option<PathBuf>,
    dispatch: impl Fn(&Path, &str) -> Option<PathBuf>,
) -> Vec<BinaryReport> {
    names
        .iter()
        .map(|name| {
            let mut copies: Vec<Copy> = Vec::new();
            let mut seen: Vec<PathBuf> = Vec::new();
            for dir in dirs {
                let candidate = dir.join(name);
                if !is_exec(&candidate) {
                    continue;
                }
                // Two `$PATH` entries commonly reach the same file, either as
                // literal duplicates or through a symlinked directory. Identity
                // is the resolved file, so the same executable is reported once,
                // under the spelling `$PATH` reaches first.
                let mut real = canonicalize(&candidate).unwrap_or_else(|| candidate.clone());
                // A version manager's shim is one file standing in for every
                // program it has ever installed: what runs is whatever it
                // resolves the name to. Identity, ownership and version are
                // therefore the target's, and a later `$PATH` entry reaching
                // that same target is the same executable, not a second copy
                // shadowed behind the shim.
                let dispatches_to = dispatch(&real, name);
                if let Some(target) = &dispatches_to {
                    real = canonicalize(target).unwrap_or_else(|| target.clone());
                }
                if seen.contains(&real) {
                    continue;
                }
                seen.push(real.clone());
                let (manager, version) = attribution(&real, installs);
                copies.push(Copy {
                    path: candidate,
                    dispatches_to,
                    manager,
                    version,
                });
            }
            BinaryReport {
                name: name.clone(),
                copies,
            }
        })
        .collect()
}

/// The manager and version to report for a resolved executable.
///
/// Only managers holding the package are probed, so a copy can belong to a
/// manager this run never asked about. A Homebrew keg still names its owner in
/// the path, and calling such a file unmanaged would be a false claim about a
/// file brew installed. Anything else is left unmanaged with no version, since
/// a version we have not read is not a version we may print.
fn attribution(real: &Path, installs: &[Install]) -> (Option<Manager>, Option<String>) {
    if let Some(owner) = attribute(real, installs) {
        return (Some(owner.manager), Some(owner.version.clone()));
    }
    match brew_keg_version(real) {
        Some(version) => (Some(Manager::Brew), Some(version)),
        None => (None, None),
    }
}

/// Attribute a resolved executable to the install that owns it: an exact match
/// against an executable the manager reported, else containment in the tree the
/// manager installed into.
///
/// This is file identity, never directory convention. `~/.local/bin` holds uv
/// shims next to hand-copied binaries, so "lives in the uv bin directory" would
/// credit uv with files it has never seen.
fn attribute<'a>(real: &Path, installs: &'a [Install]) -> Option<&'a Install> {
    installs
        .iter()
        .find(|i| i.bin_paths.iter().any(|p| p == real))
        .or_else(|| {
            installs
                .iter()
                .find(|i| i.root.as_deref().is_some_and(|root| real.starts_with(root)))
        })
}

/// The executable a mise shim of `name` hands off to, or `None` when `real` is
/// not the mise binary.
///
/// mise fills its shims directory with one symlink to itself per executable of
/// every tool it has installed, keeps the shim after the tool is gone, and
/// puts that directory ahead of everything else on `$PATH`. Invoked, a shim
/// runs whatever `mise which <name>` resolves, which falls through to the next
/// copy on `$PATH` when mise manages none: a cargo or uv install, typically.
/// The very binary the shim links to is asked, so the answer is the one the
/// shell would get. A shim mise resolves nothing for stays a copy of its own
/// and is reported unmanaged, which is also what running it would amount to.
pub(crate) fn mise_shim_target(real: &Path, name: &str) -> Option<PathBuf> {
    if !is_mise(real) {
        return None;
    }
    let output = std::process::Command::new(real)
        .args(["which", name])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let target = String::from_utf8(output.stdout).ok()?;
    let target = target.trim();
    (!target.is_empty()).then(|| PathBuf::from(target))
}

/// Whether a resolved executable is the mise binary, which is what every mise
/// shim is a symlink to.
fn is_mise(real: &Path) -> bool {
    real.file_stem().is_some_and(|stem| stem == "mise")
}

/// Whether `path` is a file this shell would execute. Follows symlinks, so a
/// dangling link is not an executable.
pub(crate) fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::managers::Source;
    use super::*;

    fn install(manager: Manager, version: &str, bins: &[&str], root: Option<&str>) -> Install {
        Install {
            manager,
            package: "rumdl".to_string(),
            version: version.to_string(),
            source: Source::Registry,
            bins: vec!["rumdl".to_string()],
            bin_paths: bins.iter().map(PathBuf::from).collect(),
            root: root.map(PathBuf::from),
        }
    }

    fn dirs(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    /// A machine mirroring the real one: uv's shim in `~/.local/bin` resolving
    /// into uv's tool tree, a cargo copy, an unmanaged copy, and a brew copy
    /// resolving into its keg.
    fn machine() -> (Vec<PathBuf>, Vec<Install>, Vec<(PathBuf, PathBuf)>) {
        let path = dirs(&[
            "/home/u/.local/bin",
            "/home/u/.cargo/bin",
            "/home/u/.local/bin",
            "/home/u/mise/python/bin",
            "/opt/brew/bin",
        ]);
        let installs = vec![
            install(
                Manager::Uv,
                "0.2.48",
                &["/home/u/.local/share/uv/tools/rumdl/bin/rumdl"],
                Some("/home/u/.local/share/uv/tools/rumdl"),
            ),
            install(
                Manager::Cargo,
                "0.2.29",
                &["/home/u/.cargo/bin/rumdl"],
                None,
            ),
            install(
                Manager::Brew,
                "0.2.42",
                &["/opt/brew/Cellar/rumdl/0.2.42/bin/rumdl"],
                Some("/opt/brew/Cellar/rumdl/0.2.42"),
            ),
        ];
        let links = vec![
            (
                PathBuf::from("/home/u/.local/bin/rumdl"),
                PathBuf::from("/home/u/.local/share/uv/tools/rumdl/bin/rumdl"),
            ),
            (
                PathBuf::from("/opt/brew/bin/rumdl"),
                PathBuf::from("/opt/brew/Cellar/rumdl/0.2.42/bin/rumdl"),
            ),
        ];
        (path, installs, links)
    }

    fn scan_machine(path: &[PathBuf], installs: &[Install]) -> BinaryReport {
        let (_, _, links) = machine();
        let names = vec!["rumdl".to_string()];
        let existing = [
            "/home/u/.local/bin/rumdl",
            "/home/u/.cargo/bin/rumdl",
            "/home/u/mise/python/bin/rumdl",
            "/opt/brew/bin/rumdl",
        ];
        scan(
            path,
            &names,
            installs,
            |p| existing.iter().any(|e| Path::new(e) == p),
            move |p| {
                Some(
                    links
                        .iter()
                        .find(|(from, _)| from == p)
                        .map(|(_, to)| to.clone())
                        .unwrap_or_else(|| p.to_path_buf()),
                )
            },
            |_, _| None,
        )
        .remove(0)
    }

    #[test]
    fn every_copy_is_found_in_path_order_and_attributed_by_file_identity() {
        let (path, installs, _) = machine();
        let report = scan_machine(&path, &installs);
        let found: Vec<(&str, Option<Manager>, Option<&str>)> = report
            .copies
            .iter()
            .map(|c| (c.path.to_str().unwrap(), c.manager, c.version.as_deref()))
            .collect();
        assert_eq!(
            found,
            vec![
                (
                    "/home/u/.local/bin/rumdl",
                    Some(Manager::Uv),
                    Some("0.2.48")
                ),
                (
                    "/home/u/.cargo/bin/rumdl",
                    Some(Manager::Cargo),
                    Some("0.2.29")
                ),
                ("/home/u/mise/python/bin/rumdl", None, None),
                ("/opt/brew/bin/rumdl", Some(Manager::Brew), Some("0.2.42")),
            ]
        );
    }

    #[test]
    fn a_duplicate_path_entry_yields_one_copy() {
        let (path, installs, _) = machine();
        assert_eq!(
            path.iter().filter(|d| d.ends_with(".local/bin")).count(),
            2,
            "the fixture must contain the duplicate this test is about"
        );
        let report = scan_machine(&path, &installs);
        assert_eq!(report.copies.len(), 4);
    }

    #[test]
    fn the_first_path_entry_is_the_winner_and_the_rest_are_shadowed() {
        let (path, installs, _) = machine();
        let report = scan_machine(&path, &installs);
        assert_eq!(
            report.winner().unwrap().path,
            PathBuf::from("/home/u/.local/bin/rumdl")
        );
        assert_eq!(report.shadowed().len(), 3);

        // Reordering `$PATH` reorders the answer: cargo's stale copy now wins.
        let reordered = dirs(&["/home/u/.cargo/bin", "/home/u/.local/bin"]);
        let report = scan_machine(&reordered, &installs);
        assert_eq!(report.winner().unwrap().manager, Some(Manager::Cargo));
        assert_eq!(report.winner().unwrap().version.as_deref(), Some("0.2.29"));
    }

    #[test]
    fn a_hand_copied_binary_is_not_credited_to_the_manager_sharing_its_directory() {
        // uv's shim lives in `/home/u/.local/bin`, but this copy resolves to
        // itself, so directory-based attribution would wrongly call it uv's.
        let installs = vec![install(
            Manager::Uv,
            "0.2.48",
            &["/home/u/.local/share/uv/tools/rumdl/bin/rumdl"],
            Some("/home/u/.local/share/uv/tools/rumdl"),
        )];
        let report = scan(
            &dirs(&["/home/u/.local/bin"]),
            &["vership".to_string()],
            &installs,
            |_| true,
            |p| Some(p.to_path_buf()),
            |_, _| None,
        )
        .remove(0);
        assert_eq!(report.copies.len(), 1);
        assert_eq!(report.winner().unwrap().manager, None);
    }

    #[test]
    fn a_mise_shim_is_the_copy_it_dispatches_to() {
        // mise's shim directory precedes cargo's on `$PATH`; the shim is a
        // symlink to the mise binary, which resolves the name to cargo's copy.
        let path = dirs(&["/home/u/mise/shims", "/home/u/.cargo/bin"]);
        let installs = vec![install(
            Manager::Cargo,
            "0.5.22",
            &["/home/u/.cargo/bin/vership"],
            None,
        )];
        let shim = PathBuf::from("/home/u/mise/shims/vership");
        let mise = PathBuf::from("/home/u/.local/bin/mise");
        let target = PathBuf::from("/home/u/.cargo/bin/vership");
        let scan_with = |dispatch: &dyn Fn(&Path, &str) -> Option<PathBuf>| {
            scan(
                &path,
                &["vership".to_string()],
                &installs,
                |_| true,
                |p| {
                    Some(if p == shim {
                        mise.clone()
                    } else {
                        p.to_path_buf()
                    })
                },
                dispatch,
            )
            .remove(0)
        };

        let report =
            scan_with(&|real, name| (real == mise && name == "vership").then(|| target.clone()));
        let winner = report.winner().unwrap();
        assert_eq!(winner.path, shim);
        assert_eq!(winner.dispatches_to.as_deref(), Some(target.as_path()));
        assert_eq!(winner.manager, Some(Manager::Cargo));
        assert_eq!(winner.version.as_deref(), Some("0.5.22"));
        assert_eq!(
            winner.location(),
            "/home/u/mise/shims/vership -> /home/u/.cargo/bin/vership"
        );
        // Cargo's copy is the target itself, not a second copy behind the shim.
        assert!(report.shadowed().is_empty());

        // Negative control: a shim mise resolves nothing for is a copy of its
        // own, unmanaged, with cargo's copy shadowed behind it.
        let report = scan_with(&|_, _| None);
        let winner = report.winner().unwrap();
        assert_eq!(winner.dispatches_to, None);
        assert_eq!(winner.manager, None);
        assert_eq!(winner.location(), "/home/u/mise/shims/vership");
        assert_eq!(report.shadowed().len(), 1);
    }

    #[test]
    fn only_the_mise_binary_is_asked_where_a_shim_leads() {
        assert!(is_mise(Path::new("/home/u/.local/bin/mise")));
        assert!(is_mise(Path::new("C:/Users/u/AppData/Local/mise/mise.exe")));
        assert!(!is_mise(Path::new("/home/u/.cargo/bin/vership")));
        assert!(!is_mise(Path::new("/home/u/.cargo/bin/mise-en-place")));
        // The production dispatcher never runs anything that is not mise.
        assert_eq!(
            mise_shim_target(Path::new("/home/u/.cargo/bin/vership"), "vership"),
            None
        );
    }

    #[test]
    fn a_copy_inside_a_managers_tree_is_attributed_without_an_exact_bin_path() {
        // npm reports the package directory but no executable paths.
        let installs = vec![Install {
            manager: Manager::Npm,
            package: "@doist/todoist-cli".to_string(),
            version: "1.62.1".to_string(),
            source: Source::Registry,
            bins: vec!["td".to_string()],
            bin_paths: Vec::new(),
            root: Some(PathBuf::from(
                "/opt/brew/lib/node_modules/@doist/todoist-cli",
            )),
        }];
        let report = scan(
            &dirs(&["/opt/brew/bin"]),
            &["td".to_string()],
            &installs,
            |_| true,
            |_| {
                Some(PathBuf::from(
                    "/opt/brew/lib/node_modules/@doist/todoist-cli/dist/index.js",
                ))
            },
            |_, _| None,
        )
        .remove(0);
        assert_eq!(report.winner().unwrap().manager, Some(Manager::Npm));
    }

    #[test]
    fn a_keg_copy_is_attributed_to_brew_even_when_brew_was_not_probed() {
        // Brew is only probed when the project's tap is detected, so a keg copy
        // is routinely present with no brew install to match it against.
        let (path, installs, _) = machine();
        let unprobed: Vec<Install> = installs
            .into_iter()
            .filter(|i| i.manager != Manager::Brew)
            .collect();
        let report = scan_machine(&path, &unprobed);
        let copy = |p: &str| {
            report
                .copies
                .iter()
                .find(|c| c.path.as_path() == Path::new(p))
                .unwrap_or_else(|| panic!("the fixture must contain {p}"))
        };

        let keg = copy("/opt/brew/bin/rumdl");
        assert_eq!(keg.manager, Some(Manager::Brew));
        assert_eq!(keg.version.as_deref(), Some("0.2.42"));

        // Negative control: the copy that sits in no keg stays unmanaged, with
        // no version invented for it.
        let loose = copy("/home/u/mise/python/bin/rumdl");
        assert_eq!(loose.manager, None);
        assert_eq!(loose.version, None);
    }

    #[test]
    fn a_name_absent_from_path_reports_no_copies() {
        let report = scan(
            &dirs(&["/usr/bin"]),
            &["rumdl".to_string()],
            &[],
            |_| false,
            |p| Some(p.to_path_buf()),
            |_, _| None,
        )
        .remove(0);
        assert!(report.copies.is_empty());
        assert!(report.winner().is_none());
    }
}
