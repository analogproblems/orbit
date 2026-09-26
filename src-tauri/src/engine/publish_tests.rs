use super::test_support::*;

#[tokio::test]
async fn holds_automatic_publication_for_external_pull_requests() {
    let (_d, e, id) = running_engine().await;
    ready_for_autopost(&e, "No actionable defects found.", false).await;
    let error = e.publish(&id, true, false).await.unwrap_err();
    assert!(
        error.contains("collaborators, or trusted accounts"),
        "{error}"
    );
    assert_eq!(e.snapshot().await.reviews[0].status, "ready");
}
#[tokio::test]
async fn holds_automatic_publication_of_credential_shaped_reports() {
    let (_d, e, id) = running_engine().await;
    let report = format!("Found ghp_{} in the fixture.", "a1".repeat(18));
    ready_for_autopost(&e, &report, true).await;
    // Confirmation is a manual-only escape hatch; automatic runs ignore it.
    let error = e.publish(&id, true, true).await.unwrap_err();
    assert!(error.contains("GitHub token"), "{error}");
    assert_eq!(e.snapshot().await.reviews[0].status, "ready");
}
#[tokio::test]
async fn manual_publication_of_credential_shaped_reports_needs_confirmation() {
    let (_d, e, id) = running_engine().await;
    let report = format!("-----BEGIN RSA {} KEY-----", "PRIVATE");
    e.update(|s| {
        s.reviews[0].status = "ready".into();
        s.reviews[0].report = report;
    })
    .await;
    let error = e.publish(&id, false, false).await.unwrap_err();
    assert!(
        error.contains("private key") && error.contains("confirm"),
        "{error}"
    );
    // Stopping early releases the publication claim.
    assert_eq!(e.snapshot().await.reviews[0].status, "ready");
}
#[tokio::test]
async fn publication_waits_for_a_running_follow_up() {
    let (_d, e, id) = running_engine().await;
    start_follow_up(&e, "ready").await;
    assert!(e.publish(&id, false, true).await.is_err());
    let r = e.snapshot().await.reviews[0].clone();
    assert_eq!(r.status, "reviewing");
    assert!(r.before_follow_up.is_some());
}

/// Publication against a fake `gh`, which is a shell script.
#[cfg(unix)]
mod fake_gh {
    use super::super::test_support::*;
    use crate::types::*;

