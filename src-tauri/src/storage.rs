use crate::types::{active, Review, Snapshot, Verdict};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeUsage {
    pub review_id: String,
    pub bytes: u64,
    pub removable: bool,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub state_bytes: u64,
    pub worktree_bytes: u64,
    pub unrecognized_bytes: u64,
    pub unrecognized: Vec<UnrecognizedUsage>,
    pub worktrees: Vec<WorktreeUsage>,
    pub errors: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct UnrecognizedUsage {
    pub name: String,
    pub bytes: u64,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub review_id: String,
    pub error: Option<String>,
    pub warning: Option<String>,
}

pub fn inactive(r: &Review) -> bool {
    !active(&r.status) && r.status != "publishing" && r.before_follow_up.is_none()
}

/// Accept only the exact recorded UUID directory. Never traverse a symlink at
/// the worktree boundary, even if its destination happens to be inside Orbit.
pub fn worktree_path(root: &Path, id: &str, recorded: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid review workspace identifier.")?;
    let parent = root.join("worktrees");
    let expected = parent.join(id);
    if Path::new(recorded) != expected {
        return Err("Workspace is outside its registered Orbit location.".into());
    }
    for path in [&parent, &expected] {
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                return Err("Workspace location must be a directory, not a symlink.".into());
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("Cannot inspect workspace: {e}")),
        }
    }
    if expected.exists() {
        let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
        if expected.canonicalize().map_err(|e| e.to_string())?
            != canonical_root.join("worktrees").join(id)
        {
            return Err("Workspace resolves outside its registered Orbit location.".into());
        }
    }
    Ok(expected)
}

/// Logical file sizes, not allocated disk blocks. Symlinks are deliberately
/// excluded, so repository links cannot make the scan traverse host files.
fn bytes(path: &Path, errors: &mut Vec<String>) -> u64 {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return 0,
        Err(e) => {
            errors.push(format!("{}: {e}", path.display()));
            return 0;
        }
    };
    if metadata.file_type().is_symlink() {
        return 0;
    }
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    directory_bytes(path, errors)
}

fn directory_bytes(path: &Path, errors: &mut Vec<String>) -> u64 {
    match std::fs::read_dir(path) {
        Ok(entries) => entries
            .map(|entry| match entry {
                Ok(entry) => entry_bytes(&entry, errors),
                Err(e) => {
                    errors.push(format!("{}: {e}", path.display()));
                    0
                }
            })
            .sum(),
        Err(e) => {
            errors.push(format!("{}: {e}", path.display()));
            0
        }
    }
}

fn entry_bytes(entry: &std::fs::DirEntry, errors: &mut Vec<String>) -> u64 {
    let size: std::io::Result<u64> = (|| {
        // The entry type is cached on most filesystems. Only regular files
        // need metadata for their sizes; never read metadata through a symlink.
        let kind = entry.file_type()?;
        if kind.is_dir() {
            return Ok(directory_bytes(&entry.path(), errors));
        }
        if kind.is_file() {
            let metadata = entry.metadata()?;
            return Ok(if metadata.is_file() {
                metadata.len()
            } else {
                0
            });
        }
        Ok(0)
    })();
    match size {
        Ok(size) => size,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => {
            errors.push(format!("{}: {e}", entry.path().display()));
            0
        }
    }
}

