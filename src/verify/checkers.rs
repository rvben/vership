use super::CheckResult;
use base64::Engine;

pub const CRATES_IO: &str = "https://crates.io";
pub const PYPI: &str = "https://pypi.org";
pub const NPM: &str = "https://registry.npmjs.org";
pub const RAW_GITHUB: &str = "https://raw.githubusercontent.com";
pub const GHCR: &str = "https://ghcr.io";

const USER_AGENT: &str = concat!(
    "vership/",
    env!("CARGO_PKG_VERSION"),
    " (github.com/rvben/vership)"
);

pub fn default_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .build()
}

/// GET a URL, mapping the response to the common found/not-found/error shape.
/// `on_ok` extracts the published version string from a 200 body.
fn get_version(
    agent: &ureq::Agent,
    url: &str,
    on_ok: impl FnOnce(&serde_json::Value) -> Option<String>,
) -> CheckResult {
    let response = agent.get(url).set("User-Agent", USER_AGENT).call();
    match response {
        Ok(resp) => match resp.into_json::<serde_json::Value>() {
            Ok(body) => match on_ok(&body) {
                Some(version) => CheckResult::Found(version),
                None => CheckResult::Error("unexpected response shape".to_string()),
            },
            Err(e) => CheckResult::Error(format!("parse response: {e}")),
        },
        Err(ureq::Error::Status(404, _)) => CheckResult::NotFound,
        Err(ureq::Error::Status(code, _)) => CheckResult::Error(format!("HTTP {code}")),
        Err(e) => CheckResult::Error(e.to_string()),
    }
}

/// crates.io: exact-version endpoint. Requires a User-Agent header.
pub fn crates(agent: &ureq::Agent, base: &str, name: &str, version: &str) -> CheckResult {
    get_version(
        agent,
        &format!("{base}/api/v1/crates/{name}/{version}"),
        |body| body["version"]["num"].as_str().map(String::from),
    )
}

/// PyPI: /pypi/<name>/<version>/json checks the exact version, not latest.
pub fn pypi(agent: &ureq::Agent, base: &str, name: &str, version: &str) -> CheckResult {
    get_version(
        agent,
        &format!("{base}/pypi/{name}/{version}/json"),
        |body| body["info"]["version"].as_str().map(String::from),
    )
}

/// PyPI simple index, the PEP 691 JSON representation an installer negotiates.
/// `name` must already be PEP 503 normalized.
///
/// [`pypi`] above asks `/pypi/<name>/<version>/json`, which no installer reads.
/// PyPI serves the JSON API and the simple index as separately cached
/// documents, so the two can disagree about a version published moments ago,
/// and it is the simple index that decides whether an install resolves. Asking
/// it is how a pre-flight check reaches the same answer the installer will.
///
/// The version list is PEP 700's `versions`. An index that does not carry one
/// is an unexpected shape rather than an absent version, because "this index
/// cannot answer" and "this version is not published" are different facts and
/// only the second one means waiting will help.
pub fn pypi_simple(agent: &ureq::Agent, base: &str, name: &str, version: &str) -> CheckResult {
    let response = agent
        .get(&format!("{base}/simple/{name}/"))
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.pypi.simple.v1+json")
        .call();
    let body = match response {
        Ok(resp) => match resp.into_json::<serde_json::Value>() {
            Ok(body) => body,
            Err(e) => return CheckResult::Error(format!("parse response: {e}")),
        },
        Err(ureq::Error::Status(404, _)) => return CheckResult::NotFound,
        Err(ureq::Error::Status(code, _)) => return CheckResult::Error(format!("HTTP {code}")),
        Err(e) => return CheckResult::Error(e.to_string()),
    };
    let Some(versions) = body["versions"].as_array() else {
        return CheckResult::Error("index serves no PEP 700 version list".to_string());
    };
    match versions.iter().any(|v| v.as_str() == Some(version)) {
        true => CheckResult::Found(version.to_string()),
        // PEP 700 does not order `versions`, so naming the version the index
        // does serve would be a guess. The absence is the actionable fact.
        false => CheckResult::NotFound,
    }
}