    /// A stand-in for `gh` that answers from files in `dir`: the PR from
    /// `pr.json`, and a one-file diff. The diff waits for `release` when `pause`
    /// exists; with `reject`, the first review POST waits for `release-post` and
    /// is refused as GitHub refuses a comment on a line outside the diff.
    fn fake_gh(dir: &std::path::Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let d = dir.display();
        let script = format!(
            r#"#!/bin/sh
    echo "$*" >> {d}/log
    case "$*" in
      *"--method POST repos/a/b/pulls/1/reviews"*)
        cat > {d}/posted
        if [ -e {d}/reject ] && [ ! -e {d}/rejected ]; then
          touch {d}/rejected
          while [ ! -e {d}/release-post ]; do sleep 0.01; done
          echo "gh: Unprocessable Entity (HTTP 422)" >&2
          exit 1
        fi
        printf '%s' '{{"id":5,"html_url":"https://github.com/a/b/pull/1#pullrequestreview-5"}}' ;;
      *"repos/a/b/pulls/1/reviews/5/comments"*)
        printf '%s' '[{{"id":9,"body":"x <!-- orbit-finding:f-0 -->"}}]' ;;
      *"repos/a/b/pulls/1/files"*)
        touch {d}/fetching
        if [ -e {d}/pause ]; then
          while [ ! -e {d}/release ]; do sleep 0.01; done
        fi
        printf '%s' '[{{"filename":"src/a.rs","patch":"@@ -1 +1,2 @@\n a\n+b"}}]' ;;
      *"repos/a/b/pulls/1") cat {d}/pr.json ;;
      *) echo '[]' ;;
    esac
    "#
        );
        let path = dir.join("gh");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        set_head(dir, 'a');
        path
    }
    fn set_head(dir: &std::path::Path, sha: char) {
        let pr = serde_json::json!({
            "state": "open", "number": 1, "title": "Fix", "html_url": "https://github.com/a/b/pull/1",
            "user": {"login": "a"}, "author_association": "MEMBER",
            "head": {"sha": sha.to_string().repeat(40), "ref": "fix", "repo": {"full_name": "a/b"}},
            "base": {"sha": "b".repeat(40), "ref": "main", "repo": {"full_name": "a/b"}}
        });
        std::fs::write(dir.join("pr.json"), pr.to_string()).unwrap();
    }
    async fn wait_for_file(path: std::path::PathBuf) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !path.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{} never appeared", path.display()));
    }
    /// An automatic, structured review of a trusted PR, ready to publish.
    async fn structured_autopost() -> (
        tempfile::TempDir,
        std::sync::Arc<crate::engine::Engine>,
        String,
    ) {
        let (d, e, id) = running_engine().await;
        ready_for_autopost(&e, "One finding.", true).await;
        e.update(|s| {
            s.reviews[0].findings = Some(vec![Finding {
                id: "f-0".into(),
                title: "Check".into(),
                priority: 1,
                path: Some("src/a.rs".into()),
                start_line: 2,
                end_line: 2,
                body: "Explain.".into(),
                confidence: 0.9,
            }]);
        })
        .await;
        (d, e, id)
    }
    fn posts(dir: &std::path::Path) -> usize {
        std::fs::read_to_string(dir.join("log"))
            .unwrap_or_default()
            .matches("--method POST")
            .count()
    }
    #[tokio::test]
    async fn publishes_a_structured_run_as_a_review_with_inline_comments() {
        let (d, e, id) = structured_autopost().await;
        let gh = fake_gh(d.path());
        crate::github::GH
            .scope(gh, e.publish(&id, true, false))
            .await
            .unwrap();
        let r = e.snapshot().await.reviews[0].clone();
        assert_eq!(r.status, "posted");
        assert_eq!(r.review_id, Some(5));
        assert_eq!(
            r.inline_comments,
            [InlineComment {
                finding_id: "f-0".into(),
                comment_id: 9
            }]
        );
        let posted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(d.path().join("posted")).unwrap())
                .unwrap();
        assert_eq!(posted["event"], "COMMENT");
        assert_eq!(posted["comments"][0]["line"], 2);
    }
    #[tokio::test]
    async fn switching_autopost_off_during_the_diff_fetch_cancels_publication() {
        let (d, e, id) = structured_autopost().await;
        let gh = fake_gh(d.path());
        std::fs::write(d.path().join("pause"), "").unwrap();
        let change = async {
            wait_for_file(d.path().join("fetching")).await;
            e.update(|s| s.settings.auto_post = false).await;
            std::fs::write(d.path().join("release"), "").unwrap();
        };
        let (result, ()) = crate::github::GH
            .scope(gh, async {
                tokio::join!(e.publish(&id, true, false), change)
            })
            .await;
        result.unwrap();
        assert_eq!(posts(d.path()), 0);
        assert_eq!(e.snapshot().await.reviews[0].status, "ready");
    }
    #[tokio::test]
    async fn a_push_during_the_diff_fetch_stops_publication() {
        let (d, e, id) = structured_autopost().await;
        let gh = fake_gh(d.path());
        std::fs::write(d.path().join("pause"), "").unwrap();
        let change = async {
            wait_for_file(d.path().join("fetching")).await;
            set_head(d.path(), 'c');
            std::fs::write(d.path().join("release"), "").unwrap();
        };
        let (result, ()) = crate::github::GH
            .scope(gh, async {
                tokio::join!(e.publish(&id, true, false), change)
            })
            .await;
        assert!(result.unwrap_err().contains("changed since this review"));
        assert_eq!(posts(d.path()), 0);
        assert_eq!(e.snapshot().await.reviews[0].status, "ready");
    }
    #[tokio::test]
    async fn the_fallback_post_rechecks_the_head_and_switches() {
        for push in [false, true] {
            let (d, e, id) = structured_autopost().await;
            let gh = fake_gh(d.path());
            std::fs::write(d.path().join("reject"), "").unwrap();
            let change = async {
                wait_for_file(d.path().join("rejected")).await;
                if push {
                    set_head(d.path(), 'c');
                } else {
                    e.update(|s| s.settings.auto_post = false).await;
                }
                std::fs::write(d.path().join("release-post"), "").unwrap();
            };
            let (result, ()) = crate::github::GH
                .scope(gh, async {
                    tokio::join!(e.publish(&id, true, false), change)
                })
                .await;
            assert_eq!(result.is_err(), push);
            assert_eq!(posts(d.path()), 1, "only the refused POST was sent");
            assert_eq!(e.snapshot().await.reviews[0].status, "ready");
        }
    }
    #[tokio::test]
    async fn falls_back_to_the_body_when_github_refuses_a_line() {
        let (d, e, id) = structured_autopost().await;
        let gh = fake_gh(d.path());
        std::fs::write(d.path().join("reject"), "").unwrap();
        std::fs::write(d.path().join("release-post"), "").unwrap();
        crate::github::GH
            .scope(gh, e.publish(&id, true, false))
            .await
            .unwrap();
        assert_eq!(posts(d.path()), 2);
        let posted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(d.path().join("posted")).unwrap())
                .unwrap();
        assert_eq!(posted["comments"], serde_json::json!([]));
        assert!(posted["body"]
            .as_str()
            .unwrap()
            .contains("### Other findings"));
        assert_eq!(e.snapshot().await.reviews[0].status, "posted");
    }
}
