use super::*;
use serde_json::json;

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NEWER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn a_workflow_runs_on_pull_requests_when_its_on_says_so() {
    let yes = [
        "on: pull_request",
        "on: [push, pull_request]",
        "on: [ 'pull_request_target' ]",
        "\"on\": pull_request",
        "on: push",
        "on: push # every branch",
        "on:\n  pull_request:\n    branches: [main]\n",
        "on:\n  - push\n  - workflow_dispatch\n",
        "on:\n  pull_request_target:\n    types: [opened, synchronize]\n",
        "on:\n  push:\n",
        "on:\n  push: {}\n",
        "on:\n  push:\n    paths:\n      - 'src/**'\n",
        "on:\n  push:\n    branches-ignore: [main]\n",
        "on:\n  push:\n    branches:\n      - main\n      - 'feature/**'\n",
        "on:\n  push:\n    branches: [main, \"release-*\"]\n",
        // Comments, a document start and other keys around it.
        "# CI\n---\nname: CI\n\non:\n  # pull requests\n  pull_request:\n\njobs:\n  test:\n    runs-on: ubuntu-latest\n",
    ];
    for text in yes {
        assert_eq!(on_pulls(text), OnPulls::Yes, "{text}");
    }
}

#[test]
fn a_workflow_that_only_runs_elsewhere_doesnt_run_on_pull_requests() {
    let no = [
        "on: workflow_dispatch",
        "on: [release, schedule]",
        "on:\n  schedule:\n    - cron: '0 0 * * *'\n  workflow_dispatch:\n",
        "on:\n  push:\n    branches: [main]\n",
        "on:\n  push:\n    branches:\n      - main\n      - develop\n    paths-ignore:\n      - docs/**\n",
        "on:\n  push:\n    tags:\n      - 'v*'\n",
        "on:\n  push:\n    tags-ignore: [nightly]\n  release:\n    types: [published]\n",
        "on:\n  workflow_call:\n    inputs:\n      name:\n        type: string\n",
        "name: Deploy\non:\n  push:\n    branches: [ main ] # only main\njobs:\n  deploy:\n    runs-on: ubuntu-latest\n",
    ];
    for text in no {
        assert_eq!(on_pulls(text), OnPulls::No, "{text}");
    }
}

#[test]
fn a_workflow_orbit_cant_read_for_sure_is_unknown() {
    let unknown = [
        "",
        "name: CI\njobs: {}\n",
        // GitHub reads `on`, not YAML 1.1's `true`.
        "true: push",
        "on: {push: {branches: [main]}}",
        "on: [push,\n  pull_request]",
        "on: &triggers\n  push:\n",
        "on:\n  push:\n    branches: main\n",
        "on:\n  push: {branches: [main]}\n",
        "on:\n  push:\n    types: [created]\n",
        "on:\n  - push:\n      branches: [main]\n",
        "on:\n  - workflow_dispatch\n  schedule:\n",
        "on:\n    push:\n  pull_request:\n",
        "on:\n\tpush:\n",
        "on: workflow_dispatch\non: push\n",
        "on: Push",
        "on: workflow_dispatch\n---\non: push\n",
        "on: workflow_dispatch\n---\njobs: {}\n",
        // A list item that goes on to the next line.
        "on:\n  - workflow_dispatch\n    pull_request\n",
    ];
    for text in unknown {
        assert_eq!(on_pulls(text), OnPulls::Unknown, "{text:?}");
    }
    // Something Orbit can't read beside an event it can still says no only
    // when nothing runs on pull requests for sure.
    let mixed = "on:\n  push:\n    branches: main\n  pull_request:\n";
    assert_eq!(on_pulls(mixed), OnPulls::Yes);
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|n| n.to_string()).collect()
}

