// @vitest-environment jsdom
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  onTestFinished,
  vi,
} from "vitest";
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
import "./test-setup";
import { Theme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral/built";
import App from "./App";
import { Chat } from "./Chat";
import { Inbox } from "./Inbox";
import { demoAnswer, freshDemo } from "./demo";
import { useOrbit } from "./store";
import type { RemoteAccessStatus } from "./RemoteAccess";
import { buddyOf, questionLabels, withPatch } from "./types";
import type {
  Snapshot,
  SettingsPatch,
  ChangedFile,
  Question,
  Repository,
} from "./types";

const { invoke, listeners } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    async (name: string, handler: (event: { payload: unknown }) => void) => {
      listeners.set(name, handler);
      return () => {};
    },
  ),
}));
let snapshot: Snapshot;
const user = () => userEvent.setup();
async function chooseOption(control: HTMLElement, label: string) {
  await user().click(control);
  await user().click(await screen.findByRole("option", { name: label }));
}
function emit() {
  useOrbit.setState({ snapshot: structuredClone(snapshot) });
}
/** Opens a page of Settings, once Settings is open. */
async function openSettingsTab(name: string) {
  await user().click(
    await screen.findByRole("tab", { name: new RegExp(`^${name}`) }),
  );
}
async function mount() {
  render(
    <Theme theme={neutralTheme} mode="dark">
      <App />
    </Theme>,
  );
  await waitFor(() =>
    expect(screen.queryByText("Loading your workspace…")).toBeNull(),
  );
}
/** Opens All reviews on the loops page and returns the row titled `label`. */
async function allReviewsRow(label: string) {
  await user().click(screen.getByRole("button", { name: "Review loops" }));
  await user().click(screen.getByRole("button", { name: /^All reviews/ }));
  return within(screen.getByRole("list", { name: "All reviews" }))
    .getByText(label)
    .closest("li") as HTMLElement;
}
/** The alert with text. Settings' port field keeps an empty alert region. */
function banner() {
  const found = screen.queryAllByRole("alert").find((a) => a.textContent);
  if (!found) throw Error("No alert with text");
  return found;
}
const findBanner = () => waitFor(banner);
/** The first worktree row in Storage. Settings has other checkboxes, such as
 * the buddy's colour and hat cards. */
async function findWorktreeRow() {
  const list = await screen.findByRole("group", { name: "Review worktrees" });
  return within(list).getByRole("checkbox");
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
    Object.assign(
      snapshot.settings,
      withPatch(snapshot.settings, args.patch as SettingsPatch),
    );
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
  if (command === "get_remote_access") return remoteAccess;
  if (command === "set_remote_access") {
    const { enabled, port } = args as { enabled: boolean; port: number };
    remoteAccess = {
      enabled,
      port,
      running: enabled,
      link: enabled
        ? `http://127.0.0.1:${port}/#token=${"a".repeat(64)}`
        : null,
      tailnetHost: enabled ? "desk.example.ts.net" : null,
    };
    return remoteAccess;
  }
  if (command === "reset_remote_link") {
    remoteAccess = {
      ...remoteAccess,
      link: `http://127.0.0.1:${remoteAccess.port}/#token=${"b".repeat(64)}`,
    };
    return remoteAccess;
  }
}
let remoteAccess: RemoteAccessStatus;
beforeEach(() => {
  remoteAccess = { enabled: false, port: 4317, running: false };
  snapshot = freshDemo();
  useOrbit.setState({
    snapshot,
    loaded: false,
    error: null,
    saves: {},
    cleanup: { running: false, results: [] },
    trustedInput: "",
    settingsTab: "connections",
    chatDrafts: {},
  });
  invoke.mockReset().mockImplementation(backend);
  Element.prototype.scrollIntoView = vi.fn();
});
afterEach(() => {
  cleanup();
  // Astryx's document-level announcement regions intentionally outlive roots.
  // Remove them between fixtures; the library reattaches them on next use.
  document.querySelectorAll("[data-astryx-live-region]").forEach((region) => {
    region.textContent = "";
    region.remove();
  });
});

