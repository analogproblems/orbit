// A fake `claude` for tests. Usage: node claude_fixture.cjs <mode>[@log] <claude args...>
// Modes: ok, signed-out, wrong-model, no-model, crash, slow, denied, schema-fail, incomplete,
// no-sandbox. A write-up (--no-session-persistence) answers with <log>.answer.
const fs = require("node:fs");
const [modeArg, ...args] = process.argv.slice(2);
const [mode, log] = modeArg.split("@");
const send = (value) => process.stdout.write(JSON.stringify(value) + "\n");
const after = (flag) => args[args.indexOf(flag) + 1];
// A prompt after -p is an argument; otherwise it comes on standard input,
// which the log records after a made-up --stdin flag.
const piped = args.includes("-p") && after("-p").startsWith("--");
const stdin = piped ? fs.readFileSync(0, "utf8") : undefined;
if (log)
  fs.appendFileSync(
    log,
    JSON.stringify(piped ? [...args, "--stdin", stdin] : args) + "\n",
  );
if (args[0] === "--version") {
  console.log("2.1.295 (Claude Code)");
  process.exit(0);
}
if (args[0] === "auth") {
  console.log(
    JSON.stringify({
      loggedIn: mode !== "signed-out",
      authMethod: "oauth_token",
    }),
  );
  process.exit(0);
}
const prompt = piped ? stdin : after("-p");
const session = args.includes("--resume")
  ? after("--resume")
  : after("--session-id");
const model =
  mode === "wrong-model"
    ? "claude-other"
    : mode === "no-model"
      ? undefined
      : after("--model");
if (mode === "no-sandbox") {
  // Like the real CLI on a computer without bubblewrap: it refuses to start
  // when the sandbox is required, and otherwise runs commands unsandboxed,
  // which here stands for a diff driver from the pull request running.
  const settings = JSON.parse(after("--settings") || "{}");
  if (settings.sandbox?.failIfUnavailable) {
    process.stderr.write(
      "\nError: sandbox required but unavailable: sandbox is enabled but dependencies are missing: bubblewrap (bwrap) not installed\n  sandbox.failIfUnavailable is set — refusing to start without a working sandbox.\n",
    );
    send({
      type: "result",
      subtype: "error_during_execution",
      is_error: true,
      errors: ["Sandbox required but unavailable"],
    });
    process.exit(1);
  }
  process.stderr.write("Sandbox disabled\n");
  if (log) fs.writeFileSync(`${log}.unsandboxed`, "ran");
}
send({ type: "system", subtype: "task_started", session_id: session });
send({
  type: "system",
  subtype: "init",
  model,
  session_id: session,
  permissionMode: "dontAsk",
});
if (mode === "crash") {
  process.stderr.write("fixture crashed\n");
  process.exit(3);
}
if (mode === "slow") {
  // A command still running, as a review's git command would be.
  const child = require("node:child_process").spawn("sleep", ["30"], {
    stdio: "ignore",
  });
  if (log) fs.writeFileSync(`${log}.child`, String(child.pid));
  setTimeout(() => {}, 30000);
  return;
}
const result = (extra) =>
  send({
    type: "result",
    subtype: "success",
    is_error: false,
    session_id: session,
    permission_denials:
      mode === "denied"
        ? [
            {
              tool_name: "Bash",
              tool_use_id: "d1",
              tool_input: { command: "cargo test" },
            },
          ]
        : [],
    ...extra,
  });
if (args.includes("--no-session-persistence")) {
  // A write-up: the pull request arrives on standard input, and the answer
  // is the one the test left beside the log, if any.
  if (log) fs.writeFileSync(`${log}.stdin`, stdin);
  const answer =
    log && fs.existsSync(`${log}.answer`)
      ? JSON.parse(fs.readFileSync(`${log}.answer`, "utf8"))
      : { summary: "A change.", decisions: [], findingNotes: [] };
  result({ result: JSON.stringify(answer), structured_output: answer });
  return;
}
if (prompt.startsWith("/code-review")) {
  const parent = "forked-command-code-review";
  send({
    type: "assistant",
    parent_tool_use_id: parent,
    message: {
      id: "m1",
      content: [
        {
          type: "tool_use",
          id: "t1",
          name: "Bash",
          input: { command: "git diff base...HEAD" },
        },
      ],
    },
  });
  send({
    type: "user",
    parent_tool_use_id: parent,
    message: {
      content: [
        {
          type: "tool_result",
          tool_use_id: "t1",
          content: "diff --git a/src/lib.rs b/src/lib.rs",
        },
      ],
    },
  });
  const findings =
    '```json\n[{"file":"src/lib.rs","line":3,"summary":"Off by one.","failure_scenario":"first() returns the second item."}]\n```';
  send({
    type: "assistant",
    parent_tool_use_id: null,
    message: { id: "m2", content: [{ type: "text", text: findings }] },
  });
  result({ result: findings });
} else if (args.includes("--json-schema")) {
  if (mode === "schema-fail") {
    send({
      type: "result",
      subtype: "error_max_structured_output_retries",
      is_error: true,
      session_id: session,
      errors: ["Could not produce output matching the schema."],
    });
  } else {
    const structured = {
      verdict: "changes_requested",
      summary: mode === "incomplete" ? "git diff was refused." : "One bug.",
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
    send({
      type: "assistant",
      parent_tool_use_id: null,
      message: {
        id: "m3",
        content: [
          {
            type: "tool_use",
            id: "s1",
            name: "StructuredOutput",
            input: structured,
          },
        ],
      },
    });
    result({
      result: JSON.stringify(structured),
      structured_output: structured,
    });
  }
} else {
  send({
    type: "assistant",
    parent_tool_use_id: null,
    message: {
      id: "m4",
      content: [{ type: "text", text: `Answer: ${prompt}` }],
    },
  });
  result({ result: `Answer: ${prompt}` });
}
