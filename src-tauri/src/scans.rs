//! Repository security scans with OpenAI's Codex Security plugin: finding the
//! plugin and its skills, reading progress from its tool calls, and importing
//! the files a finished scan writes. The engine runs the scans
//! (`engine/scans.rs`); everything here is plain data handling.

use crate::types::{Finding, ScanProgress};
use serde::Serialize;
use serde_json::Value;
use std::path::{Component, Path};

pub const PLUGIN: &str = "codex-security";
/// The files a scan keeps once it is imported; `artifacts/` is the bulk.
pub const KEPT_FILES: [&str; 4] = [
    "findings.json",
    "report.md",
    "scan-manifest.json",
    "coverage.json",
];
/// More than any real scan reports; keeps a malformed file from flooding Orbit.
const MAX_FINDINGS: usize = 2000;
const MAX_FINDINGS_BYTES: u64 = 64 * 1024 * 1024;
const MAX_REPORT_BYTES: u64 = 4 * 1024 * 1024;

/// Whether the Codex Security plugin can be used, as shown before a scan.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    /// A marketplace Codex knows offers the plugin.
    pub available: bool,
    pub installed: bool,
    pub version: Option<String>,
}
/// Where the plugin is listed, so it can be installed and its skills found.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginSource {
    pub status: PluginStatus,
    /// The plugin's id (`codex-security@marketplace`), which its skills carry.
    pub id: String,
    pub marketplace_name: String,
    pub marketplace_path: Option<String>,
    /// A remote marketplace's own id for the plugin, which Codex expects as
    /// the plugin's name there.
    pub remote_plugin_id: Option<String>,
}
impl PluginSource {
    /// The `plugin/install` parameters for this plugin.
    pub fn params(&self) -> Value {
        match (&self.marketplace_path, &self.remote_plugin_id) {
            (Some(path), _) => serde_json::json!({"marketplacePath": path, "pluginName": PLUGIN}),
            (None, remote) => serde_json::json!({
                "remoteMarketplaceName": self.marketplace_name,
                "pluginName": remote,
            }),
        }
    }
}
/// Finds the plugin in a `plugin/list` response. An installed copy wins over
/// one that is only offered. A remote marketplace's entry without its remote
/// id cannot be installed, so it is skipped.
pub fn find_plugin(list: &Value) -> Option<PluginSource> {
    let mut found: Option<PluginSource> = None;
    for market in list["marketplaces"].as_array().into_iter().flatten() {
        let marketplace_path = market["path"].as_str().map(str::to_owned);
        for plugin in market["plugins"].as_array().into_iter().flatten() {
            let named = plugin["name"].as_str() == Some(PLUGIN)
                || plugin["id"]
                    .as_str()
                    .is_some_and(|id| id.split('@').next() == Some(PLUGIN));
            let remote_plugin_id = plugin["remotePluginId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
            if !named || (marketplace_path.is_none() && remote_plugin_id.is_none()) {
                continue;
            }
            let installed = plugin["installed"].as_bool().unwrap_or(false);
            if found
                .as_ref()
                .is_some_and(|f| f.status.installed || !installed)
            {
                continue;
            }
            let marketplace_name: String = market["name"].as_str().unwrap_or_default().into();
            found = Some(PluginSource {
                status: PluginStatus {
                    available: true,
                    installed,
                    version: plugin["localVersion"]
                        .as_str()
                        .or_else(|| plugin["version"].as_str())
                        .map(str::to_owned),
                },
                id: plugin["id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{PLUGIN}@{marketplace_name}")),
                marketplace_name,
                marketplace_path: marketplace_path.clone(),
                remote_plugin_id,
            });
        }
    }
    found
}
/// The skill a scan mode runs.
pub fn skill_for(mode: &str) -> Option<&'static str> {
    match mode {
        "standard" => Some("security-scan"),
        "deep" => Some("deep-security-scan"),
        _ => None,
    }
}
/// The name and `SKILL.md` path of the plugin's skill in a `skills/list`
/// response. Only a skill the installed plugin provides counts, so a skill of
/// the same name elsewhere is never run. Names may carry the plugin's name as
/// a prefix (`codex-security:security-scan`).
pub fn find_skill(list: &Value, plugin_id: &str, skill: &str) -> Option<(String, String)> {
    list["data"]
        .as_array()?
        .iter()
        .flat_map(|entry| entry["skills"].as_array().into_iter().flatten())
        .find(|s| {
            let name = s["name"].as_str().unwrap_or_default();
            s["pluginId"].as_str() == Some(plugin_id)
                && s["enabled"].as_bool() != Some(false)
                && (name == skill || name.rsplit(':').next() == Some(skill))
                && s["path"].as_str().is_some_and(|p| !p.is_empty())
        })
        .and_then(|s| {
            Some((
                s["name"].as_str()?.to_owned(),
                s["path"].as_str()?.to_owned(),
            ))
        })
}