#[test]
fn a_node_repository_gets_its_scripts_run_with_its_package_manager() {
    let package = json!({
        "scripts": { "lint": "eslint .", "build": "vite build", "test": "vitest run", "check": "npm run lint && npm test", "deploy": "rm -rf /" }
    })
    .to_string();
    let (workflow, from) = propose(
        &names(&["package.json", "package-lock.json", ".nvmrc", "src"]),
        Some(&package),
        "main",
    );
    assert_eq!(from, ["package.json", ".nvmrc", "package-lock.json"]);
    assert_eq!(
        workflow,
        "name: CI\n\non:\n  pull_request:\n  push:\n    branches:\n      - main\n\npermissions:\n  contents: read\n\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v5\n      - uses: actions/setup-node@v5\n        with:\n          node-version-file: .nvmrc\n          cache: npm\n      - run: npm ci\n      - run: npm run lint\n      - run: npm run build\n      - run: npm test\n"
    );
    assert_eq!(on_pulls(&workflow), OnPulls::Yes);

    // pnpm and yarn come from Corepack.
    let (workflow, from) = propose(
        &names(&["package.json", "pnpm-lock.yaml"]),
        Some(&package),
        "trunk",
    );
    assert_eq!(from, ["package.json", "pnpm-lock.yaml"]);
    // setup-node's own caching is off, as pnpm isn't set up when it runs.
    assert!(workflow.contains("          node-version: lts/*\n          package-manager-cache: false\n      - run: corepack enable\n      - run: pnpm install --frozen-lockfile\n      - run: pnpm run lint\n"), "{workflow}");
    assert!(workflow.contains("      - run: pnpm test\n"));
    assert!(!workflow.contains("          cache:"));
    assert!(workflow.contains("      - trunk\n"));
    let yarn = json!({ "packageManager": "yarn@4.5.0" }).to_string();
    let (workflow, _) = propose(&names(&["package.json", "yarn.lock"]), Some(&yarn), "main");
    assert!(
        workflow.contains(
            "          package-manager-cache: false\n      - run: corepack enable\n      - run: yarn install --frozen-lockfile\n"
        ),
        "{workflow}"
    );

    // Without a lock file, npm installs; with only `check`, that runs; npm's
    // placeholder test doesn't.
    let only = json!({ "scripts": { "check": "tsc && node test.js", "test": "echo \"Error: no test specified\" && exit 1" } }).to_string();
    let (workflow, from) = propose(&names(&["package.json"]), Some(&only), "main");
    assert_eq!(from, ["package.json"]);
    assert!(
        workflow.ends_with("      - run: npm install\n      - run: npm run check\n"),
        "{workflow}"
    );
    // With no lock file to cache by, setup-node's own caching is off, even
    // when `package.json` names npm.
    let named =
        json!({ "packageManager": "npm@10.9.0", "scripts": { "test": "node --test" } }).to_string();
    let (workflow, _) = propose(&names(&["package.json"]), Some(&named), "main");
    assert!(
        workflow.contains("      - uses: actions/setup-node@v5\n        with:\n          node-version: lts/*\n          package-manager-cache: false\n      - run: npm install\n      - run: npm test\n"),
        "{workflow}"
    );
    // A package.json that isn't JSON still installs.
    let (workflow, _) = propose(&names(&["package.json"]), Some("{ not json"), "main");
    assert!(
        workflow.ends_with("      - run: npm install\n"),
        "{workflow}"
    );
}

#[test]
fn rust_and_go_get_their_tests_run() {
    let (workflow, from) = propose(
        &names(&["Cargo.toml", "Cargo.lock", "go.mod"]),
        None,
        "main",
    );
    assert_eq!(from, ["Cargo.toml", "Cargo.lock", "go.mod"]);
    assert!(workflow.ends_with("      - uses: actions/checkout@v5\n      - run: cargo test --locked\n      - uses: actions/setup-go@v5\n        with:\n          go-version-file: go.mod\n      - run: go test ./...\n"), "{workflow}");
    let (workflow, _) = propose(&names(&["Cargo.toml"]), None, "main");
    assert!(workflow.contains("      - run: cargo test\n"));
    // Node and Rust together run both.
    let package = json!({ "scripts": { "test": "vitest" } }).to_string();
    let (workflow, _) = propose(
        &names(&["package.json", "Cargo.toml"]),
        Some(&package),
        "main",
    );
    assert!(
        workflow.ends_with("      - run: npm test\n      - run: cargo test\n"),
        "{workflow}"
    );
}

#[test]
fn nothing_is_proposed_when_orbit_cant_tell_how_it_builds() {
    for top in [
        names(&["README.md", "src"]),
        names(&["pyproject.toml"]),
        // Bun isn't on GitHub's runner.
        names(&["package.json", "bun.lockb"]),
    ] {
        assert_eq!(
            propose(&top, Some("{}"), "main"),
            (String::new(), vec![]),
            "{top:?}"
        );
    }
}

