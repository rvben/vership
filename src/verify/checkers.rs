use super::CheckResult;

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
    formula: &str,
    version: &str,
) -> CheckResult {
    let url = format!("{raw_base}/{tap}/HEAD/Formula/{formula}.rb");
    let body = match agent.get(&url).set("User-Agent", USER_AGENT).call() {
        Ok(resp) => match resp.into_string() {
            Ok(body) => body,
            Err(e) => return CheckResult::Error(format!("read formula: {e}")),
        },
        Err(ureq::Error::Status(404, _)) => return CheckResult::NotFound,
        Err(ureq::Error::Status(code, _)) => {
            return CheckResult::Error(format!("HTTP {code}"));
        }
        Err(e) => return CheckResult::Error(e.to_string()),
    };
    if body.contains(version) {
        return CheckResult::Found(version.to_string());
    }
    // Look for whatever version the formula does carry.
    let re = regex::Regex::new(r#"(?:version\s+"|/v)(\d+\.\d+\.\d+)"#).expect("valid regex");
    match re.captures(&body) {
        Some(captures) => CheckResult::FoundOld(captures[1].to_string()),
        None => CheckResult::NotFound,
    }
}

/// ghcr: resolve an anonymous pull token, then probe the tag manifest.
/// Tries the bare version tag first, then the v-prefixed tag.
pub fn ghcr(agent: &ureq::Agent, base: &str, image: &str, version: &str) -> CheckResult {
    let token_url = format!("{base}/token?scope=repository:{image}:pull");
    let token = match agent.get(&token_url).set("User-Agent", USER_AGENT).call() {
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