describe("PR review regressions", () => {
  it.each([
    ["Approve once", true],
    ["Deny", false],
  ] as const)(
    "shows %s failures inside the approval dialog and retries",
    async (label, accept) => {
      snapshot.approvals = [
        {
          id: "approval-1",
          reviewId: snapshot.reviews[0].id,
          method: "item/commandExecution/requestApproval",
          params: { reason: "Run the disposable check" },
        },
      ];
      const pending = deferred<void>();
      let attempts = 0;
      invoke.mockImplementation(async (cmd, args) => {
        if (cmd !== "answer_approval") return backend(cmd, args);
        if (++attempts === 1) return pending.promise;
        snapshot.approvals = [];
        emit();
      });
      await mount();
      await user().click(
        screen.getByRole("button", { name: "Review request" }),
      );
      const dialog = screen.getByRole("dialog", {
        name: "Codex needs your approval",
      });
      await user().click(within(dialog).getByRole("button", { name: label }));
      expect(
        within(dialog).getByRole("button", { name: "Approve once" }),
      ).toHaveProperty("disabled", true);
      expect(
        within(dialog).getByRole("button", { name: "Deny" }),
      ).toHaveProperty("disabled", true);
      await act(async () =>
        pending.reject(Error("Codex disconnected; reconnect and retry.")),
      );
      expect((await within(dialog).findByRole("alert")).textContent).toContain(
        "Codex disconnected",
      );
      expect(useOrbit.getState().error).toBeNull();
      await user().click(within(dialog).getByRole("button", { name: label }));
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
      expect(
        invoke.mock.calls
          .filter(([cmd]) => cmd === "answer_approval")
          .map(([, args]) => args),
      ).toEqual([
        { id: "approval-1", accept },
        { id: "approval-1", accept },
      ]);
    },
  );

  it("opens the approval of the item chosen, not the oldest one", async () => {
    const second = {
      ...structuredClone(snapshot.reviews[0]),
      id: "second-review",
      prId: snapshot.prs[2].id,
      status: "awaitingApproval" as const,
    };
    snapshot.reviews.push(second);
    const request = (reviewId: string, id: string, reason: string) => ({
      id,
      reviewId,
      method: "item/commandExecution/requestApproval",
      params: { reason },
    });
    snapshot.approvals = [
      request(snapshot.reviews[0].id, "approval-1", "Run the first check"),
      request(second.id, "approval-2", "Run the second check"),
    ];
    snapshot.attention = snapshot.approvals.map((a, i) => ({
      key: `approval:${a.reviewId}:${a.id}`,
      kind: "approval" as const,
      prId: [snapshot.prs[0].id, snapshot.prs[2].id][i],
      reviewId: a.reviewId,
      approvalId: a.id,
      repo: null,
      reason: `Request ${i + 1}`,
    }));
    await mount();
    await user().click(screen.getByRole("button", { name: "Review loops" }));
    await user().click(await screen.findByText("Request 2"));
    const dialog = screen.getByRole("dialog", {
      name: "Codex needs your approval",
    });
    expect(within(dialog).getByText("Run the second check")).toBeTruthy();
    await user().click(
      within(dialog).getByRole("button", { name: "Approve once" }),
    );
    expect(invoke).toHaveBeenCalledWith("answer_approval", {
      id: "approval-2",
      accept: true,
    });
    expect(invoke).not.toHaveBeenCalledWith("answer_approval", {
      id: "approval-1",
      accept: true,
    });
  });

  it("turns desktop notifications off from Settings", async () => {
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Automation");
    await user().click(
      await screen.findByRole("switch", {
        name: /Notify me when a review needs attention/,
      }),
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { notify: false },
    });
  });

  it("turns write-ups on and picks Claude to write them", async () => {
    snapshot.settings.writeUps = { enabled: false, writer: "codex" };
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Decisions");
    await user().click(
      await screen.findByRole("switch", {
        name: /Write up each pull request in plain words/,
      }),
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { writeUps: { enabled: true, writer: "codex" } },
    });
    await chooseOption(
      await screen.findByRole("combobox", { name: "Writer" }),
      "Claude",
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { writeUps: { enabled: true, writer: "claude" } },
    });
  });

  it("turns on browser access and shows the Tailscale link", async () => {
    const shows = (text: string) =>
      waitFor(() => expect(document.body.textContent).toContain(text));
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Other devices");
    const toggle = await screen.findByRole("switch", {
      name: /Allow access from a browser/,
    });
    expect(document.body.textContent).not.toContain("tailscale serve");
    await user().click(toggle);
    expect(invoke).toHaveBeenCalledWith("set_remote_access", {
      enabled: true,
      port: 4317,
    });
    await shows("tailscale serve --bg 4317");
    await shows(`https://desk.example.ts.net/#token=${"a".repeat(64)}`);
    await user().click(screen.getByRole("button", { name: "Make a new link" }));
    expect(invoke).toHaveBeenCalledWith("reset_remote_link", {});
    await shows(`https://desk.example.ts.net/#token=${"b".repeat(64)}`);
  });

  it("opens a notified approval when Orbit comes forward, if still open", async () => {
    snapshot.reviews[0].status = "awaitingApproval";
    snapshot.approvals = [
      {
        id: 7,
        reviewId: snapshot.reviews[0].id,
        method: "item/commandExecution/requestApproval",
        params: { reason: "Run the notified check" },
      },
    ];
    const item = {
      key: `approval:${snapshot.reviews[0].id}:7`,
      kind: "approval" as const,
      prId: snapshot.prs[0].id,
      reviewId: snapshot.reviews[0].id,
      approvalId: 7,
      repo: null,
      reason: "The reviewer asks to run a command.",
    };
    snapshot.attention = [item];
    await mount();
    await user().click(screen.getByRole("button", { name: "Review loops" }));
    // An item resolved in the meantime is not opened.
    act(() => listeners.get("orbit-open")!({ payload: { ...item, key: "x" } }));
    expect(screen.queryByRole("dialog")).toBeNull();
    act(() => listeners.get("orbit-open")!({ payload: item }));
    const dialog = await screen.findByRole("dialog", {
      name: "Codex needs your approval",
    });
    expect(within(dialog).getByText("Run the notified check")).toBeTruthy();
  });

  it("keeps export, publication links, and stop usable without PR metadata", async () => {
    snapshot.prs = [];
    snapshot.historyPrs = [];
    const saved = snapshot.reviews[0];
    saved.status = "posted";
    saved.commentUrl = "https://github.com/acme/orbit/pull/148#issuecomment-7";
    const running = {
      ...structuredClone(saved),
      id: "orphan-active",
      prId: "acme/missing#12",
      status: "reviewing" as const,
      report: "",
      commentUrl: undefined,
    };
    snapshot.reviews.push(running);
    await mount();
    const row = await allReviewsRow(saved.prId);
    const exportButton = within(row).getByRole("button", {
      name: "Export report",
    });
    expect(exportButton.closest('[aria-disabled="true"]')).toBeNull();
    await user().click(exportButton);
    await user().click(
      within(row).getByRole("button", { name: "View comment on GitHub" }),
    );
    const stop = screen.getByRole("button", {
      name: "Stop review acme/missing#12",
    });
    expect(stop.closest('[aria-disabled="true"]')).toBeNull();
    await user().click(stop);
    expect(invoke).toHaveBeenCalledWith("export_report", {
      reviewId: saved.id,
    });
    expect(invoke).toHaveBeenCalledWith("open_github", {
      url: saved.commentUrl,
    });
    expect(invoke).toHaveBeenCalledWith("cancel_review", {
      reviewId: running.id,
    });
    expect(
      screen.getByText(
        "Each pull request Orbit reviews on its own, round by round.",
      ),
    ).toBeTruthy();
  });

  it("counts what needs you in the sidebar and opens the list", async () => {
    // Wide enough that the sidebar shows its counts.
    const wide = vi.spyOn(window, "innerWidth", "get").mockReturnValue(1400);
    onTestFinished(() => wide.mockRestore());
    await mount();
    const item = screen.getByRole("button", { name: /^Needs you/ });
    expect(item.parentElement?.textContent).toContain("3");
    await user().click(item);
    expect(
      await screen.findByRole("heading", { name: "Needs you" }),
    ).toBeTruthy();
    snapshot.attention = [];
    emit();
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /^Needs you/ })).toBeNull(),
    );
  });

  it("marks Operations while CI fails, and while runs wait for a runner", async () => {
    const wide = vi.spyOn(window, "innerWidth", "get").mockReturnValue(1400);
    onTestFinished(() => wide.mockRestore());
    await mount();
    const operations = () =>
      screen.getByRole("button", { name: /^Operations/ }).parentElement!;
    expect(within(operations()).getByLabelText("CI failing")).toBeTruthy();
    const waiting = {
      key: "problem:waits:1",
      kind: "runs_waiting" as const,
      prId: null,
      reviewId: null,
      approvalId: null,
      repo: "acme/api",
      reason:
        "A run of Release in acme/api has waited more than 30 minutes for a runner.",
    };
    snapshot.attention = [waiting];
    emit();
    await waitFor(() =>
      expect(within(operations()).getByLabelText("Runs waiting")).toBeTruthy(),
    );
    expect(within(operations()).queryByLabelText("CI failing")).toBeNull();
    snapshot.attention = [];
    emit();
    await waitFor(() =>
      expect(within(operations()).queryByLabelText("Runs waiting")).toBeNull(),
    );
  });

  it("opens a pull request from Ready to merge and says what is in its way", async () => {
    await mount();
    await user().click(screen.getByRole("button", { name: /^Operations/ }));
    await user().click(
      within(
        await screen.findByRole("list", { name: "Ready to merge" }),
      ).getByText(/^acme\/orbit#145/),
    );
    await user().click(screen.getByRole("tab", { name: /Overview/ }));
    expect(screen.getByRole("heading", { name: "Merging" })).toBeTruthy();
    expect(
      screen.getByText(
        "main moved on since its checks ran: merge main in and let them run again.",
      ),
    ).toBeTruthy();
  });
  it("opens on Live under autopilot and keeps inbox search on Ctrl+K", async () => {
    snapshot.settings.automatic = true;
    await mount();
    expect(
      screen.getByRole("heading", { level: 1, name: "Live" }),
    ).toBeTruthy();
    expect(
      screen.getByText("Autopilot · 2 loops running · 3 need you"),
    ).toBeTruthy();
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    const search = await screen.findByPlaceholderText("Search pull requests…");
    await waitFor(() => expect(document.activeElement).toBe(search));
    expect(
      screen.getByRole("heading", { level: 1, name: "All pull requests" }),
    ).toBeTruthy();
    // Landing happens once; switching autopilot off later stays put.
    snapshot.settings.automatic = false;
    act(emit);
    expect(await screen.findByText("Manual mode")).toBeTruthy();
    expect(
      screen.getByRole("heading", { level: 1, name: "All pull requests" }),
    ).toBeTruthy();
  });
  it("returns to the loop overview from the sidebar while a loop is open", async () => {
    await mount();
    for (const name of [/^Needs you/, /^Review loops/]) {
      await user().click(screen.getByRole("button", { name: "Review loops" }));
      await user().click(
        within(screen.getByRole("list", { name: "Loops" })).getByText(
          snapshot.prs[2].title,
        ),
      );
      expect(screen.getByRole("list", { name: "Rounds" })).toBeTruthy();
      await user().click(screen.getByRole("button", { name }));
      expect(screen.queryByRole("list", { name: "Rounds" })).toBeNull();
      expect(screen.getByRole("list", { name: "Needs you" })).toBeTruthy();
    }
  });
  it("opens Decisions from the sidebar, shows the read and reads again", async () => {
    const { demoDecisions } = await import("./demoDecisions");
    const decisions = demoDecisions();
    snapshot.ledger = {
      refreshing: true,
      progress: { repo: "acme/api", done: 3, total: 12 },
      repos: [
        { repo: "acme/orbit", refreshedAt: new Date().toISOString() },
        { repo: "acme/api", error: "HTTP 403: rate limit exceeded." },
      ],
      version: 1,
    };
    invoke.mockImplementation(async (command: string, args) =>
      command === "get_decisions" ? decisions : backend(command, args),
    );
    await mount();
    await user().click(screen.getByRole("button", { name: "Decisions" }));
    expect(await screen.findByText("Show build times per branch")).toBeTruthy();
    expect(
      screen.getByText("Reading acme/api from GitHub · 3 of 12"),
    ).toBeTruthy();
    expect(screen.getByText("Couldn't read acme/api")).toBeTruthy();
    snapshot.ledger = {
      ...snapshot.ledger,
      refreshing: false,
      progress: null,
      writing: { repo: "acme/orbit", number: 142, done: 1, total: 4 },
      writeError: "Write-ups are waiting for Codex to connect.",
    };
    emit();
    expect(
      await screen.findByText("Writing up orbit#142 in plain words · 2 of 4"),
    ).toBeTruthy();
    expect(
      screen.getByText("Write-ups are waiting for Codex to connect."),
    ).toBeTruthy();
    await user().click(
      await screen.findByRole("button", { name: "Read again" }),
    );
    expect(invoke).toHaveBeenCalledWith("refresh_decisions", {});
    const reads = () =>
      invoke.mock.calls.filter(([c]) => c === "get_decisions").length;
    const before = reads();
    snapshot.ledger = { ...snapshot.ledger, version: 2 };
    emit();
    await waitFor(() => expect(reads()).toBe(before + 1));
  });
  it("opens on all pull requests in manual mode", async () => {
    await mount();
    expect(
      screen.getByRole("heading", { level: 1, name: "All pull requests" }),
    ).toBeTruthy();
  });

  it("labels every tab panel while mounting only the active tab contents", async () => {
    await mount();
    for (const name of [/Review chat/, /Files changed/, /Overview/]) {
      await user().click(screen.getByRole("tab", { name }));
      const tabs = screen.getAllByRole("tab");
      for (const tab of tabs) {
        const panel = document.getElementById(
          tab.getAttribute("aria-controls")!,
        );
        expect(panel).not.toBeNull();
        expect(panel?.getAttribute("aria-labelledby")).toBe(tab.id);
        expect(panel?.hidden).toBe(
          tab.getAttribute("aria-selected") !== "true",
        );
      }
      expect(screen.getAllByRole("tabpanel")).toHaveLength(1);
      expect(screen.getAllByRole("tabpanel", { hidden: true })).toHaveLength(3);
    }
    expect(
      invoke.mock.calls.filter(([cmd]) => cmd === "get_files"),
    ).toHaveLength(1);
  });

  it("reports repository result truncation and narrows to matches past the initial limit", async () => {
    const repos = Array.from({ length: 61 }, (_, i) => ({
      name: `acme/repository-${String(i).padStart(2, "0")}`,
      description: "Example repository",
      private: true,
    }));
    snapshot.settings.repositories = [
      { ...snapshot.settings.repositories[0], name: repos[0].name },
    ];
    invoke.mockImplementation((cmd, args) =>
      cmd === "list_repositories" ? Promise.resolve(repos) : backend(cmd, args),
    );
    await mount();
    await user().click(screen.getByRole("button", { name: "Repositories" }));
    await user().click(
      await screen.findByRole("button", { name: "Add repository" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Connect a repository" });
    await user().click(
      within(dialog).getByRole("button", { name: "Refresh repositories" }),
    );
    const input = within(dialog).getByRole("combobox", {
      name: "Search your repositories",
    });
    await user().click(input);
    expect(
      await within(dialog).findByText(
        "Showing 50 of 60 matches. Keep typing to narrow the results.",
      ),
    ).toBeTruthy();
    expect(await screen.findAllByRole("option")).toHaveLength(50);
    expect(
      screen.queryByRole("option", { name: /acme\/repository-00/ }),
    ).toBeNull();
    await user().type(input, "repository-60");
    await user().click(
      await screen.findByRole("option", { name: /acme\/repository-60/ }),
    );
    expect(within(dialog).queryByText(/Showing 50/)).toBeNull();
    expect(within(dialog).getByText("acme/repository-60")).toBeTruthy();
    await user().click(within(dialog).getByRole("button", { name: "Change" }));
    expect(
      within(dialog).getByText(
        "Showing 50 of 60 matches. Keep typing to narrow the results.",
      ),
    ).toBeTruthy();
  });

  it("adds a repository without a folder as Orbit's own copy", async () => {
    const repos = [{ name: "acme/web", description: "", private: true }];
    invoke.mockImplementation((cmd, args) =>
      cmd === "list_repositories"
        ? Promise.resolve(repos)
        : cmd === "save_repository"
          ? Promise.resolve(snapshot.settings)
          : backend(cmd, args),
    );
    await mount();
    await user().click(screen.getByRole("button", { name: "Repositories" }));
    await user().click(
      await screen.findByRole("button", { name: "Add repository" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Connect a repository" });
    await user().click(
      within(dialog).getByRole("button", { name: "Refresh repositories" }),
    );
    await user().click(
      within(dialog).getByRole("combobox", {
        name: "Search your repositories",
      }),
    );
    await user().click(
      await screen.findByRole("option", { name: /acme\/web/ }),
    );
    const path = within(dialog).getByRole("textbox", {
      name: /Local checkout path/,
    });
    expect(path).toHaveProperty("value", "");
    // Orbit keeps its own copy up to date.
    const keep = within(dialog).getByRole("switch", {
      name: "Keep this clone up to date",
    });
    expect(keep).toHaveProperty("checked", true);
    expect(keep).toHaveProperty("disabled", true);
    await user().click(
      within(dialog).getByRole("button", { name: "Download and connect" }),
    );
    const saved = invoke.mock.calls.find(([cmd]) => cmd === "save_repository");
    expect(saved?.[1]).toMatchObject({
      repository: { name: "acme/web", path: "" },
      original: null,
    });
    // With a folder, it's an ordinary save.
    await user().click(
      await screen.findByRole("button", { name: "Add repository" }),
    );
    const again = screen.getByRole("dialog", { name: "Connect a repository" });
    await user().type(
      within(again).getByRole("textbox", { name: /Local checkout path/ }),
      "/Users/you/web",
    );
    expect(
      within(again).getByRole("button", { name: "Save repository" }),
    ).toBeTruthy();
  });

  it("bounds live timestamp timers for a long inbox and conversation", () => {
    vi.useFakeTimers();
    try {
      const pr = snapshot.prs[0];
      snapshot.prs = Array.from({ length: 120 }, (_, index) => ({
        ...pr,
        id: `acme/orbit#${index}`,
        number: index,
      }));
      const review = snapshot.reviews[0];
      const message = review.messages.find((m) => m.role === "agent")!;
      review.messages = Array.from({ length: 120 }, (_, index) => ({
        ...message,
        id: `message-${index}`,
        text: `Review note ${index}`,
      }));
      emit();
      const view = render(
        <Theme theme={neutralTheme} mode="dark">
          <Inbox
            search=""
            setSearch={() => {}}
            filter="all"
            setFilter={() => {}}
            repoFilter="all"
            setRepoFilter={() => {}}
            searchRef={{ current: null }}
            onChoose={() => {}}
            onRepositories={() => {}}
          />
          <Chat
            pr={pr}
            review={review}
            onStart={() => {}}
            onPublish={() => {}}
            onHistory={() => {}}
          />
        </Theme>,
      );
      // Library timestamps previously created 240+ intervals for this fixture.
      expect(vi.getTimerCount()).toBeLessThan(40);
      expect(
        view.container.querySelectorAll('[data-format="date_time"]').length,
      ).toBeGreaterThan(200);
      view.unmount();
      act(() => vi.runOnlyPendingTimers());
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});

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
      within(dialog).getByRole("button", { name: "Publish to GitHub" }),
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
      await screen.findByRole("button", { name: /View comment on GitHub/ }),
    );
    expect(invoke).toHaveBeenCalledWith("open_github", {
      url: snapshot.reviews[0].commentUrl,
    });
    const row = await allReviewsRow(snapshot.prs[0].title);
    expect(
      within(row).getByRole("button", { name: /View comment on GitHub/ }),
    ).toBeTruthy();
  });
});
describe("Bit", () => {
  it("shows Bit's notes as a conversation and opens what each one is about", async () => {
    await mount();
    await user().click(screen.getByRole("button", { name: "Bit" }));
    expect(
      screen.getByRole("heading", {
        name: /^Bit is (happy|busy|worried|sleepy)\.$/,
      }),
    ).toBeTruthy();
    const chat = () => screen.getByRole("log", { name: "Bit's notes" });
    const text = chat().textContent!;
    expect(text.indexOf("is clean after 3 rounds!")).toBeLessThan(
      text.indexOf("acme/dashboard can't sync"),
    );
    await user().click(
      within(chat()).getByRole("button", { name: "Open repositories" }),
    );
    expect(
      screen.getByRole("heading", { level: 1, name: "Repositories" }),
    ).toBeTruthy();
    await user().click(screen.getByRole("button", { name: "Bit" }));
    await user().click(
      within(chat()).getAllByRole("button", { name: "Open loop" })[1],
    );
    expect(await screen.findByRole("list", { name: "Rounds" })).toBeTruthy();
  });

  it("opens what a note says to decide, and the sidebar goes back to every pull request", async () => {
    const { demoDecisions } = await import("./demoDecisions");
    const decisions = demoDecisions();
    invoke.mockImplementation(async (command: string, args) =>
      command === "get_decisions" ? decisions : backend(command, args),
    );
    await mount();
    await user().click(screen.getByRole("button", { name: "Bit" }));
    const chat = screen.getByRole("log", { name: "Bit's notes" });
    expect(chat.textContent).toContain("Something to decide on api#318");
    await user().click(
      within(chat).getByRole("button", { name: "Open decisions" }),
    );
    expect(
      await screen.findByRole("heading", {
        level: 2,
        name: "Sign webhook payloads with a rotating secret",
      }),
    ).toBeTruthy();
    await user().click(screen.getByRole("button", { name: "Decisions" }));
    expect(await screen.findByText("Show build times per branch")).toBeTruthy();
  });

  it("points a new workspace to Bit's setup help", async () => {
    const answer = async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd !== "ask_buddy") return backend(cmd, args);
      const question = args!.question as Question;
      const notes = snapshot.buddyFeed!.notes;
      notes.push({
        id: 100 + notes.length,
        at: new Date().toISOString(),
        kind: "answer",
        prId: null,
        reviewId: null,
        repo: null,
        question: questionLabels[question],
        ...demoAnswer(snapshot, question),
      });
      emit();
    };
    invoke.mockImplementation(answer);
    const repositories = snapshot.settings.repositories;
    snapshot.settings.repositories = [];
    snapshot.prs = [];
    await mount();
    await user().click(
      await screen.findByRole("button", { name: "Set up with Bit" }),
    );
    expect(invoke).toHaveBeenCalledWith("ask_buddy", { question: "setup" });
    const chat = await screen.findByRole("log", { name: "Bit's notes" });
    expect(
      await within(chat).findByText(
        "No repositories are connected yet, and GitHub lists none of yours to connect. You can add one under Repositories.",
      ),
    ).toBeTruthy();
    // A command in a line shows as code.
    act(() => {
      snapshot.settings.repositories = repositories.map((r) => ({
        ...r,
        setupCommand: "npm ci",
      }));
      emit();
    });
    await user().click(screen.getByRole("button", { name: "Help me set up" }));
    expect(
      (await within(chat).findAllByText("npm ci")).some(
        (e) => e.tagName === "CODE",
      ),
    ).toBe(true);
  });

  it("answers quick questions and offers fixes as buttons", async () => {
    invoke.mockImplementation(async (cmd, args) => {
      const a = args as Record<string, string>;
      if (cmd === "ask_buddy") {
        const notes = snapshot.buddyFeed!.notes;
        notes.push({
          id: 100 + notes.length,
          at: new Date().toISOString(),
          kind: "answer",
          prId: null,
          reviewId: null,
          repo: null,
          question: questionLabels[a.question as Question],
          ...demoAnswer(snapshot, a.question as Question),
        });
        emit();
        return;
      }
      if (cmd === "use_fix") {
        snapshot.attention = snapshot.attention!.filter((i) => i.key !== a.key);
        emit();
        return;
      }
      return backend(cmd, args);
    });
    await mount();
    await user().click(screen.getByRole("button", { name: "Bit" }));
    const chat = () => screen.getByRole("log", { name: "Bit's notes" });
    // The Needs you notes offer their fixes while the items are open.
    expect(
      within(chat()).getAllByRole("button", { name: "Continue loop" }),
    ).toHaveLength(1);
    await user().click(screen.getByRole("button", { name: "What needs me?" }));
    expect(invoke).toHaveBeenCalledWith("ask_buddy", {
      question: "needs_you",
    });
    const answer = await within(chat()).findByRole("list", {
      name: "3 things need you. Here's the list:",
    });
    expect(within(answer).getAllByRole("listitem")[0].textContent).toContain(
      "Round limit reached.",
    );
    expect(within(chat()).getByText("What needs me?")).toBeTruthy();
    await user().click(
      within(answer).getByRole("button", { name: "Continue loop" }),
    );
    expect(invoke).toHaveBeenCalledWith("use_fix", {
      key: snapshot.buddyFeed!.notes.at(-1)!.lines![0].key,
    });
    // Once fixed, the item's buttons go.
    await waitFor(() =>
      expect(
        within(chat()).queryAllByRole("button", { name: "Continue loop" }),
      ).toHaveLength(0),
    );
    expect(
      within(chat()).getAllByRole("button", { name: "Sync now" }),
    ).toHaveLength(2);
    // Leave it to me: no buttons.
    snapshot.settings.buddy = {
      ...buddyOf(snapshot.settings),
      rules: { ...buddyOf(snapshot.settings).rules, sync_now: "leave_it" },
    };
    act(emit);
    expect(
      within(chat()).queryAllByRole("button", { name: "Sync now" }),
    ).toHaveLength(0);
  });

  it("answers a typed question and runs its buttons only when tapped", async () => {
    invoke.mockImplementation(async (cmd, args) => {
      const a = args as Record<string, unknown>;
      const notes = snapshot.buddyFeed!.notes;
      if (cmd === "ask_buddy_anything") {
        if (a.question !== "Can you sync less often?")
          throw Error(
            "Bit answers typed questions with Codex, which isn't ready: Codex is disconnected.",
          );
        notes.push({
          id: 200,
          at: new Date().toISOString(),
          kind: "answer",
          prId: null,
          reviewId: null,
          repo: null,
          question: a.question,
          text: "",
          pending: true,
        });
        emit();
        return;
      }
      if (cmd === "use_proposal") {
        if (a.proposalId === 1)
          throw Error("This loop isn't running any more.");
        const note = notes.find((n) => n.id === a.noteId)!;
        note.proposals![a.proposalId as number].done = true;
        emit();
        return;
      }
      return backend(cmd, args);
    });
    await mount();
    await user().click(screen.getByRole("button", { name: "Bit" }));
    const chat = () => screen.getByRole("log", { name: "Bit's notes" });
    const send = () => screen.getByRole("button", { name: "Send" });
    const box = screen.getByRole("textbox", { name: "Ask Bit anything" });
    await user().type(box, "Can you sync less often?{Enter}");
    expect(invoke).toHaveBeenCalledWith("ask_buddy_anything", {
      question: "Can you sync less often?",
    });
    expect(within(chat()).getByText("Can you sync less often?")).toBeTruthy();
    expect(await within(chat()).findByText("Thinking…")).toBeTruthy();
    expect(box.textContent).toBe("");
    // One question at a time.
    await user().type(box, "And the others?");
    expect(send()).toHaveProperty("disabled", true);

    // The answer's buttons wait for a tap.
    const note = snapshot.buddyFeed!.notes.find((n) => n.id === 200)!;
    Object.assign(note, {
      pending: false,
      text: "Sure. Tap below to sync **every ten minutes**.",
      proposals: [
        { id: 0, action: "change_settings", settings: { pollSeconds: 600 } },
        { id: 1, action: "stop_loop", prId: snapshot.prs[0].id },
      ],
      memory: [
        { repo: null, line: "You check in from your phone.", kind: "remember" },
        { repo: "acme/api", line: "Uses npm.", kind: "forget" },
        { repo: "acme/api", line: "Runs on Node 22.", kind: "full" },
      ],
    });
    act(emit);
    expect(within(chat()).queryByText("Thinking…")).toBeNull();
    for (const text of [
      "Remembered: You check in from your phone.",
      "Forgot (acme/api): Uses npm.",
      "No room to remember (acme/api): Runs on Node 22.",
    ])
      expect(within(chat()).getByText(text)).toBeTruthy();
    expect(within(chat()).getByText("every ten minutes").tagName).toBe(
      "STRONG",
    );
    expect(within(chat()).getByText("Poll interval: 600 seconds")).toBeTruthy();
    expect(invoke.mock.calls.some(([c]) => c === "use_proposal")).toBe(false);
    await user().click(
      within(chat()).getByRole("button", { name: "Change settings" }),
    );
    expect(invoke).toHaveBeenCalledWith("use_proposal", {
      noteId: 200,
      proposalId: 0,
    });
    await waitFor(() =>
      expect(
        within(chat()).getByRole("button", { name: "Change settings" }),
      ).toHaveProperty("disabled", true),
    );
    expect(within(chat()).getByText("Done")).toBeTruthy();
    // Orbit checks a button again when it's tapped.
    const short = snapshot.prs[0].id.split("/").pop();
    await user().click(
      within(chat()).getByRole("button", { name: `Stop loop: ${short}` }),
    );
    expect(
      await within(chat()).findByText(/This loop isn't running any more\./),
    ).toBeTruthy();

    // A question that isn't taken keeps its draft and says why.
    await user().click(send());
    expect(await screen.findByText(/which isn't ready/)).toBeTruthy();
    expect(box.textContent).toBe("And the others?");
  });

  it("signs in from Bit's setup help and opens pages where you are", async () => {
    snapshot.buddyFeed!.notes.push(
      {
        id: 300,
        at: new Date().toISOString(),
        kind: "answer",
        prId: null,
        reviewId: null,
        repo: null,
        question: "Help me set up",
        text: "Let's get you set up!",
        lines: [
          {
            text: "GitHub isn't signed in. Tap Sign in to GitHub and I'll give you a code to enter on its website.",
            prId: null,
            key: null,
          },
        ],
        proposals: [
          {
            id: 0,
            action: "sign_in",
            service: "github",
            label: "Sign in to GitHub",
          },
          {
            id: 1,
            action: "open_page",
            link: "https://github.com/openai/codex",
            label: "Get Codex CLI",
          },
        ],
      },
      {
        id: 301,
        at: new Date().toISOString(),
        kind: "sign_in",
        prId: null,
        reviewId: null,
        repo: null,
        text: "Let's sign you in to GitHub.",
        lines: [{ text: "Your code: `ABCD-1234`", prId: null, key: null }],
        proposals: [
          {
            id: 0,
            action: "open_page",
            link: "https://github.com/login/device",
            label: "Open github.com/login/device",
          },
        ],
      },
    );
    await mount();
    await user().click(screen.getByRole("button", { name: "Bit" }));
    const chat = () => screen.getByRole("log", { name: "Bit's notes" });
    await user().click(
      within(chat()).getByRole("button", { name: "Sign in to GitHub" }),
    );
    expect(invoke).toHaveBeenCalledWith("use_proposal", {
      noteId: 300,
      proposalId: 0,
    });
    // Pages open in the browser, without asking Orbit to do anything.
    await user().click(
      within(chat()).getByRole("button", { name: "Get Codex CLI" }),
    );
    expect(invoke).toHaveBeenCalledWith("open_github", {
      url: "https://github.com/openai/codex",
    });
    expect(within(chat()).getByText("ABCD-1234").tagName).toBe("CODE");
    await user().click(
      within(chat()).getByRole("button", {
        name: "Open github.com/login/device",
      }),
    );
    expect(invoke).toHaveBeenCalledWith("open_github", {
      url: "https://github.com/login/device",
    });
    expect(
      invoke.mock.calls.filter(([c]) => c === "use_proposal"),
    ).toHaveLength(1);
  });

  it("shows exactly how a button would set up a repository", async () => {
    snapshot.buddyFeed!.notes.push({
      id: 300,
      at: new Date().toISOString(),
      kind: "answer",
      prId: null,
      reviewId: null,
      repo: null,
      question: "Can you set up acme/web?",
      text: "Tap to connect it.",
      proposals: [
        {
          id: 0,
          action: "set_up_repository",
          repo: "acme/web",
          connect: true,
          repository: { setupCommand: "npm ci", networkAccess: true },
        },
        {
          id: 1,
          action: "set_up_repository",
          repo: "acme/api",
          repository: { roundLimit: 6 },
        },
      ],
    });
    await mount();
    await user().click(screen.getByRole("button", { name: "Bit" }));
    const chat = screen.getByRole("log", { name: "Bit's notes" });
    expect(
      within(chat).getByText(/^Orbit downloads its own copy of acme\/web/),
    ).toBeTruthy();
    expect(
      within(chat).getByText("Setup command: npm ci; Network access: On"),
    ).toBeTruthy();
    expect(
      within(chat).getByText("Automatic rounds per pull request: 6"),
    ).toBeTruthy();
    expect(invoke.mock.calls.some(([c]) => c === "use_proposal")).toBe(false);
    await user().click(
      within(chat).getByRole("button", { name: "Connect acme/web" }),
    );
    expect(invoke).toHaveBeenCalledWith("use_proposal", {
      noteId: 300,
      proposalId: 0,
    });
    expect(
      within(chat).getByRole("button", { name: "Set up acme/api" }),
    ).toBeTruthy();
  });

  it("reaches Bit from the menu on a phone-sized screen", async () => {
    const wide = window.matchMedia;
    window.matchMedia = vi.fn((media: string) => ({
      ...wide(media),
      matches: media.startsWith("(width <"),
    })) as typeof window.matchMedia;
    onTestFinished(() => {
      window.matchMedia = wide;
    });
    await mount();
    await user().click(screen.getByRole("button", { name: "Open navigation" }));
    await user().click(
      within(await screen.findByRole("dialog")).getByRole("button", {
        name: "Bit",
      }),
    );
    expect(
      await screen.findByRole("heading", { name: /^Bit is / }),
    ).toBeTruthy();
  });

  it("reads the morning brief in Bit's conversation and in Needs you, then marks it read", async () => {
    const brief = snapshot.brief!.latest!;
    brief.open = true;
    const item = {
      key: `brief:${brief.day}`,
      kind: "brief" as const,
      prId: null,
      reviewId: null,
      approvalId: null,
      repo: null,
      reason: brief.text,
    };
    snapshot.attention = [item, ...snapshot.attention!];
    invoke.mockImplementation(async (cmd: string, args) => {
      if (cmd !== "close_brief") return backend(cmd, args);
      snapshot.brief!.latest!.open = false;
      snapshot.attention = snapshot.attention!.filter(
        (a) => a.key !== item.key,
      );
      emit();
    });
    await mount();
    await user().click(screen.getByRole("button", { name: /^Needs you/ }));
    const needs = await screen.findByRole("list", { name: "Needs you" });
    const row = within(needs).getByText(brief.text).closest("li")!;
    expect(within(row).getAllByText("Morning brief")).toHaveLength(2);
    // Its row opens Bit's conversation, where the brief lists each thing,
    // with what's new marked.
    await user().click(within(row).getByText(brief.text));
    const chat = () => screen.getByRole("log", { name: "Bit's notes" });
    const lines = within(chat()).getByRole("list", { name: brief.text });
    const items = within(lines).getAllByRole("listitem");
    expect(items).toHaveLength(brief.lines.length);
    expect(within(lines).getAllByText("New")).toHaveLength(2);
    expect(within(items[1]).getByText("New")).toBeTruthy();
    expect(within(items[0]).queryByText("New")).toBeNull();
    expect(
      within(chat()).queryAllByRole("button", { name: "Open settings" }),
    ).toHaveLength(0);
    await user().click(within(chat()).getByRole("button", { name: "Got it" }));
    expect(invoke).toHaveBeenCalledWith("close_brief", { day: brief.day });
    await waitFor(() =>
      expect(
        within(chat()).queryByRole("button", { name: "Got it" }),
      ).toBeNull(),
    );
    // The brief stays in the conversation.
    expect(within(chat()).getByText(brief.text)).toBeTruthy();
  });

  it("marks the morning brief read from Needs you", async () => {
    const brief = snapshot.brief!.latest!;
    snapshot.attention = [
      {
        key: `brief:${brief.day}`,
        kind: "brief",
        prId: null,
        reviewId: null,
        approvalId: null,
        repo: null,
        reason: brief.text,
      },
    ];
    await mount();
    await user().click(screen.getByRole("button", { name: /^Needs you/ }));
    const needs = await screen.findByRole("list", { name: "Needs you" });
    await user().click(within(needs).getByRole("button", { name: "Got it" }));
    expect(invoke).toHaveBeenCalledWith("close_brief", { day: brief.day });
  });

  it("sends the morning brief at the time you choose, or not at all", async () => {
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Buddy");
    const brief = await screen.findByRole("switch", { name: /^Morning brief/ });
    expect((brief as HTMLInputElement).checked).toBe(true);
    const time = screen.getByRole("textbox", { name: /^Brief time/ });
    expect((time as HTMLInputElement).value).toBe("09:00");
    // Passing through the box without changing it saves nothing.
    await user().click(time);
    await user().tab();
    expect(invoke).not.toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { briefAt: "09:00" } },
    });
    await user().clear(time);
    await user().type(time, "07:30");
    await user().tab();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_settings", {
        patch: { buddy: { briefAt: "07:30" } },
      }),
    );
    expect(buddyOf(snapshot.settings).briefAt).toBe("07:30");
    await user().click(brief);
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { brief: false } },
    });
    await waitFor(() =>
      expect(screen.queryByRole("textbox", { name: /^Brief time/ })).toBeNull(),
    );
  });

  it("sends the morning brief in the time zone you choose", async () => {
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Buddy");
    const zone = await screen.findByRole("combobox", { name: /^Time zone/ });
    expect(screen.getByText(/^This computer/)).toBeTruthy();
    await user().click(zone);
    // A click puts the choice in the box to edit, a frame later: wait for
    // it, or it lands after the box is cleared and the search misses.
    await waitFor(() =>
      expect((zone as HTMLInputElement).value).toBe("This computer"),
    );
    await user().clear(zone);
    await user().type(zone, "tokyo");
    await user().click(
      await screen.findByRole("option", { name: "Asia/Tokyo" }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_settings", {
        patch: { buddy: { timeZone: "Asia/Tokyo" } },
      }),
    );
    expect(buddyOf(snapshot.settings).timeZone).toBe("Asia/Tokyo");
  });

  it("sets what Bit may do on its own in Settings", async () => {
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Buddy");
    await chooseOption(
      await screen.findByRole("combobox", { name: /^Publish report/ }),
      "Do it",
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { rules: { publish: "do_it" } } },
    });
    expect(buddyOf(snapshot.settings).rules).toMatchObject({
      publish: "do_it",
      continue_loop: "ask_first",
    });
  });

  it("chooses who answers Bit's questions in Settings", async () => {
    snapshot.claude = { ready: false, error: "Claude is not signed in." };
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Buddy");
    await chooseOption(
      await screen.findByRole("combobox", {
        name: /^Who answers your questions/,
      }),
      "Claude Code",
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { answerer: "claude" } },
    });
    expect(buddyOf(snapshot.settings).answerer).toBe("claude");
    expect(
      await screen.findByText(/^Claude Code isn't connected yet, so Bit can't/),
    ).toBeTruthy();
    await user().click(screen.getByRole("button", { name: "Bit" }));
    expect(
      await screen.findByText(/Bit answers with Claude Code/),
    ).toBeTruthy();
  });

  it("sets up Ollama to answer Bit's questions", async () => {
    snapshot.ollama = { ready: true };
    snapshot.ollamaModels = ["llama3.2:3b", "qwen3:8b"];
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Buddy");
    expect(
      screen.queryByRole("textbox", { name: /^Ollama address/ }),
    ).toBeNull();
    await chooseOption(
      await screen.findByRole("combobox", {
        name: /^Who answers your questions/,
      }),
      "Ollama",
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { answerer: "ollama" } },
    });
    const address = await screen.findByRole("textbox", {
      name: /^Ollama address/,
    });
    expect((address as HTMLInputElement).value).toBe("http://localhost:11434");
    await user().clear(address);
    await user().type(address, "http://jupiter:11434{Enter}");
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { ollamaUrl: "http://jupiter:11434" } },
    });
    await chooseOption(
      screen.getByRole("combobox", { name: /^Ollama model/ }),
      "qwen3:8b",
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { ollamaModel: "qwen3:8b" } },
    });
    expect(buddyOf(snapshot.settings)).toMatchObject({
      answerer: "ollama",
      ollamaUrl: "http://jupiter:11434",
      ollamaModel: "qwen3:8b",
    });
  });

  it("saves an API key for Bit and never keeps it in the page", async () => {
    const key = "sk-ant-api03-test-key";
    invoke.mockImplementation(async (cmd, args) => {
      if (cmd === "set_api_key") {
        const saved = (args as { key: string | null }).key !== null;
        snapshot.apiKeys = {
          openai: { saved: false, status: { ready: false }, models: [] },
          google: { saved: false, status: { ready: false }, models: [] },
          anthropic: saved
            ? {
                saved: true,
                status: { ready: true },
                models: ["claude-opus-5-5", "claude-sonnet-5-5"],
              }
            : { saved: false, status: { ready: false }, models: [] },
        };
        emit();
        return;
      }
      return backend(cmd, args);
    });
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Buddy");
    await chooseOption(
      await screen.findByRole("combobox", {
        name: /^Who answers your questions/,
      }),
      "Anthropic (API key)",
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { answerer: "anthropic" } },
    });
    const field = (await screen.findByLabelText(
      /^Anthropic API key/,
    )) as HTMLInputElement;
    expect(field.type).toBe("password");
    expect(
      screen.getByText("Anthropic's models show here once your key is saved."),
    ).toBeTruthy();
    await user().type(field, `${key}{Enter}`);
    expect(invoke).toHaveBeenCalledWith("set_api_key", {
      provider: "anthropic",
      key,
    });
    await waitFor(() => expect(field.value).toBe(""));
    expect(JSON.stringify(snapshot)).not.toContain(key);
    expect(
      screen.getByText("A key is saved. Paste a new one here to replace it."),
    ).toBeTruthy();
    await chooseOption(
      screen.getByRole("combobox", { name: /^Anthropic model/ }),
      "claude-sonnet-5-5",
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { anthropicModel: "claude-sonnet-5-5" } },
    });
    await user().click(screen.getByRole("button", { name: "Remove key" }));
    expect(invoke).toHaveBeenCalledWith("set_api_key", {
      provider: "anthropic",
      key: null,
    });
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Remove key" })).toBeNull(),
    );
  });

  it("shows what Bit remembers in Settings and forgets what you choose", async () => {
    invoke.mockImplementation(async (cmd, args) => {
      const memory = snapshot.buddyFeed!.memory!;
      if (cmd === "forget_memory") {
        const { repo, line } = args as { repo: string | null; line: string };
        const lines = repo ? memory.repositories[repo] : memory.general;
        lines.splice(lines.indexOf(line), 1);
        if (repo && !lines.length) delete memory.repositories[repo];
        emit();
        return;
      }
      if (cmd === "clear_memory") {
        snapshot.buddyFeed!.memory = { general: [], repositories: {} };
        emit();
        return;
      }
      return backend(cmd, args);
    });
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Buddy");
    const general = await screen.findByRole("list", {
      name: "Bit's memory: General",
    });
    expect(
      within(general).getByText(
        "You like short answers, with a button when there is one.",
      ),
    ).toBeTruthy();
    expect(screen.getByText("1 of 20")).toBeTruthy();
    expect(screen.getByText("1 of 10")).toBeTruthy();
    const line = "Its reviews often flag missing tests for new routes.";
    await user().click(
      within(
        screen.getByRole("list", { name: "Bit's memory: acme/api" }),
      ).getByRole("button", { name: `Forget “${line}”` }),
    );
    expect(invoke).toHaveBeenCalledWith("forget_memory", {
      repo: "acme/api",
      line,
    });
    await waitFor(() =>
      expect(
        screen.queryByRole("list", { name: "Bit's memory: acme/api" }),
      ).toBeNull(),
    );
    // Forgetting everything asks first.
    await user().click(
      screen.getByRole("button", { name: "Forget everything" }),
    );
    expect(invoke.mock.calls.some(([c]) => c === "clear_memory")).toBe(false);
    const confirm = await screen.findByRole("alertdialog");
    await user().click(
      within(confirm).getByRole("button", { name: "Forget everything" }),
    );
    expect(invoke).toHaveBeenCalledWith("clear_memory", {});
    expect(await screen.findByText("Nothing yet.")).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Forget everything" }),
    ).toBeNull();
  });

  it("renames, dresses and hides Bit from Settings, keeping a failed name", async () => {
    let fail = true;
    invoke.mockImplementation((cmd, args) =>
      cmd === "update_settings" &&
      (args as { patch: SettingsPatch }).patch.buddy?.name &&
      fail
        ? Promise.reject(Error("Keep your buddy's name to 24 characters."))
        : backend(cmd, args),
    );
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Buddy");
    const name = await screen.findByRole("textbox", { name: "Name" });
    await user().clear(name);
    await user().type(name, "Orbie{Enter}");
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { name: "Orbie" } },
    });
    await waitFor(() =>
      expect(document.body.textContent).toContain(
        "Keep your buddy's name to 24 characters.",
      ),
    );
    expect(name).toHaveProperty("value", "Orbie");
    fail = false;
    await user().click(screen.getByRole("button", { name: "Rename" }));
    expect(await screen.findByRole("button", { name: "Orbie" })).toBeTruthy();
    await user().click(screen.getByRole("checkbox", { name: "Crown" }));
    await user().click(screen.getByRole("checkbox", { name: "Gold" }));
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { buddy: { hat: "crown" } },
    });
    expect(snapshot.settings.buddy).toMatchObject({
      name: "Orbie",
      hat: "crown",
      colour: "gold",
    });
    expect(screen.getByRole("checkbox", { name: "Crown" })).toHaveProperty(
      "checked",
      true,
    );
    await user().click(screen.getByRole("switch", { name: /Show Orbie/ }));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Orbie" })).toBeNull(),
    );
    expect(snapshot.settings.buddy?.enabled).toBe(false);
  });
});