#[test]
fn a_branch_that_would_need_quoting_is_left_out() {
    for branch in ["", "-main", "1.0", "dev#2", "a b", "main'", "ma\"in", "`x`"] {
        let (workflow, _) = propose(&names(&["Cargo.toml"]), None, branch);
        assert!(
            workflow.contains("on:\n  pull_request:\n\npermissions:"),
            "{branch}: {workflow}"
        );
        assert!(!workflow.contains("push:"), "{branch}");
    }
    let (workflow, _) = propose(&names(&["Cargo.toml"]), None, "release/v1.2_x");
    assert!(workflow.contains("      - release/v1.2_x\n"));
}

#[test]
fn checks_each_workflow_for_risky_settings_and_reads_whether_the_repository_is_private() {
    let mut v = answer(
        json!({ "entries": [
            file("ci.yml", "on: pull_request\npermissions: {}\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: other/tool@v1\n      - uses: a/own@v1\n"),
            file("safe.yml", "on: pull_request\npermissions: {}\njobs:\n  a:\n    runs-on: x\n"),
            file("broken.yml", "on: [pull_request"),
            { "name": "cut.yml", "type": "blob", "object": { "text": "on: push", "isTruncated": true, "isBinary": false } },
        ] }),
        &[],
        Value::Null,
    );
    let setup = understand("a/b", "main", SHA, &v, now()).unwrap();
    let checks: Vec<(&str, bool, usize)> = setup
        .workflows
        .iter()
        .map(|w| (w.path.as_str(), w.checked, w.risks.len()))
        .collect();
    assert_eq!(
        checks,
        [
            (".github/workflows/ci.yml", true, 1),
            (".github/workflows/safe.yml", true, 0),
            (".github/workflows/broken.yml", false, 0),
            (".github/workflows/cut.yml", false, 0),
        ]
    );
    let why: Vec<&str> = setup.workflows.iter().map(|w| w.why.as_str()).collect();
    assert_eq!(
        (why[0], why[1], why[3]),
        ("", "", "GitHub didn't send all of it")
    );
    assert!(why[2].starts_with("it isn't valid YAML"), "{}", why[2]);
    // The owner's own actions aren't a third party's.
    assert_eq!(setup.workflows[0].risks[0].subject, "other/tool@v1");
    assert_eq!(setup.checks, CHECKS);
    // Private only when GitHub says so, and not internal.
    assert!(!setup.private);
    for (private, visibility, says) in [
        (true, "PRIVATE", true),
        (true, "INTERNAL", false),
        (false, "PUBLIC", false),
    ] {
        v["data"]["repository"]["isPrivate"] = json!(private);
        v["data"]["repository"]["visibility"] = json!(visibility);
        let setup = understand("a/b", "main", SHA, &v, now()).unwrap();
        assert_eq!(setup.private, says, "{visibility}");
    }
    assert!(query().contains("isPrivate visibility"));
}

fn answer(workflows: Value, root: &[&str], package: Value) -> Value {
    let entries: Vec<Value> = root
        .iter()
        .map(|n| json!({ "name": n, "type": "blob" }))
        .collect();
    json!({ "data": { "repository": {
        "workflows": workflows,
        "root": { "entries": entries },
        "package": package,
    } } })
}
fn file(name: &str, text: &str) -> Value {
    json!({ "name": name, "type": "blob", "object": { "text": text, "isTruncated": false, "isBinary": false } })
}

