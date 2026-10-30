// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import {
  Operations,
  answerText,
  asksToRerun,
  branchSummary,
  brokeText,
  noCiText,
  rerunText,
  runnersText,
  waitingText,
  whyText,
} from "./Operations";
import { freshDemo } from "./demo";
import { useOrbit } from "./store";
import {
  checkNames,
  type Broke,
  type CiFailure,
  type OpsAnswer,
  type OpsItem,
  type OpsRerun,
  type OpsThread,
  type Problem,
  type RepoCi,
  type RunnerPlace,
  type Snapshot,
  type WaitingRun,
} from "./types";

afterEach(cleanup);

function show(snapshot: Snapshot) {
  useOrbit.setState({ snapshot });
  const go = {
    onChoose: vi.fn(),
    onRepositories: vi.fn(),
    onSettings: vi.fn(),
    onApproval: vi.fn(),
    onPublish: vi.fn(),
    onOperations: vi.fn(),
  };
  render(<Operations {...go} />);
  return go;
}

describe("operations page", () => {
  it("lists open problems first, a default branch before a pull request", () => {
    const s = freshDemo();
    const go = show(s);
    const open = screen.getByRole("list", { name: "Open problems" });
    const items = within(open).getAllByRole("listitem");
    expect(items).toHaveLength(3);
    expect(items[0].textContent).toContain("acme/orbit: main");
    expect(items[0].textContent).toContain("Failing: CI / Rust tests.");
    expect(items[0].textContent).toContain("Still failing at 9f3c2a1.");
    expect(within(items[0]).getByLabelText("Needs you")).toBeTruthy();
    expect(items[1].textContent).toContain("Its author’s to fix");
    // A pull request's problem opens the pull request.
    fireEvent.click(within(items[1]).getByText(new RegExp(`^${s.prs[2].id}`)));
    expect(go.onChoose).toHaveBeenCalledWith(s.prs[2]);
  });

  it("shows each repository's CI, with what couldn't be read", () => {
    show(freshDemo());
    const repos = screen.getByRole("list", { name: "CI by repository" });
    const items = within(repos).getAllByRole("listitem");
    expect(items.map((i) => i.textContent)).toEqual([
      expect.stringContaining("Failing on main at 9f3c2a1: CI / Rust tests."),
      expect.stringContaining(
        "Passing on main at 4be81aa.1 pull request is failing.",
      ),
      expect.stringContaining(
        "Not read yet.Couldn’t read CI: Could not resolve",
      ),
      expect.stringContaining("No checks ran on main at e7a51c3."),
    ]);
  });

  it("keeps recently closed problems with why they closed", () => {
    show(freshDemo());
    const closed = screen.getByRole("list", { name: "Recently closed" });
    expect(closed.textContent).toContain("acme/api: main");
    expect(closed.textContent).toContain("Passed at 4be81aa.");
  });

  it("says all clear only once every check it follows was read", () => {
    const s = freshDemo();
    s.problems = s.problems!.filter((p) => p.closedAt);
    const notAll = (n: string) =>
      `No problems found so far, but Orbit couldn’t read every check on ${n}.`;
    show(s);
    // The demo can't read acme/dashboard.
    expect(screen.getByText(notAll("1 repository"))).toBeTruthy();
    cleanup();
    s.ci = s.ci!.filter((c) => c.repo !== "acme/dashboard");
    s.settings.repositories = s.settings.repositories.filter(
      (r) => r.name !== "acme/dashboard",
    );
    show(s);
    expect(screen.getByText("All clear. Nothing is failing.")).toBeTruthy();
    cleanup();

    // Checks left to read on a default branch, or on a pull request Orbit
    // follows, keep it from saying all clear.
    const [orbit, api] = s.ci!;
    orbit.head.incomplete = true;
    api.pulls[0].ci.incomplete = true;
    show(s);
    expect(screen.getByText(notAll("2 repositories"))).toBeTruthy();
    cleanup();
    orbit.head.incomplete = false;
    show(s);
    expect(screen.getByText(notAll("1 repository"))).toBeTruthy();
    cleanup();
    // A draft isn't followed, so its unread checks don't count.
    s.prs = s.prs.map((pr) =>
      pr.number === api.pulls[0].number && pr.repo === api.repo
        ? { ...pr, draft: true }
        : pr,
    );
    show(s);
    expect(screen.getByText("All clear. Nothing is failing.")).toBeTruthy();
  });

  it("offers to turn watching back on", () => {
    const updateSettings = vi.fn().mockResolvedValue({ ok: true });
    const s = freshDemo();
    s.settings.watchCi = false;
    useOrbit.setState({ updateSettings });
    show(s);
    fireEvent.click(screen.getByRole("button", { name: "Turn on" }));
    expect(updateSettings).toHaveBeenCalledWith({ watchCi: true });
    expect(screen.queryByRole("list", { name: "Open problems" })).toBeNull();
  });

  it("says when turning watching on didn't save", () => {
    const s = freshDemo();
    s.settings.watchCi = false;
    useOrbit.setState({
      saves: {
        watchCi: {
          state: "error",
          error: "Could not save local state.",
          command: "update_settings",
          args: { patch: { watchCi: true } },
        },
      },
    });
    show(s);
    expect(screen.getByText("Could not save local state.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Retry saving" })).toBeTruthy();
    useOrbit.setState({ saves: {} });
  });

  it("says where Orbit told each agent", () => {
    const s = freshDemo();
    show(s);
    const open = screen.getByRole("list", { name: "Open problems" });
    const [branch, pull] = within(open).getAllByRole("listitem");
    expect(branch.textContent).toContain(
      "Orbit told the repository’s agent in issue #212.",
    );
    expect(pull.textContent).toContain(
      "Orbit told the agent in a comment on the pull request.",
    );
    expect(
      within(branch).getByRole("button", { name: "Open on GitHub" }),
    ).toBeTruthy();
  });

  it("asks first with a button, or says why it posts nothing", () => {
    const action = vi.fn().mockResolvedValue({ ok: true });
    useOrbit.setState({ action });
    const s = freshDemo();
    const issue = s.ops![0];
    issue.url = "";
    issue.number = null;
    s.settings.buddy = {
      ...s.settings.buddy!,
      rules: { ...s.settings.buddy!.rules, open_issues: "ask_first" },
    };
    show(s);
    const row = () =>
      within(screen.getByRole("list", { name: "Open problems" })).getAllByRole(
        "listitem",
      )[0];
    expect(row().textContent).toContain(
      "Bit asks before opening an issue for the repository’s agent.",
    );
    fireEvent.click(within(row()).getByRole("button", { name: "Open issue" }));
    expect(action).toHaveBeenCalledWith("tell_agent", {
      key: "issue:demo-problem-1",
    });
    cleanup();
    s.settings.buddy.rules.open_issues = "leave_it";
    show(s);
    expect(row().textContent).toContain(
      "Your rule is Leave it to me, so Orbit doesn’t tell the agent.",
    );
    expect(within(row()).queryByRole("button")).toBeNull();
    cleanup();
    s.settings.buddy.rules.open_issues = "do_it";
    s.settings.buddy.enabled = false;
    show(s);
    expect(row().textContent).toContain(
      "Bit is off, so Orbit doesn’t tell the agent.",
    );
    cleanup();
    // A post that may have gone is looked for, whatever the rule says.
    s.settings.buddy.enabled = true;
    s.settings.buddy.rules.open_issues = "ask_first";
    issue.unsure = true;
    show(s);
    expect(row().textContent).toContain(
      "Orbit stopped before GitHub answered its post, so it looks on GitHub for it before posting again.",
    );
    expect(within(row()).queryByRole("button")).toBeNull();
    cleanup();
    s.settings.buddy.rules.open_issues = "do_it";
    issue.unsure = false;
    issue.error = "HTTP 502: Bad Gateway";
    show(s);
    expect(row().textContent).toContain(
      "Orbit couldn’t tell the agent yet: HTTP 502: Bad Gateway",
    );
    cleanup();
    issue.gone = true;
    issue.error =
      "Orbit's issue is gone from GitHub, so Orbit stopped updating it.";
    show(s);
    expect(row().textContent).toContain(
      "Orbit's issue is gone from GitHub, so Orbit stopped updating it.",
    );
  });

  it("lists a repository with no CI with its repository, with a tap to propose some", () => {
    const action = vi.fn().mockResolvedValue({ ok: true });
    useOrbit.setState({ action });
    const s = freshDemo();
    show(s);
    const docs = () =>
      within(screen.getByRole("list", { name: "CI by repository" }))
        .getAllByRole("listitem")
        .find((i) => i.textContent?.includes("acme/docs"))!;
    expect(docs().textContent).toContain(
      "No CI runs on its pull requests. None of its workflows on main runs on pull requests, at e7a51c3. Orbit can propose one built from package.json and package-lock.json.",
    );
    expect(docs().textContent).toContain(
      "Bit asks before opening an issue with a workflow for the repository’s agent.",
    );
    expect(within(docs()).getByLabelText("No CI")).toBeTruthy();
    fireEvent.click(within(docs()).getByRole("button", { name: "Propose CI" }));
    expect(action).toHaveBeenCalledWith("tell_agent", {
      key: "issue:demo-problem-5",
    });
    // Nothing fails, so it isn't an open problem.
    const open = screen.getByRole("list", { name: "Open problems" });
    expect(within(open).getAllByRole("listitem")).toHaveLength(3);
    expect(open.textContent).not.toContain("acme/docs");
    cleanup();
    // Once posted, it links the issue.
    const t = s.ops!.find((o) => o.key === "issue:demo-problem-5")!;
    t.url = "https://github.com/acme/docs/issues/7";
    t.number = 7;
    show(s);
    expect(docs().textContent).toContain(
      "Orbit proposed a workflow to the repository’s agent in issue #7.",
    );
    expect(within(docs()).queryByRole("button", { name: "Propose CI" })).toBe(
      null,
    );
    expect(
      within(docs()).getByRole("button", { name: "Open issue" }),
    ).toBeTruthy();
    cleanup();
    // Its own rule decides, not the rule for failing branches.
    t.url = "";
    t.number = null;
    s.settings.buddy = {
      ...s.settings.buddy!,
      rules: {
        ...s.settings.buddy!.rules,
        open_issues: "ask_first",
        propose_ci: "leave_it",
      },
    };
    show(s);
    expect(docs().textContent).toContain(
      "Your rule is Leave it to me, so Orbit doesn’t tell the agent.",
    );
    expect(within(docs()).queryByRole("button")).toBeNull();
    cleanup();
    // Once closed, it's gone from the row.
    s.problems!.find((p) => p.id === "demo-problem-5")!.closedAt =
      new Date().toISOString();
    show(s);
    expect(docs().textContent).not.toContain("No CI runs");
  });

  it("shows what each agent answered", () => {
    const s = freshDemo();
    show(s);
    const open = screen.getByRole("list", { name: "Open problems" });
    const [branch, pull] = within(open).getAllByRole("listitem");
    expect(branch.textContent).toContain(
      "I1 · CI / Rust tests: acme-bot is on it.",
    );
    expect(pull.textContent).toContain(
      `I1 · CI / Frontend: ${s.prs[2].author} says it’s fixed in ${s.prs[2].headSha.slice(0, 7)}, but it still fails there.`,
    );
  });

  it("shows why each job failed, with the end of its log", () => {
    const s = freshDemo();
    const [rust, frontend] = s.failures ?? [];
    s.failures = [
      rust,
      // Named as the problem names it.
      { ...frontend, check: "Something else", held: true, lines: ["hidden"] },
    ];
    show(s);
    const open = screen.getByRole("list", { name: "Open problems" });
    const [branch, pull] = within(open).getAllByRole("listitem");
    expect(branch.textContent).toContain(
      "CI / Rust tests failed at step “Run cargo test”.",
    );
    expect(branch.textContent).toContain(
      "test result: FAILED. 213 passed; 1 failed; 0 ignored",
    );
    expect(pull.textContent).toContain(
      "CI / Frontend: Orbit held back its log, as it looks like it holds a secret.",
    );
    expect(pull.textContent).not.toContain("hidden");
  });

  it("says when a pull request passed before its base moved on", () => {
    const s = freshDemo();
    show(s);
    const open = screen.getByRole("list", { name: "Open problems" });
    const behind = within(open).getAllByRole("listitem")[2];
    expect(behind.textContent).toContain(s.prs[4].id);
    expect(behind.textContent).toContain(
      "Its checks pass, but main moved on to 9f3c2a1 after they began. Its author’s to merge it in and let CI run again.",
    );
    expect(behind.textContent).toContain("main moved on again, to 9f3c2a1.");
    expect(behind.textContent).not.toContain("Failing:");
    expect(within(behind).getByLabelText("Author’s to fix")).toBeTruthy();
    expect(
      within(behind).getByRole("button", { name: "Open on GitHub" }),
    ).toBeTruthy();
    // An answer about it names it the same way.
    const moved = s.problems!.find((p) => p.kind === "base_moved")!;
    const t = s.ops!.find((t) => t.problemId === moved.id)!;
    const item = {
      ...t.items[0],
      answer: {
        kind: "taking",
        author: "morgan",
        at: "",
        url: "",
        replyId: 1,
        head: "",
      },
    } as OpsItem;
    expect(answerText(t, item)).toBe("I1 · main moved on: morgan is on it.");
  });

  it("names the pull request that broke a default branch", () => {
    show(freshDemo());
    const open = screen.getByRole("list", { name: "Open problems" });
    const [branch, pull] = within(open).getAllByRole("listitem");
    expect(branch.textContent).toContain(
      "It started failing with #208, merged as 7d41e09; 3b8e6f4 before it passed.",
    );
    expect(pull.textContent).not.toContain("It started failing");
  });

  it("lists what Orbit did on GitHub, newest first", () => {
    const s = freshDemo();
    s.journal!.push({
      ...s.journal![0],
      id: "late",
      action: "edit_comment",
      repo: "acme/api",
      number: 7,
      state: "failed",
      url: "",
      error: "HTTP 502: Bad Gateway",
    });
    show(s);
    const posted = screen.getByRole("list", {
      name: "What Orbit did on GitHub",
    });
    const items = within(posted).getAllByRole("listitem");
    expect(items[0].textContent).toContain(
      "Updated its comment on acme/api #7.HTTP 502: Bad Gateway",
    );
    expect(within(items[0]).getByLabelText("Didn’t go through")).toBeTruthy();
    expect(items[1].textContent).toContain(
      "Updated its issue on acme/orbit #212.",
    );
    expect(items.map((i) => i.textContent)).toContainEqual(
      expect.stringContaining("Closed its issue on acme/api #88."),
    );
    expect(items.map((i) => i.textContent)).toContainEqual(
      expect.stringContaining(
        `Reran a job on ${s.prs[2].id.replace("#", " #")}.`,
      ),
    );
  });

  it("says when a job looks like bad luck, and asks before a second rerun", () => {
    const action = vi.fn().mockResolvedValue({ ok: true });
    useOrbit.setState({ action });
    const s = freshDemo();
    show(s);
    const row = () =>
      within(screen.getByRole("list", { name: "Open problems" })).getAllByRole(
        "listitem",
      )[1];
    expect(row().textContent).toContain(
      "Looks like bad luck: The runner went away.",
    );
    expect(row().textContent).toContain(
      "It failed again after a rerun, so it’s likely real. Bit asks before rerunning it again.",
    );
    // The comment is posted, so both buttons show.
    expect(
      within(row()).getByRole("button", { name: "Open on GitHub" }),
    ).toBeTruthy();
    fireEvent.click(within(row()).getByRole("button", { name: "Rerun again" }));
    expect(action).toHaveBeenCalledWith("rerun_job", {
      key: `pr:${s.prs[2].id}`,
      number: 2,
    });
    // Do it reruns it without a button.
    cleanup();
    s.settings.buddy!.rules.rerun_again = "do_it";
    show(s);
    expect(
      within(row()).queryByRole("button", { name: "Rerun again" }),
    ).toBeNull();
    expect(row().textContent).toContain(
      "It failed again after a rerun, so it’s likely real.",
    );
    expect(row().textContent).not.toContain("asks before");
  });

  it("lists runs waiting for a runner as yours, with the run to open", () => {
    const open = vi.spyOn(window, "open").mockImplementation(() => null);
    const s = freshDemo();
    const since = new Date(Date.now() - 42 * 60000).toISOString();
    const run = (id: number, name: string): WaitingRun => ({
      id,
      name,
      url: `https://github.com/acme/api/actions/runs/${id}`,
      since,
    });
    s.problems!.push({
      id: "waits",
      key: "wait:acme/api",
      kind: "waiting",
      repo: "acme/api",
      branch: "",
      sha: "",
      sinceSha: "",
      failing: [],
      openedAt: since,
      updatedAt: since,
      timeline: [
        {
          at: since,
          text: "2 runs in acme/api have waited more than 30 minutes for a runner: Release.",
        },
      ],
      alerts: 1,
      waiting: [run(9120, "Release"), run(9121, "")],
    });
    show(s);
    const items = within(
      screen.getByRole("list", { name: "Open problems" }),
    ).getAllByRole("listitem");
    expect(items).toHaveLength(4);
    // With the default branch, before the pull requests.
    const row = items.find((i) => i.textContent?.includes("acme/api: runs"))!;
    expect(items.indexOf(row)).toBeLessThan(2);
    expect(row.textContent).toContain("acme/api: runs waiting for a runner");
    expect(within(row).getByLabelText("Needs you")).toBeTruthy();
    expect(row.textContent).toContain(
      "2 runs have waited more than 30 minutes for a runner: Release.",
    );
    expect(row.textContent).toContain("Release · queued");
    expect(row.textContent).toContain("A run · queued");
    expect(row.textContent).toContain(
      "Only a person can add or fix a runner, so Orbit doesn’t tell an agent.",
    );
    fireEvent.click(within(row).getByRole("button", { name: "Open run" }));
    expect(open).toHaveBeenCalledWith(
      "https://github.com/acme/api/actions/runs/9120",
      "_blank",
      "noopener,noreferrer",
    );
    open.mockRestore();
  });

  it("lists the self-hosted runners Orbit sees, and where it can't see them", () => {
    const s = freshDemo();
    show(s);
    const list = screen.getByRole("list", { name: "Self-hosted runners" });
    const rows = within(list)
      .getAllByRole("listitem")
      .map((i) => i.textContent);
    // Places with none aren't listed.
    expect(rows).toEqual([
      expect.stringContaining(
        "acme/api1 runner, offline.mac-mini-1 · macOS · offline · self-hosted, macOS, ARM64",
      ),
      expect.stringContaining(
        "acme/dashboardOrbit can't see the self-hosted runners of acme/dashboard: that needs admin rights on it.",
      ),
      expect.stringContaining(
        "acme · organization2 runners, 2 online, 1 busy.linux-big-1 · Linux · busy · self-hosted, Linux, X64linux-big-2 · Linux · free",
      ),
    ]);
    const api = within(list).getAllByRole("listitem")[0];
    expect(within(api).getByLabelText("Offline")).toBeTruthy();
    cleanup();
    // A read that failed says so.
    s.runners![0].error =
      "Orbit couldn't read the runners yet: Bad Gateway (HTTP 502). It tries again later.";
    show(s);
    expect(
      within(
        screen.getByRole("list", { name: "Self-hosted runners" }),
      ).getAllByRole("listitem")[0].textContent,
    ).toContain("acme/orbitOrbit couldn't read the runners yet: Bad Gateway");
    cleanup();
    // With none anywhere, one line says so.
    s.runners = s.runners!.map((p) => ({
      ...p,
      runners: [],
      total: 0,
      unseen: null,
      error: null,
    }));
    show(s);
    expect(screen.queryByRole("list", { name: "Self-hosted runners" })).toBe(
      null,
    );
    expect(
      screen.getByText(
        /Orbit sees no self-hosted runners on your repositories/,
      ),
    ).toBeTruthy();
    cleanup();
    // Nothing read yet, or only a person's account: no section.
    s.runners = [
      {
        name: "acme",
        kind: "person",
        readAt: new Date().toISOString(),
        runners: [],
        total: 0,
        waiting: [],
        tries: 0,
      },
    ];
    show(s);
    expect(screen.queryByText("Self-hosted runners")).toBe(null);
  });

  it("asks you to connect a repository first", () => {
    const s = freshDemo();
    s.settings.repositories = [];
    const go = show(s);
    fireEvent.click(screen.getByRole("button", { name: "Open repositories" }));
    expect(go.onRepositories).toHaveBeenCalled();
  });
});

