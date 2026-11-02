use super::*;

const PATH: &str = ".github/workflows/ci.yml";

fn check_ok(text: &str) -> Vec<Risk> {
    check(PATH, text, "acme").unwrap().risks
}
fn kinds(risks: &[Risk]) -> Vec<(RiskKind, &str, &str, usize)> {
    risks
        .iter()
        .map(|r| (r.kind, r.job.as_str(), r.subject.as_str(), r.line))
        .collect()
}

#[test]
fn a_safe_workflow_has_no_risks() {
    let text = "name: CI\non:\n  pull_request:\n  push:\n    branches: [main]\npermissions:\n  contents: read\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v5\n      - uses: github/codeql-action/init@v3\n      - uses: acme/build@v1\n      - uses: ./local-action\n      - uses: other/tool@0123456789abcdef0123456789abcdef01234567\n      - uses: docker://alpine@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n      - run: echo \"${{ github.sha }} ${{ github.event.issue.number }}\"\n        env:\n          TITLE: ${{ github.event.issue.title }}\n";
    assert_eq!(check_ok(text), vec![]);
}

#[test]
fn the_three_risks_the_issue_names_are_found_with_their_lines() {
    let text = "on:\n  pull_request_target:\n  issues:\npermissions:\n  contents: read\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: codecov/codecov-action@v4\n      - uses: actions/checkout@v5\n        with:\n          ref: ${{ github.event.pull_request.head.sha }}\n      - run: |\n          echo hello\n          echo \"${{ github.event.issue.title }}\"\n";
    let risks = check_ok(text);
    assert_eq!(
        kinds(&risks),
        vec![
            (
                RiskKind::PullCheckout,
                "build",
                "github.event.pull_request.head.sha",
                13
            ),
            (RiskKind::Injection, "build", "github.event.issue.title", 16),
            (RiskKind::Unpinned, "", "codecov/codecov-action@v4", 10),
        ]
    );
    assert_eq!(risks[0].severity, Severity::High);
    assert_eq!(risks[2].severity, Severity::Medium);
    assert!(risks.iter().all(|r| r.path == PATH && r.number == 0));
}

#[test]
fn unpinned_actions_images_and_reusable_workflows() {
    let text = "on: push\npermissions: {}\njobs:\n  a:\n    uses: other/workflows/.github/workflows/build.yml@main\n  b:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: docker://alpine:3.20\n      - uses: docker://alpine@sha256:abc\n      - uses: Other/Tool@v1.2.3\n      - uses: other/tool@0123456\n      - uses: ACME/own@v1\n      - uses: Actions/setup-node@v5\n      - uses: no-version\n  c:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: Other/Tool@v1.2.3\n";
    let risks = check_ok(text);
    // Once per file, wherever it is used, at its first line.
    assert_eq!(
        kinds(&risks),
        vec![
            (
                RiskKind::Unpinned,
                "",
                "other/workflows/.github/workflows/build.yml@main",
                5
            ),
            (RiskKind::Unpinned, "", "docker://alpine:3.20", 9),
            (RiskKind::Unpinned, "", "docker://alpine@sha256:abc", 10),
            (RiskKind::Unpinned, "", "Other/Tool@v1.2.3", 11),
            (RiskKind::Unpinned, "", "other/tool@0123456", 12),
        ]
    );
    assert_eq!(
        risks[0].advice(),
        "pin it to a full commit (40 characters), with the version in a comment after it"
    );
    assert!(risks[1].advice().contains("digest"));
}