#[test]
fn reads_the_workflows_and_other_ci_from_githubs_answer() {
    let v = answer(
        json!({ "entries": [
            file("ci.yml", "on: [pull_request]"),
            file("deploy.yaml", "on:\n  push:\n    branches: [main]\n"),
            file("README.md", "on: pull_request"),
            { "name": "old", "type": "tree", "object": {} },
            // GitHub runs no workflow from a folder, whatever its name.
            { "name": "more.yml", "type": "tree", "object": {} },
            { "name": "big.yml", "type": "blob", "object": { "text": null, "isTruncated": false, "isBinary": false } },
            { "name": "cut.yml", "type": "blob", "object": { "text": "on: push", "isTruncated": true, "isBinary": false } },
        ] }),
        &["package.json", ".circleci", "Jenkinsfile", "jenkinsfile"],
        json!({ "text": "{\"scripts\":{\"test\":\"jest\"}}", "isTruncated": false }),
    );
    let setup = understand("a/b", "main", SHA, &v, now()).unwrap();
    let paths: Vec<(&str, OnPulls)> = setup
        .workflows
        .iter()
        .map(|w| (w.path.as_str(), w.on_pulls))
        .collect();
    assert_eq!(
        paths,
        [
            (".github/workflows/ci.yml", OnPulls::Yes),
            (".github/workflows/deploy.yaml", OnPulls::No),
            (".github/workflows/big.yml", OnPulls::Unknown),
            (".github/workflows/cut.yml", OnPulls::Unknown),
        ]
    );
    assert_eq!(setup.others, [".circleci", "Jenkinsfile"]);
    assert_eq!(setup.from, ["package.json"]);
    assert!(setup.proposal.contains("      - run: npm test\n"));
    assert_eq!((setup.sha.as_str(), setup.branch.as_str()), (SHA, "main"));
    assert_eq!(setup.read_at, "2026-10-30T12:00:00.000Z");
    assert_eq!(
        setup.verdict(),
        Some(Verdict::Covered(".github/workflows/ci.yml".into()))
    );

    // No folder, and a package.json GitHub cut short.
    let v = answer(
        Value::Null,
        &["package.json"],
        json!({ "text": "{\"scripts\":{\"test\":\"jest\"}}", "isTruncated": true }),
    );
    let setup = understand("a/b", "main", SHA, &v, now()).unwrap();
    assert!(setup.workflows.is_empty() && setup.others.is_empty());
    assert!(
        setup.proposal.ends_with("      - run: npm install\n"),
        "{}",
        setup.proposal
    );
    assert_eq!(setup.verdict(), Some(Verdict::NoWorkflows));

    // Errors.
    let refused = json!({ "errors": [{ "message": "Something went wrong" }] });
    assert_eq!(
        understand("a/b", "main", SHA, &refused, now()),
        Err("Something went wrong".into())
    );
    let gone = json!({ "data": { "repository": null } });
    assert!(understand("a/b", "main", SHA, &gone, now()).is_err());
    let no_commit =
        json!({ "data": { "repository": { "workflows": null, "root": null, "package": null } } });
    assert_eq!(
        understand("a/b", "main", SHA, &no_commit, now()),
        Err("GitHub didn't return the files at aaaaaaa.".into())
    );
}

#[test]
fn says_what_the_files_mean_for_ci_on_pull_requests() {
    let flow = |list: &[(&str, OnPulls)], others: &[&str]| Setup {
        workflows: list
            .iter()
            .map(|(p, o)| Workflow {
                path: p.to_string(),
                on_pulls: *o,
                ..Default::default()
            })
            .collect(),
        others: names(others),
        ..Default::default()
    };
    assert_eq!(flow(&[], &[]).verdict(), Some(Verdict::NoWorkflows));
    assert_eq!(
        flow(&[("d.yml", OnPulls::No)], &[]).verdict(),
        Some(Verdict::NotOnPulls)
    );
    assert_eq!(
        flow(&[("d.yml", OnPulls::No), ("x.yml", OnPulls::Unknown)], &[]).verdict(),
        Some(Verdict::Unknown("x.yml".into()))
    );
    assert_eq!(
        flow(&[("x.yml", OnPulls::Unknown)], &[".travis.yml"]).verdict(),
        Some(Verdict::Covered(".travis.yml".into()))
    );
    assert_eq!(
        flow(
            &[("x.yml", OnPulls::Unknown), ("ci.yml", OnPulls::Yes)],
            &[".travis.yml"]
        )
        .verdict(),
        Some(Verdict::Covered("ci.yml".into()))
    );
    let failed = Setup {
        error: Some("down".into()),
        ..Default::default()
    };
    assert_eq!(failed.verdict(), None);
    let found = flow(&[("d.yml", OnPulls::No)], &[]);
    assert_eq!(found.found().workflows, ["d.yml"]);
}