describe("files and navigation", () => {
  it("shows the inbox list and an opened pull request one at a time on a phone", async () => {
    const wide = window.matchMedia;
    window.matchMedia = vi.fn((media: string) => ({
      ...wide(media),
      matches: media.startsWith("(width <"),
    })) as typeof window.matchMedia;
    onTestFinished(() => {
      window.matchMedia = wide;
    });
    await mount();
    const pr = snapshot.prs[1];
    const title = new RegExp(pr.title);
    expect(
      screen.queryByRole("region", { name: "Pull request inbox" }),
    ).toBeNull();
    expect(screen.queryByRole("heading", { name: pr.title })).toBeNull();
    await user().click(screen.getByRole("button", { name: title }));
    expect(screen.getByRole("heading", { name: pr.title })).toBeTruthy();
    expect(screen.queryByPlaceholderText("Search pull requests…")).toBeNull();
    await user().click(
      screen.getByRole("button", { name: "All pull requests" }),
    );
    expect(screen.queryByRole("heading", { name: pr.title })).toBeNull();
    expect(screen.getByPlaceholderText("Search pull requests…")).toBeTruthy();
  });
  it("keeps the inbox and the opened pull request side by side on wider screens", async () => {
    await mount();
    const inbox = screen.getByRole("region", { name: "Pull request inbox" });
    expect(
      screen.getByRole("heading", { name: snapshot.prs[0].title }),
    ).toBeTruthy();
    expect(
      within(inbox).getByPlaceholderText("Search pull requests…"),
    ).toBeTruthy();
    // Only the menu's own item; the opened pull request has no back button.
    expect(
      screen.getAllByRole("button", { name: "All pull requests" }),
    ).toHaveLength(1);
  });
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
    expect(await screen.findByText("A review flow that fits.")).toBeTruthy();
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
    await openSettingsTab("Automation");
    const select = await screen.findByRole("combobox", {
      name: "Poll interval",
    });
    await chooseOption(select, "1 minute");
    expect(await findBanner()).toHaveProperty(
      "textContent",
      expect.stringContaining("Disk is full"),
    );
    expect(snapshot.settings.pollSeconds).toBe(120);
    await user().type(
      screen.getByRole("textbox", { name: "Add a trusted account" }),
      "half-typed",
    );
    await user().click(screen.getByRole("button", { name: "Review loops" }));
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(
      await screen.findByRole("combobox", { name: "Poll interval" }),
    ).toHaveProperty("textContent", "2 minutes");
    expect(banner().textContent).toContain(
      "Not applied — Poll interval: 60 seconds",
    );
    expect(
      screen.getByRole("textbox", { name: "Add a trusted account" }),
    ).toHaveProperty("value", "half-typed");
    expect(banner().textContent).toContain("Disk is full");
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
    const composer = await screen.findByRole("textbox", {
      name: "Message Codex",
    });
    await user().type(composer, "Keep this question");
    await act(async () => {
      snapshot.codexReady = false;
      emit();
    });
    expect(screen.getByText(/Codex is disconnected. Reconnect/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Send" })).toHaveProperty(
      "disabled",
      true,
    );
    fireEvent.keyDown(composer, { key: "Enter" });
    expect(invoke.mock.calls.some(([c]) => c === "send_message")).toBe(false);
    await user().click(screen.getByRole("button", { name: "Review loops" }));
    await user().click(screen.getByRole("button", { name: /^All reviews/ }));
    await user().click(
      screen.getAllByRole("button", {
        name: new RegExp(snapshot.prs[0].title),
      })[0],
    );
    expect(
      await screen.findByRole("textbox", { name: "Message Codex" }),
    ).toHaveProperty("textContent", "Keep this question");
  });
});