#[test]
fn permissions_missing_or_write_all() {
    // None at the top, and a job with none.
    let text = "on: push\njobs:\n  a:\n    runs-on: x\n    permissions:\n      contents: read\n  b:\n    runs-on: x\n";
    assert_eq!(
        kinds(&check_ok(text)),
        vec![(RiskKind::Permissions, "", "missing", 2)]
    );
    assert_eq!(check_ok(text)[0].severity, Severity::Low);
    // Every job has its own.
    let text = "on: push\njobs:\n  a:\n    runs-on: x\n    permissions: read-all\n";
    assert_eq!(check_ok(text), vec![]);
    // A workflow only others call gets what its caller gives it.
    let text = "on: workflow_call\njobs:\n  a:\n    runs-on: x\n";
    assert_eq!(check_ok(text), vec![]);
    let text = "on:\n  workflow_call:\n  push:\njobs:\n  a:\n    runs-on: x\n";
    assert_eq!(check_ok(text).len(), 1);
    // write-all, at the top and in a job.
    let text = "on: push\npermissions: write-all\njobs:\n  a:\n    runs-on: x\n    permissions: write-all\n";
    assert_eq!(
        kinds(&check_ok(text)),
        vec![
            (RiskKind::Permissions, "", "write-all", 2),
            (RiskKind::Permissions, "a", "write-all", 6),
        ]
    );
    assert_eq!(check_ok(text)[0].severity, Severity::Medium);
}

#[test]
fn checking_out_a_pull_request_counts_only_where_secrets_are_exposed() {
    let job = "jobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@v5\n        with:\n          ref: refs/pull/${{ github.event.number }}/merge\n      - uses: actions/checkout@v5\n        with:\n          repository: ${{ github.event.pull_request.head.repo.full_name }}\n      - run: gh pr checkout ${{ github.event.number }}\n      - run: git fetch origin ${{ github.event.workflow_run.head_sha }}\n      - run: echo ${{ github.event.pull_request.head.sha }}\n";
    let on_target = format!("on: pull_request_target\npermissions: {{}}\n{job}");
    assert_eq!(
        kinds(&check_ok(&on_target)),
        vec![
            (RiskKind::PullCheckout, "a", "refs/pull/", 9),
            (
                RiskKind::PullCheckout,
                "a",
                "github.event.pull_request.head.repo.full_name",
                12
            ),
            (RiskKind::PullCheckout, "a", "gh pr checkout", 13),
            (
                RiskKind::PullCheckout,
                "a",
                "github.event.workflow_run.head_sha",
                14
            ),
        ]
    );
    let on_run = format!("on:\n  workflow_run:\n    workflows: [CI]\npermissions: {{}}\n{job}");
    assert_eq!(check_ok(&on_run).len(), 4);
    // A pull request's own workflow gets no secrets from a fork.
    let on_pull = format!("on: pull_request\npermissions: {{}}\n{job}");
    assert_eq!(check_ok(&on_pull), vec![]);
}

#[test]
fn text_anyone_can_write_in_a_script() {
    let text = "on: [issues, issue_comment]\npermissions: {}\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo \"${{ github.event.issue.title }}\"\n      - run: echo \"${{github.event.issue.title}}\"\n      - run: >\n          echo ${{ github.event.comment.body }}\n      - run: echo '${{ toJSON(github.event) }}'\n      - run: echo ${{ github.event.commits[0].message }} ${{ github.event.pages.*.page_name }}\n      - run: echo ${{ github.event.issue.title_x }} ${{ github.event.issue.number }} ${{ github.actor }}\n      - uses: actions/github-script@v7\n        with:\n          script: |\n            console.log(\"${{ github.head_ref }}\")\n      - uses: other/action@0123456789abcdef0123456789abcdef01234567\n        with:\n          script: ${{ github.event.issue.body }}\n";
    assert_eq!(
        kinds(&check_ok(text)),
        vec![
            (RiskKind::Injection, "a", "github.event.issue.title", 7),
            (RiskKind::Injection, "a", "github.event.comment.body", 10),
            (RiskKind::Injection, "a", "github.event", 11),
            (RiskKind::Injection, "a", "github.event.commits.message", 12),
            (RiskKind::Injection, "a", "github.event.pages.page_name", 12),
            (RiskKind::Injection, "a", "github.head_ref", 17),
        ]
    );
}

