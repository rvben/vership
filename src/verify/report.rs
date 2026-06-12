use serde_json::json;

use super::TargetReport;
use crate::output::OutputConfig;

pub fn render(version: &str, reports: &[TargetReport], output: &OutputConfig) {
    let ok = reports.iter().all(|r| r.ok);
    if output.is_json() {
        let targets: Vec<_> = reports
            .iter()
            .map(|r| {
                json!({
                    "name": r.name,
                    "ok": r.ok,
                    "found": r.found,
                    "detail": r.detail,
                })
            })
            .collect();
        println!(
            "{}",
            json!({"version": version, "ok": ok, "targets": targets})
        );
    } else {
        println!("verify {version}");
        for r in reports {
            let mark = if r.ok { "ok  " } else { "FAIL" };
            let detail = match (&r.found, &r.detail) {
                (Some(found), None) => found.clone(),
                (_, Some(detail)) => detail.clone(),
                (None, None) => String::new(),
            };
            println!("  {mark} {:<10} {detail}", r.name);
        }
    }
}