describe("initial loading and repository errors", () => {
  it("focuses search when a shortcut arrives before the initial snapshot", async () => {
    const loading = deferred<Snapshot>();
    invoke.mockImplementation((cmd) =>
      cmd === "get_snapshot" ? loading.promise : Promise.resolve(),
    );
    render(
      <Theme theme={neutralTheme} mode="dark">
        <App />
      </Theme>,
    );
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
      await screen.findByRole("button", { name: "Configure acme/orbit" }),
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
  it("fills the checkout path from the folder picker", async () => {
    invoke.mockImplementation((cmd, args) =>
      cmd === "choose_checkout"
        ? Promise.resolve("/Users/jason/Projects/orbit")
        : backend(cmd, args),
    );
    await mount();
    await user().click(screen.getByRole("button", { name: "Repositories" }));
    await user().click(
      await screen.findByRole("button", { name: "Configure acme/orbit" }),
    );
    const dialog = screen.getByRole("dialog");
    await user().click(within(dialog).getByRole("button", { name: "Browse…" }));
    expect(invoke).toHaveBeenCalledWith("choose_checkout", {
      current: snapshot.settings.repositories[0].path,
    });
    expect(
      within(dialog).getByRole("textbox", { name: /Local checkout path/ }),
    ).toHaveProperty("value", "/Users/jason/Projects/orbit");
  });
  it("switches a repository with a setup command to Claude only once the command is cleared", async () => {
    snapshot.settings.repositories[0].setupCommand = "npm ci";
    await mount();
    await user().click(screen.getByRole("button", { name: "Repositories" }));
    await user().click(
      await screen.findByRole("button", { name: "Configure acme/orbit" }),
    );
    const dialog = screen.getByRole("dialog");
    await chooseOption(
      within(dialog).getByRole("combobox", { name: "Reviewer" }),
      "Claude",
    );
    // Codex-only options go; the setup command stays, flagged, until cleared.
    expect(
      within(dialog).queryByRole("switch", {
        name: "Allow sandbox network access",
      }),
    ).toBeNull();
    expect(
      within(dialog).getByText(
        "Claude reviews can only run a setup command in the dev container. Turn it on or clear the command to save.",
      ),
    ).toBeTruthy();
    await user().clear(
      within(dialog).getByRole("textbox", { name: /Environment setup/ }),
    );
    expect(
      within(dialog).queryByRole("textbox", { name: /Environment setup/ }),
    ).toBeNull();
    await user().click(
      within(dialog).getByRole("button", { name: "Save repository" }),
    );
    const saved = invoke.mock.calls.find(([cmd]) => cmd === "save_repository");
    expect(
      (saved?.[1] as { repository: { provider: string; setupCommand: string } })
        .repository,
    ).toMatchObject({ provider: "claude", setupCommand: "" });
    act(() => {
      Object.assign(snapshot.settings.repositories[0], {
        provider: "claude",
        setupCommand: "",
      });
      emit();
    });
    expect(
      await screen.findByText(
        "Reviewed by Claude · Sandboxed worktree · No network",
      ),
    ).toBeTruthy();
  });
  it("runs a Claude repository's setup and checks in its dev container", async () => {
    snapshot.settings.repositories[0].setupCommand = "npm ci";
    await mount();
    await user().click(screen.getByRole("button", { name: "Repositories" }));
    await user().click(
      await screen.findByRole("button", { name: "Configure acme/orbit" }),
    );
    const dialog = screen.getByRole("dialog");
    await chooseOption(
      within(dialog).getByRole("combobox", { name: "Reviewer" }),
      "Claude",
    );
    expect(
      within(dialog).queryByRole("textbox", { name: /Check command/ }),
    ).toBeNull();
    await user().click(
      within(dialog).getByRole("switch", {
        name: "Run setup and checks in the dev container",
      }),
    );
    // The setup command can stay, and the box gets its own network switch.
    expect(
      within(dialog).queryByText(/can only run a setup command/),
    ).toBeNull();
    const check = within(dialog).getByRole("textbox", {
      name: /Check command/,
    });
    await user().clear(check);
    await user().type(check, "npm test");
    await user().click(
      within(dialog).getByRole("switch", {
        name: "Allow network access in the dev container",
      }),
    );
    await user().click(
      within(dialog).getByRole("button", { name: "Save repository" }),
    );
    const saved = invoke.mock.calls.find(([cmd]) => cmd === "save_repository");
    expect((saved?.[1] as { repository: Repository }).repository).toMatchObject(
      {
        provider: "claude",
        setupCommand: "npm ci",
        networkAccess: true,
        devContainer: { enabled: true, checkCommand: "npm test" },
      },
    );
    act(() => {
      Object.assign(
        snapshot.settings.repositories[0],
        (saved?.[1] as { repository: Repository }).repository,
      );
      emit();
    });
    expect(
      await screen.findByText(
        "Reviewed by Claude · Sandboxed worktree · Setup and checks in dev container · Container network enabled",
      ),
    ).toBeTruthy();
  });
  it("flags dev container settings that can't be saved and forgets unsaved ones", async () => {
    snapshot.settings.repositories[0].setupCommand = "npm ci";
    await mount();
    await user().click(screen.getByRole("button", { name: "Repositories" }));
    const open = async () => {
      await user().click(
        await screen.findByRole("button", { name: "Configure acme/orbit" }),
      );
      return screen.getByRole("dialog");
    };
    let dialog = await open();
    await chooseOption(
      within(dialog).getByRole("combobox", { name: "Reviewer" }),
      "Claude",
    );
    const box = within(dialog).getByRole("switch", {
      name: "Run setup and checks in the dev container",
    });
    // Claude can keep the setup command only in the dev container.
    expect(
      within(dialog).getByText(
        /can only run a setup command in the dev container/,
      ),
    ).toBeTruthy();
    await user().click(box);
    expect(
      within(dialog).queryByText(/can only run a setup command/),
    ).toBeNull();
    // The box needs a setup or a check command.
    await user().clear(
      within(dialog).getByRole("textbox", { name: /Environment setup/ }),
    );
    const check = within(dialog).getByRole("textbox", {
      name: /Check command/,
    });
    await user().clear(check);
    expect(check.getAttribute("aria-invalid")).toBe("true");
    await user().click(check);
    expect(
      await within(dialog).findByText(
        "Add a setup or check command, or let the reviewer run commands, to save.",
      ),
    ).toBeTruthy();
    // Or the reviewer runs its own commands there, which is off by default.
    const tool = within(dialog).getByRole("switch", {
      name: "Let the reviewer run commands in the dev container",
    });
    expect(tool).toHaveProperty("checked", false);
    await user().click(tool);
    expect(check.getAttribute("aria-invalid")).not.toBe("true");
    await user().click(tool);
    await user().type(check, "npm test");
    expect(check.getAttribute("aria-invalid")).not.toBe("true");
    // Closing without saving keeps the saved settings.
    await user().keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(invoke).not.toHaveBeenCalledWith(
      "save_repository",
      expect.anything(),
    );
    dialog = await open();
    expect(
      within(dialog).getByRole("switch", {
        name: "Run setup and checks in the dev container",
      }),
    ).toHaveProperty("checked", false);
    expect(
      within(dialog).getByRole("textbox", { name: /Environment setup/ }),
    ).toHaveProperty("value", "npm ci");
    expect(
      within(dialog).queryByRole("textbox", { name: /Check command/ }),
    ).toBeNull();
  });

  it("shows whether Docker and the Dev Container CLI are ready", async () => {
    snapshot.devContainers = {
      ready: false,
      error: "Install Docker and start it to run checks in dev containers.",
    };
    snapshot.settings.repositories[0].devContainer = {
      enabled: true,
      checkCommand: "npm test",
    };
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    expect(
      await screen.findByText(
        "Install Docker and start it to run checks in dev containers.",
      ),
    ).toBeTruthy();
    expect(
      screen
        .getByLabelText("Dev container connection")
        .getAttribute("data-variant"),
    ).toBe("warning");
  });
  it("says why Claude isn't set up and locks its settings until it is", async () => {
    snapshot.claude = {
      ready: false,
      error:
        "Cannot start Claude. Install Claude Code, then run claude once to sign in.",
    };
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findAllByText("Not installed")).toBeTruthy();
    await openSettingsTab("Reviewers");
    expect(
      screen.getByText(/Claude isn’t set up: Cannot start Claude/),
    ).toBeTruthy();
    expect(
      screen.getByRole("combobox", { name: /^Claude model/ }),
    ).toHaveProperty("disabled", true);
    expect(
      screen.getByRole("combobox", { name: /^Claude effort/ }),
    ).toHaveProperty("disabled", true);
  });
  it("turns blocking findings off and locks the blocking priorities", async () => {
    await mount();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Automation");
    await user().click(
      screen.getByRole("switch", { name: /Block on findings/ }),
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { blockFindings: false },
    });
    expect(
      screen.getByRole("combobox", { name: /^Blocking priorities/ }),
    ).toHaveProperty("disabled", true);
  });
  it("chooses Antigravity as a repository's reviewer and its model in Settings", async () => {
    await mount();
    await user().click(screen.getByRole("button", { name: "Repositories" }));
    await user().click(
      await screen.findByRole("button", { name: "Configure acme/orbit" }),
    );
    const dialog = screen.getByRole("dialog");
    await chooseOption(
      within(dialog).getByRole("combobox", { name: "Reviewer" }),
      "Antigravity",
    );
    expect(
      within(dialog).getByText(
        /Antigravity follows Orbit's review instructions/,
      ),
    ).toBeTruthy();
    expect(
      within(dialog).queryByRole("switch", {
        name: "Allow sandbox network access",
      }),
    ).toBeNull();
    await user().click(
      within(dialog).getByRole("button", { name: "Save repository" }),
    );
    const saved = invoke.mock.calls.find(([cmd]) => cmd === "save_repository");
    expect(
      (saved?.[1] as { repository: { provider: string } }).repository,
    ).toMatchObject({ provider: "antigravity" });
    act(() => {
      snapshot.settings.repositories[0].provider = "antigravity";
      emit();
    });
    expect(
      await screen.findByText(
        "Reviewed by Antigravity · Sandboxed worktree · No network",
      ),
    ).toBeTruthy();
    await user().click(screen.getByRole("button", { name: "Settings" }));
    await openSettingsTab("Reviewers");
    expect(
      await screen.findByText(
        /Antigravity reviews are blocked: Choose an Antigravity model/,
      ),
    ).toBeTruthy();
    await chooseOption(
      screen.getByRole("combobox", { name: "Antigravity model" }),
      "Gemini 3.1 Pro (High)",
    );
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: {
        antigravityModel: "gemini-3.1-pro-high",
        antigravityEffort: "high",
      },
    });
    await waitFor(() =>
      expect(document.body.textContent).not.toContain(
        "Antigravity reviews are blocked",
      ),
    );
  });
  it("saves keeping a clone up to date and shows its latest update", async () => {
    await mount();
    await user().click(screen.getByRole("button", { name: "Repositories" }));
    await user().click(
      await screen.findByRole("button", { name: "Configure acme/orbit" }),
    );
    const dialog = screen.getByRole("dialog");
    await user().click(
      within(dialog).getByRole("switch", {
        name: "Keep this clone up to date",
      }),
    );
    await user().click(
      within(dialog).getByRole("button", { name: "Save repository" }),
    );
    const saved = invoke.mock.calls.find(([cmd]) => cmd === "save_repository");
    expect(
      (saved?.[1] as { repository: { keepUpdated: boolean } }).repository
        .keepUpdated,
    ).toBe(true);
    act(() => {
      snapshot.settings.repositories[0].keepUpdated = true;
      snapshot.cloneUpdates = [
        {
          repo: "acme/orbit",
          state: "skipped",
          message: "Not updated: main has uncommitted changes.",
          at: new Date().toISOString(),
        },
      ];
      emit();
    });
    expect(
      await screen.findByText("Not updated: main has uncommitted changes."),
    ).toBeTruthy();
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
    await openSettingsTab("Storage");
    expect(invoke.mock.calls.some(([c]) => c === "get_storage_usage")).toBe(
      false,
    );
    await user().click(screen.getByRole("button", { name: "Inspect storage" }));
    const row = await findWorktreeRow();
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
    const dialog = screen.getByRole("alertdialog", {
      name: "Remove selected worktrees",
    });
    expect(dialog.textContent).toContain("2.0 KiB");
    expect(dialog.textContent).toContain("no longer support follow-ups");
    await user().click(
      within(dialog).getByRole("button", { name: "Confirm removal" }),
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(banner().textContent).toContain("Directory is busy");
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
    const row = await allReviewsRow(snapshot.prs[0].title);
    const exportButton = within(row).getByRole("button", {
      name: "Export report",
    });
    await user().click(exportButton);
    expect(screen.queryByText(/Report exported/)).toBeNull();
    await user().click(exportButton);
    expect(await findBanner()).toHaveProperty(
      "textContent",
      expect.stringContaining("Export disk full"),
    );
    await user().click(exportButton);
    expect(
      await screen.findByText("Report exported: /chosen/report.md"),
    ).toHaveProperty("role", "status");
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
    await openSettingsTab("Automation");
    await user().click(
      screen.getByRole("switch", {
        name: "Automatically review new pull requests",
      }),
    );
    expect((await findBanner()).textContent).toContain(
      "Not applied — Automatic reviews: On",
    );
    for (const name of [
      "Automatic PR reviews",
      "Automatically review new pull requests",
    ]) {
      expect(
        (screen.getByRole("switch", { name }) as HTMLInputElement).checked,
      ).toBe(false);
    }
    await user().click(screen.getByRole("button", { name: "Review loops" }));
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(
      (
        screen.getByRole("switch", {
          name: "Automatically review new pull requests",
        }) as HTMLInputElement
      ).checked,
    ).toBe(false);
    expect(banner().textContent).toContain("Automatic reviews: On");
    fail = false;
    await user().click(screen.getByRole("button", { name: "Retry saving" }));
    for (const name of [
      "Automatic PR reviews",
      "Automatically review new pull requests",
    ]) {
      expect(
        (screen.getByRole("switch", { name }) as HTMLInputElement).checked,
      ).toBe(true);
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
    await openSettingsTab("Storage");
    await user().click(screen.getByRole("button", { name: "Inspect storage" }));
    await user().click(await findWorktreeRow());
    await user().click(
      screen.getByRole("button", { name: "Remove worktrees (1)" }),
    );
    await user().click(screen.getByRole("button", { name: "Confirm removal" }));
    expect((await findBanner()).textContent).toContain(
      "Completion record could not be saved",
    );
    expect(
      screen.getByText(/Worktree removed; report and chat retained/),
    ).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Remove worktrees (0)" }),
    ).toHaveProperty("disabled", true);
    const list = screen.queryByRole("group", { name: "Review worktrees" });
    expect(list && within(list).queryByRole("checkbox")).toBeNull();
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
    await openSettingsTab("Reviewers");
    await chooseOption(
      screen.getByRole("combobox", { name: /^Model/ }),
      "Codex · demo",
    );
    await findBanner();
    expect(
      screen.getByRole("combobox", { name: /^Reasoning effort/ }),
    ).toHaveProperty("disabled", true);
    await user().click(screen.getByRole("button", { name: "Review loops" }));
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(
      screen.getByRole("combobox", { name: /^Reasoning effort/ }),
    ).toHaveProperty("disabled", true);
    fail = false;
    await user().click(screen.getByRole("button", { name: "Retry saving" }));
    const effort = screen.getByRole("combobox", { name: /^Reasoning effort/ });
    await waitFor(() => expect(effort).toHaveProperty("disabled", false));
    await chooseOption(effort, "High");
    expect(snapshot.settings.effort).toBe("high");
    expect(effort).toHaveProperty("textContent", "High");
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
    await openSettingsTab("Storage");
    await user().click(screen.getByRole("button", { name: "Inspect storage" }));
    await user().click(await findWorktreeRow());
    await user().click(
      screen.getByRole("button", { name: "Remove worktrees (1)" }),
    );
    await user().click(screen.getByRole("button", { name: "Confirm removal" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByText(/keep using Orbit/).getAttribute("role")).toBe(
      "status",
    );
    await user().click(screen.getByRole("button", { name: "Review loops" }));
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    expect(screen.getByText(/Removing worktrees…/).getAttribute("role")).toBe(
      "status",
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
    expect(banner().textContent).toContain("Could not remove test worktree");
    expect(useOrbit.getState().cleanup.running).toBe(false);
  });
});

describe("deterministic model controls", () => {
  it("shows pending questions immediately and keeps failed questions and their draft across navigation", async () => {
    const sending = deferred<void>();
    const run = snapshot.reviews[0];
    invoke.mockImplementation((cmd, args) => {
      if (cmd !== "send_message") return backend(cmd, args);
      run.status = "reviewing";
      run.messages.push({
        id: "pending-question",
        role: "user",
        text: args.text,
        createdAt: new Date().toISOString(),
        delivery: "pending",
      });
      emit();
      return sending.promise;
    });
    await mount();
    await user().click(screen.getByRole("tab", { name: /Review chat/ }));
    await user().type(
      await screen.findByRole("textbox", { name: "Message Codex" }),
      "Question during cold resume",
    );
    await user().click(screen.getByRole("button", { name: "Send" }));
    expect(screen.getByText("Sending…")).toBeTruthy();
    expect(
      within(screen.getByRole("article", { name: "You" })).getByText(
        "Question during cold resume",
      ),
    ).toBeTruthy();
    expect(screen.getByRole("button", { name: "Send" })).toHaveProperty(
      "disabled",
      true,
    );
    await act(async () => {
      run.status = "ready";
      run.messages.at(-1)!.delivery = "failed";
      emit();
      sending.reject(Error("Resume verification failed"));
    });
    expect(screen.getByText("Send failed")).toBeTruthy();
    expect(
      await screen.findByRole("textbox", { name: "Message Codex" }),
    ).toHaveProperty("textContent", "Question during cold resume");
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    await user().click(screen.getByRole("tab", { name: /Review chat/ }));
    expect(screen.getByText("Send failed")).toBeTruthy();
    expect(
      await screen.findByRole("textbox", { name: "Message Codex" }),
    ).toHaveProperty("textContent", "Question during cold resume");
  });
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
    await openSettingsTab("Reviewers");
    const model = screen.getByRole("combobox", { name: /^Model/ });
    const effort = screen.getByRole("combobox", { name: /^Reasoning effort/ });
    expect(model).toHaveProperty("textContent", "Choose a model");
    expect(effort).toHaveProperty("textContent", "Choose an effort");
    await user().click(model);
    expect(screen.queryByRole("option", { name: "Choose a model" })).toBeNull();
    expect(screen.queryByRole("option", { name: "Codex default" })).toBeNull();
    expect(screen.queryByRole("option", { name: "Model default" })).toBeNull();
    await user().keyboard("{Escape}");
    await chooseOption(model, "Second model");
    expect(invoke).toHaveBeenCalledWith("update_settings", {
      patch: { model: "second-model", effort: "high" },
    });
    expect(effort).toHaveProperty("textContent", "High");
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
    await openSettingsTab("Reviewers");
    expect(screen.getByRole("combobox", { name: /^Model/ })).toHaveProperty(
      "textContent",
      "missing-model (unavailable)",
    );
    expect(screen.getByRole("combobox", { name: /^Model/ })).toHaveProperty(
      "disabled",
      true,
    );
    expect(
      screen.getByText(/Codex reviews are blocked: Model discovery failed/),
    ).toBeTruthy();
    await openSettingsTab("Automation");
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
    const composer = await screen.findByRole("textbox", {
      name: "Message Codex",
    });
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
    ).toHaveProperty("textContent", snapshot.models[0].displayName);
    await chooseOption(
      within(dialog).getByRole("combobox", { name: "Chat reasoning effort" }),
      "low",
    );
    await user().click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(
      invoke.mock.calls.some(([cmd]) => cmd === "set_legacy_chat_model"),
    ).toBe(false);
    act(() => {
      snapshot.settings.effort = "high";
      emit();
    });
    await user().click(
      screen.getByRole("button", { name: "Choose chat model" }),
    );
    dialog = screen.getByRole("dialog", { name: "Choose chat model" });
    expect(
      within(dialog).getByRole("combobox", { name: "Chat reasoning effort" }),
    ).toHaveProperty("textContent", "high");
    await chooseOption(
      within(dialog).getByRole("combobox", { name: "Chat reasoning effort" }),
      "low",
    );
    await user().click(
      within(dialog).getByRole("button", { name: "Save chat model" }),
    );
    expect((await within(dialog).findByRole("alert")).textContent).toContain(
      "disk full",
    );
    expect(composer).toHaveProperty("textContent", "Keep this question");
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
    expect(screen.getByRole("button", { name: "Send" })).toHaveProperty(
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
    const composer = await screen.findByRole("textbox", {
      name: "Message Codex",
    });
    await user().type(composer, "Explain this finding");
    await user().click(screen.getByRole("button", { name: "Send" }));
    expect((await findBanner()).textContent).toContain("did not confirm");
    expect(composer).toHaveProperty("textContent", "Explain this finding");
    expect(snapshot.reviews[0].status).toBe("posted");
    expect(snapshot.reviews[0].report).not.toBe("");
    expect(
      screen.getAllByRole("button", { name: /View comment on GitHub/ }).length,
    ).toBeGreaterThan(0);
  });
});

