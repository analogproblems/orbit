// Disposable JSON-RPC peer. It never invokes a model, Git, or GitHub.
const readline = require("node:readline");
const fs = require("node:fs");
let options = {};
let requests = [];
let subscribed = false;
let held = [];
const send = (id, result) =>
  process.stdout.write(JSON.stringify({ id, result }) + "\n");
readline.createInterface({ input: process.stdin }).on("line", (line) => {
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  if (m.method === "fixture/configure") {
    options = m.params;
    requests = [];
    subscribed = !!options.warm;
    return send(m.id, {});
  }
  if (m.method === "fixture/requests") return send(m.id, requests);
  if (m.method === "fixture/release") {
    const replies = held;
    held = [];
    replies.forEach((reply) => reply());
    return send(m.id, {});
  }
  requests.push(m);
  if (m.method === options.failMethod) {
    return process.stdout.write(
      JSON.stringify({ id: m.id, error: { message: "fixture failure" } }) +
        "\n",
    );
  }
  const handle = () => {
    switch (m.method) {
      case "account/read":
        return send(m.id, { account: { type: "chatgpt" } });
      case "account/rateLimits/read":
        return send(m.id, options.rateLimits ?? {});
      case "model/list":
        return send(m.id, { data: options.catalog ?? [], nextCursor: null });
      case "config/read":
        return send(m.id, {
          config: {
            model: options.effectiveModel ?? null,
            model_reasoning_effort: options.effectiveEffort ?? null,
          },
        });
      case "thread/unsubscribe":
        subscribed = !!options.otherSubscriber;
        return send(m.id, { status: "unsubscribed" });
      case "thread/start":
      case "thread/resume": {
        let response =
          m.method === "thread/start"
            ? options.startResponse
            : options.resumeResponse;
        if (response === undefined)
          response = {
            model: subscribed ? "previous-model" : m.params.model,
            reasoningEffort: subscribed
              ? "low"
              : m.params.config.model_reasoning_effort,
          };
        subscribed = true;
        return send(m.id, { ...response, thread: { id: "fixture-thread" } });
      }
      case "review/start":
      case "turn/start": {
        if (options.statePath) {
          const state = JSON.parse(fs.readFileSync(options.statePath, "utf8"));
          const r = state.reviews[0];
          const confirmed =
            m.method === "review/start"
              ? r.confirmedReviewSelection
              : m.params.outputSchema
                ? r.extractionSelection
                : r.messages.at(-1)?.confirmedSelection;
          if (!confirmed?.model || !confirmed?.effort) {
            return process.stdout.write(
              JSON.stringify({
                id: m.id,
                error: {
                  message: "confirmation not persisted before inference",
                },
              }) + "\n",
            );
          }
        }
        return send(m.id, { turn: { id: "fixture-turn" } });
      }
      default:
        return send(m.id, {});
    }
  };
  if (m.method === options.holdMethod) held.push(handle);
  else if (m.method === options.delayMethod) setTimeout(handle, 50);
  else handle();
});