/// The plugin's own scans for an Orbit scan, from `list_codex_security_scans`:
/// those run on the scan's copy or from its thread, with each one's status.
pub fn plugin_scans(list: &Value, id: &str, thread: Option<&str>) -> Vec<(String, String)> {
    list["scans"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|x| {
            x["targetPath"]
                .as_str()
                .is_some_and(|p| Path::new(p).file_name().is_some_and(|n| n == id))
                || (thread.is_some() && x["continuationThreadId"].as_str() == thread)
        })
        .filter_map(|x| {
            Some((
                x["scanId"].as_str()?.to_owned(),
                x["progress"]["status"]
                    .as_str()
                    .or_else(|| x["status"].as_str())
                    .unwrap_or("running")
                    .to_owned(),
            ))
        })
        .collect()
}

/// A tool call's arguments, which Codex may send as an object or as text.
fn arguments(item: &Value) -> Value {
    match &item["arguments"] {
        Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        other => other.clone(),
    }
}
fn structured(item: &Value) -> &Value {
    let result = &item["result"];
    if result["structuredContent"].is_null() {
        &result["structured_content"]
    } else {
        &result["structuredContent"]
    }
}
fn count(v: &Value) -> Option<u64> {
    v.as_u64()
}
/// What a Codex Security tool call tells Orbit about the scan.
#[derive(Debug, Default, PartialEq)]
pub struct ToolUpdate {
    pub progress: Option<ScanProgress>,
    pub plugin_scan_id: Option<String>,
    pub scan_dir: Option<String>,
    pub daybreak: Option<String>,
}
/// Reads a `mcpToolCall` item from the plugin's tool server. `completed` is
/// false while the call runs, when only its arguments are known.
pub fn tool_update(item: &Value, completed: bool, at: &str) -> ToolUpdate {
    let mut update = ToolUpdate::default();
    if item["type"] != "mcpToolCall" {
        return update;
    }
    let args = arguments(item);
    let result = structured(item);
    match item["tool"].as_str().unwrap_or_default() {
        "update_codex_security_scan_progress" => {
            let phase = args["phase"].as_str().map(str::to_owned);
            let (completed, total, unit) = if args["reviewItemsTotal"].is_u64() {
                (
                    count(&args["reviewItemsCompleted"]),
                    count(&args["reviewItemsTotal"]),
                    Some("reviews".to_string()),
                )
            } else {
                (
                    count(&args["phaseItemsCompleted"]),
                    count(&args["phaseItemsTotal"]),
                    args["phaseProgressUnit"].as_str().map(str::to_owned),
                )
            };
            if phase.is_some() || total.is_some() {
                update.progress = Some(ScanProgress {
                    phase,
                    completed,
                    total,
                    unit,
                    findings: count(&args["reportableFindingsCount"]),
                    updated_at: at.into(),
                });
            }
        }
        "start_codex_security_deep_scan" if !completed => {
            update.progress = Some(ScanProgress {
                phase: Some("discovery".into()),
                updated_at: at.into(),
                ..ScanProgress::default()
            });
        }
        "start_codex_security_deep_scan" => {
            update.scan_dir = result["manifestPath"]
                .as_str()
                .and_then(|p| Path::new(p).parent())
                .map(|p| p.to_string_lossy().into_owned());
        }
        "start_codex_security_standard_scan" | "start_codex_security_prompt_only_scan"
            if completed =>
        {
            update.plugin_scan_id = result["scanId"].as_str().map(str::to_owned);
            update.scan_dir = result["scanDir"].as_str().map(str::to_owned);
        }
        "get_codex_security_daybreak_access" if completed => {
            update.daybreak = result["status"].as_str().map(str::to_owned);
        }
        _ => {}
    }
    update
}
/// The latest `CODEX_SECURITY_SCAN_PROGRESS {...}` marker in some output.
pub fn progress_marker(text: &str, at: &str) -> Option<ScanProgress> {
    const MARKER: &str = "CODEX_SECURITY_SCAN_PROGRESS";
    let line = text.lines().rev().find(|l| l.contains(MARKER))?;
    let json = &line[line.find(MARKER)? + MARKER.len()..];
    let v: Value = serde_json::from_str(json.trim()).ok()?;
    Some(ScanProgress {
        phase: v["phase"].as_str().map(str::to_owned),
        completed: count(&v["filesCompleted"]),
        total: count(&v["filesTotal"]),
        unit: Some("files".into()),
        findings: None,
        updated_at: at.into(),
    })
}

