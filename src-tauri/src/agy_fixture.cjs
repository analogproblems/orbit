// A fake `agy` for tests, following Antigravity's documented headless
// stream. Usage: node agy_fixture.cjs <mode>[@log] <agy args...>
// Modes: ok, signed-out, wrong-model, no-model, always-proceed, wrong-conversation,
// crash, slow, linger, denied, incomplete, no-sandbox.
// With a log, each call's arguments and prompt are appended to it as JSON,
// and a structured answer is <log>.answer when the test left one.
const fs = require("node:fs");
const [modeArg, ...args] = process.argv.slice(2);
const [mode, log] = modeArg.split("@");
const send = (value) => process.stdout.write(JSON.stringify(value) + "\n");
const after = (flag) =>
  args.includes(flag) ? args[args.indexOf(flag) + 1] : undefined;
if (args[0] === "models") {
  if (log)
    fs.appendFileSync(log, JSON.stringify({ args, prompt: null }) + "\n");
  if (mode === "signed-out") {
    process.stderr.write(
      "Error: authentication required. Run agy to sign in.\n",
    );
    process.exit(1);
  }
  console.log(
    "gemini-3.1-pro-high (Gemini 3.1 Pro (High))\ngemini-3.8-flash-medium (Gemini 3.8 Flash (Medium))",
  );
  process.exit(0);
}
let input = "";
process.stdin.on("data", (chunk) => (input += chunk));
process.stdin.on("end", () => {
  const prompt = input
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l))
    .filter((e) => e.event === "user")
    .map((e) => e.message.content)
    .join("\n");
  if (log) fs.appendFileSync(log, JSON.stringify({ args, prompt }) + "\n");
  run(prompt);
});

function run(prompt) {
  if (mode === "signed-out") {
    process.stderr.write("Error: authentication required\n");
    process.exit(1);
  }
  const resumed = after("--conversation");
  const conversation =
    mode === "wrong-conversation" && resumed
      ? "conv-other"
      : resumed || "conv-1";
  const model =
    mode === "wrong-model"
      ? "gemini-other"
      : mode === "no-model"
        ? undefined
        : after("--model");
  if (mode === "no-sandbox") {
    if (args.includes("--sandbox")) {
      // What a sandbox that cannot start might look like; Orbit must stop
      // the run rather than let it go on unsandboxed.
      process.stderr.write(
        "Error: terminal sandbox unavailable: user namespaces are disabled\n",
      );
      if (log) {
        setTimeout(() => fs.writeFileSync(`${log}.unsandboxed`, "ran"), 500);
      }
      setTimeout(() => {}, 30000);
      return;
    }
    if (log) fs.writeFileSync(`${log}.unsandboxed`, "ran");
  }
  send({
    event: "init",
    conversation_id: conversation,
    init: {
      cwd: process.cwd(),
      tools: ["view_file", "run_command"],
      permission_mode:
        mode === "always-proceed" ? "always-proceed" : "request-review",
      model,
      ...(args.includes("--json-schema")
        ? { json_schema: JSON.parse(after("--json-schema")) }
        : {}),
    },
  });
  if (mode === "crash") {
    process.stderr.write("fixture crashed\n");
    process.exit(3);
  }
  if (mode === "slow") {
    const child = require("node:child_process").spawn("sleep", ["30"], {
      stdio: "ignore",
    });
    if (log) fs.writeFileSync(`${log}.child`, String(child.pid));
    setTimeout(() => {}, 30000);
    return;
  }
  let step = 0;
  const update = (fields) =>
    send({
      event: "step_update",
      step_update: {
        conversation_id: conversation,
        step_index: step,
        ...fields,
      },
    });
  const say = (text) => {
    step += 1;
    const half = Math.ceil(text.length / 2);
    update({
      state: "ACTIVE",
      step_type: "agent_response",
      text_delta: text.slice(0, half),
    });
    update({
      state: "DONE",
      step_type: "agent_response",
      text_delta: text.slice(half),
    });
  };
  const result = (fields) =>
    send({
      event: "result",
      result: {
        conversation_id: conversation,
        status: "SUCCESS",
        duration_seconds: 1,
        num_turns: 1,
        ...fields,
      },
    });
  if (prompt.startsWith("You are reviewing a pull request.")) {
    step += 1;
    const tool = {
      name: "view_file",
      parameters: { AbsolutePath: `${process.cwd()}/src/lib.rs` },
    };
    update({
      state: "ACTIVE",
      step_type: "tool",
      tool_name: "view_file",
      tool_info: tool,
    });
    update({
      state: "DONE",
      step_type: "tool",
      tool_name: "view_file",
      tool_info: { ...tool, output: "fn first() {}" },
    });
    if (mode === "denied") {
      step += 1;
      process.stderr.write(
        "Permission denied (soft): run_command `cargo test` needs approval\n",
      );
    }
    const report =
      "Not ready to merge.\n\n1. **Off by one** (`src/lib.rs:3`): first() returns the second item.";
    say(report);
    result({ response: report });
    if (mode === "linger") {
      // Answers, then never exits, with a command still running.
      const child = require("node:child_process").spawn("sleep", ["30"], {
        stdio: "ignore",
      });
      if (log) fs.writeFileSync(`${log}.child`, String(child.pid));
      setTimeout(() => {}, 30000);
    }
  } else if (log && fs.existsSync(`${log}.answer`)) {
    // An answer the test left beside the log.
    const structured = JSON.parse(fs.readFileSync(`${log}.answer`, "utf8"));
    result({
      response: JSON.stringify(structured),
      structured_output: structured,
    });
  } else if (args.includes("--json-schema")) {
    const structured = {
      verdict: "changes_requested",
      summary: mode === "incomplete" ? "The diff was missing." : "One bug.",
      completed: mode !== "incomplete",
      notChecked: [],
      priorFindings: [],
      findings: [
        {
          title: "Off by one",
          priority: 1,
          category: "bug",
          locations: [
            { path: "src/lib.rs", startLine: 3, endLine: 3 },
            { path: "src/main.rs", startLine: 40, endLine: 40 },
          ],
          problem: "first() returns the second item.",
          failureScenario: "A list with one item returns nothing.",
          evidence: "Read src/lib.rs:3 and its caller in src/main.rs:40.",
          confirmed: true,
          suggestedFix: "Index 0.",
          doneWhen: "A test covers a one-item list.",
          reviewerNotes: null,
          confidence: 0.9,
        },
      ],
    };
    result({
      response: JSON.stringify(structured),
      structured_output: structured,
    });
  } else {
    say(`Answer: ${prompt}`);
    result({ response: `Answer: ${prompt}` });
  }
}
