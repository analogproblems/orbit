// Disposable stand-in for `srt`, Anthropic's sandbox runtime. It never
// sandboxes anything: it logs each call and runs the command with the
// host's shell in the folder it was started in. Usage: node
// sandbox_fixture.cjs <state folder> <arguments...>. Each call is logged as
// a JSON line in <state>/log, with the settings file's contents, the
// folder it ran in, TMPDIR and the whole environment. Files in the state folder change behaviour:
// `missing` makes it fail as if it weren't installed, and `no-sandbox`
// makes every sandboxed command fail as srt does when it can't start one.
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const [state, ...args] = process.argv.slice(2);
fs.mkdirSync(state, { recursive: true });
const has = (name) => fs.existsSync(path.join(state, name));
const after = (flag) => args[args.indexOf(flag) + 1];
const settings = args.includes("--settings")
  ? JSON.parse(fs.readFileSync(after("--settings"), "utf8"))
  : null;
fs.appendFileSync(
  path.join(state, "log"),
  JSON.stringify({
    args,
    settings,
    cwd: process.cwd(),
    tmpdir: process.env.TMPDIR ?? null,
    env: process.env,
  }) + "\n",
);
if (has("missing")) {
  process.stderr.write("srt: command not found\n");
  process.exit(127);
}
if (args[0] === "--version") {
  process.stdout.write("0.0.81\n");
  process.exit(0);
}
if (has("no-sandbox")) {
  process.stderr.write(
    "Error: Sandbox dependencies are not available: bubblewrap (bwrap) not installed\n",
  );
  process.exit(1);
}
const command = args.slice(args.indexOf("--") + 1);
const ran = spawnSync(command[0], command.slice(1), { stdio: "inherit" });
process.exit(ran.status ?? 1);