fn priority(level: &str) -> u8 {
    match level {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        _ => 3,
    }
}
fn confidence(level: &str) -> f64 {
    match level {
        "high" => 0.9,
        "medium" => 0.6,
        "low" => 0.3,
        _ => 0.5,
    }
}
/// A path inside the repository, or None for anything that could point
/// elsewhere (absolute, `..`, or empty).
fn repository_path(path: &str) -> Option<String> {
    let p = Path::new(path.trim_start_matches("./"));
    let inside = !path.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    inside.then(|| p.to_string_lossy().replace('\\', "/"))
}
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| {
        c.to_uppercase().collect::<String>() + chars.as_str()
    })
}
/// The finding's explanation as Markdown: summary, severity, weakness, the
/// suggested fix and tests, and any other locations.
fn body(f: &Value, level: &str, shown: Option<&Value>) -> String {
    let mut parts = vec![];
    if let Some(summary) = f["summary"].as_str().filter(|s| !s.trim().is_empty()) {
        parts.push(summary.trim().to_string());
    }
    let mut facts = vec![format!("**Severity:** {}", capitalized(level))];
    if let Some(score) = f["severity"]["score"].as_f64() {
        let system = f["severity"]["scoringSystem"].as_str().unwrap_or("score");
        facts[0].push_str(&format!(" ({system} {score})"));
    }
    let cwe: Vec<&str> = f["taxonomy"]["cwe"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !cwe.is_empty() {
        facts.push(format!("**Weakness:** {}", cwe.join(", ")));
    }
    if let Some(level) = f["confidence"]["level"].as_str() {
        facts.push(format!("**Confidence:** {}", capitalized(level)));
    }
    parts.push(facts.join(" · "));
    if let Some(rationale) = f["confidence"]["rationale"].as_str() {
        parts.push(rationale.trim().to_string());
    }
    let remediation = match &f["remediation"] {
        Value::String(text) => Some(text.clone()),
        Value::Object(o) => o.get("summary").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    };
    if let Some(fix) = remediation.filter(|t| !t.trim().is_empty()) {
        parts.push(format!("**Suggested fix:** {}", fix.trim()));
    }
    let tests: Vec<String> = f["remediationTests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|t| format!("- {}", t.trim()))
        .collect();
    if !tests.is_empty() {
        parts.push(format!("**Suggested tests:**\n\n{}", tests.join("\n")));
    }
    let others: Vec<String> = f["locations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| !shown.is_some_and(|shown| std::ptr::eq(*l, shown)))
        .filter_map(|l| {
            let path = l["path"].as_str()?;
            let line = l["startLine"].as_u64().unwrap_or(1);
            let role = l["role"]
                .as_str()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default();
            Some(format!("- `{path}:{line}`{role}"))
        })
        .collect();
    if !others.is_empty() {
        parts.push(format!("**Other locations:**\n\n{}", others.join("\n")));
    }
    // Findings are kept in Orbit's state, so a long write-up is cut short;
    // the full text stays in the kept findings.json and report.
    let text = parts.join("\n\n");
    text.chars().take(8_000).collect()
}
/// The findings of a `findings.json` file, most severe first. Each ID is
/// prefixed with Orbit's scan ID, since the plugin reuses a finding's ID when
/// a later scan finds it again.
pub fn parse_findings(doc: &Value, scan_id: &str) -> Result<Vec<Finding>, String> {
    if let Some(kind) = doc["documentType"].as_str() {
        if kind != "codex-security.findings" {
            return Err(format!("findings.json is a {kind} document."));
        }
    }
    let list = doc["findings"]
        .as_array()
        .ok_or("findings.json has no findings list.")?;
    let mut findings: Vec<Finding> = list
        .iter()
        .take(MAX_FINDINGS)
        .enumerate()
        .map(|(index, f)| {
            let level = f["severity"]["level"].as_str().unwrap_or("medium");
            // The sink is where the harm happens, so it is the place to look.
            let locations = f["locations"].as_array();
            let location = locations
                .and_then(|l| l.iter().find(|l| l["role"] == "sink"))
                .or_else(|| locations.and_then(|l| l.first()));
            let start = location
                .and_then(|l| l["startLine"].as_u64())
                .unwrap_or(1)
                .clamp(1, u32::MAX.into()) as u32;
            let end = location
                .and_then(|l| l["endLine"].as_u64())
                .unwrap_or(start.into())
                .clamp(start.into(), u32::MAX.into()) as u32;
            let finding_id = f["findingId"]
                .as_str()
                .map_or_else(|| index.to_string(), str::to_owned);
            Finding {
                id: format!("{scan_id}:{finding_id}"),
                title: f["title"]
                    .as_str()
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or("Untitled finding")
                    .trim()
                    .chars()
                    .take(300)
                    .collect(),
                priority: priority(level),
                path: location
                    .and_then(|l| l["path"].as_str())
                    .and_then(repository_path),
                start_line: start,
                end_line: end,
                body: body(f, level, location),
                confidence: confidence(f["confidence"]["level"].as_str().unwrap_or("")),
            }
        })
        .collect();
    findings.sort_by_key(|f| f.priority);
    Ok(findings)
}

/// What importing a finished scan's folder produced.
#[derive(Debug, Default, PartialEq)]
pub struct Imported {
    pub findings: Vec<Finding>,
    pub has_report: bool,
    /// How the plugin's sealed manifest says the scan ended: `completed`,
    /// `failed`, `canceled` or `interrupted`.
    pub manifest_status: String,
    /// Problems that did not stop the import, such as artifacts left behind.
    pub warnings: Vec<String>,
}
/// Whether `path` is a real directory inside `root`, so Orbit may delete it.
pub fn owned_dir(root: &Path, path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return false;
    }
    match (root.canonicalize(), path.canonicalize()) {
        (Ok(root), Ok(path)) => path != root && path.starts_with(root),
        _ => false,
    }
}
fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.len() > limit {
        return Err(format!("{} is too large to import.", path.display()));
    }
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}
/// The plugin scans an Orbit scan may be, and the commit it scanned, so a
/// manifest from any other scan is refused.
#[derive(Debug, Default)]
pub struct Expected {
    pub plugin_scan_ids: Vec<String>,
    pub commit: Option<String>,
}
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
/// Checks the plugin's completion record, `scan-manifest.json`: it must be
/// the plugin's sealed manifest for one of the expected scans and commit,
/// with every sealed file unchanged. A turn that ended is not proof the
/// scan finished: the plugin seals the manifest only when it completes,
/// fails or is cancelled, so a draft or missing manifest is refused. Returns
/// the status the manifest records.
pub fn completion(dir: &Path, expected: &Expected) -> Result<String, String> {
    let bytes = read_limited(&dir.join("scan-manifest.json"), MAX_FINDINGS_BYTES)
        .map_err(|_| "it wrote no scan-manifest.json".to_string())?;
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "its scan-manifest.json is not valid JSON".to_string())?;
    if manifest["documentType"] != "codex-security.scan-manifest" {
        return Err("its scan-manifest.json is not a Codex Security manifest".into());
    }
    let scan = &manifest["scan"];
    let status = scan["status"].as_str().unwrap_or_default();
    if !matches!(status, "completed" | "failed" | "canceled" | "interrupted") {
        return Err("its manifest records no finished status".into());
    }
    let sealed = scan["sealedAt"].as_str().filter(|t| !t.is_empty());
    if sealed.is_none() || sealed != scan["completedAt"].as_str() {
        return Err("its manifest was never sealed".into());
    }
    let id = scan["id"].as_str().unwrap_or_default();
    if !expected.plugin_scan_ids.iter().any(|x| x == id) {
        return Err(if expected.plugin_scan_ids.is_empty() {
            "Orbit could not tell which Codex Security scan was this one".into()
        } else {
            format!("its manifest belongs to another scan ({id})")
        });
    }
    if let (Some(commit), Some(revision)) = (&expected.commit, scan["target"]["revision"].as_str())
    {
        if revision != commit {
            return Err(format!("its manifest is for commit {revision}"));
        }
    }
    if scan["findingsRef"] != "findings.json" || scan["coverageRef"] != "coverage.json" {
        return Err("its manifest does not point at findings.json and coverage.json".into());
    }
    let artifacts = scan["artifacts"].as_array().filter(|a| !a.is_empty());
    let artifacts = artifacts.ok_or("its manifest lists no sealed files")?;
    let mut sealed = vec![];
    for artifact in artifacts {
        let path = artifact["path"].as_str().unwrap_or_default();
        let relative =
            repository_path(path).ok_or("its manifest lists a file outside its folder")?;
        let contents = read_limited(&dir.join(&relative), MAX_FINDINGS_BYTES)
            .map_err(|_| format!("its sealed {relative} is missing"))?;
        if artifact["sha256"].as_str() != Some(sha256_hex(&contents).as_str()) {
            return Err(format!("its sealed {relative} has changed"));
        }
        sealed.push((relative, contents));
    }
    // The plugin seals its findings and coverage together, each naming the
    // scan it belongs to.
    for (name, kind) in [
        ("findings.json", "codex-security.findings"),
        ("coverage.json", "codex-security.coverage"),
    ] {
        let (_, contents) = sealed
            .iter()
            .find(|(path, _)| path == name)
            .ok_or_else(|| format!("its manifest does not seal {name}"))?;
        let doc: Value = serde_json::from_slice(contents)
            .map_err(|_| format!("its sealed {name} is not valid JSON"))?;
        if doc["documentType"] != kind || doc["scanId"] != id {
            return Err(format!("its sealed {name} is not this scan's"));
        }
    }
    Ok(status.into())
}
/// Imports a finished scan from the plugin's folder `dir`: reads its findings,
/// copies the files worth keeping to `kept`, and, unless `keep_artifacts`,
/// deletes the bulky `artifacts/` folder. Only a folder inside `scan_root`
/// (where Orbit asks the plugin to work) is ever deleted from.
///
/// Nothing is imported or deleted unless the plugin's sealed manifest checks
/// out ([`completion`]), and working files are deleted only after a scan the
/// plugin records as completed.
pub fn import(
    dir: &Path,
    kept: &Path,
    scan_root: &Path,
    scan_id: &str,
    expected: &Expected,
    keep_artifacts: bool,
) -> Result<Imported, String> {
    let status = completion(dir, expected).map_err(|reason| {
        format!(
            "Codex Security did not record this scan as finished: {reason}. Its files are kept in {}.",
            dir.display()
        )
    })?;
    let findings_path = dir.join("findings.json");
    let doc: Value = serde_json::from_slice(&read_limited(&findings_path, MAX_FINDINGS_BYTES)?)
        .map_err(|e| format!("findings.json is not valid JSON: {e}"))?;
    let findings = parse_findings(&doc, scan_id)?;
    std::fs::create_dir_all(kept).map_err(|e| e.to_string())?;
    let mut imported = Imported {
        findings,
        manifest_status: status,
        ..Imported::default()
    };
    for name in KEPT_FILES {
        let from = dir.join(name);
        if from.is_file() {
            std::fs::copy(&from, kept.join(name)).map_err(|e| format!("{name}: {e}"))?;
            imported.has_report |= name == "report.md";
        }
    }
    let artifacts = dir.join("artifacts");
    if !keep_artifacts && imported.manifest_status == "completed" && artifacts.exists() {
        if owned_dir(scan_root, dir) && owned_dir(scan_root, &artifacts) {
            if let Err(e) = std::fs::remove_dir_all(&artifacts) {
                imported
                    .warnings
                    .push(format!("Could not delete the scan's working files: {e}"));
            }
        } else {
            imported.warnings.push(format!(
                "The scan's working files are outside Orbit's folder, so they were left in {}.",
                artifacts.display()
            ));
        }
    }
    Ok(imported)
}
/// The kept `report.md`, for reading in Orbit.
pub fn report(kept: &Path) -> Result<String, String> {
    let bytes = read_limited(&kept.join("report.md"), MAX_REPORT_BYTES)
        .map_err(|_| "This scan has no report.".to_string())?;
    String::from_utf8(bytes).map_err(|_| "The report is not valid text.".into())
}