/// npm: version manifest path. Scoped names need the slash percent-encoded.
pub fn npm(agent: &ureq::Agent, base: &str, name: &str, version: &str) -> CheckResult {
    let encoded = name.replace('/', "%2f");
    get_version(agent, &format!("{base}/{encoded}/{version}"), |body| {
        body["version"].as_str().map(String::from)
    })
}

/// Homebrew tap: fetch the formula file and look for the version string.
/// Found when the exact version appears; FoundOld when another
/// `version "x.y.z"` or `/vx.y.z/` marker appears instead.
pub fn homebrew(
    agent: &ureq::Agent,
    raw_base: &str,
    tap: &str,
    formulas: &[String],
    version: &str,
) -> CheckResult {
    // Digit boundaries so 1.2.3 never matches inside v1.2.30 or 11.2.3.
    let exact = regex::Regex::new(&format!(
        r"(?:^|[^\d]){}(?:[^\d]|$)",
        regex::escape(version)
    ))
    .expect("valid regex");
    let any_version =
        regex::Regex::new(r#"(?:version\s+"|/v)(\d+\.\d+(?:\.\d+)*)"#).expect("valid regex");

    // The formula is conventionally named after the binary, which may differ
    // from the repo name. Probe each candidate; an exact match on any wins, a
    // stale version on the first existing formula is reported, and only if none
    // of the candidates exist do we report NotFound.
    let mut found_old: Option<String> = None;
    for formula in formulas {
        let url = format!("{raw_base}/{tap}/HEAD/Formula/{formula}.rb");
        let body = match agent.get(&url).set("User-Agent", USER_AGENT).call() {
            Ok(resp) => match resp.into_string() {
                Ok(body) => body,
                Err(e) => return CheckResult::Error(format!("read formula: {e}")),
            },
            Err(ureq::Error::Status(404, _)) => continue,
            Err(ureq::Error::Status(code, _)) => {
                return CheckResult::Error(format!("HTTP {code}"));
            }
            Err(e) => return CheckResult::Error(e.to_string()),
        };
        if exact.is_match(&body) {
            return CheckResult::Found(version.to_string());
        }
        if found_old.is_none()
            && let Some(captures) = any_version.captures(&body)
        {
            found_old = Some(captures[1].to_string());
        }
    }
    match found_old {
        Some(v) => CheckResult::FoundOld(v),
        None => CheckResult::NotFound,
    }
}

/// ghcr: resolve a pull token, then probe the tag manifest. Tries the bare
/// version tag first, then the v-prefixed tag. When `cred` is supplied it is sent
/// as Basic auth on the token request so PRIVATE packages resolve a scoped pull
/// token; without it the request is anonymous (public images still work).
pub fn ghcr(
    agent: &ureq::Agent,
    base: &str,
    image: &str,
    version: &str,
    cred: Option<&(String, String)>,
) -> CheckResult {
    let token_url = format!("{base}/token?scope=repository:{image}:pull");
    let mut token_req = agent.get(&token_url).set("User-Agent", USER_AGENT);
    if let Some((username, secret)) = cred {
        let basic =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{secret}"));
        token_req = token_req.set("Authorization", &format!("Basic {basic}"));
    }
    let token = match token_req.call() {
        Ok(resp) => match resp.into_json::<serde_json::Value>() {
            Ok(body) => match body["token"].as_str() {
                Some(token) => token.to_string(),
                None => return CheckResult::Error("no token in ghcr response".to_string()),
            },
            Err(e) => return CheckResult::Error(format!("parse token: {e}")),
        },
        Err(e) => return CheckResult::Error(format!("ghcr token: {e}")),
    };

    let mut last = CheckResult::NotFound;
    for tag in [version.to_string(), format!("v{version}")] {
        let manifest_url = format!("{base}/v2/{image}/manifests/{tag}");
        let response = agent
            .get(&manifest_url)
            .set("User-Agent", USER_AGENT)
            .set("Authorization", &format!("Bearer {token}"))
            .set(
                "Accept",
                "application/vnd.oci.image.index.v1+json, \
                 application/vnd.docker.distribution.manifest.list.v2+json, \
                 application/vnd.docker.distribution.manifest.v2+json",
            )
            .call();
        match response {
            Ok(_) => return CheckResult::Found(tag),
            Err(ureq::Error::Status(404, _)) => {}
            Err(ureq::Error::Status(code, _)) => {
                last = CheckResult::Error(format!("HTTP {code}"));
            }
            Err(e) => last = CheckResult::Error(e.to_string()),
        }
    }
    last
}

/// Resolve a ghcr Basic-auth credential `(username, secret)`. Order: a
/// `GH_TOKEN` / `GITHUB_TOKEN` env var (CI and explicit scripting), then the
/// inline `auth` in the docker config (`docker login` on a credsStore-less host,
/// e.g. CI runners), then None (anonymous - public images still verify). This
/// lets `verify` reach a private package the operator is logged in to, instead of
/// failing on an anonymous 401.
///
/// It deliberately does NOT invoke docker credential HELPERS (`credsStore` /
/// `credHelpers`, e.g. the macOS `osxkeychain` binary): a helper can block
/// indefinitely or pop a GUI keychain prompt, and `verify` must never hang. On a
/// helper-backed host, pass `GH_TOKEN` (or make the package public) instead.
pub fn resolve_ghcr_credential() -> Option<(String, String)> {
    resolve_ghcr_credential_with(|key| std::env::var(key).ok(), docker_ghcr_credential)
}

/// Pure core of [`resolve_ghcr_credential`] with the env and docker lookups
/// injected, so the precedence rules are unit-testable without touching the real
/// environment or docker config.
fn resolve_ghcr_credential_with(
    env: impl Fn(&str) -> Option<String>,
    docker: impl Fn() -> Option<(String, String)>,
) -> Option<(String, String)> {
    if let Some(token) = env("GH_TOKEN").or_else(|| env("GITHUB_TOKEN"))
        && !token.is_empty()
    {
        // ghcr validates the token (password); the username is cosmetic but must
        // be non-empty. Prefer GITHUB_ACTOR (set in Actions), else the
        // conventional token-auth placeholder.
        let username = env("GITHUB_ACTOR")
            .filter(|actor| !actor.is_empty())
            .unwrap_or_else(|| "x-access-token".to_string());
        return Some((username, token));
    }
    docker()
}

/// Resolve a ghcr credential from the INLINE `auth` in the docker config file
/// (`$DOCKER_CONFIG/config.json` or `~/.docker/config.json`). Never spawns a
/// credential helper, so it cannot block or prompt.
fn docker_ghcr_credential() -> Option<(String, String)> {
    let path = match std::env::var("DOCKER_CONFIG") {
        Ok(dir) if !dir.is_empty() => std::path::PathBuf::from(dir).join("config.json"),
        _ => dirs::home_dir()?.join(".docker").join("config.json"),
    };
    let contents = std::fs::read_to_string(path).ok()?;
    parse_inline_ghcr_auth(&contents)
}

/// Parse the inline ghcr Basic credential from a docker `config.json` body:
/// `auths["ghcr.io"].auth` (or the `https://ghcr.io` key) is base64 of
/// `user:secret`. Returns None when absent, helper-backed (no inline `auth`), or
/// missing a secret.
fn parse_inline_ghcr_auth(config_json: &str) -> Option<(String, String)> {
    let json: serde_json::Value = serde_json::from_str(config_json).ok()?;
    let auths = json.get("auths")?.as_object()?;
    let entry = auths
        .get("ghcr.io")
        .or_else(|| auths.get("https://ghcr.io"))?;
    let encoded = entry.get("auth")?.as_str()?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let pair = String::from_utf8(decoded).ok()?;
    let (username, secret) = pair.split_once(':')?;
    if secret.is_empty() {
        return None;
    }
    Some((username.to_string(), secret.to_string()))
}

/// Interpret `gh release view <tag> --json name,assets` output.
/// A release with zero assets counts as an error, because a publish job
/// that died before uploading leaves an empty release behind.
pub fn parse_release(version: &str, body: &serde_json::Value) -> CheckResult {
    match body["assets"].as_array() {
        Some(assets) if !assets.is_empty() => CheckResult::Found(version.to_string()),
        Some(_) => CheckResult::Error("release exists but has no assets".to_string()),
        None => CheckResult::Error("unexpected gh output".to_string()),
    }
}

/// GitHub release for the tag, via the authenticated gh CLI.
pub fn release(root: &std::path::Path, tag: &str, version: &str) -> CheckResult {
    let output = std::process::Command::new("gh")
        .args(["release", "view", tag, "--json", "name,assets"])
        .current_dir(root)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            match serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                Ok(body) => parse_release(version, &body),
                Err(e) => CheckResult::Error(format!("parse gh output: {e}")),
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("release not found") {
                CheckResult::NotFound
            } else {
                CheckResult::Error(stderr.trim().to_string())
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            CheckResult::Error("gh is not installed".to_string())
        }
        Err(e) => CheckResult::Error(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| {
            owned
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn env_token_resolves_with_x_access_token_username_by_default() {
        let cred = resolve_ghcr_credential_with(env_from(&[("GH_TOKEN", "ght")]), || None);
        assert_eq!(
            cred,
            Some(("x-access-token".to_string(), "ght".to_string()))
        );
    }

    #[test]
    fn github_actor_is_used_as_the_basic_username() {
        let cred = resolve_ghcr_credential_with(
            env_from(&[("GITHUB_TOKEN", "tok"), ("GITHUB_ACTOR", "octocat")]),
            || None,
        );
        assert_eq!(cred, Some(("octocat".to_string(), "tok".to_string())));
    }

    #[test]
    fn gh_token_takes_precedence_over_github_token() {
        let cred = resolve_ghcr_credential_with(
            env_from(&[("GH_TOKEN", "primary"), ("GITHUB_TOKEN", "secondary")]),
            || None,
        );
        assert_eq!(cred.unwrap().1, "primary");
    }

    #[test]
    fn empty_env_token_is_ignored_and_falls_back_to_docker() {
        let cred = resolve_ghcr_credential_with(env_from(&[("GH_TOKEN", "")]), || {
            Some(("dockeruser".to_string(), "dockerpass".to_string()))
        });
        assert_eq!(
            cred,
            Some(("dockeruser".to_string(), "dockerpass".to_string()))
        );
    }

    #[test]
    fn no_env_falls_back_to_docker_credential() {
        let cred = resolve_ghcr_credential_with(env_from(&[]), || {
            Some(("du".to_string(), "dp".to_string()))
        });
        assert_eq!(cred, Some(("du".to_string(), "dp".to_string())));
    }

    #[test]
    fn no_credential_anywhere_is_none() {
        let cred = resolve_ghcr_credential_with(env_from(&[]), || None);
        assert_eq!(cred, None);
    }

    fn config_with_ghcr_auth(key: &str, user_pass: &str) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(user_pass);
        serde_json::json!({ "auths": { key: { "auth": b64 } } }).to_string()
    }

    #[test]
    fn inline_auth_decodes_user_and_secret() {
        let cfg = config_with_ghcr_auth("ghcr.io", "alice:s3cret");
        assert_eq!(
            parse_inline_ghcr_auth(&cfg),
            Some(("alice".to_string(), "s3cret".to_string()))
        );
    }

    #[test]
    fn inline_auth_accepts_https_prefixed_registry_key() {
        let cfg = config_with_ghcr_auth("https://ghcr.io", "bob:tok");
        assert_eq!(
            parse_inline_ghcr_auth(&cfg),
            Some(("bob".to_string(), "tok".to_string()))
        );
    }

    #[test]
    fn inline_auth_absent_or_helper_backed_is_none() {
        // credsStore entry with no inline `auth` (helper-backed) -> None.
        let helper = serde_json::json!({ "auths": { "ghcr.io": {} }, "credsStore": "osxkeychain" })
            .to_string();
        assert_eq!(parse_inline_ghcr_auth(&helper), None);
        // A different registry only.
        let other = config_with_ghcr_auth("registry.example.com", "u:p");
        assert_eq!(parse_inline_ghcr_auth(&other), None);
        // Empty config.
        assert_eq!(parse_inline_ghcr_auth("{}"), None);
    }

    #[test]
    fn inline_auth_with_empty_secret_is_none() {
        let cfg = config_with_ghcr_auth("ghcr.io", "alice:");
        assert_eq!(parse_inline_ghcr_auth(&cfg), None);
    }
}