fn repo_ci(repo: &str, branch: &str, sha: &str) -> ci::RepoCi {
    ci::RepoCi {
        repo: repo.into(),
        branch: branch.into(),
        head: ci::CommitCi {
            sha: sha.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn reads_each_new_commit_once_and_tries_again_after_an_error() {
    let ci = vec![
        repo_ci("a/b", "main", SHA),
        repo_ci("c/d", "main", SHA),
        repo_ci("e/f", "main", NEWER),
        // Nothing to read yet.
        repo_ci("g/h", "main", ""),
        repo_ci("i/j", "", SHA),
    ];
    let read = |repo: &str, sha: &str| Setup {
        repo: repo.into(),
        branch: "main".into(),
        sha: sha.into(),
        checks: CHECKS,
        ..Default::default()
    };
    let ran = Some("Checks ran on #7, so something runs them.".to_string());
    let seen = Setup {
        ran: ran.clone(),
        ..read("e/f", SHA)
    };
    let mut setups = vec![read("A/B", SHA), seen];
    let want: Vec<String> = wanted(&ci, &setups, now())
        .into_iter()
        .map(|(r, _, _)| r)
        .collect();
    assert_eq!(want, ["c/d", "e/f"]);
    assert_eq!(
        wanted(&ci[2..3], &setups, now()),
        [("e/f".into(), "main".into(), NEWER.into())]
    );

    // At most PER_PASS, and those tried least first.
    record(
        &mut setups,
        "e/f",
        "main",
        NEWER,
        Err("gh: Bad Gateway (HTTP 502)".into()),
        now(),
    );
    let e = setups.iter().find(|s| s.repo == "e/f").unwrap();
    assert_eq!(e.sha, NEWER);
    assert_eq!(e.tries, 1);
    assert_eq!(e.retry_at.as_deref(), Some("2026-10-30T12:01:00.000Z"));
    assert_eq!(
        e.error.as_deref(),
        Some(
            "Orbit couldn't read the workflows yet: Bad Gateway (HTTP 502). It tries again later."
        )
    );
    assert_eq!(e.verdict(), None);
    // The checks Orbit saw are kept, through errors and newer commits.
    assert_eq!(e.ran, ran);
    let want: Vec<String> = wanted(&ci, &setups, now())
        .into_iter()
        .map(|(r, _, _)| r)
        .collect();
    assert_eq!(want, ["c/d"]);
    let later = now() + Duration::minutes(1);
    let want: Vec<String> = wanted(&ci, &setups, later)
        .into_iter()
        .map(|(r, _, _)| r)
        .collect();
    assert_eq!(want, ["c/d", "e/f"]);
    record(&mut setups, "e/f", "main", NEWER, Err("down".into()), later);
    let e = setups.iter().find(|s| s.repo == "e/f").unwrap();
    assert_eq!(
        (e.tries, e.retry_at.as_deref()),
        (2, Some("2026-10-30T12:03:00.000Z"))
    );
    // A good read replaces it.
    record(
        &mut setups,
        "e/f",
        "main",
        NEWER,
        Ok(read("e/f", NEWER)),
        later,
    );
    let e = setups.iter().find(|s| s.repo == "e/f").unwrap();
    assert_eq!((e.tries, e.error.as_ref(), &e.ran), (0, None, &ran));
    assert!(wanted(&ci[2..3], &setups, later).is_empty());
    // A new repository's first read is added.
    record(&mut setups, "c/d", "main", SHA, Ok(read("c/d", SHA)), later);
    assert!(wanted(&ci, &setups, later).is_empty());
    // A branch renamed at the same commit is read again, as the proposal
    // names it.
    let renamed = [repo_ci("c/d", "trunk", SHA)];
    assert_eq!(
        wanted(&renamed, &setups, later),
        [("c/d".into(), "trunk".into(), SHA.into())]
    );
    // A commit read before today's checks of workflow files is read
    // again, but one that failed waits for its turn as before.
    let older = Setup {
        checks: 0,
        ..read("c/d", SHA)
    };
    let same = [repo_ci("c/d", "main", SHA)];
    assert_eq!(wanted(&same, std::slice::from_ref(&older), later).len(), 1);
    let failed = Setup {
        error: Some("down".into()),
        retry_at: Some(stamp(later + Duration::minutes(5))),
        ..older
    };
    assert!(wanted(&same, &[failed], later).is_empty());
    // Those tried least go first.
    let tried = Setup {
        error: Some("down".into()),
        tries: 3,
        ..read("a/b", NEWER)
    };
    let three = [
        repo_ci("a/b", "main", NEWER),
        repo_ci("x/y", "main", SHA),
        repo_ci("z/w", "main", SHA),
    ];
    let want: Vec<String> = wanted(&three, &[tried], later)
        .into_iter()
        .map(|(r, _, _)| r)
        .collect();
    assert_eq!(want, ["x/y", "z/w"]);
    // Disconnected ones are forgotten.
    prune(&mut setups, &["a/b".into()]);
    assert_eq!(setups.len(), 1);
}
