use super::*;
use crate::workflows::Workflow;

const PATH: &str = ".github/workflows/ci.yml";
/// The issue's workflow, with what only the tools find.
const CI: &str = "on: [issues]\npermissions: {}\njobs:\n  build:\n    runs-on: x\n    steps:\n      - uses: other/tool@v1\n      - uses: actions/checkout@v5\n      - run: echo \"${{ github.event.issue.title }}\"\n      - run: echo hi # actionlint: property \"foo\" is not defined\n      - run: echo hi # zizmor: cache-poisoning\n      - run: echo hi # zizmor: ignore\n      - run: echo \"${{ inputs.name }}\"\n";

/// Fake tools that run in `mode`, logging to `log`.
pub(crate) fn fake(mode: &str, log: &Path) -> Tools {
    let launcher = |tool: &str| Launcher {
        program: "node".into(),
        prefix: vec![
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/lint_fixture.cjs").into(),
            format!("{tool}@{mode}@{}", log.display()),
        ],
    };
    Tools {
        actionlint: launcher("actionlint"),
        zizmor: launcher("zizmor"),
    }
}
/// What Orbit's own checks make of `text` at `PATH`.
fn setup(text: &str) -> Setup {
    let found = crate::risks::check(PATH, text, "acme").unwrap();
    Setup {
        repo: "acme/api".into(),
        workflows: vec![Workflow {
            path: PATH.into(),
            checked: true,
            risks: found.risks,
            more: found.more,
            ..Default::default()
        }],
        ..Default::default()
    }
}
fn kinds(s: &Setup) -> Vec<(RiskKind, Severity, &str, usize, &str)> {
    s.workflows[0]
        .risks
        .iter()
        .map(|r| {
            (
                r.kind,
                r.severity,
                r.subject.as_str(),
                r.line,
                r.job.as_str(),
            )
        })
        .collect()
}

#[tokio::test]
async fn the_tools_add_what_orbit_doesnt_find_and_each_counts_once() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("log");
    let work = d.path().join("lint");
    let files = vec![(PATH.to_string(), CI.to_string())];
    let ran = run(&fake("ok", &log), &work, &files).await;
    assert_eq!(ran.len(), 2);
    let mut s = setup(CI);
    merge(&mut s, ran);
    assert_eq!(
        s.tools,
        [
            ToolRun {
                tool: RiskKind::Actionlint,
                error: String::new()
            },
            ToolRun {
                tool: RiskKind::Zizmor,
                error: String::new()
            }
        ]
    );
    // zizmor's pins are Orbit's own, by Orbit's rules, and the issue title
    // in a script is Orbit's own on that line; the ignored one is left out.
    assert_eq!(
        kinds(&s),
        [
            (
                RiskKind::Injection,
                Severity::High,
                "github.event.issue.title",
                9,
                "build"
            ),
            (RiskKind::Unpinned, Severity::Medium, "other/tool@v1", 7, ""),
            (RiskKind::Zizmor, Severity::Medium, "artipacked", 8, "build"),
            (RiskKind::Actionlint, Severity::Low, "syntax-check", 10, ""),
            (
                RiskKind::Zizmor,
                Severity::Low,
                "cache-poisoning",
                11,
                "build"
            ),
            (
                RiskKind::Zizmor,
                Severity::Low,
                "template-injection",
                13,
                "build"
            ),
        ]
    );
    let r = &s.workflows[0].risks;
    assert_eq!(
        r[2].name(),
        "zizmor: credential persistence through GitHub Actions artifacts, in ci.yml, job build"
    );
    assert_eq!(
        r[2].advice(),
        "fix what zizmor says, as explained at https://docs.zizmor.sh/audits/#artipacked"
    );
    assert_eq!(
        r[3].name(),
        "actionlint: property \"foo\" is not defined, in ci.yml"
    );
    // What it is about names it, so two in a job are two.
    assert_eq!(
        r[5].detail,
        "code injection via template expansion (inputs.name)"
    );
    assert_eq!(s.workflows[0].more, 0);

    // Each ran in a folder holding only the files, with no network, no
    // configuration and none of actionlint's helpers, and left nothing.
    let lines: Vec<Value> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    for l in &lines {
        assert_eq!(l["files"], serde_json::json!([PATH]));
        assert_eq!(l["token"], false);
    }
    let args = |i: usize| -> Vec<&str> {
        lines[i]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap())
            .collect()
    };
    assert_eq!(
        args(0),
        [
            "-format",
            "{{json .}}",
            "-no-color",
            "-shellcheck=",
            "-pyflakes=",
            PATH
        ]
    );
    for flag in ["--offline", "--no-config", "--no-exit-codes", "json-v1"] {
        assert!(args(1).contains(&flag), "{flag}");
    }
    assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
}

