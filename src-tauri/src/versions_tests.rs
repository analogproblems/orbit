use super::*;

const SHA: &str = "abc1234def5678abc1234def5678abc1234def56";

fn package_json(version: &str) -> String {
    format!(r#"{{ "name": "app", "version": "{version}", "private": true }}"#)
}
fn package_lock(version: &str) -> String {
    format!(
        r#"{{ "name": "app", "version": "{version}", "lockfileVersion": 3,
            "packages": {{ "": {{ "name": "app", "version": "{version}" }},
                "node_modules/left-pad": {{ "version": "9.9.9" }} }} }}"#
    )
}
fn cargo_toml(name: &str, version: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1\"\n")
}
fn cargo_lock(entries: &[(&str, &str)]) -> String {
    let mut out = "version = 4\n".to_string();
    for (name, version) in entries {
        out.push_str(&format!(
            "\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n"
        ));
    }
    out
}
fn tauri_conf(version: &str) -> String {
    format!(r#"{{ "productName": "App", "version": "{version}", "identifier": "dev.app" }}"#)
}
/// What `found` makes of `files`, as `(path, of, version)`, and the paths
/// it couldn't read.
fn read(files: &[(&str, Option<&str>)]) -> (Vec<(String, String, String)>, Vec<String>) {
    let (found, unread) = found(files);
    let found = found
        .into_iter()
        .map(|f| (f.path, f.of, f.version))
        .collect();
    (found, unread)
}
fn says(path: &str, of: &str, version: &str) -> (String, String, String) {
    (path.into(), of.into(), version.into())
}
/// The mismatches in `files`, as "path says x, want y (others)".
fn differ(files: &[(&str, Option<&str>)]) -> Vec<String> {
    let (found, _) = found(files);
    mismatches(&found, SHA)
        .into_iter()
        .map(|m| {
            assert_eq!(m.since, SHA);
            format!("{}, want {} ({})", m.name(), m.want, m.others.join(", "))
        })
        .collect()
}

#[test]
fn a_tauri_app_whose_files_agree() {
    let (package, lock) = (package_json("0.2.0"), package_lock("0.2.0"));
    let (cargo, cargo_lock) = (
        cargo_toml("orbit", "0.2.0"),
        cargo_lock(&[("orbit", "0.2.0"), ("serde", "1.0.200")]),
    );
    let tauri = tauri_conf("0.2.0");
    let files = [
        ("package.json", Some(package.as_str())),
        ("package-lock.json", Some(lock.as_str())),
        ("src-tauri/tauri.conf.json", Some(tauri.as_str())),
        ("src-tauri/Cargo.toml", Some(cargo.as_str())),
        ("src-tauri/Cargo.lock", Some(cargo_lock.as_str())),
    ];
    let (found, unread) = read(&files);
    assert_eq!(
        found,
        [
            says("package.json", "", "0.2.0"),
            says("package-lock.json", "package.json", "0.2.0"),
            says("src-tauri/Cargo.toml", "", "0.2.0"),
            says("src-tauri/Cargo.lock", "src-tauri/Cargo.toml", "0.2.0"),
            says("src-tauri/tauri.conf.json", "", "0.2.0"),
        ]
    );
    assert!(unread.is_empty());
    assert!(differ(&files).is_empty());
}

#[test]
fn a_tauri_app_with_one_file_behind() {
    // The release was bumped in two of the three files, and the Rust side
    // and its lockfile were missed.
    let (package, lock) = (package_json("0.3.0"), package_lock("0.3.0"));
    let (cargo, cargo_lock) = (
        cargo_toml("orbit", "0.2.0"),
        cargo_lock(&[("orbit", "0.2.0")]),
    );
    let tauri = tauri_conf("0.3.0");
    let files = [
        ("package.json", Some(package.as_str())),
        ("package-lock.json", Some(lock.as_str())),
        ("src-tauri/tauri.conf.json", Some(tauri.as_str())),
        ("src-tauri/Cargo.toml", Some(cargo.as_str())),
        ("src-tauri/Cargo.lock", Some(cargo_lock.as_str())),
    ];
    assert_eq!(
        differ(&files),
        [
            "src-tauri/Cargo.toml says 0.2.0, want 0.3.0 (package.json, src-tauri/tauri.conf.json)",
            "src-tauri/Cargo.lock says 0.2.0, want 0.3.0 (package.json, src-tauri/tauri.conf.json)",
        ]
    );
    let (found, _) = found(&files);
    let m = mismatches(&found, SHA);
    assert_eq!((m[0].kind, m[0].fix()), (Kind::Cargo, ""));
    assert_eq!(
        (m[1].kind, m[1].of.as_str(), m[1].fix()),
        (
            Kind::CargoLock,
            "src-tauri/Cargo.toml",
            "cargo update --workspace"
        )
    );
    assert!(m[0].same(&Mismatch {
        path: "src-tauri/Cargo.toml".into(),
        says: "0.2.1".into(),
        ..Default::default()
    }));
    assert!(!m[0].same(&m[1]));
}

#[test]
fn a_manifest_and_its_lockfile() {
    // With two files, the manifest is the one: the lockfile copies it.
    let (package, lock) = (package_json("1.4.0"), package_lock("1.3.0"));
    let files = [
        ("package.json", Some(package.as_str())),
        ("package-lock.json", Some(lock.as_str())),
    ];
    assert_eq!(
        differ(&files),
        ["package-lock.json says 1.3.0, want 1.4.0 (package.json)"]
    );
    let (found, _) = found(&files);
    assert_eq!(
        mismatches(&found, SHA)[0].fix(),
        "npm install --package-lock-only"
    );
    // An old lockfile, with no packages, says it in its own fields; one for
    // another package says nothing.
    let old = r#"{ "name": "app", "version": "1.3.0", "lockfileVersion": 1 }"#;
    let other = r#"{ "name": "else", "version": "1.3.0", "lockfileVersion": 1 }"#;
    assert_eq!(
        differ(&[
            ("package.json", Some(&package)),
            ("package-lock.json", Some(old))
        ]),
        ["package-lock.json says 1.3.0, want 1.4.0 (package.json)"]
    );
    assert!(differ(&[
        ("package.json", Some(&package)),
        ("package-lock.json", Some(other))
    ])
    .is_empty());
    // A lockfile alone is never read.
    let (found, unread) = read(&[("package-lock.json", Some(lock.as_str()))]);
    assert!(found.is_empty() && unread.is_empty());
}

#[test]
fn most_manifests_win_and_ties_go_to_the_first() {
    // package.json is the odd one out, though its lockfile agrees with it.
    let (package, lock) = (package_json("0.1.0"), package_lock("0.1.0"));
    let cargo = cargo_toml("app", "0.2.0");
    let tauri = tauri_conf("0.2.0");
    let files = [
        ("package.json", Some(package.as_str())),
        ("package-lock.json", Some(lock.as_str())),
        ("src-tauri/Cargo.toml", Some(cargo.as_str())),
        ("src-tauri/tauri.conf.json", Some(tauri.as_str())),
    ];
    assert_eq!(
        differ(&files),
        [
            "package.json says 0.1.0, want 0.2.0 (src-tauri/Cargo.toml, src-tauri/tauri.conf.json)",
            "package-lock.json says 0.1.0, want 0.2.0 (src-tauri/Cargo.toml, src-tauri/tauri.conf.json)",
        ]
    );
    // Two that differ: the first, in the order Orbit reads them, is the one.
    let tauri = tauri_conf("0.1.0");
    assert_eq!(
        differ(&[
            ("src-tauri/Cargo.toml", Some(cargo.as_str())),
            ("src-tauri/tauri.conf.json", Some(tauri.as_str())),
        ]),
        ["src-tauri/tauri.conf.json says 0.1.0, want 0.2.0 (src-tauri/Cargo.toml)"]
    );
}

#[test]
fn files_that_needn_t_agree_are_never_compared() {
    // A Rust crate and a JavaScript package side by side, and a Tauri app
    // that takes its version from package.json.
    let package = package_json("3.0.0");
    let cargo = cargo_toml("core", "0.9.0");
    let python = "[project]\nname = \"tool\"\nversion = \"2.0.0\"\n";
    let tauri = r#"{ "version": "../package.json", "identifier": "dev.app" }"#;
    let files = [
        ("package.json", Some(package.as_str())),
        ("Cargo.toml", Some(cargo.as_str())),
        ("pyproject.toml", Some(python)),
        ("src-tauri/tauri.conf.json", Some(tauri)),
    ];
    let (found, unread) = read(&files);
    assert_eq!(
        found,
        [
            says("package.json", "", "3.0.0"),
            says("Cargo.toml", "", "0.9.0"),
            says("pyproject.toml", "", "2.0.0"),
        ]
    );
    assert!(unread.is_empty());
    assert!(differ(&files).is_empty());
}

#[test]
fn a_cargo_workspace() {
    // The top has no package of its own, and its one lockfile records the
    // app, which takes the workspace's version.
    let top =
        "[workspace]\nmembers = [\"src-tauri\"]\n\n[workspace.package]\nversion = \"1.2.0\"\n";
    let app = "[package]\nname = \"app\"\nversion.workspace = true\n";
    let lock = cargo_lock(&[("app", "1.1.0"), ("serde", "1.0.200")]);
    let files = [
        ("Cargo.toml", Some(top)),
        ("Cargo.lock", Some(lock.as_str())),
        ("src-tauri/Cargo.toml", Some(app)),
    ];
    let (found, unread) = read(&files);
    assert_eq!(
        found,
        [
            says("src-tauri/Cargo.toml", "", "1.2.0"),
            says("Cargo.lock", "src-tauri/Cargo.toml", "1.1.0"),
        ]
    );
    assert!(unread.is_empty());
    assert_eq!(
        differ(&files),
        ["Cargo.lock says 1.1.0, want 1.2.0 (src-tauri/Cargo.toml)"]
    );
    // A package at the top and the app share the lockfile, each with its
    // own entry.
    let top = cargo_toml("core", "0.5.0");
    let app = cargo_toml("app", "1.2.0");
    let lock = cargo_lock(&[("app", "1.2.0"), ("core", "0.4.0")]);
    let files = [
        ("Cargo.toml", Some(top.as_str())),
        ("Cargo.lock", Some(lock.as_str())),
        ("src-tauri/Cargo.toml", Some(app.as_str())),
    ];
    let (found, _) = read(&files);
    assert_eq!(
        found,
        [
            says("Cargo.toml", "", "0.5.0"),
            says("Cargo.lock", "Cargo.toml", "0.4.0"),
            says("src-tauri/Cargo.toml", "", "1.2.0"),
            says("Cargo.lock", "src-tauri/Cargo.toml", "1.2.0"),
        ]
    );
    assert_eq!(
        differ(&files),
        ["Cargo.lock says 0.4.0, want 0.5.0 (Cargo.toml)"]
    );
    // A member that takes a version no one gives says none.
    let app = "[package]\nname = \"app\"\nversion = { workspace = true }\n";
    let (found, unread) = read(&[("src-tauri/Cargo.toml", Some(app))]);
    assert!(found.is_empty() && unread.is_empty());
}

#[test]
fn a_workspace_member_uses_the_top_lockfile() {
    // Cargo uses the top lockfile for a member, so one left over in the
    // member's folder isn't the one to compare, whatever it says.
    let top =
        "[workspace]\nmembers = [\"src-tauri\"]\n\n[workspace.package]\nversion = \"1.2.0\"\n";
    let app = "[package]\nname = \"app\"\nversion.workspace = true\n";
    let (fresh, stale) = (
        cargo_lock(&[("app", "1.2.0")]),
        cargo_lock(&[("app", "1.1.0")]),
    );
    let files = [
        ("Cargo.toml", Some(top)),
        ("Cargo.lock", Some(fresh.as_str())),
        ("src-tauri/Cargo.toml", Some(app)),
        ("src-tauri/Cargo.lock", Some(stale.as_str())),
    ];
    let (found, unread) = read(&files);
    assert_eq!(
        found,
        [
            says("src-tauri/Cargo.toml", "", "1.2.0"),
            says("Cargo.lock", "src-tauri/Cargo.toml", "1.2.0"),
        ]
    );
    assert!(unread.is_empty());
    assert!(differ(&files).is_empty());
    // And when the top lockfile is the stale one, it is the one flagged.
    let files = [
        ("Cargo.toml", Some(top)),
        ("Cargo.lock", Some(stale.as_str())),
        ("src-tauri/Cargo.toml", Some(app)),
        ("src-tauri/Cargo.lock", Some(fresh.as_str())),
    ];
    assert_eq!(
        differ(&files),
        ["Cargo.lock says 1.1.0, want 1.2.0 (src-tauri/Cargo.toml)"]
    );

    // Whether the app is a member, as Cargo decides it, so which lockfile
    // is compared: the top's, or its own in its folder.
    let app = cargo_toml("app", "1.2.0");
    let lock = |top: &str, app: &str| {
        let files = [
            ("Cargo.toml", Some(top)),
            ("Cargo.lock", Some(fresh.as_str())),
            ("src-tauri/Cargo.toml", Some(app)),
            ("src-tauri/Cargo.lock", Some(stale.as_str())),
        ];
        let (found, _) = read(&files);
        found
            .into_iter()
            .find(|(_, of, _)| of == "src-tauri/Cargo.toml")
            .map(|(path, _, _)| path)
            .unwrap_or_default()
    };
    let workspace = |rest: &str| format!("[workspace]\n{rest}");
    for (top, own, member) in [
        // Listed, as written in any of the ways Cargo takes.
        (workspace("members = [\"src-tauri\"]"), app.clone(), true),
        (workspace("members = [\"./src-tauri/\"]"), app.clone(), true),
        (workspace("members = [\"src-*\"]"), app.clone(), true),
        (workspace("members = [\"src-?auri\"]"), app.clone(), true),
        // Not listed, or listed and then left out.
        (workspace("members = [\"crates/*\"]"), app.clone(), false),
        (
            workspace("members = [\"*\"]\nexclude = [\"src-tauri\"]"),
            app.clone(),
            false,
        ),
        // A path dependency of the top's package is a member.
        (
            format!(
                "{}\n[workspace]\n\n[dependencies]\napp = {{ path = \"src-tauri\" }}\n",
                cargo_toml("core", "0.5.0").replace("[dependencies]\nserde = \"1\"\n", "")
            ),
            app.clone(),
            true,
        ),
        // The member's own word decides.
        (
            workspace(""),
            format!("{app}\n").replace("edition", "workspace = \"..\"\nedition"),
            true,
        ),
        (
            workspace("members = [\"src-tauri\"]"),
            format!("{app}\n").replace("edition", "workspace = \"../other\"\nedition"),
            false,
        ),
        // A top with no workspace has no members.
        (cargo_toml("core", "0.5.0"), app.clone(), false),
    ] {
        let want = if member {
            "Cargo.lock"
        } else {
            "src-tauri/Cargo.lock"
        };
        assert_eq!(lock(&top, &own), want, "{top}\n---\n{own}");
    }
    // Cargo's globs: `*` and `?` in a name, character sets and `**`.
    for (pattern, takes) in [
        ("*", true),
        ("src", false),
        ("src-[st]auri", true),
        ("src-[!t]auri", false),
        ("src-[a-z]auri", true),
        ("src-[0-9]auri", false),
        ("**", true),
        ("**/src-tauri", true),
        ("apps/**", false),
        // Not patterns Cargo takes.
        ("src-**", false),
        ("src-[t", false),
    ] {
        assert_eq!(glob(pattern, "src-tauri"), takes, "{pattern}");
    }
    assert!(glob("crates/*", "crates/a") && !glob("crates/*", "crates/a/b"));
    assert!(glob("crates/**", "crates/a/b") && glob("**/b", "crates/a/b"));
    assert!(glob("[]]", "]") && glob("[!]]", "a") && !glob("[!]]", "]"));
    assert!(!glob("src-?auri", "src-/auri") && glob("", ""));
}

#[test]
fn python_versions_are_compared_as_pep_440_does() {
    // uv writes the version its own way, which is the same version.
    let python = |v: &str| format!("[project]\nname = \"tool\"\nversion = \"{v}\"\n");
    let uv = |v: &str| {
        format!("version = 1\n\n[[package]]\nname = \"tool\"\nversion = \"{v}\"\nsource = {{ editable = \".\" }}\n")
    };
    for (written, locked) in [
        ("1.0-alpha1", "1.0a1"),
        ("1.0.0", "1.0"),
        ("2.0.0-beta.2", "2.0.0b2"),
        ("1.0-1", "1.0.post1"),
        ("1.0.dev", "1.0.dev0"),
    ] {
        let (p, u) = (python(written), uv(locked));
        let files = [
            ("pyproject.toml", Some(p.as_str())),
            ("uv.lock", Some(u.as_str())),
        ];
        assert!(differ(&files).is_empty(), "{written} and {locked}");
    }
    // A local part's numbers are numbers, as uv writes them.
    let (p, u) = (python("1.0+build.001"), uv("1.0+build.1"));
    let files = [
        ("pyproject.toml", Some(p.as_str())),
        ("uv.lock", Some(u.as_str())),
    ];
    assert!(differ(&files).is_empty());
    // Another version is still another, said as each file writes it.
    let (p, u) = (python("1.0-alpha1"), uv("1.0a2"));
    let files = [
        ("pyproject.toml", Some(p.as_str())),
        ("uv.lock", Some(u.as_str())),
    ];
    assert_eq!(
        differ(&files),
        ["uv.lock says 1.0a2, want 1.0-alpha1 (pyproject.toml)"]
    );
    // Only Python's files: npm and Cargo versions are compared as written.
    let (package, lock) = (package_json("1.0.0"), package_lock("1.0"));
    let files = [
        ("package.json", Some(package.as_str())),
        ("package-lock.json", Some(lock.as_str())),
    ];
    assert_eq!(differ(&files).len(), 1);

    for (v, canonical) in [
        ("1.0-alpha1", Some("1a1")),
        ("1.0.0a1", Some("1a1")),
        ("1.0.a1", Some("1a1")),
        ("V1.2", Some("1.2")),
        ("1.0b", Some("1b0")),
        ("1.0rc1", Some("1rc1")),
        ("1.0c1", Some("1rc1")),
        ("1.0.pre2", Some("1rc2")),
        ("1.0preview", Some("1rc0")),
        ("1.0-1", Some("1.post1")),
        ("1.0.post1", Some("1.post1")),
        ("1.0-r1", Some("1.post1")),
        ("1.0rev", Some("1.post0")),
        ("1.0-dev", Some("1.dev0")),
        ("1.0a1.post2.dev3", Some("1a1.post2.dev3")),
        ("1!2.0", Some("1!2")),
        ("0!2.0", Some("2")),
        ("1.0+Ubuntu-1", Some("1+ubuntu.1")),
        ("1.0+build.001", Some("1+build.1")),
        ("1.0+00.a010", Some("1+0.a010")),
        ("1.01.0a01", Some("1.1a1")),
        ("10.20.0", Some("10.20")),
        ("abc", None),
        ("1.0foo", None),
        ("1.0+", None),
        ("1.0+a..b", None),
    ] {
        assert_eq!(pep440(v).as_deref(), canonical, "{v}");
    }
}

#[test]
fn a_lockfile_entry_is_the_package_s_own() {
    // Two of the same name are the package and a dependency of that name,
    // so Orbit can't tell which is which; none says nothing.
    let cargo = cargo_toml("app", "1.0.0");
    for lock in [
        cargo_lock(&[("app", "1.0.0"), ("app", "0.1.0")]),
        cargo_lock(&[("other", "0.1.0")]),
        "version = 4\n".to_string(),
    ] {
        let (found, unread) = read(&[
            ("Cargo.toml", Some(cargo.as_str())),
            ("Cargo.lock", Some(lock.as_str())),
        ]);
        assert_eq!(found, [says("Cargo.toml", "", "1.0.0")], "{lock}");
        assert!(unread.is_empty());
    }
    // uv.lock compares names as Python's package index does.
    let python = "[project]\nname = \"My_Tool.Cli\"\nversion = \"2.1.0\"\n";
    let uv = "version = 1\n\n[[package]]\nname = \"my-tool-cli\"\nversion = \"2.0.0\"\nsource = { editable = \".\" }\n\n[[package]]\nname = \"requests\"\nversion = \"2.32.3\"\n";
    let files = [("pyproject.toml", Some(python)), ("uv.lock", Some(uv))];
    assert_eq!(
        differ(&files),
        ["uv.lock says 2.0.0, want 2.1.0 (pyproject.toml)"]
    );
    let (found, _) = found(&files);
    assert_eq!(mismatches(&found, SHA)[0].fix(), "uv lock");
    assert_eq!(python_name("My_Tool..Cli"), "my-tool-cli");
    assert_eq!(python_name("a-_.b"), "a-b");
}

#[test]
fn python_projects() {
    let poetry = "[tool.poetry]\nname = \"tool\"\nversion = \"0.4.0\"\n";
    // Worked out when building, from a tag or a file.
    let dynamic = "[project]\nname = \"tool\"\ndynamic = [\"version\"]\n";
    let (found, unread) = read(&[("pyproject.toml", Some(poetry))]);
    assert_eq!(found, [says("pyproject.toml", "", "0.4.0")]);
    assert!(unread.is_empty());
    let (found, unread) = read(&[
        ("pyproject.toml", Some(dynamic)),
        ("uv.lock", Some("version = 1\n")),
    ]);
    assert!(found.is_empty() && unread.is_empty());
    let (found, unread) = read(&[("pyproject.toml", Some("[build-system]\nrequires = []\n"))]);
    assert!(found.is_empty() && unread.is_empty());
}

#[test]
fn files_orbit_can_t_make_out() {
    let lock = package_lock("1.0.0");
    let long = "1".repeat(65);
    let odd_versions = [
        r#"{ "name": "app", "version": 3 }"#.to_string(),
        r#"{ "name": "app", "version": "" }"#.to_string(),
        r#"{ "name": "app", "version": "1.0 beta" }"#.to_string(),
        format!(r#"{{ "name": "app", "version": "{long}" }}"#),
        "{ not json".to_string(),
    ];
    for package in odd_versions {
        let (found, unread) = read(&[
            ("package.json", Some(package.as_str())),
            ("package-lock.json", Some(lock.as_str())),
        ]);
        assert!(found.is_empty(), "{package}");
        assert_eq!(unread, ["package.json"], "{package}");
    }
    // GitHub didn't send all of it, or it isn't TOML: Orbit says so, and
    // reads the rest.
    let package = package_json("1.0.0");
    let (found, unread) = read(&[
        ("package.json", Some(package.as_str())),
        ("package-lock.json", None),
        ("Cargo.toml", Some("[package\nname = ")),
        (
            "pyproject.toml",
            Some("[project]\nname = \"tool\"\nversion = 2\n"),
        ),
        (
            "src-tauri/Cargo.toml",
            Some("[package]\nversion = \"1.0.0\"\n"),
        ),
        ("src-tauri/tauri.conf.json", Some(r#"{ "version": "  " }"#)),
    ]);
    assert_eq!(found, [says("package.json", "", "1.0.0")]);
    assert_eq!(
        unread,
        [
            "Cargo.toml",
            "package-lock.json",
            "pyproject.toml",
            "src-tauri/Cargo.toml",
            "src-tauri/tauri.conf.json",
        ]
    );
    // A lockfile Orbit can't make out, for a manifest it can.
    let cargo = cargo_toml("app", "1.0.0");
    let (found, unread) = read(&[
        ("Cargo.toml", Some(cargo.as_str())),
        ("Cargo.lock", Some("[[package]\n")),
    ]);
    assert_eq!(found, [says("Cargo.toml", "", "1.0.0")]);
    assert_eq!(unread, ["Cargo.lock"]);
    // A version with a pre-release part, or a space around it, is fine.
    let (found, _) = read(&[(
        "package.json",
        Some(r#"{ "version": " 2.0.0-beta.1+build.5 " }"#),
    )]);
    assert_eq!(found, [says("package.json", "", "2.0.0-beta.1+build.5")]);
}

#[test]
fn what_is_kept() {
    // The fields the snapshot and the problem carry, in their saved form.
    let f = VersionFile {
        path: "src-tauri/Cargo.lock".into(),
        kind: Kind::CargoLock,
        version: "0.2.0".into(),
        of: "src-tauri/Cargo.toml".into(),
    };
    let v = serde_json::to_value(&f).unwrap();
    assert_eq!(v["kind"], "cargo_lock");
    assert_eq!(v["of"], "src-tauri/Cargo.toml");
    let back: VersionFile = serde_json::from_value(v).unwrap();
    assert_eq!(back, f);
    let manifest = VersionFile {
        path: "package.json".into(),
        version: "1.0.0".into(),
        ..Default::default()
    };
    let v = serde_json::to_value(&manifest).unwrap();
    assert!(v.get("of").is_none());
    let m: Mismatch = serde_json::from_str(r#"{ "path": "package.json" }"#).unwrap();
    assert_eq!((m.kind, m.number, m.says.as_str()), (Kind::Package, 0, ""));
    assert_eq!(FILES.len(), 9);
}

#[test]
fn files_in_github_s_answer() {
    // A file that isn't there is null, and a folder has no text; one too
    // big is cut short.
    let package = package_json("1.0.0");
    let cargo = cargo_toml("app", "0.9.0");
    let r = serde_json::json!({
        "package": { "text": package, "isTruncated": false },
        "packageLock": { "text": "{", "isTruncated": true },
        "cargo": { "text": cargo, "isTruncated": false, "isBinary": false },
        "cargoLock": { "text": null, "isTruncated": false, "isBinary": true },
        "pyproject": null,
        "tauri": {},
    });
    let (found, unread) = crate::versions::read(&r);
    assert_eq!(
        found
            .iter()
            .map(|f| (f.path.as_str(), f.version.as_str()))
            .collect::<Vec<_>>(),
        [("package.json", "1.0.0"), ("Cargo.toml", "0.9.0")]
    );
    assert_eq!(unread, ["Cargo.lock", "package-lock.json"]);
    assert_eq!(
        crate::versions::read(&serde_json::json!({})),
        (vec![], vec![])
    );
}

#[test]
fn files_that_disagree_in_a_sentence() {
    let m = |path: &str, says: &str| Mismatch {
        path: path.into(),
        says: says.into(),
        ..Default::default()
    };
    assert_eq!(names(&[]), "");
    assert_eq!(names(&[m("a", "1.0.0")]), "a says 1.0.0");
    assert_eq!(
        names(&[m("a", "1.0.0"), m("b", "2.0.0")]),
        "a says 1.0.0 and b says 2.0.0"
    );
    assert_eq!(
        names(&[m("a", "1"), m("b", "2"), m("c", "3"), m("d", "4")]),
        "a says 1, b says 2 and 2 more"
    );
    assert_eq!(count(1), "1 version file");
    assert_eq!(count(3), "3 version files");
}

/// Python versions come in PEP 440's order, so a `pyproject.toml` keeps a
/// tag when it says its version or a newer one, as Python writes it.
#[test]
fn python_versions_are_ordered_as_pep_440_orders_them() {
    let order = |v: &str| Pep440Order::of(v).unwrap();
    let list = [
        "1.0.dev1",
        "1.0a1.dev1",
        "1.0a1",
        "1.0a1.post1",
        "1.0b2",
        "1.0rc1",
        "1.0",
        "1.0.post1.dev1",
        "1.0.post1",
        "1.0.1",
        "1.1",
        "1!0.1",
    ];
    for w in list.windows(2) {
        assert!(order(w[0]) < order(w[1]), "{} < {}", w[0], w[1]);
    }
    assert_eq!(order("1.0"), order("v1.0.0"));
    assert_eq!(order("1.0-alpha.1"), order("v1.0.0-a1"));
    assert_eq!(order("1.0+build.7"), order("1.0"));
    assert!(Pep440Order::of("banana").is_none());

    let file = |kind, version: &str| VersionFile {
        path: "x".into(),
        kind,
        version: version.into(),
        of: String::new(),
    };
    assert!(says_at_least(&file(Kind::Pyproject, "1.1"), "v1.1.0"));
    assert!(says_at_least(&file(Kind::Pyproject, "1.2"), "v1.1.0"));
    assert!(says_at_least(
        &file(Kind::Pyproject, "1.2rc2"),
        "v1.2.0-rc.2"
    ));
    assert!(!says_at_least(
        &file(Kind::Pyproject, "1.2rc1"),
        "v1.2.0-rc.2"
    ));
    // Elsewhere, only a whole version counts.
    assert!(!says_at_least(&file(Kind::Package, "1.1"), "v1.1.0"));
    assert!(says_at_least(&file(Kind::Cargo, "1.2.0"), "v1.1.0"));
    assert!(!says_at_least(&file(Kind::Cargo, "1.1.0-rc.1"), "v1.1.0"));
}