#[test]
fn names_in_brackets_read_as_properties() {
    let text = "on: [issues, pull_request_target]\npermissions: {}\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo \"${{ github.event['issue'].title }}\"\n      - run: echo \"${{ github['event'][\"comment\"]['body'] }}\"\n      - run: echo ${{ github.event[ 'pull_request' ][ 'title' ] }} ${{ github.event.issue['title_x'] }}\n      - run: echo '${{ toJSON(github['event']) }}'\n      - uses: actions/checkout@v5\n        with:\n          ref: ${{ github.event['pull_request'].head.sha }}\n";
    assert_eq!(
        kinds(&check_ok(text)),
        vec![
            (RiskKind::Injection, "a", "github.event.issue.title", 7),
            (RiskKind::Injection, "a", "github.event.comment.body", 8),
            (
                RiskKind::Injection,
                "a",
                "github.event.pull_request.title",
                9
            ),
            (RiskKind::Injection, "a", "github.event", 10),
            (
                RiskKind::PullCheckout,
                "a",
                "github.event.pull_request.head.sha",
                13
            ),
        ]
    );
    // Only a quoted name is a property; other indexes are taken out.
    for (expression, reads) in [
        (" github.event['issue'].title ", "github.event.issue.title"),
        (
            "github[\"event\"][ 'pull_request' ]['head'].sha",
            "github.event.pull_request.head.sha",
        ),
        (
            "github.event.commits[0].message",
            "github.event.commits.message",
        ),
        (
            "github.event.commits.*.message",
            "github.event.commits.message",
        ),
        (
            "github.event[github.event['x']].title",
            "github.event.title",
        ),
        (
            "fromJSON(env.X)[github.event.issue.number]",
            "fromjson(env.x)",
        ),
        ("github.event['a]b'].c", "github.event.a]b.c"),
    ] {
        assert_eq!(normalize(expression), reads, "{expression}");
    }
}

#[test]
fn files_orbit_cant_check_say_why() {
    assert!(check(PATH, "on: [push", "acme")
        .unwrap_err()
        .contains("isn't valid YAML"));
    let big = format!("on: push\n#{}\n", "x".repeat(MAX_TEXT));
    assert_eq!(
        check(PATH, &big, "acme").unwrap_err(),
        "it's too big to check"
    );
    let deep = format!("on: {}{}\n", "[".repeat(100), "]".repeat(100));
    assert_eq!(
        check(PATH, &deep, "acme").unwrap_err(),
        "it nests too deeply to check"
    );
    // A few lines of aliases that would expand to billions of nodes.
    let mut laughs = String::from("a: &a [x, x, x, x, x, x, x, x, x, x]\n");
    for (i, c) in ('b'..='k').enumerate() {
        let prev = (b'a' + i as u8) as char;
        let refs = vec![format!("*{prev}"); 10].join(", ");
        laughs.push_str(&format!("{c}: &{c} [{refs}]\n"));
    }
    assert!(laughs.len() < 1000);
    assert!(check(PATH, &laughs, "acme")
        .unwrap_err()
        .contains("too big to check once its aliases are expanded"));
    assert!(check(PATH, "x: *nothing\n", "acme").is_err());
}

#[test]
fn anything_that_isnt_a_workflow_has_no_risks() {
    assert_eq!(check_ok(""), vec![]);
    assert_eq!(check_ok("# nothing\n"), vec![]);
    assert_eq!(check_ok("- a\n- b\n"), vec![]);
    assert_eq!(check_ok("just text\n"), vec![]);
    // YAML 1.2: `on` is a key, not true.
    assert_eq!(
        kinds(&check_ok("on: push\njobs:\n  a:\n    runs-on: x\n")),
        vec![(RiskKind::Permissions, "", "missing", 2)]
    );
}

#[test]
fn aliases_are_followed() {
    let text = "on: pull_request_target\npermissions: {}\nx-steps: &steps\n  - uses: other/tool@v1\n  - run: echo ${{ github.event.issue.title }}\njobs:\n  a:\n    runs-on: x\n    steps: *steps\n  b:\n    runs-on: x\n    steps: *steps\n";
    assert_eq!(
        kinds(&check_ok(text)),
        vec![
            (RiskKind::Injection, "a", "github.event.issue.title", 5),
            (RiskKind::Injection, "b", "github.event.issue.title", 5),
            (RiskKind::Unpinned, "", "other/tool@v1", 4),
        ]
    );
}