pub fn inspect(root: &Path, snapshot: &Snapshot) -> StorageUsage {
    let mut usage = StorageUsage::default();
    usage.state_bytes = bytes(&root.join("state.json"), &mut usage.errors);
    let mut known = std::collections::HashSet::new();
    for review in &snapshot.reviews {
        let Some(recorded) = &review.worktree else {
            continue;
        };
        // Invalid recorded paths still belong to this review's row. Do not
        // list its UUID directory again as unrecognized or count it twice.
        if uuid::Uuid::parse_str(&review.id).is_ok() {
            known.insert(root.join("worktrees").join(&review.id));
        }
        let mut errors = vec![];
        let size = match worktree_path(root, &review.id, recorded) {
            Ok(path) => {
                known.insert(path.clone());
                bytes(&path, &mut errors)
            }
            Err(e) => {
                errors.push(e);
                let expected = root.join("worktrees").join(&review.id);
                // Keep any measurable owned bytes on the review row even when
                // its recorded path is invalid. The error still prevents removal.
                worktree_path(root, &review.id, &expected.to_string_lossy())
                    .map(|path| bytes(&path, &mut errors))
                    .unwrap_or(0)
            }
        };
        usage.worktree_bytes += size;
        usage.worktrees.push(WorktreeUsage {
            review_id: review.id.clone(),
            bytes: size,
            removable: inactive(review) && !review.cleanup_pending && errors.is_empty(),
            error: (!errors.is_empty()).then(|| errors.join("; ")),
        });
    }
    let parent = root.join("worktrees");
    if parent.is_symlink() {
        usage
            .errors
            .push("Worktree directory is a symlink; storage scan refused.".into());
    } else if parent.exists() {
        match std::fs::read_dir(&parent) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) if !known.contains(&entry.path()) => {
                            let mut errors = vec![];
                            let size = entry_bytes(&entry, &mut errors);
                            usage.unrecognized_bytes += size;
                            usage.unrecognized.push(UnrecognizedUsage {
                                name: entry.file_name().to_string_lossy().into_owned(),
                                bytes: size,
                                error: (!errors.is_empty()).then(|| errors.join("; ")),
                            });
                        }
                        Ok(_) => {}
                        Err(e) => usage.errors.push(e.to_string()),
                    }
                }
            }
            Err(e) => usage.errors.push(e.to_string()),
        }
    }
    usage
}

pub fn report(snapshot: &Snapshot, review_id: &str) -> Result<(String, String), String> {
    let r = snapshot
        .reviews
        .iter()
        .find(|r| r.id == review_id)
        .ok_or("Unknown review.")?;
    if !inactive(r) || r.report.trim().is_empty() {
        return Err("Export requires an inactive review with a report.".into());
    }
    let pr = snapshot
        .prs
        .iter()
        .chain(&snapshot.history_prs)
        .find(|p| p.id == r.pr_id)
        .ok_or("Pull request details are unavailable.")?;
    let name = format!(
        "orbit-{}-pr-{}-{}.md",
        pr.repo.replace('/', "-"),
        pr.number,
        r.head_sha.chars().take(7).collect::<String>()
    );
    let selection = crate::models::report_selection_label(r);
    let mut body = format!("# Orbit review: {}\n\nPR: {} ({})\n\nCreated: {}\n\nHead: `{}`\n\nBase: `{}`\n\n{selection}\n\n",
        pr.title, r.pr_id, pr.url, r.created_at, r.head_sha, r.base_sha);
    if let Some(url) = &r.comment_url {
        body.push_str(&format!("Published comment: {url}\n\n"));
    }
    body.push_str(&findings_section(r));
    // Kept apart from the findings, which an unstructured run lacks.
    for note in &r.declined_approvals {
        body.push_str(&format!("{note}\n\n"));
    }
    body.push_str(&r.report);
    body.push('\n');
    Ok((name, body))
}