#[tokio::test]
async fn a_tool_that_isnt_installed_isnt_run_and_one_that_fails_says_why() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("log");
    let work = d.path().join("lint");
    let files = vec![(PATH.to_string(), CI.to_string())];
    let only_zizmor = Tools {
        actionlint: Launcher::new("orbit-no-such-actionlint"),
        ..fake("ok", &log)
    };
    let ran = run(&only_zizmor, &work, &files).await;
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].tool, RiskKind::Zizmor);
    // Neither, or nothing to check, runs nothing.
    let neither = Tools::default();
    assert!(run(&neither, &work, &files).await.is_empty());
    assert!(run(&fake("ok", &log), &work, &[]).await.is_empty());

    for (mode, why) in [
        ("fail", "it failed: error: something went wrong"),
        ("garbled", "Orbit couldn't read what it said"),
        ("slow", "it took longer than 3 seconds"),
    ] {
        let ran = run(&fake(mode, &log), &work, &files).await;
        assert_eq!(ran.len(), 2, "{mode}");
        assert_eq!(ran[0].found.as_ref().unwrap_err(), why, "{mode}");
        let mut s = setup(CI);
        let before = s.workflows[0].risks.clone();
        merge(&mut s, ran);
        assert!(s.tools.iter().all(|t| t.error == why), "{mode}");
        assert_eq!(s.workflows[0].risks, before, "{mode}");
    }
    assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
}

#[test]
fn folders_a_stopped_run_left_go() {
    let d = tempfile::tempdir().unwrap();
    let left = d.path().join("zizmor-left");
    std::fs::create_dir_all(left.join(".github/workflows")).unwrap();
    sweep(d.path(), Duration::from_secs(60 * 60));
    assert!(left.exists(), "a new one may be in use");
    sweep(d.path(), Duration::ZERO);
    assert!(!left.exists());
    sweep(&d.path().join("none"), Duration::ZERO);
}

#[test]
fn only_plain_file_names_are_written() {
    assert_eq!(name(".github/workflows/ci.yml"), Some("ci.yml"));
    assert_eq!(
        name(".github/workflows/re lease#1.yml"),
        Some("re lease#1.yml")
    );
    for path in [
        "ci.yml",
        ".github/workflows/",
        ".github/workflows/..",
        ".github/workflows/a/b.yml",
        ".github/workflows/a\\b.yml",
        ".github/workflows/c:d.yml",
    ] {
        assert_eq!(name(path), None, "{path}");
    }
    assert_eq!(
        rule("template-injection").as_deref(),
        Some("template-injection")
    );
    assert_eq!(rule("Bad rule"), None);
    assert_eq!(rule(""), None);
}

#[test]
fn what_the_tools_find_keeps_the_most_severe_and_counts_the_rest() {
    let mut text =
        String::from("on: issues\npermissions: {}\njobs:\n  a:\n    runs-on: x\n    steps:\n");
    for i in 0..MAX_PER_FILE {
        text.push_str(&format!("      - uses: other/tool{i}@v1\n"));
    }
    let mut s = setup(&text);
    assert_eq!(s.workflows[0].risks.len(), MAX_PER_FILE);
    let finding = |line: usize, severity: Severity, rule: &str| Finding {
        risk: Risk {
            kind: RiskKind::Zizmor,
            severity,
            path: PATH.into(),
            line,
            job: "a".into(),
            subject: rule.into(),
            detail: "words".into(),
            ..Default::default()
        },
        script: String::new(),
    };
    let ran = Ran {
        tool: RiskKind::Zizmor,
        found: Ok(vec![
            finding(7, Severity::High, "cache-poisoning"),
            finding(8, Severity::Low, "artipacked"),
            // The same again, elsewhere in its job, is one.
            finding(9, Severity::High, "cache-poisoning"),
        ]),
    };
    merge(&mut s, vec![ran]);
    let w = &s.workflows[0];
    assert_eq!(w.risks.len(), MAX_PER_FILE);
    assert_eq!((w.risks[0].kind, w.risks[0].line), (RiskKind::Zizmor, 7));
    assert_eq!(w.more, 2);
    // A file Orbit couldn't check gets nothing from them either.
    let mut s = Setup {
        workflows: vec![Workflow {
            path: PATH.into(),
            why: "it isn't valid YAML".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    merge(
        &mut s,
        vec![Ran {
            tool: RiskKind::Zizmor,
            found: Ok(vec![finding(1, Severity::High, "cache-poisoning")]),
        }],
    );
    assert!(s.workflows[0].risks.is_empty());
}
