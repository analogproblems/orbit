// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import App from "./App";
import { freshDemo } from "./demo";
import { useOrbit } from "./store";
import type { Snapshot, SettingsPatch, ChangedFile } from "./types";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
let snapshot: Snapshot;
const user = () => userEvent.setup();
function emit() {
  useOrbit.setState({ snapshot: structuredClone(snapshot) });
}
async function mount() {
  render(<App />);
  await screen.findByPlaceholderText("Search pull requests…");
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
async function backend(
  command: string,
  args: Record<string, unknown> = {},
): Promise<unknown> {
  if (command === "get_snapshot") return snapshot;
  if (command === "update_settings") {
    Object.assign(snapshot.settings, args.patch as SettingsPatch);
    emit();
    return snapshot.settings;
  }
  if (command === "publish_review") {
    snapshot.reviews[0].status = "posted";
    snapshot.reviews[0].commentUrl =
      "https://github.com/acme/orbit/pull/148#issuecomment-7";
    emit();
    return;
  }
  if (command === "get_files" || command === "list_repositories") return [];
}
beforeEach(() => {
  snapshot = freshDemo();
  useOrbit.setState({
    snapshot,
    loaded: false,
    error: null,
    saves: {},
    cleanup: { running: false, results: [] },
    trustedInput: "",
    chatDrafts: {},
  });
  invoke.mockReset().mockImplementation(backend);
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(cleanup);

describe("publication", () => {
  it("keeps a failed preview open, retries, and shows the confirmed comment link", async () => {
    let attempts = 0;
    invoke.mockImplementation((command, args) => {
      if (command === "publish_review" && ++attempts === 1)
        return Promise.reject(Error("GitHub timed out"));
      return backend(command, args);
    });
    await mount();
    await user().click(
      screen.getByRole("button", { name: /^Publish review$/ }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Publish review to GitHub",
    });
    await user().click(
      within(dialog).getByRole("button", { name: "Publish comment" }),
    );
    expect(await within(dialog).findByRole("alert")).toHaveProperty(
      "textContent",
      "Error: GitHub timed out",
    );
    expect(dialog.textContent).toContain(
      snapshot.reviews[0].report.split("\n")[0].replace(/^#+ /, ""),
    );
    await user().click(
      within(dialog).getByRole("button", { name: "Retry publication" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await user().click(
      screen.getByRole("button", { name: /View comment on GitHub/ }),
    );
    expect(invoke).toHaveBeenCalledWith("open_github", {
      url: snapshot.reviews[0].commentUrl,
    });
    await user().click(screen.getByRole("button", { name: "Review activity" }));
    expect(
      screen.getByRole("button", { name: /View comment on GitHub/ }),
    ).toBeTruthy();
  });
});
describe("files and navigation", () => {
  it("retries failed files and discards responses from old head, base, and PR selections", async () => {
    const pending: ReturnType<typeof deferred<ChangedFile[]>>[] = [];
    invoke.mockImplementation((cmd, args) => {
      if (cmd !== "get_files") return backend(cmd, args);
      const request = deferred<ChangedFile[]>();
      pending.push(request);
      return request.promise;
    });
    await mount();
    await user().click(screen.getByRole("tab", { name: /Files changed/ }));
    await act(async () => pending[0].reject(Error("Diff unavailable")));
    expect(screen.getByText(/Diff unavailable/)).toBeTruthy();
    await user().click(screen.getByRole("button", { name: "Retry" }));
    expect(screen.getByText("Loading changes…")).toBeTruthy();
    await act(async () => {
      snapshot.prs[0].headSha = "new-head";
      emit();
    });
    await act(async () => {
      snapshot.prs[0].baseSha = "new-base";
      emit();
    });
    const file = (filename: string): ChangedFile => ({
      filename,
      status: "modified",
      additions: 1,
      deletions: 0,
      patch: "@@ -1 +1 @@\n+new",
    });
    await act(async () => {
      pending[1].resolve([file("old-head.txt")]);
      pending[2].resolve([file("old-base.txt")]);
      pending[3].resolve([file("current.txt")]);
    });
    expect(screen.queryByText("old-head.txt")).toBeNull();
    expect(screen.queryByText("old-base.txt")).toBeNull();
    expect(screen.getByText("current.txt")).toBeTruthy();
    await act(async () => {
      snapshot.prs[0].headSha = "next-head";
      emit();
    });
    const inbox = screen.getByRole("region", { name: "Pull request inbox" });
    await user().click(
      within(inbox).getByRole("button", {
        name: new RegExp(snapshot.prs[1].title),
      }),
    );
    await act(async () => pending[4].resolve([file("previous-pr.txt")]));
    expect(screen.queryByText("previous-pr.txt")).toBeNull();
  });
  it("opens Settings and focuses inbox search across pages, leaving modal shortcuts alone", async () => {
    await mount();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(screen.getByText("A review flow that fits.")).toBeTruthy();
    fireEvent.keyDown(document, { key: "k", metaKey: true });
    expect(document.activeElement).toBe(
      screen.getByPlaceholderText("Search pull requests…"),
    );
    await user().click(
      screen.getByRole("button", { name: /^Publish review$/ }),
    );
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    fireEvent.keyDown(document, { key: "k", metaKey: true });
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.queryByText("A review flow that fits.")).toBeNull();
  });
});
describe("autosave and follow-ups", () => {
  it("retains failed selections and typed accounts across pages and retries the specific patch", async () => {
    let fail = true;
    invoke.mockImplementation((cmd, args) =>
      cmd === "update_settings" && fail
        ? Promise.reject(Error("Disk is full"))
        : backend(cmd, args),
    );
    await mount();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    const select = screen.getByRole("combobox", { name: "Poll interval" });
    await user().selectOptions(select, "60");
    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      expect.stringContaining("Disk is full"),
    );
    expect(snapshot.settings.pollSeconds).toBe(120);
    await user().type(
      screen.getByRole("textbox", { name: "Add a trusted account" }),
      "half-typed",
    );
    await user().click(screen.getByRole("button", { name: "Review activity" }));
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(
      screen.getByRole("combobox", { name: "Poll interval" }),
    ).toHaveProperty("value", "120");
    expect(screen.getByRole("alert").textContent).toContain(
      "Not applied — Poll interval: 60 seconds",
    );
    expect(
      screen.getByRole("textbox", { name: "Add a trusted account" }),
    ).toHaveProperty("value", "half-typed");
    expect(screen.getByRole("alert").textContent).toContain("Disk is full");
    fail = false;
    await user().click(screen.getByRole("button", { name: "Retry saving" }));
    await screen.findByText("Saved");
    expect(snapshot.settings.pollSeconds).toBe(60);
    expect(invoke).toHaveBeenLastCalledWith("update_settings", {
      patch: { pollSeconds: 60 },
    });
  });
  it("blocks Enter and clicks for an unavailable follow-up without losing composed text", async () => {
    await mount();
    await user().click(screen.getByRole("tab", { name: /Review chat/ }));
    const composer = screen.getByRole("textbox", { name: "Message Codex" });
    await user().type(composer, "Keep this question");
    await act(async () => {
      snapshot.codexReady = false;
      emit();
    });
    expect(screen.getByText(/Codex is disconnected. Reconnect/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Send message" })).toHaveProperty(
      "disabled",
      true,
    );
    fireEvent.keyDown(composer, { key: "Enter" });
    expect(invoke.mock.calls.some(([c]) => c === "send_message")).toBe(false);
    await user().click(screen.getByRole("button", { name: "Review activity" }));
    await user().click(
      screen.getAllByRole("button", {
        name: new RegExp(snapshot.prs[0].title),
      })[0],
    );
    expect(
      screen.getByRole("textbox", { name: "Message Codex" }),
    ).toHaveProperty("value", "Keep this question");
  });
});

describe("initial loading and repository errors", () => {
  it("focuses search when a shortcut arrives before the initial snapshot", async () => {
    const loading = deferred<Snapshot>();
    invoke.mockImplementation((cmd) =>
      cmd === "get_snapshot" ? loading.promise : Promise.resolve(),
    );
    render(<App />);
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    await act(async () => loading.resolve(snapshot));
    expect(document.activeElement).toBe(
      screen.getByPlaceholderText("Search pull requests…"),
    );
  });
  it("keeps repository validation failures inside the open configuration dialog", async () => {
    invoke.mockImplementation((cmd, args) =>
      cmd === "save_repository"
        ? Promise.reject(Error("Checkout origin does not match"))
        : backend(cmd, args),
    );
    await mount();
    await user().click(screen.getByRole("button", { name: "Repositories" }));
    await user().click(
      screen.getByRole("button", { name: "Configure acme/orbit" }),
    );
    const dialog = screen.getByRole("dialog");
    await user().click(
      within(dialog).getByRole("button", { name: "Save repository" }),
    );
    expect((await within(dialog).findByRole("alert")).textContent).toContain(
      "Checkout origin does not match",
    );
    expect(screen.getByRole("dialog")).toBe(dialog);
    expect(useOrbit.getState().error).toBeNull();
  });
});

describe("storage and export", () => {
  it("scans only on request and confirms cleanup with per-run retry feedback", async () => {
    invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "get_storage_usage")
        return {
          stateBytes: 80,
          worktreeBytes: 2048,
          unrecognizedBytes: 32,
          unrecognized: [{ name: "unclaimed", bytes: 32 }],
          errors: [],
          worktrees: [
            { reviewId: snapshot.reviews[0].id, bytes: 2048, removable: true },
          ],
        };
      if (cmd === "remove_worktrees")
        return [
          { reviewId: snapshot.reviews[0].id, error: "Directory is busy" },
        ];
      return backend(cmd, args);
    });
    await mount();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(invoke.mock.calls.some(([c]) => c === "get_storage_usage")).toBe(
      false,
    );
    await user().click(screen.getByRole("button", { name: "Inspect storage" }));
    const row = await screen.findByRole("checkbox");
    expect(screen.getByText(/unclaimed/)).toBeTruthy();
    await act(async () => {
      snapshot.reviews[0].messages.push({
        id: "update",
        role: "system",
        text: "streamed update",
        createdAt: new Date().toISOString(),
      });
      emit();
    });
    expect(
      invoke.mock.calls.filter(([c]) => c === "get_storage_usage"),
    ).toHaveLength(1);
    await user().click(row);
    await user().click(
      screen.getByRole("button", { name: "Remove worktrees (1)" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Remove selected worktrees",
    });
    expect(dialog.textContent).toContain("2.0 KiB");
    expect(dialog.textContent).toContain("no longer support follow-ups");
    await user().click(
      within(dialog).getByRole("button", { name: "Confirm removal" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByRole("alert").textContent).toContain(
      "Directory is busy",
    );
    expect(snapshot.reviews[0].report).not.toBe("");
    expect(
      screen.getByRole("button", { name: "Remove worktrees (1)" }),
    ).toHaveProperty("disabled", false);
    expect(invoke).toHaveBeenCalledWith("remove_worktrees", {
      reviewIds: [snapshot.reviews[0].id],
    });
  });
  it("handles cancelled exports, write errors, and successful destinations without publishing", async () => {
    let attempt = 0;
    invoke.mockImplementation((cmd, args) => {
      if (cmd === "export_report") {
        attempt++;
        return attempt === 1
          ? Promise.resolve(null)
          : attempt === 2
            ? Promise.reject(Error("Export disk full"))
            : Promise.resolve("/chosen/report.md");
      }
      return backend(cmd, args);
    });
    await mount();
    await user().click(screen.getByRole("button", { name: "Review activity" }));
    const exportButton = screen.getByRole("button", { name: "Export report" });
    await user().click(exportButton);
    expect(screen.queryByText(/Report exported/)).toBeNull();
    await user().click(exportButton);
    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      expect.stringContaining("Export disk full"),
    );
    await user().click(exportButton);
    expect(await screen.findByRole("status")).toHaveProperty(
      "textContent",
      "Report exported: /chosen/report.md",
    );
    expect(invoke).toHaveBeenCalledWith("export_report", {
      reviewId: snapshot.reviews[0].id,
    });
    expect(invoke.mock.calls.some(([c]) => c === "publish_review")).toBe(false);
  });
  it("serializes partial mutations without reviving automation or reporting a queued write as saved", async () => {
    const first = deferred<void>();
    let calls = 0;
    snapshot.settings.automatic = true;
    invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "update_settings" && ++calls === 1) await first.promise;
      return backend(cmd, args);
    });
    const a = useOrbit.getState().updateSettings({ automatic: false });
    const b = useOrbit.getState().updateSettings({ pollSeconds: 60 });
    const c = useOrbit.getState().updateSettings({ pollSeconds: 300 });
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(useOrbit.getState().saves.pollSeconds.state).toBe("saving");
    first.resolve();
    const results = await Promise.all([a, b, c]);
    expect(results.every((r) => r.ok)).toBe(true);
    expect(snapshot.settings.automatic).toBe(false);
    expect(snapshot.settings.pollSeconds).toBe(300);
    expect(useOrbit.getState().saves.pollSeconds.state).toBe("saved");
    expect(invoke.mock.calls.map(([, args]) => args)).toEqual([
      { patch: { automatic: false } },
      { patch: { pollSeconds: 60 } },
      { patch: { pollSeconds: 300 } },
    ]);
  });
});

describe("review regression cases", () => {
  it("shows active automation consistently and retains the failed request beside Retry", async () => {
    let fail = true;
    invoke.mockImplementation((cmd, args) =>
      cmd === "update_settings" && fail
        ? Promise.reject(Error("Read-only state directory"))
        : backend(cmd, args),
    );
    await mount();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    await user().click(
      screen.getByRole("switch", {
        name: "Automatically review new pull requests",
      }),
    );
    expect((await screen.findByRole("alert")).textContent).toContain(
      "Not applied — Automatic reviews: On",
    );
    for (const name of [
      "Automatic PR reviews",
      "Automatically review new pull requests",
    ]) {
      expect(
        screen.getByRole("switch", { name }).getAttribute("aria-checked"),
      ).toBe("false");
    }
    await user().click(screen.getByRole("button", { name: "Review activity" }));
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(
      screen
        .getByRole("switch", { name: "Automatically review new pull requests" })
        .getAttribute("aria-checked"),
    ).toBe("false");
    expect(screen.getByRole("alert").textContent).toContain(
      "Automatic reviews: On",
    );
    fail = false;
    await user().click(screen.getByRole("button", { name: "Retry saving" }));
    for (const name of [
      "Automatic PR reviews",
      "Automatically review new pull requests",
    ]) {
      expect(
        screen.getByRole("switch", { name }).getAttribute("aria-checked"),
      ).toBe("true");
    }
  });
  it("reports removed worktrees with a persistence warning without offering removal again", async () => {
    invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "get_storage_usage")
        return {
          stateBytes: 80,
          worktreeBytes: 20,
          unrecognizedBytes: 0,
          unrecognized: [],
          errors: [],
          worktrees: snapshot.reviews[0].worktree
            ? [{ reviewId: snapshot.reviews[0].id, bytes: 20, removable: true }]
            : [],
        };
      if (cmd === "remove_worktrees") {
        snapshot.reviews[0].worktree = undefined;
        emit();
        return [
          {
            reviewId: snapshot.reviews[0].id,
            warning: "Completion record could not be saved",
          },
        ];
      }
      return backend(cmd, args);
    });
    await mount();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    await user().click(screen.getByRole("button", { name: "Inspect storage" }));
    await user().click(await screen.findByRole("checkbox"));
    await user().click(
      screen.getByRole("button", { name: "Remove worktrees (1)" }),
    );
    await user().click(screen.getByRole("button", { name: "Confirm removal" }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "Completion record could not be saved",
    );
    expect(
      screen.getByText(/Worktree removed; report and chat retained/),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Remove worktrees (0)" }),
    ).toHaveProperty("disabled", true);
    expect(screen.queryByRole("checkbox")).toBeNull();
  });
  it("disables Autopilot while repository validation is still pending", async () => {
    const validation = deferred<void>();
    snapshot.settings.automatic = true;
    invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "save_repository") {
        await validation.promise;
        return snapshot.settings;
      }
      return backend(cmd, args);
    });
    const repository = useOrbit
      .getState()
      .mutate(
        "save_repository",
        { repository: snapshot.settings.repositories[0] },
        "repository:acme/orbit",
      );
    await Promise.resolve();
    const disabled = await useOrbit
      .getState()
      .updateSettings({ automatic: false });
    expect(disabled.ok).toBe(true);
    expect(snapshot.settings.automatic).toBe(false);
    expect(useOrbit.getState().saves["repository:acme/orbit"].state).toBe(
      "saving",
    );
    validation.resolve();
    await repository;
    expect(snapshot.settings.automatic).toBe(false);
  });
  it("requires the failed model change to resolve before an effort can be saved", async () => {
    let fail = true;
    invoke.mockImplementation((cmd, args) =>
      cmd === "update_settings" && fail
        ? Promise.reject(Error("Model unavailable"))
        : backend(cmd, args),
    );
    await mount();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    await user().selectOptions(
      screen.getByRole("combobox", { name: /^Model/ }),
      "demo-review-model",
    );
    await screen.findByRole("alert");
    expect(
      screen.getByRole("combobox", { name: /^Reasoning effort/ }),
    ).toHaveProperty("disabled", true);
    await user().click(screen.getByRole("button", { name: "Review activity" }));
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(
      screen.getByRole("combobox", { name: /^Reasoning effort/ }),
    ).toHaveProperty("disabled", true);
    fail = false;
    await user().click(screen.getByRole("button", { name: "Retry saving" }));
    const effort = screen.getByRole("combobox", { name: /^Reasoning effort/ });
    await waitFor(() => expect(effort).toHaveProperty("disabled", false));
    await user().selectOptions(effort, "high");
    expect(snapshot.settings.effort).toBe("high");
    expect(effort).toHaveProperty("value", "high");
  });
  it("keeps the app navigable and cleanup progress available while removal waits", async () => {
    const removal = deferred<unknown>();
    invoke.mockImplementation((cmd, args) => {
      if (cmd === "get_storage_usage")
        return Promise.resolve({
          stateBytes: 80,
          worktreeBytes: 20,
          unrecognizedBytes: 0,
          unrecognized: [],
          errors: [],
          worktrees: [
            { reviewId: snapshot.reviews[0].id, bytes: 20, removable: true },
          ],
        });
      if (cmd === "remove_worktrees") return removal.promise;
      return backend(cmd, args);
    });
    await mount();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    await user().click(screen.getByRole("button", { name: "Inspect storage" }));
    await user().click(await screen.findByRole("checkbox"));
    await user().click(
      screen.getByRole("button", { name: "Remove worktrees (1)" }),
    );
    await user().click(screen.getByRole("button", { name: "Confirm removal" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByRole("status").textContent).toContain(
      "keep using Orbit",
    );
    await user().click(screen.getByRole("button", { name: "Review activity" }));
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(screen.getByRole("status").textContent).toContain(
      "Removing worktrees",
    );
    await act(async () =>
      removal.resolve([
        {
          reviewId: snapshot.reviews[0].id,
          error: "Could not remove test worktree",
        },
      ]),
    );
    await user().click(screen.getByRole("button", { name: "Inspect storage" }));
    expect(screen.getByRole("alert").textContent).toContain(
      "Could not remove test worktree",
    );
    expect(useOrbit.getState().cleanup.running).toBe(false);
  });
});

