pub mod targets;

/// Result of checking one target for a specific version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    /// The exact expected version is live.
    Found(String),
    /// The target exists but serves a different version.
    FoundOld(String),
    /// The target has no trace of the package or version.
    NotFound,
    /// The check itself could not complete (network, auth).
    Error(String),
}

/// One row of the verification report.
#[derive(Debug)]
pub struct TargetReport {
    pub name: String,
    pub ok: bool,
    pub found: Option<String>,
    pub detail: Option<String>,
}

impl TargetReport {
    pub fn from_result(name: &str, result: CheckResult) -> Self {
        match result {
            CheckResult::Found(v) => TargetReport {
                name: name.to_string(),
                ok: true,
                found: Some(v),
                detail: None,
            },
            CheckResult::FoundOld(v) => TargetReport {
                name: name.to_string(),
                ok: false,
                found: Some(v.clone()),
                detail: Some(format!("found {v} instead")),
            },
            CheckResult::NotFound => TargetReport {
                name: name.to_string(),
                ok: false,
                found: None,
                detail: Some("not found".to_string()),
            },
            CheckResult::Error(e) => TargetReport {
                name: name.to_string(),
                ok: false,
                found: None,
                detail: Some(format!("check error: {e}")),
            },
        }
    }
}