/// The verdict and a findings table, ahead of the Markdown report.
fn findings_section(r: &Review) -> String {
    let Some(findings) = &r.findings else {
        return "Findings: unstructured run (not extracted).\n\n".into();
    };
    let cell = |text: &str| text.replace('|', "\\|").replace(['\r', '\n'], " ");
    let mut out = format!(
        "Verdict: {}\n\n",
        match r.verdict {
            Some(Verdict::Clean) => "clean",
            Some(Verdict::ChangesRequested) => "changes requested",
            None => "not reported",
        }
    );
    if let Some(summary) = r.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!("{}\n\n", summary.trim()));
    }
    if !findings.is_empty() {
        out.push_str("| Priority | Location | Finding |\n| --- | --- | --- |\n");
        for f in findings {
            let lines = if f.start_line == f.end_line {
                f.start_line.to_string()
            } else {
                format!("{}-{}", f.start_line, f.end_line)
            };
            let location = f
                .path
                .as_deref()
                .map_or("(outside the repository)".into(), |p| {
                    format!("`{}:{lines}`", cell(p))
                });
            out.push_str(&format!(
                "| P{} | {location} | {} |\n",
                f.priority,
                cell(&f.title)
            ));
        }
        out.push('\n');
    }
    if !r.tests_not_run.is_empty() {
        out.push_str(&format!("Not run: {}\n\n", r.tests_not_run.join("; ")));
    }
    if let Some(round) = &r.previous_round {
        out.push_str(&format!(
            "Since the last round (head `{}`):\n\n| Status | Earlier finding | Note |\n| --- | --- | --- |\n",
            &round.head_sha[..round.head_sha.len().min(7)]
        ));
        for f in &round.findings {
            let prior = r.prior_findings.iter().find(|p| p.id == f.id);
            out.push_str(&format!(
                "| {} | P{} {} | {} |\n",
                prior.map_or("not reported", |p| p.status.label()),
                f.priority,
                cell(&f.title),
                prior.map_or(String::new(), |p| cell(&p.note))
            ));
        }
        out.push('\n');
    }
    out.push_str("## Report\n\n");
    out
}

pub fn select_report_path(
    mut suggested: PathBuf,
    mut choose: impl FnMut(&Path) -> Result<Option<PathBuf>, String>,
) -> Result<Option<PathBuf>, String> {
    loop {
        let Some(path) = choose(&suggested)? else {
            return Ok(None);
        };
        if path
            .extension()
            .is_none_or(|extension| extension.is_empty())
        {
            // Ask the native dialog to confirm the completed filename and any
            // overwrite. Never silently change the user-selected destination.
            suggested = path.with_extension("md");
        } else {
            return Ok(Some(path));
        }
    }
}

