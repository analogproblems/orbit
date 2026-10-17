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

You're welcome to fork Orbit and change it for yourself. The [README](README.md) covers what you need installed and how to run it, and [docs/architecture.md](docs/architecture.md) explains how it fits together. Before you share a change, run the same checks the project uses:

```sh
npm ci
npm run check
```

`npm run check` runs the linters, the frontend build, the frontend and Rust tests, and a build check of the desktop app.

UI code uses the [Astryx](https://www.npmjs.com/package/@astryxdesign/core) design system; [AGENTS.md](AGENTS.md) lists its rules.

## License

By contributing, you agree that your contributions are licensed under the project's [Apache License 2.0](LICENSE).