describe("Astryx dialog and composer integration", () => {
  it("returns focus to the publication opener after cancellation", async () => {
    await mount();
    const trigger = screen.getByRole("button", { name: "Publish review" });
    await user().click(trigger);
    await user().click(
      within(
        screen.getByRole("dialog", { name: "Publish review to GitHub" }),
      ).getByRole("button", { name: "Cancel" }),
    );
    expect(document.activeElement).toBe(trigger);
  });

  it("keeps publication non-dismissable until the request settles", async () => {
    const publishing = deferred<void>();
    invoke.mockImplementation((cmd, args) =>
      cmd === "publish_review" ? publishing.promise : backend(cmd, args),
    );
    await mount();
    await user().click(screen.getByRole("button", { name: "Publish review" }));
    await user().click(
      screen.getByRole("button", { name: "Publish to GitHub" }),
    );
    const dialog = screen.getByRole("alertdialog", {
      name: "Publish review to GitHub",
    });
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toHaveProperty("disabled", true);
    expect(within(dialog).queryByRole("button", { name: /Close/ })).toBeNull();
    fireEvent(
      dialog,
      new Event("cancel", { bubbles: false, cancelable: true }),
    );
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    fireEvent.keyDown(document, { key: "k", ctrlKey: true });
    expect(screen.getByRole("alertdialog")).toBe(dialog);
    expect(screen.queryByText("A review flow that fits.")).toBeNull();
    await act(async () => publishing.reject(Error("Try again later")));
    expect(await screen.findByRole("dialog")).toBe(dialog);
    expect(
      within(dialog).getByRole("button", { name: "Retry publication" }),
    ).toBeTruthy();
  });
  it("ignores navigation shortcuts in the Storage alert dialog and resumes after closing", async () => {
    invoke.mockImplementation((cmd, args) =>
      cmd === "get_storage_usage"
        ? Promise.resolve({
            stateBytes: 10,
            worktreeBytes: 20,
            unrecognizedBytes: 0,
            unrecognized: [],
            errors: [],
            worktrees: [
              { reviewId: snapshot.reviews[0].id, bytes: 20, removable: true },
            ],
          })
        : backend(cmd, args),
    );
    await mount();
    fireEvent.keyDown(document, { key: ",", ctrlKey: true });
    await openSettingsTab("Storage");
    await user().click(screen.getByRole("button", { name: "Inspect storage" }));
    await user().click(await findWorktreeRow());
    await user().click(
      screen.getByRole("button", { name: "Remove worktrees (1)" }),
    );
    const dialog = screen.getByRole("alertdialog", {
      name: "Remove selected worktrees",
    });
    fireEvent.keyDown(document, { key: "k", metaKey: true });
    expect(screen.getByRole("alertdialog")).toBe(dialog);
    expect(screen.queryByPlaceholderText("Search pull requests…")).toBeNull();
    await user().click(within(dialog).getByRole("button", { name: "Cancel" }));
    fireEvent.keyDown(document, { key: "k", metaKey: true });
    expect(document.activeElement).toBe(
      screen.getByPlaceholderText("Search pull requests…"),
    );
  });
  it("retains Enter-submitted drafts on failure and clears only after acceptance", async () => {
    let fail = true;
    invoke.mockImplementation((cmd, args) =>
      cmd === "send_message" && fail
        ? Promise.reject(Error("Cannot resume chat"))
        : backend(cmd, args),
    );
    await mount();
    await user().click(screen.getByRole("tab", { name: /Review chat/ }));
    const composer = await screen.findByRole("textbox", {
      name: "Message Codex",
    });
    await user().type(composer, "Keep on failure");
    fireEvent.keyDown(composer, { key: "Enter" });
    expect((await findBanner()).textContent).toContain("Cannot resume chat");
    expect(composer.textContent).toBe("Keep on failure");
    fail = false;
    fireEvent.keyDown(composer, { key: "Enter" });
    await waitFor(() => expect(composer.textContent).toBe(""));
    expect(
      invoke.mock.calls.filter(([cmd]) => cmd === "send_message"),
    ).toHaveLength(2);
  });
});