pub fn write_report(path: Option<&Path>, text: &str) -> Result<Option<String>, String> {
    let Some(path) = path else { return Ok(None) };
    std::fs::write(path, text).map_err(|e| format!("Could not export report: {e}"))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Message, Settings};
    fn snapshot() -> Snapshot {
        let pr = crate::types::tests::pr();
        let mut review = Review::new(&pr, &Settings::default(), false);
        review.status = "posted".into();
        review.report = "## Findings\nA useful report.".into();
        review.comment_url = Some("https://github.com/a/b/pull/1#issuecomment-2".into());
        review
            .messages
            .push(Message::new("system", "PRIVATE EXECUTION LOG"));
        Snapshot {
            prs: vec![pr],
            reviews: vec![review],
            ..Default::default()
        }
    }
    #[test]
    fn exports_report_metadata_without_transcripts_or_logs() {
        let s = snapshot();
        let r = &s.reviews[0];
        let (name, body) = report(&s, &r.id).unwrap();
        assert!(name.ends_with(".md"));
        for value in [
            &r.report,
            &r.created_at,
            &r.head_sha,
            &r.base_sha,
            &r.pr_id,
            &s.prs[0].url,
            r.comment_url.as_ref().unwrap(),
        ] {
            assert!(body.contains(value), "Missing {value}");
        }
        assert!(body.contains("Model: not recorded · Effort: not recorded"));
        assert!(!body.contains("PRIVATE EXECUTION LOG"));
        let mut invalid = s.clone();
        invalid.reviews[0].report = "  ".into();
        assert!(report(&invalid, &r.id).is_err());
        invalid.reviews[0] = r.clone();
        invalid.reviews[0].status = "publishing".into();
        assert!(report(&invalid, &r.id).is_err());
        assert!(report(&s, "missing").is_err());
        let mut verified = s.clone();
        verified.reviews[0].legacy_chat_selection = Some(crate::types::ModelSelection {
            model: "chat-only".into(),
            effort: "low".into(),
        });
        assert!(!report(&verified, &r.id).unwrap().1.contains("chat-only"));
        verified.reviews[0].confirmed_review_selection = Some(crate::types::ModelSelection {
            model: "confirmed-report".into(),
            effort: "high".into(),
        });
        assert!(report(&verified, &r.id)
            .unwrap()
            .1
            .contains("Model: confirmed-report (confirmed) · Effort: high (confirmed)"));
    }
    #[test]
    fn exports_the_verdict_and_a_findings_table() {
        let mut s = snapshot();
        assert!(report(&s, &s.reviews[0].id.clone())
            .unwrap()
            .1
            .contains("Findings: unstructured run"));
        let r = &mut s.reviews[0];
        r.verdict = Some(Verdict::ChangesRequested);
        r.summary = Some("Two defects.".into());
        r.tests_not_run = vec!["cargo test".into()];
        let finding = crate::types::Finding {
            id: format!("{}-0", r.id),
            title: "Escape | pipes\nand lines".into(),
            priority: 1,
            path: Some("src/a.rs".into()),
            start_line: 4,
            end_line: 9,
            body: "Details.".into(),
            confidence: 0.9,
        };
        let outside = crate::types::Finding {
            path: None,
            start_line: 2,
            end_line: 2,
            ..finding.clone()
        };
        r.findings = Some(vec![finding, outside]);
        let body = report(&s, &s.reviews[0].id).unwrap().1;
        assert!(
            body.contains("Verdict: changes requested\n\nTwo defects."),
            "{body}"
        );
        assert!(
            body.contains("| P1 | `src/a.rs:4-9` | Escape \\| pipes and lines |"),
            "{body}"
        );
        assert!(body.contains("| P1 | (outside the repository) |"), "{body}");
        assert!(body.contains("Not run: cargo test"));
        assert!(!body.contains("Since the last round"));
        let r = &mut s.reviews[0];
        let mut earlier = r.findings.clone().unwrap();
        earlier[1].id = "r0-1".into();
        r.previous_round = Some(crate::types::PreviousRound {
            review_id: "r0".into(),
            head_sha: "c".repeat(40),
            verdict: Some(Verdict::ChangesRequested),
            findings: earlier.clone(),
            decisions: vec![],
            comments_included: 0,
            comments_omitted: 0,
        });
        r.prior_findings = vec![crate::types::PriorFinding {
            id: earlier[0].id.clone(),
            status: crate::types::PriorStatus::DeclinedAccepted,
            note: "Intended | by design.".into(),
        }];
        let body = report(&s, &s.reviews[0].id).unwrap().1;
        assert!(
            body.contains("Since the last round (head `ccccccc`):"),
            "{body}"
        );
        assert!(
            body.contains(
                "| declined (accepted) | P1 Escape \\| pipes and lines | Intended \\| by design. |"
            ),
            "{body}"
        );
        assert!(body.contains("| not reported | P1 "), "{body}");
        assert!(body.find("## Report").unwrap() < body.find("A useful report.").unwrap());
    }
    #[test]
    fn export_cancellation_and_write_failures_are_explicit() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(write_report(None, "report").unwrap(), None);
        assert!(write_report(Some(d.path()), "report")
            .unwrap_err()
            .contains("Could not export"));
        let path = d.path().join("review.md");
        assert!(write_report(Some(&path), "report").unwrap().is_some());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "report");
    }
    #[test]
    fn extensionless_export_requires_confirmation_of_the_final_destination() {
        let d = tempfile::tempdir().unwrap();
        let bare = d.path().join("chosen");
        let markdown = bare.with_extension("md");
        std::fs::write(&markdown, "existing report").unwrap();
        let mut selections = std::collections::VecDeque::from([Some(bare.clone()), None]);
        let mut suggestions = vec![];
        let selected = select_report_path("default.md".into(), |suggested| {
            suggestions.push(suggested.to_path_buf());
            Ok(selections.pop_front().unwrap())
        })
        .unwrap();
        assert!(selected.is_none());
        assert_eq!(suggestions[1], markdown);
        assert_eq!(
            std::fs::read_to_string(&markdown).unwrap(),
            "existing report"
        );
        assert!(write_report(selected.as_deref(), "cancelled")
            .unwrap()
            .is_none());

        let mut selections = std::collections::VecDeque::from([Some(bare), Some(markdown.clone())]);
        let selected =
            select_report_path("default.md".into(), |_| Ok(selections.pop_front().unwrap()))
                .unwrap();
        assert_eq!(selected, Some(markdown.clone()));
        write_report(selected.as_deref(), "confirmed").unwrap();
        assert_eq!(std::fs::read_to_string(markdown).unwrap(), "confirmed");
        let explicit = d.path().join("chosen.txt");
        assert_eq!(
            select_report_path("default.md".into(), |_| Ok(Some(explicit.clone()))).unwrap(),
            Some(explicit)
        );
    }
    #[test]
    fn invalid_registered_paths_are_counted_once_on_the_review_row() {
        let d = tempfile::tempdir().unwrap();
        let mut s = snapshot();
        let path = d.path().join("worktrees").join(&s.reviews[0].id);
        std::fs::create_dir_all(path.join("nested")).unwrap();
        std::fs::write(path.join("nested/file"), "12345").unwrap();
        s.reviews[0].worktree = Some("/wrong/location".into());
        let usage = inspect(d.path(), &s);
        assert_eq!(usage.worktree_bytes, 5);
        assert_eq!(usage.worktrees[0].bytes, 5);
        assert!(usage.worktrees[0].error.is_some());
        assert!(!usage.worktrees[0].removable);
        assert!(usage.unrecognized.is_empty());
        assert_eq!(usage.unrecognized_bytes, 0);
    }
    #[test]
    fn rejects_unregistered_paths_and_accepts_missing_owned_directories() {
        let d = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let expected = d.path().join("worktrees").join(&id);
        assert!(worktree_path(d.path(), &id, expected.to_str().unwrap()).is_ok());
        assert!(worktree_path(d.path(), &id, d.path().to_str().unwrap()).is_err());
        assert!(worktree_path(d.path(), "../outside", "../outside").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn scans_without_following_links_and_flags_invalid_boundaries() {
        use std::os::unix::fs::symlink;
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "outside data").unwrap();
        let mut s = snapshot();
        let path = d.path().join("worktrees").join(&s.reviews[0].id);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("tracked"), "12345").unwrap();
        symlink(outside.path(), path.join("link")).unwrap();
        s.reviews[0].worktree = Some(path.to_string_lossy().into());
        std::fs::write(d.path().join("state.json"), "abc").unwrap();
        let unknown = d.path().join("worktrees/unknown");
        std::fs::create_dir(&unknown).unwrap();
        std::fs::write(unknown.join("other"), "1234").unwrap();
        let usage = inspect(d.path(), &s);
        assert_eq!(
            (
                usage.state_bytes,
                usage.worktree_bytes,
                usage.unrecognized_bytes
            ),
            (3, 5, 4)
        );
        assert!(usage.worktrees[0].removable);
        std::fs::remove_dir_all(&path).unwrap();
        symlink(outside.path(), &path).unwrap();
        let usage = inspect(d.path(), &s);
        assert_eq!(usage.worktree_bytes, 0);
        assert!(!usage.worktrees[0].removable);
        assert!(usage.worktrees[0].error.is_some());
        assert_eq!(usage.unrecognized.len(), 1);
        assert_eq!(usage.unrecognized[0].name, "unknown");
        std::fs::remove_dir_all(d.path().join("worktrees")).unwrap();
        symlink(outside.path(), d.path().join("worktrees")).unwrap();
        assert!(!inspect(d.path(), &s).errors.is_empty());
    }
    #[test]
    fn older_reviews_load_without_cleanup_fields_and_settings_reject_snapshots() {
        let s = snapshot();
        let mut json = serde_json::to_value(&s).unwrap();
        let review = json["reviews"][0].as_object_mut().unwrap();
        review.remove("cleanupPending");
        review.remove("cleanupError");
        let loaded: Snapshot = serde_json::from_value(json).unwrap();
        assert!(!loaded.reviews[0].cleanup_pending);
        assert!(loaded.reviews[0].cleanup_error.is_none());
        assert_eq!(loaded.reviews[0].report, s.reviews[0].report);
        assert!(serde_json::from_value::<crate::types::SettingsPatch>(
            serde_json::json!({"repositories":[]})
        )
        .is_err());
    }
}
