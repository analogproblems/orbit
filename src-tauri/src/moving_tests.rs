use super::*;

const SCAN: &str = "5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f";

fn scans() -> Vec<String> {
    vec![SCAN.into()]
}

#[test]
fn only_history_write_ups_and_kept_scan_files_have_a_place() {
    let root = Path::new("/data");
    let allowed = [
        "ledger/octo/app/12.json",
        "ledger/octo/my.app_2/writeups/7.json",
        &format!("scans/{SCAN}/report.md"),
        &format!("scans/{SCAN}/findings.json"),
    ];
    for path in allowed {
        assert_eq!(
            destination(root, path, &scans()),
            Some(root.join(path)),
            "{path}"
        );
    }
    let refused = [
        "state.json",
        "remote-token",
        "ledger/octo/app/12.json.tmp",
        "ledger/octo/app/notes.json",
        "ledger/octo/app/.json",
        "ledger/octo/../12.json",
        "ledger/../app/12.json",
        "ledger/-x/app/12.json",
        "ledger/octo/app/writeups/../12.json",
        "ledger/octo/app/other/12.json",
        "ledger/octo/app",
        "/ledger/octo/app/12.json",
        "ledger//app/12.json",
        "ledger\\octo\\app\\12.json",
        "clones/octo/app/README.md",
        "worktrees/abc/file",
        "scans/another-scan/report.md",
        &format!("scans/{SCAN}/artifacts/big.json"),
        &format!("scans/{SCAN}/notes.md"),
        "scans/../report.md",
    ];
    for path in refused {
        assert_eq!(destination(root, path, &scans()), None, "{path}");
    }
}

#[test]
fn collects_what_moves_and_puts_it_in_place_elsewhere() {
    let from = tempfile::tempdir().unwrap();
    let write = |path: &str, text: &str| {
        let place = from.path().join(path);
        std::fs::create_dir_all(place.parent().unwrap()).unwrap();
        std::fs::write(place, text).unwrap();
    };
    write("ledger/octo/app/12.json", r#"{"pr":12}"#);
    write("ledger/octo/app/writeups/12.json", r#"{"writeup":12}"#);
    write("ledger/octo/app/13.json.tmp", "half");
    write(&format!("scans/{SCAN}/report.md"), "# Report");
    write("scans/gone/report.md", "# Old");
    write("clones/octo/app/README.md", "a copy");
    write("state.json", "{}");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        from.path().join("state.json"),
        from.path().join("ledger/octo/app/14.json"),
    )
    .unwrap();
    let mut files = collect(from.path(), &scans()).unwrap();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "ledger/octo/app/12.json",
            "ledger/octo/app/writeups/12.json",
            &format!("scans/{SCAN}/report.md"),
        ]
    );
    assert_eq!(files[0].text, r#"{"pr":12}"#);

    let to = tempfile::tempdir().unwrap();
    put(to.path(), &files, &scans()).unwrap();
    for file in &files {
        assert_eq!(
            std::fs::read_to_string(to.path().join(&file.path)).unwrap(),
            file.text
        );
    }
    // Nothing is written when any file has no place.
    let other = tempfile::tempdir().unwrap();
    let mut bad = files.clone();
    bad.push(MovedFile {
        path: "remote-token".into(),
        text: "x".into(),
    });
    let error = put(other.path(), &bad, &scans()).unwrap_err();
    assert!(error.contains("remote-token"), "{error}");
    assert_eq!(std::fs::read_dir(other.path()).unwrap().count(), 0);
}

#[test]
fn an_orbit_with_nothing_kept_moves_no_files() {
    let empty = tempfile::tempdir().unwrap();
    assert!(collect(empty.path(), &[]).unwrap().is_empty());
}