/// Writes a sealed manifest the way the plugin does when a scan ends.
#[cfg(test)]
pub(crate) fn seal(dir: &Path, plugin_scan_id: &str, commit: &str, status: &str) {
    // The plugin's findings and coverage name the scan they belong to.
    if let Ok(bytes) = std::fs::read(dir.join("findings.json")) {
        if let Ok(mut doc) = serde_json::from_slice::<Value>(&bytes) {
            doc["documentType"] = "codex-security.findings".into();
            doc["scanId"] = plugin_scan_id.into();
            std::fs::write(dir.join("findings.json"), doc.to_string()).unwrap();
        }
    }
    let coverage = serde_json::json!({"documentType": "codex-security.coverage", "schemaVersion": "1.0",
        "scanId": plugin_scan_id, "mode": "standard", "completeness": "complete"});
    std::fs::write(dir.join("coverage.json"), coverage.to_string()).unwrap();
    let artifacts: Vec<Value> = ["findings.json", "coverage.json"]
        .iter()
        .filter_map(|name| {
            let bytes = std::fs::read(dir.join(name)).ok()?;
            Some(serde_json::json!({"path": name, "sha256": sha256_hex(&bytes), "mediaType": "application/json"}))
        })
        .collect();
    let manifest = serde_json::json!({
        "documentType": "codex-security.scan-manifest",
        "schemaVersion": "1.0",
        "scan": {
            "id": plugin_scan_id, "status": status,
            "producer": {"name": "codex-security-plugin", "version": "0.1.24"},
            "startedAt": "2026-10-07T00:00:00Z", "completedAt": "2026-10-07T01:00:00Z", "sealedAt": "2026-10-07T01:00:00Z",
            "target": {"kind": "git_worktree", "targetId": "t", "displayName": "a/b", "revision": commit},
            "scope": {"includePaths": ["."], "excludePaths": []},
            "coverageRef": "coverage.json", "findingsRef": "findings.json",
            "artifacts": artifacts,
        },
    });
    std::fs::write(dir.join("scan-manifest.json"), manifest.to_string()).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const EXAMPLE: &str = include_str!("scan_fixture/findings.json");

    #[test]
    fn finds_the_plugin_and_its_skills() {
        // Shaped like Codex's plugin/list: a remote marketplace has no path,
        // and its plugins carry their own remote id.
        let list = json!({"marketplaces": [
            {"name": "local", "path": "/m/local.json", "plugins": [{"id": "other@local", "name": "other", "installed": true}]},
            {"name": "openai-curated", "path": null, "plugins": [
                {"id": "codex-security@openai-curated", "remotePluginId": "plugin_6f1c2a", "name": "codex-security",
                 "installed": false, "version": "0.1.24"}
            ]},
        ]});
        let found = find_plugin(&list).unwrap();
        assert_eq!(
            found.status,
            PluginStatus {
                available: true,
                installed: false,
                version: Some("0.1.24".into())
            }
        );
        assert_eq!(found.id, "codex-security@openai-curated");
        assert_eq!(
            found.params(),
            json!({"remoteMarketplaceName": "openai-curated", "pluginName": "plugin_6f1c2a"})
        );
        // A remote entry without its remote id cannot be installed.
        let mut unnamed = list.clone();
        unnamed["marketplaces"][1]["plugins"][0]["remotePluginId"] = Value::Null;
        assert_eq!(find_plugin(&unnamed), None);
        // An installed copy in another marketplace wins.
        let mut both = list.clone();
        both["marketplaces"][0]["plugins"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "codex-security@local", "name": "codex-security", "installed": true, "localVersion": "0.1.25"}));
        let found = find_plugin(&both).unwrap();
        assert!(found.status.installed);
        assert_eq!(found.status.version.as_deref(), Some("0.1.25"));
        assert_eq!(found.id, "codex-security@local");
        assert_eq!(
            found.params(),
            json!({"marketplacePath": "/m/local.json", "pluginName": "codex-security"})
        );
        assert_eq!(find_plugin(&json!({"marketplaces": []})), None);

        // Shaped like skills/list. Only the plugin's own skills count.
        let skills = json!({"data": [{"cwd": "/", "errors": [], "skills": [
            {"name": "security-scan", "path": "/repo/.codex/skills/security-scan/SKILL.md", "scope": "repo", "enabled": true},
            {"name": "codex-security:deep-security-scan", "path": "/c/plugins/codex-security/skills/deep-security-scan/SKILL.md",
             "scope": "user", "enabled": true, "pluginId": "codex-security@openai-curated"},
            {"name": "codex-security:security-scan", "path": "/c/plugins/codex-security/skills/security-scan/SKILL.md",
             "scope": "user", "enabled": true, "pluginId": "codex-security@openai-curated"},
        ]}]});
        let plugin = "codex-security@openai-curated";
        assert_eq!(
            find_skill(&skills, plugin, skill_for("standard").unwrap()),
            Some((
                "codex-security:security-scan".into(),
                "/c/plugins/codex-security/skills/security-scan/SKILL.md".into()
            ))
        );
        assert_eq!(
            find_skill(&skills, plugin, skill_for("deep").unwrap())
                .unwrap()
                .0,
            "codex-security:deep-security-scan"
        );
        assert_eq!(
            find_skill(&skills, "codex-security@local", "security-scan"),
            None
        );
        let mut disabled = skills.clone();
        disabled["data"][0]["skills"][2]["enabled"] = json!(false);
        assert_eq!(find_skill(&disabled, plugin, "security-scan"), None);
        assert_eq!(skill_for("diff"), None);
    }

    #[test]
    fn matches_the_plugins_scans_to_an_orbit_scan() {
        let list = json!({"scans": [
            {"scanId": "p1", "targetPath": "/o/scan-worktrees/scan-1", "progress": {"status": "running"}},
            {"scanId": "p2", "targetPath": "/elsewhere", "continuationThreadId": "t1", "progress": {"status": "canceled"}},
            {"scanId": "p3", "targetPath": "/o/scan-worktrees/scan-10", "progress": {"status": "running"}},
        ]});
        assert_eq!(
            plugin_scans(&list, "scan-1", Some("t1")),
            vec![
                ("p1".to_string(), "running".to_string()),
                ("p2".to_string(), "canceled".to_string())
            ]
        );
        assert!(plugin_scans(&list, "scan-2", None).is_empty());
    }

    #[test]
    fn reads_progress_and_the_scan_folder_from_tool_calls() {
        let progress = json!({
            "type": "mcpToolCall", "server": "codex-security", "tool": "update_codex_security_scan_progress",
            "arguments": {"scanId": "s", "phase": "discovery", "reviewItemsCompleted": 3, "reviewItemsTotal": 8, "reportableFindingsCount": 1},
        });
        let update = tool_update(&progress, false, "t");
        assert_eq!(
            update.progress,
            Some(ScanProgress {
                phase: Some("discovery".into()),
                completed: Some(3),
                total: Some(8),
                unit: Some("reviews".into()),
                findings: Some(1),
                updated_at: "t".into(),
            })
        );
        // Arguments sent as text, and phase-level counts.
        let text = json!({
            "type": "mcpToolCall", "tool": "update_codex_security_scan_progress",
            "arguments": "{\"phase\":\"validation\",\"phaseItemsCompleted\":1,\"phaseItemsTotal\":4,\"phaseProgressUnit\":\"candidates\"}",
        });
        let p = tool_update(&text, true, "t").progress.unwrap();
        assert_eq!(
            (p.phase.as_deref(), p.completed, p.total, p.unit.as_deref()),
            (Some("validation"), Some(1), Some(4), Some("candidates"))
        );
        let started = json!({
            "type": "mcpToolCall", "tool": "start_codex_security_standard_scan",
            "result": {"content": [], "structuredContent": {"scanId": "abc", "scanDir": "/scans/abc"}},
        });
        let update = tool_update(&started, true, "t");
        assert_eq!(update.plugin_scan_id.as_deref(), Some("abc"));
        assert_eq!(update.scan_dir.as_deref(), Some("/scans/abc"));
        let deep = json!({
            "type": "mcpToolCall", "tool": "start_codex_security_deep_scan",
            "result": {"structuredContent": {"manifestPath": "/scans/deep/scan-manifest.json"}},
        });
        assert_eq!(
            tool_update(&deep, true, "t").scan_dir.as_deref(),
            Some("/scans/deep")
        );
        assert_eq!(
            tool_update(&deep, false, "t")
                .progress
                .unwrap()
                .phase
                .as_deref(),
            Some("discovery")
        );
        let daybreak = json!({
            "type": "mcpToolCall", "tool": "get_codex_security_daybreak_access",
            "result": {"structuredContent": {"status": "not_granted"}},
        });
        assert_eq!(
            tool_update(&daybreak, true, "t").daybreak.as_deref(),
            Some("not_granted")
        );
        assert_eq!(
            tool_update(&json!({"type": "agentMessage"}), true, "t"),
            ToolUpdate::default()
        );

        let output = "working\nCODEX_SECURITY_SCAN_PROGRESS {\"phase\":\"discovery\",\"filesCompleted\":3,\"filesTotal\":8}\nmore";
        let p = progress_marker(output, "t").unwrap();
        assert_eq!(
            (p.completed, p.total, p.unit.as_deref()),
            (Some(3), Some(8), Some("files"))
        );
        assert_eq!(progress_marker("nothing here", "t"), None);
    }

    #[test]
    fn turns_scan_findings_into_orbit_findings() {
        let doc: Value = serde_json::from_str(EXAMPLE).unwrap();
        let findings = parse_findings(&doc, "scan-1").unwrap();
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "scan-1:csf_orbit_fixture_0001");
        assert_eq!(f.title, "Search query is built from request text");
        assert_eq!(f.priority, 1);
        assert_eq!(f.path.as_deref(), Some("server/db/search.ts"));
        assert_eq!((f.start_line, f.end_line), (30, 33));
        assert_eq!(f.confidence, 0.9);
        assert!(f.body.starts_with("The search handler joins"));
        assert!(f.body.contains("**Severity:** High (CVSS:3.1 8.6)"));
        assert!(f.body.contains("**Weakness:** CWE-89"));
        assert!(f.body.contains("**Suggested fix:** Pass the search text"));
        assert!(f.body.contains("- Search for"));
        assert!(f.body.contains("- `server/routes/search.ts:12` (source)"));

        // Severity order, paths that leave the repository, and missing fields.
        let doc = json!({"findings": [
            {"title": "Low", "severity": {"level": "informational"}, "locations": [{"path": "../etc/passwd", "startLine": 5, "endLine": 2}]},
            {"title": "Critical", "severity": {"level": "critical"}, "locations": [
                {"path": "/abs/a.rs", "startLine": 1, "role": "source"},
                {"path": "./src/b.rs", "startLine": 9, "role": "sink"},
            ]},
            {},
        ]});
        let findings = parse_findings(&doc, "s").unwrap();
        let summary: Vec<_> = findings
            .iter()
            .map(|f| {
                (
                    f.title.as_str(),
                    f.priority,
                    f.path.as_deref(),
                    f.start_line,
                    f.end_line,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Critical", 0, Some("src/b.rs"), 9, 9),
                ("Untitled finding", 2, None, 1, 1),
                ("Low", 3, None, 5, 5),
            ]
        );
        assert_eq!(findings[1].id, "s:2");
        assert!(parse_findings(&json!({"documentType": "codex-security.coverage"}), "s").is_err());
        assert!(parse_findings(&json!({}), "s").is_err());
    }

    #[test]
    fn imports_a_finished_scan_and_removes_its_working_files() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("scan-output");
        let dir = root.join("abc");
        std::fs::create_dir_all(dir.join("artifacts/01_context")).unwrap();
        std::fs::write(dir.join("artifacts/01_context/big.json"), "{}").unwrap();
        std::fs::write(dir.join("findings.json"), EXAMPLE).unwrap();
        std::fs::write(dir.join("report.md"), "# Report").unwrap();
        seal(&dir, "p-1", "abc123", "completed");
        let expected = Expected {
            plugin_scan_ids: vec!["p-1".into()],
            commit: Some("abc123".into()),
        };
        let kept = d.path().join("scans/scan-1");
        let imported = import(&dir, &kept, &root, "scan-1", &expected, false).unwrap();
        assert_eq!(imported.findings.len(), 1);
        assert!(imported.has_report);
        assert_eq!(imported.manifest_status, "completed");
        assert!(imported.warnings.is_empty());
        assert!(!dir.join("artifacts").exists());
        assert!(kept.join("findings.json").is_file());
        assert!(kept.join("scan-manifest.json").is_file());
        assert!(kept.join("coverage.json").is_file());
        assert_eq!(report(&kept).unwrap(), "# Report");

        // Kept when asked, and never deleted outside Orbit's folder.
        std::fs::create_dir_all(dir.join("artifacts")).unwrap();
        import(&dir, &kept, &root, "scan-1", &expected, true).unwrap();
        assert!(dir.join("artifacts").exists());
        let elsewhere = d.path().join("tmp-scan");
        std::fs::create_dir_all(elsewhere.join("artifacts")).unwrap();
        std::fs::write(elsewhere.join("findings.json"), EXAMPLE).unwrap();
        seal(&elsewhere, "p-1", "abc123", "completed");
        let imported = import(&elsewhere, &kept, &root, "scan-1", &expected, false).unwrap();
        assert!(elsewhere.join("artifacts").exists());
        assert_eq!(imported.warnings.len(), 1);

        // A scan the plugin ended another way keeps its findings so far and
        // its working files.
        seal(&dir, "p-1", "abc123", "interrupted");
        let imported = import(&dir, &kept, &root, "scan-1", &expected, false).unwrap();
        assert_eq!(imported.manifest_status, "interrupted");
        assert!(dir.join("artifacts").exists());
        assert!(!owned_dir(&root, &root));
        assert!(report(&d.path().join("missing")).is_err());
    }

    #[test]
    fn refuses_a_scan_the_plugin_did_not_record_as_finished() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("scan-output");
        let dir = root.join("abc");
        std::fs::create_dir_all(dir.join("artifacts")).unwrap();
        let kept = d.path().join("scans/scan-1");
        let expected = Expected {
            plugin_scan_ids: vec!["p-1".into()],
            commit: Some("abc123".into()),
        };
        let refused = |why: &str| {
            let error = import(&dir, &kept, &root, "scan-1", &expected, false).unwrap_err();
            assert!(
                error.contains("did not record this scan as finished"),
                "{error}"
            );
            assert!(error.contains(why), "{error}");
            // Nothing is imported and no working file is deleted.
            assert!(dir.join("artifacts").exists());
            assert!(!kept.exists());
        };
        // An empty draft list of findings, without a manifest.
        std::fs::write(dir.join("findings.json"), r#"{"findings": []}"#).unwrap();
        refused("no scan-manifest.json");
        std::fs::write(dir.join("scan-manifest.json"), "{not json").unwrap();
        refused("not valid JSON");
        std::fs::write(
            dir.join("scan-manifest.json"),
            r#"{"scan": {"status": "completed"}}"#,
        )
        .unwrap();
        refused("not a Codex Security manifest");
        // A draft the plugin has not sealed.
        seal(&dir, "p-1", "abc123", "completed");
        let mut draft: Value =
            serde_json::from_slice(&std::fs::read(dir.join("scan-manifest.json")).unwrap())
                .unwrap();
        draft["scan"].as_object_mut().unwrap().remove("sealedAt");
        std::fs::write(dir.join("scan-manifest.json"), draft.to_string()).unwrap();
        refused("never sealed");
        seal(&dir, "p-1", "abc123", "running");
        refused("no finished status");
        seal(&dir, "p-2", "abc123", "completed");
        refused("another scan (p-2)");
        seal(&dir, "p-1", "def456", "completed");
        refused("for commit def456");
        // findings.json changed after the plugin sealed it.
        seal(&dir, "p-1", "abc123", "completed");
        std::fs::write(dir.join("findings.json"), EXAMPLE).unwrap();
        refused("findings.json has changed");
        std::fs::remove_file(dir.join("findings.json")).unwrap();
        refused("findings.json is missing");
        // Findings or coverage from another scan, even when sealed.
        std::fs::write(dir.join("findings.json"), EXAMPLE).unwrap();
        seal(&dir, "p-1", "abc123", "completed");
        let manifest = std::fs::read_to_string(dir.join("scan-manifest.json")).unwrap();
        std::fs::write(
            dir.join("coverage.json"),
            r#"{"documentType": "codex-security.coverage", "scanId": "p-2"}"#,
        )
        .unwrap();
        let digest = sha256_hex(&std::fs::read(dir.join("coverage.json")).unwrap());
        let mut resealed: Value = serde_json::from_str(&manifest).unwrap();
        for artifact in resealed["scan"]["artifacts"].as_array_mut().unwrap() {
            if artifact["path"] == "coverage.json" {
                artifact["sha256"] = digest.clone().into();
            }
        }
        std::fs::write(dir.join("scan-manifest.json"), resealed.to_string()).unwrap();
        refused("coverage.json is not this scan's");
        // Without knowing the plugin's scan, nothing can be trusted.
        std::fs::write(dir.join("findings.json"), EXAMPLE).unwrap();
        seal(&dir, "p-1", "abc123", "completed");
        let error = import(&dir, &kept, &root, "scan-1", &Expected::default(), false).unwrap_err();
        assert!(error.contains("could not tell which"), "{error}");
        assert!(import(&dir, &kept, &root, "scan-1", &expected, false).is_ok());
    }
}
