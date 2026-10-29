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
import { Operations, answerText, branchSummary, whyText } from "./Operations";
import { freshDemo } from "./demo";
import { useOrbit } from "./store";
import {
  checkNames,
  type CiFailure,
  type OpsAnswer,
  type OpsThread,
  type RepoCi,
  type Snapshot,
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
    expect(items).toHaveLength(2);
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

  it("lists what Orbit posted, newest first", () => {
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
    const posted = screen.getByRole("list", { name: "What Orbit posted" });
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
  it("name checks as the engine does", () => {
    const checks = ["A", "B", "C", "D"].map((name) => ({ name, url: "" }));
    expect(checkNames([])).toBe("no checks");
    expect(checkNames(checks.slice(0, 1))).toBe("A");
    expect(checkNames(checks.slice(0, 2))).toBe("A and B");
    expect(checkNames(checks)).toBe("A, B and 2 more");
  });
});