#[test]
fn many_risks_keep_the_most_severe() {
    let mut text = String::from("on: issues\njobs:\n  a:\n    runs-on: x\n    steps:\n");
    for i in 0..80 {
        text.push_str(&format!("      - uses: other/tool{i}@v1\n"));
    }
    text.push_str("      - run: echo ${{ github.event.issue.body }}\n");
    let found = check(PATH, &text, "acme").unwrap();
    let risks = &found.risks;
    assert_eq!(risks.len(), MAX_PER_FILE);
    assert_eq!(risks[0].kind, RiskKind::Injection);
    assert_eq!(risks[1].subject, "other/tool0@v1");
    // The missing permissions are low, so they go first.
    assert!(risks.iter().all(|r| r.subject != "missing"));
    // 80 actions, the script and the permissions, less the 50 kept.
    assert_eq!(found.more, 32);

    // However many come first, a high one found last is kept.
    let mut text =
        String::from("on: issues\npermissions: {}\njobs:\n  a:\n    runs-on: x\n    steps:\n");
    for i in 0..3000 {
        text.push_str(&format!("      - uses: other/tool{i}@v1\n"));
    }
    text.push_str("      - run: echo ${{ github.event.issue.body }}\n");
    let found = check(PATH, &text, "acme").unwrap();
    assert_eq!(found.risks[0].kind, RiskKind::Injection);
    assert_eq!(found.risks[1].subject, "other/tool0@v1");
    assert_eq!(found.risks.len(), MAX_PER_FILE);
    assert_eq!(found.more, 3001 - MAX_PER_FILE as u32);
    // A file with no more than it keeps has none more.
    assert_eq!(check(PATH, &ci_with(1), "acme").unwrap().more, 0);
}
fn ci_with(actions: usize) -> String {
    let mut text =
        String::from("on: push\npermissions: {}\njobs:\n  a:\n    runs-on: x\n    steps:\n");
    for i in 0..actions {
        text.push_str(&format!("      - uses: other/tool{i}@v1\n"));
    }
    text
}

#[test]
fn names_and_advice_read_plainly() {
    let risk = |kind, job: &str, subject: &str| Risk {
        kind,
        path: PATH.into(),
        job: job.into(),
        subject: subject.into(),
        ..Default::default()
    };
    assert_eq!(
        risk(RiskKind::Unpinned, "", "a/b@v1").name(),
        "a/b@v1 isn't pinned, in ci.yml"
    );
    assert_eq!(
        risk(RiskKind::Permissions, "", "missing").name(),
        "No permissions set, in ci.yml"
    );
    assert_eq!(
        risk(RiskKind::Permissions, "build", "write-all").name(),
        "write-all permissions, in ci.yml, job build"
    );
    assert_eq!(
        risk(RiskKind::PullCheckout, "build", "github.head_ref").name(),
        "Pull request code checked out, in ci.yml, job build"
    );
    assert_eq!(
        risk(RiskKind::Injection, "build", "github.head_ref").name(),
        "github.head_ref in a script, in ci.yml, job build"
    );
    let all = [
        risk(RiskKind::Unpinned, "", "a/b@v1"),
        risk(RiskKind::Unpinned, "", "c/d@v1"),
        risk(RiskKind::Unpinned, "", "e/f@v1"),
    ];
    assert_eq!(
        names(&all),
        "a/b@v1 isn't pinned, in ci.yml, c/d@v1 isn't pinned, in ci.yml and 1 more"
    );
    assert!(risk(RiskKind::Injection, "", "x").advice().contains("env:"));
    assert!(risk(RiskKind::PullCheckout, "", "x")
        .advice()
        .contains("pull_request"));
}

#[test]
fn orbits_own_proposals_have_no_risks() {
    let names = vec!["package.json".to_string(), "Cargo.toml".to_string()];
    let package = r#"{"scripts":{"lint":"eslint .","test":"vitest"}}"#;
    let (workflow, _) = crate::workflows::propose(&names, Some(package), "main");
    assert!(!workflow.is_empty());
    assert_eq!(check_ok(&workflow), vec![]);
}
