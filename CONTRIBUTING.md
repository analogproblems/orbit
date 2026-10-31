# Contributing to Orbit

Thanks for taking an interest in Orbit. It is a small project with one maintainer, so the most helpful thing you can do is open an issue.

## Reporting a problem or suggesting an idea

Open an [issue](https://github.com/analogproblems/orbit/issues/new/choose) and pick the template that fits:

- **Bug report** for something that doesn't work as described in the [user guide](docs/user-guide.md).
- **Idea** for something Orbit could do better.

A good bug report says what you did, what you expected, what happened instead, and which system and CLI versions you use (`gh --version`, `codex --version`, and `claude --version` or `agy --version` if you review with them). Remove tokens, private repository names and anything else you don't want public before you post logs or screenshots.

Please report security problems privately, as [SECURITY.md](SECURITY.md) describes, not in a public issue.

## Pull requests

Changes are made by the maintainer from issue reports. Please open an issue before writing code, so we can agree on the change first. A pull request without an issue may be closed.

## Working on your own copy

You're welcome to fork Orbit and change it for yourself. The [README](README.md#try-it) covers what you need installed, and [docs/architecture.md](docs/architecture.md) explains how it fits together. Before you share a change, run the same checks the project uses:

```sh
npm ci
npm run check
```

`npm run check` runs the linters, the frontend build, the frontend and Rust tests, and a build check of the desktop app. The [CI workflow](.github/workflows/ci.yml) runs the same parts on every pull request and on each push to `master`, one step for each, so a failed run names the part that failed. The pieces on their own:

```sh
npm run dev        # Browser-only demo with sample data at http://127.0.0.1:1420
npm run desktop    # The desktop app with hot reload (stop npm run dev first: both use port 1420)
npm run lint       # ESLint, Prettier, rustfmt, and Clippy with warnings as errors
npm run format     # Prettier and rustfmt
npm test           # Frontend tests (Vitest)
npm run test:rust  # Rust tests
npm run diagnose   # Read-only GitHub/Codex connection check; no inference or comments
npm run server     # Orbit without a window, serving the browser page (see the user guide)
npm run tauri -- build --no-bundle  # Native executable (src-tauri/target/release/orbit on Linux)
```

### Dev container

[`.devcontainer/`](.devcontainer/devcontainer.json) describes a box with everything `npm run check` needs: Node 22, stable Rust with rustfmt and Clippy, the Linux libraries Tauri needs, Git and `gh`. Open the folder in VS Code and choose **Reopen in Container**, start a GitHub Codespace, or run `devcontainer up --workspace-folder .` with the [Dev Container CLI](https://github.com/devcontainers/cli). `npm ci` runs when the container is created. The box has no display, so use `npm run check` and the browser demo (port 1420 is forwarded) there, and `npm run desktop` on your own machine.

UI code uses the [Astryx](https://www.npmjs.com/package/@astryxdesign/core) design system; [AGENTS.md](AGENTS.md) lists its rules.

## Making a release

Pushing a version tag builds every installer. Bump the version in `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (and the lockfiles, `package-lock.json` and `src-tauri/Cargo.lock`), merge that, then tag the latest `master` and push the tag. `git tag` marks the commit you have checked out, so pull first:

```sh
git checkout master
git pull
git tag v0.2.0
git push origin v0.2.0
```

The [Release workflow](.github/workflows/release.yml) checks the tag matches all three versions, builds on Linux, macOS and Windows, and attaches the files to a draft release. Check the draft on the Releases page, then publish it. If nothing runs, the tag is probably on an older commit that predates the workflow: delete it (`git tag -d v0.2.0` and `git push origin :refs/tags/v0.2.0`), pull, and tag again.

## License

By contributing, you agree that your contributions are licensed under the project's [Apache License 2.0](LICENSE).