describe("ci sentences", () => {
  it("say which runs wait for a runner, as the engine does", () => {
    const run = (name: string): WaitingRun => ({
      id: 1,
      name,
      url: "",
      since: "",
    });
    const text = (names: string[]) =>
      waitingText({ waiting: names.map(run) } as Problem);
    expect(text(["CI"])).toBe(
      "A run of CI has waited more than 30 minutes for a runner.",
    );
    expect(text([""])).toBe(
      "A run has waited more than 30 minutes for a runner.",
    );
    expect(text(["CI", "CI"])).toBe(
      "2 runs have waited more than 30 minutes for a runner: CI.",
    );
    expect(text(["CI", "", "Docs"])).toBe(
      "3 runs have waited more than 30 minutes for a runner: CI and Docs.",
    );
    expect(text(["A", "B", "C", "D"])).toBe(
      "4 runs have waited more than 30 minutes for a runner: A, B and 2 more.",
    );
    expect(text(["", ""])).toBe(
      "2 runs have waited more than 30 minutes for a runner.",
    );
  });

  it("say how a place's runners stand", () => {
    const runner = (online: boolean, busy = false) => ({
      name: "r",
      os: "",
      online,
      busy,
      labels: [],
    });
    const place = (
      runners: RunnerPlace["runners"],
      total = runners.length,
    ): RunnerPlace => ({
      name: "a/b",
      kind: "repository",
      readAt: "",
      runners,
      total,
      waiting: [],
      tries: 0,
    });
    expect(runnersText(place([runner(false)]))).toBe("1 runner, offline.");
    expect(runnersText(place([runner(false), runner(false)]))).toBe(
      "2 runners, all offline.",
    );
    expect(runnersText(place([runner(true), runner(false)]))).toBe(
      "2 runners, 1 online.",
    );
    expect(runnersText(place([runner(true, true), runner(true)]))).toBe(
      "2 runners, 2 online, 1 busy.",
    );
    expect(runnersText(place([runner(true)], 150))).toBe(
      "150 runners, 1 online. Orbit read the first 1.",
    );
  });

  const ci = (
    state: RepoCi["head"]["state"],
    sha = "a".repeat(40),
  ): RepoCi => ({
    repo: "a/b",
    branch: "main",
    head: { sha, state, failing: [], running: 0, passing: 0 },
    title: "",
    pulls: [],
    complete: true,
    checkedAt: "",
  });
  it("say where the default branch stands", () => {
    expect(branchSummary(ci("passing"))).toBe("Passing on main at aaaaaaa.");
    expect(branchSummary(ci("running"))).toBe(
      "Checks are running on main at aaaaaaa.",
    );
    expect(branchSummary(ci("none"))).toBe("No checks ran on main at aaaaaaa.");
    expect(branchSummary(ci("passing", ""))).toBe("No commits to check yet.");
    const unread = ci("running");
    unread.head.incomplete = true;
    expect(branchSummary(unread)).toBe(
      "Orbit couldn’t read every check on main at aaaaaaa, so it can’t say it passes.",
    );
  });
  it("say where a default branch broke", () => {
    const sha = (c: string) => c.repeat(40);
    const b = (more: Partial<Broke>): Broke => ({
      done: true,
      green: sha("1"),
      red: sha("2"),
      commits: [sha("2")],
      pulls: [42],
      direct: 0,
      pull: 42,
      tries: 0,
      ...more,
    });
    expect(brokeText(null)).toBeUndefined();
    expect(brokeText(b({}))).toBe(
      "It started failing with #42, merged as 2222222; 1111111 before it passed.",
    );
    expect(brokeText(b({ red: sha("3"), commits: [sha("3"), sha("2")] }))).toBe(
      "It started failing with #42, merged as 2 commits up to 3333333; 1111111 before them passed.",
    );
    expect(brokeText(b({ pull: null, pulls: [], direct: 1 }))).toBe(
      "It started failing at 2222222, pushed without a pull request; 1111111 before it passed.",
    );
    const two = { red: sha("3"), commits: [sha("3"), sha("2")], pull: null };
    expect(brokeText(b({ ...two, pulls: [43], direct: 1 }))).toBe(
      "It started failing in the 2 commits after 1111111, which passed, up to 3333333: from #43 and 1 pushed without a pull request.",
    );
    expect(brokeText(b({ ...two, pulls: [43, 42] }))).toBe(
      "It started failing in the 2 commits after 1111111, which passed, up to 3333333: from #43 and #42.",
    );
    expect(brokeText(b({ ...two, pulls: [], direct: 2 }))).toBe(
      "It started failing in the 2 commits after 1111111, which passed, up to 3333333: from 2 pushed without one.",
    );
    const waiting =
      "Orbit couldn’t read the branch’s history yet: Bad Gateway. It tries again later.";
    expect(brokeText(b({ done: false, commits: [], error: waiting }))).toBe(
      waiting,
    );
    expect(
      brokeText(b({ done: false, commits: [], error: null })),
    ).toBeUndefined();
  });
  it("say what an agent answered", () => {
    const t = { branch: "fix-it" } as OpsThread;
    const said = (answer: Partial<OpsAnswer>) =>
      answerText(t, {
        number: 2,
        check: "CI / Lint",
        url: "",
        failing: true,
        since: "",
        at: "",
        answer: {
          kind: "fixed",
          author: "agent",
          at: "",
          url: "",
          replyId: 1,
          head: "",
          ...answer,
        },
      });
    expect(said({ kind: "declined", text: "the runner is broken" })).toBe(
      "I2 · CI / Lint: agent declined it: the runner is broken",
    );
    expect(said({ kind: "question" })).toBe(
      "I2 · CI / Lint: agent needs a person.",
    );
    expect(said({})).toBe(
      "I2 · CI / Lint: agent says it’s fixed. Orbit is waiting to see it pass.",
    );
    const fixed = (check: OpsAnswer["check"]) =>
      said({ commit: "abc1234def", check });
    expect(fixed("waiting")).toBe(
      "I2 · CI / Lint: agent says it’s fixed in abc1234. Orbit is waiting for its checks there.",
    );
    expect(fixed("passes")).toContain("abc1234, and it passes there.");
    expect(fixed("not_on_branch")).toContain("abc1234, which isn’t on fix-it.");
    expect(fixed("not_found")).toContain("abc1234, which Orbit can’t find.");
    const proposal = answerText(t, {
      number: 1,
      kind: "no_ci",
      check: "main",
      url: "",
      failing: true,
      since: "",
      at: "",
      answer: {
        kind: "taking",
        author: "morgan",
        at: "",
        url: "",
        replyId: 1,
        head: "",
      },
    });
    expect(proposal).toBe("I1 · CI on pull requests: morgan is on it.");
    const added = (commit: string | null) =>
      answerText({ branch: "main" } as OpsThread, {
        number: 1,
        kind: "no_ci",
        check: "main",
        url: "",
        failing: true,
        since: "",
        at: "",
        answer: {
          kind: "fixed",
          author: "morgan",
          at: "",
          url: "",
          replyId: 1,
          head: "",
          commit,
          check: commit ? "waiting" : null,
        },
      });
    expect(added("abc1234def")).toBe(
      "I1 · CI on pull requests: morgan says it’s added in abc1234. Orbit is waiting to see a workflow that runs on pull requests on main.",
    );
    expect(added(null)).toBe(
      "I1 · CI on pull requests: morgan says it’s added. Orbit is waiting to see a workflow that runs on pull requests on main.",
    );
  });
  it("say what Orbit found where no CI runs on pull requests", () => {
    const p = {
      kind: "no_ci",
      repo: "a/b",
      branch: "main",
      sha: "a".repeat(40),
      found: { workflows: [], proposal: "", from: [] },
    } as unknown as Problem;
    expect(noCiText(p)).toBe(
      "No CI runs on its pull requests. It has no workflows on main, at aaaaaaa. Orbit can’t tell how it builds, so its issue only asks for one.",
    );
    p.found = {
      workflows: [".github/workflows/release.yml"],
      proposal: "name: CI\n",
      from: ["Cargo.toml"],
    };
    expect(noCiText(p)).toBe(
      "No CI runs on its pull requests. None of its workflows on main runs on pull requests, at aaaaaaa. Orbit can propose one built from Cargo.toml.",
    );
  });
  it("say why a job failed", () => {
    const f: CiFailure = {
      url: "",
      check: "CI / Lint",
      step: "",
      lines: [],
      held: false,
      readAt: "",
    };
    expect(whyText({ ...f, step: "Run npm run lint" })).toBe(
      "CI / Lint failed at step “Run npm run lint”.",
    );
    expect(whyText(f)).toBe("CI / Lint failed. Its log ends:");
    expect(
      whyText({ ...f, error: "GitHub no longer has this job’s log." }),
    ).toBe("CI / Lint: GitHub no longer has this job’s log.");
    expect(whyText({ ...f, held: true, step: "Run it" })).toBe(
      "CI / Lint: Orbit held back its log, as it looks like it holds a secret.",
    );
  });
  it("say what Orbit did about a job that looks like bad luck", () => {
    const s = freshDemo();
    const p = s.problems!.find((p) => p.kind === "pull_request")!;
    const job = (n: number) => `https://github.com/a/b/actions/runs/1/job/${n}`;
    const rerun = (url: string, more: Partial<OpsRerun> = {}): OpsRerun => ({
      url,
      reason: "The runner went away.",
      at: "",
      done: true,
      already: false,
      tries: 0,
      lasting: false,
      failedAgain: false,
      unsure: false,
      ...more,
    });
    const item = (reruns: OpsRerun[], flaky = true): OpsItem => ({
      number: 1,
      check: "CI / E2E",
      url: job(3),
      failing: true,
      since: "",
      at: "",
      why: {
        url: job(3),
        check: "CI / E2E",
        step: "",
        lines: [],
        held: false,
        readAt: "",
        flaky: flaky ? "The runner went away." : null,
      },
      reruns,
    });
    // Rerun on its own by default: nothing to ask, nothing to say yet.
    expect(asksToRerun(s, p, item([]))).toBe(false);
    expect(rerunText(s, p, item([]))).toBeUndefined();
    expect(rerunText(s, p, item([rerun(job(3))]))).toBe("Orbit reran it.");
    expect(rerunText(s, p, item([rerun(job(2)), rerun(job(3))]))).toBe(
      "Orbit reran it a second time.",
    );
    expect(rerunText(s, p, item([rerun(job(3), { already: true })]))).toBe(
      "It was already rerun on GitHub.",
    );
    expect(rerunText(s, p, item([rerun(job(2))], false))).toBe(
      "It failed again after a rerun, so it’s likely real.",
    );
    expect(rerunText(s, p, item([rerun(job(1)), rerun(job(2))]))).toBe(
      "It failed again after two reruns, so it’s likely real.",
    );
    expect(asksToRerun(s, p, item([rerun(job(1)), rerun(job(2))]))).toBe(false);
    const refused = "GitHub won’t rerun this job: Forbidden (HTTP 403).";
    const failed = item([
      rerun(job(3), { done: false, lasting: true, error: refused }),
    ]);
    expect(rerunText(s, p, failed)).toBe(refused);
    expect(asksToRerun(s, p, failed)).toBe(false);
    // A rerun cut off while being sent is looked for before anything else.
    const cut = item([
      rerun(job(2)),
      rerun(job(3), { done: false, unsure: true, error: "Not yet." }),
    ]);
    const checking =
      "Orbit is checking on GitHub whether its rerun went through.";
    expect(rerunText(s, p, cut)).toBe(checking);
    // Ask first, until you say yes.
    s.settings.buddy!.rules.rerun_flaky = "ask_first";
    expect(asksToRerun(s, p, item([]))).toBe(true);
    expect(rerunText(s, p, item([]))).toBe("Bit asks before rerunning it.");
    expect(asksToRerun(s, p, { ...item([]), rerunOk: job(3) })).toBe(false);
    s.settings.buddy!.rules.rerun_again = "ask_first";
    expect(asksToRerun(s, p, item([rerun(job(2))]))).toBe(true);
    expect(asksToRerun(s, p, cut)).toBe(false);
    expect(asksToRerun(s, p, item([], false))).toBe(false);
    s.settings.buddy!.enabled = false;
    expect(asksToRerun(s, p, item([]))).toBe(false);
  });
  it("name checks as the engine does", () => {
    const checks = ["A", "B", "C", "D"].map((name) => ({ name, url: "" }));
    expect(checkNames([])).toBe("no checks");
    expect(checkNames(checks.slice(0, 1))).toBe("A");
    expect(checkNames(checks.slice(0, 2))).toBe("A and B");
    expect(checkNames(checks)).toBe("A, B and 2 more");
  });
});