describe("deterministic model controls", () => {
  it("shows unresolved placeholders, blocks new reviews, and saves model plus supported default atomically", async () => {
    snapshot.settings.model = "";
    snapshot.settings.effort = "";
    snapshot.modelConfigurationError = "Choose a model and effort in Settings.";
    snapshot.models.push({
      ...snapshot.models[0],
      id: "display-id",
      model: "second-model",
      displayName: "Second model",
      defaultReasoningEffort: "high",
      isDefault: false,
    });
    await mount();
    expect(screen.queryByRole("button", { name: "Review again" })).toBeNull();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    const model = screen.getByRole("combobox", { name: /^Model/ });
    const effort = screen.getByRole("combobox", { name: /^Reasoning effort/ });
    expect(model).toHaveProperty("value", "");
    expect(effort).toHaveProperty("value", "");
    expect(
      screen.getByRole("option", { name: "Choose a model" }),
    ).toHaveProperty("disabled", true);
    expect(screen.queryByRole("option", { name: "Codex default" })).toBeNull();
    expect(screen.queryByRole("option", { name: "Model default" })).toBeNull();
    await user().selectOptions(model, "second-model");
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { model: "second-model", effort: "high" },
    });
    expect(effort).toHaveProperty("value", "high");
  });
  it("preserves an unavailable explicit selection and permits disabling automation during discovery failure", async () => {
    snapshot.settings.model = "missing-model";
    snapshot.settings.effort = "unsupported";
    snapshot.settings.automatic = true;
    snapshot.modelCatalogReady = false;
    snapshot.modelConfigurationError =
      "Model discovery failed. Reconnect in Settings.";
    await mount();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(screen.getByRole("combobox", { name: /^Model/ })).toHaveProperty(
      "value",
      "missing-model",
    );
    expect(
      screen.getByRole("option", { name: "missing-model (unavailable)" }),
    ).toHaveProperty("disabled", true);
    expect(
      screen.getByText(/Reviews are blocked: Model discovery failed/),
    ).toBeTruthy();
    const toggle = screen.getByRole("switch", {
      name: "Automatically review new pull requests",
    });
    await user().click(toggle);
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { automatic: false },
    });
    expect(snapshot.settings.model).toBe("missing-model");
  });
  it("cancels the legacy selector without mutation and retries a failed selection without losing the report or draft", async () => {
    snapshot.reviews[0].model = "";
    snapshot.reviews[0].confirmedReviewSelection = undefined;
    const original = structuredClone(snapshot.reviews[0]);
    let fail = true;
    invoke.mockImplementation(async (cmd, args) => {
      if (cmd !== "set_legacy_chat_model") return backend(cmd, args);
      if (fail) throw Error("Chat model was not saved: disk full");
      snapshot.reviews[0].legacyChatSelection = {
        model: args.model,
        effort: args.effort,
      };
      emit();
    });
    await mount();
    await user().click(screen.getByRole("tab", { name: /Review chat/ }));
    const composer = screen.getByRole("textbox", { name: "Message Codex" });
    await user().type(composer, "Keep this question");
    fireEvent.keyDown(composer, { key: "Enter" });
    expect(invoke.mock.calls.some(([cmd]) => cmd === "send_message")).toBe(
      false,
    );
    await user().click(
      screen.getByRole("button", { name: "Choose chat model" }),
    );
    let dialog = screen.getByRole("dialog", { name: "Choose chat model" });
    expect(
      within(dialog).getByRole("combobox", { name: "Chat model" }),
    ).toHaveProperty("value", snapshot.settings.model);
    await user().click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(
      invoke.mock.calls.some(([cmd]) => cmd === "set_legacy_chat_model"),
    ).toBe(false);
    await user().click(
      screen.getByRole("button", { name: "Choose chat model" }),
    );
    dialog = screen.getByRole("dialog", { name: "Choose chat model" });
    await user().selectOptions(
      within(dialog).getByRole("combobox", { name: "Chat reasoning effort" }),
      "low",
    );
    await user().click(
      within(dialog).getByRole("button", { name: "Save chat model" }),
    );
    expect((await within(dialog).findByRole("alert")).textContent).toContain(
      "disk full",
    );
    expect(composer).toHaveProperty("value", "Keep this question");
    fail = false;
    await user().click(
      within(dialog).getByRole("button", { name: "Save chat model" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(snapshot.reviews[0].model).toBe(original.model);
    expect(snapshot.reviews[0].effort).toBe(original.effort);
    expect(snapshot.reviews[0].report).toBe(original.report);
    expect(snapshot.reviews[0].confirmedReviewSelection).toBeUndefined();
    expect(snapshot.reviews[0].legacyChatSelection?.effort).toBe("low");
    expect(
      screen.getByText(/Report selection: Model: not recorded/),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Choose chat model" }),
    ).toBeNull();
    expect(screen.getByRole("button", { name: "Send message" })).toHaveProperty(
      "disabled",
      false,
    );
    expect(invoke.mock.calls.some(([cmd]) => cmd === "send_message")).toBe(
      false,
    );
  });
  it("keeps composed text and published report when resume verification fails", async () => {
    snapshot.reviews[0].status = "posted";
    snapshot.reviews[0].commentUrl =
      "https://github.com/acme/orbit/pull/148#issuecomment-7";
    invoke.mockImplementation((cmd, args) =>
      cmd === "send_message"
        ? Promise.reject(
            Error(
              "Codex did not confirm the requested model. No inference was started.",
            ),
          )
        : backend(cmd, args),
    );
    await mount();
    await user().click(screen.getByRole("tab", { name: /Review chat/ }));
    const composer = screen.getByRole("textbox", { name: "Message Codex" });
    await user().type(composer, "Explain this finding");
    await user().click(screen.getByRole("button", { name: "Send message" }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "did not confirm",
    );
    expect(composer).toHaveProperty("value", "Explain this finding");
    expect(snapshot.reviews[0].status).toBe("posted");
    expect(snapshot.reviews[0].report).not.toBe("");
    expect(
      screen.getAllByRole("button", { name: /View comment on GitHub/ }).length,
    ).toBeGreaterThan(0);
  });
});
