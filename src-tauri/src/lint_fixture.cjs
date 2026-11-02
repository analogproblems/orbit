// A fake `actionlint` or `zizmor` for tests. Usage:
// node lint_fixture.cjs <tool>[@mode][@log] <tool args...>
// Modes: ok (the default), fail, slow, garbled. With a log, each run appends
// a line with its arguments, its folder's files and whether it saw a token.
// It reads each workflow file it is given, like the real tool:
// - actionlint reports a line with `# actionlint: <message>`, and one with
//   `github.event.issue.title` in it as potentially untrusted.
// - zizmor reports `uses:` without a full commit (unpinned-uses), a
//   checkout (artipacked), `github.event.issue.title` and `${{ inputs.x }}`
//   (template-injection),
//   and `# zizmor: <audit>`; one with `# zizmor: ignore` is ignored, and a
//   file with `# zizmor: unreadable` can't be read.
const fs = require("node:fs");
const path = require("node:path");
const [toolArg, ...args] = process.argv.slice(2);
const [tool, mode = "ok", log] = toolArg.split("@");

const walk = (dir) =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    return e.isDirectory()
      ? walk(p)
      : [path.relative(".", p).split(path.sep).join("/")];
  });
if (log)
  fs.appendFileSync(
    log,
    JSON.stringify({
      tool,
      args,
      files: walk(".").sort(),
      token: Boolean(process.env.GH_TOKEN || process.env.GITHUB_TOKEN),
    }) + "\n",
  );
if (mode === "fail") {
  process.stderr.write("error: something went wrong\n");
  process.exit(3);
}
if (mode === "slow") setTimeout(() => {}, 60_000);
else if (mode === "garbled") process.stdout.write("this isn't JSON\n");
else {
  let files = args.filter((a) => a.startsWith(".github/"));
  // zizmor can't read a file marked so: it says why with
  // --strict-collection, and otherwise skips it with a warning.
  const unreadable = (f) =>
    tool === "zizmor" &&
    fs.readFileSync(f, "utf8").includes("# zizmor: unreadable");
  const bad = files.find(unreadable);
  if (bad && args.includes("--strict-collection")) {
    process.stderr.write(
      ` INFO zizmor: v1\nfatal: no audit was performed\nfailed to load file://${bad} as workflow\n\nCaused by:\n    0: input does not match expected validation schema\n\nStack backtrace:\n   0: zizmor::main\n`,
    );
    process.exit(1);
  }
  files = files.filter((f) => !unreadable(f));
  const out = tool === "actionlint" ? actionlint(files) : zizmor(files);
  process.stdout.write(JSON.stringify(out) + "\n");
  process.exit(tool === "actionlint" && out.length ? 1 : 0);
}

function actionlint(files) {
  const out = [];
  for (const file of files)
    fs.readFileSync(file, "utf8")
      .split("\n")
      .forEach((line, i) => {
        const said = line.match(/# actionlint: (.*)$/);
        if (said)
          out.push({
            message: said[1],
            filepath: file,
            line: i + 1,
            column: 1,
            kind: "syntax-check",
            snippet: line,
          });
        if (line.includes("github.event.issue.title"))
          out.push({
            message:
              '"github.event.issue.title" is potentially untrusted. avoid using it directly in inline scripts. instead, pass it through an environment variable. see https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions for more details',
            filepath: file,
            line: i + 1,
            column: line.indexOf("github.event") + 1,
            kind: "expression",
            snippet: line,
          });
      });
  return out;
}

function zizmor(files) {
  const out = [];
  const finding = (file, row, job, ident, severity, desc, fragment, extra) => ({
    ident,
    desc,
    url: `https://docs.zizmor.sh/audits/#${ident}`,
    determinations: { confidence: "High", severity, persona: "Regular" },
    locations: [
      {
        symbolic: {
          key: { Local: { verbatim_path: file } },
          annotation: "this step",
          route: { route: [{ Key: "jobs" }, { Key: job }, { Key: "steps" }] },
          feature_kind: "Normal",
          kind: "Hidden",
        },
        concrete: {
          location: { start_point: { row: 0, column: 0 } },
          feature: "",
        },
      },
      {
        symbolic: {
          key: { Local: { verbatim_path: file } },
          annotation: "here",
          route: { route: [{ Key: "jobs" }, { Key: job }, { Key: "steps" }] },
          feature_kind: fragment
            ? { Subfeature: { after: 0, fragment: { Raw: fragment } } }
            : "Normal",
          kind: "Primary",
        },
        concrete: {
          location: { start_point: { row, column: 0 } },
          feature: "the line itself",
        },
      },
    ],
    ignored: false,
    ...extra,
  });
  for (const file of files) {
    let job = "";
    fs.readFileSync(file, "utf8")
      .split("\n")
      .forEach((line, row) => {
        const named = line.match(/^ {2}([A-Za-z0-9_-]+):\s*$/);
        if (named) job = named[1];
        const uses = line.match(/uses: (\S+)@(\S+)/);
        if (uses && !/^[0-9a-f]{40}$/.test(uses[2]))
          out.push(
            finding(
              file,
              row,
              job,
              "unpinned-uses",
              "High",
              "unpinned action reference",
            ),
          );
        if (line.includes("actions/checkout@"))
          out.push(
            finding(
              file,
              row,
              job,
              "artipacked",
              "Medium",
              "credential persistence through GitHub Actions artifacts",
            ),
          );
        if (line.includes("github.event.issue.title"))
          out.push(
            finding(
              file,
              row,
              job,
              "template-injection",
              "High",
              "code injection via template expansion",
              "github.event.issue.title",
            ),
          );
        const input = line.match(/\$\{\{ (inputs\.\w+) \}\}/);
        if (input)
          out.push(
            finding(
              file,
              row,
              job,
              "template-injection",
              "Low",
              "code injection via template expansion",
              input[1],
            ),
          );
        const said = line.match(/# zizmor: (\S+)/);
        if (said && said[1] === "unreadable");
        else if (said && said[1] === "ignore")
          out.push(
            finding(
              file,
              row,
              job,
              "obfuscation",
              "Low",
              "obfuscated usage of GitHub Actions features",
              "",
              { ignored: true },
            ),
          );
        else if (said)
          out.push(
            finding(
              file,
              row,
              job,
              said[1],
              "Low",
              `zizmor's ${said[1]} audit`,
            ),
          );
      });
  }
  return out;
}
