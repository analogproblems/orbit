import { describe, it, expect } from "vitest";
import { freshDemo } from "./demo";
import reportLabelCases from "../test-fixtures/report-selection-labels.json";
import findingCategories from "../test-fixtures/finding-categories.json";
import { describeRepositoryPatch } from "./describePatch";
import {
  editedOver,
  withRepositoryPatch,
  type Repository,
  type RepositoryPatch,
} from "./types";
import {
  categoryLabels,
  claimSummary,
  countNote,
  isBlocking,
  notChecked,
  shortId,
  reviewDisplayStatus,
  followUpUnavailable,
  canStartReview,
  canPublish,
  publishTarget,
  reportSelectionLabel,
  antigravitySelection,
  claudeSelection,
  reviewerSelectionUnavailable,
  buddyOf,
  defaults,
  offeredFix,
  withPatch,
  type Attention,
  type Provider,
} from "./types";

describe("derived review eligibility", () => {
  it("keeps persisted outcomes while deriving current, outdated, saved, and published labels", () => {
    const s = freshDemo(),
      r = s.reviews[0],
      p = s.prs[0];
    expect(reviewDisplayStatus(s, r)).toBe("ready");
    for (const field of ["headSha", "baseSha"] as const) {
      const changed = {
        ...s,
        prs: s.prs.map((pr) => ({ ...pr, [field]: "changed" })),
      };
      expect(reviewDisplayStatus(changed, r)).toBe("outdated");
      expect(canPublish(r, changed.prs[0])).toBe(false);
      expect(reviewDisplayStatus(changed, { ...r, status: "posted" })).toBe(
        "posted",
      );
    }
    s.prs = s.prs.slice(1);
    s.historyPrs = [p];
    expect(reviewDisplayStatus(s, r)).toBe("saved");
    expect(canPublish(r, publishTarget(s, p))).toBe(false);
    s.settings.repositories = [];
    expect(reviewDisplayStatus(s, r)).toBe("ready");
    expect(reviewDisplayStatus(s, { ...r, cleanupPending: true })).toBe(
      "saved",
    );
    expect(reviewDisplayStatus(s, { ...r, report: " " })).toBe("saved");
    expect(r.status).toBe("ready");
  });
  it("explains each blocked send and permits new reviews only for connected open PRs", () => {
    const s = freshDemo(),
      r = s.reviews[0];
    expect(followUpUnavailable(s, r)).toBeNull();
    expect(followUpUnavailable(s, { ...r, status: "reviewing" })).toContain(
      "finish",
    );
    expect(followUpUnavailable(s, { ...r, status: "publishing" })).toContain(
      "publication",
    );
    expect(followUpUnavailable(s, { ...r, cleanupPending: true })).toContain(
      "removed",
    );
    expect(followUpUnavailable(s, { ...r, worktree: undefined })).toContain(
      "workspace removed",
    );
    expect(followUpUnavailable(s, { ...r, threadId: undefined })).toContain(
      "create its chat",
    );
    expect(followUpUnavailable({ ...s, codexReady: false }, r)).toContain(
      "disconnected",
    );
    expect(
      followUpUnavailable(
        {
          ...s,
          settings: { ...s.settings, concurrency: 1 },
          reviews: [r, { ...r, id: "busy", status: "reviewing" }],
        },
        r,
      ),
    ).toContain("slots");
    expect(
      followUpUnavailable(
        { ...s, reviews: [r, { ...r, id: "queued", status: "queued" }] },
        r,
      ),
    ).toBeNull();
    expect(canStartReview(s, r.prId)).toBe(true);
    expect(canStartReview({ ...s, codexReady: false }, r.prId)).toBe(false);
    expect(canStartReview({ ...s, prs: [] }, r.prId)).toBe(false);
    expect(
      canStartReview(
        { ...s, reviews: [{ ...r, status: "reviewing" }] },
        r.prId,
      ),
    ).toBe(false);
    expect(
      canStartReview(
        { ...s, settings: { ...s.settings, repositories: [] } },
        r.prId,
      ),
    ).toBe(false);
  });
});

describe("model eligibility and truthful metadata", () => {
  it("blocks unresolved workspace settings while leaving a valid historical chat independent", () => {
    const s = freshDemo(),
      r = s.reviews[0];
    s.settings.model = "missing";
    expect(canStartReview(s, r.prId)).toBe(false);
    expect(followUpUnavailable(s, r)).toBeNull();
    expect(followUpUnavailable(s, { ...r, model: "missing" })).toContain(
      "pinned",
    );
    expect(followUpUnavailable(s, { ...r, effort: "unsupported" })).toContain(
      "does not support",
    );
    expect(followUpUnavailable(s, { ...r, model: "" })).toContain(
      "Choose a chat model",
    );
    expect(followUpUnavailable(s, { ...r, effort: "" })).toContain(
      "Choose a chat model",
    );
    s.modelCatalogReady = false;
    expect(followUpUnavailable(s, r)).toContain("Model discovery");
    expect(canStartReview(s, r.prId)).toBe(false);
  });
});

it("labels reports using only original requested or confirmed report metadata", () => {
  const r = freshDemo().reviews[0];
  r.confirmedReviewSelection = undefined;
  r.legacyChatSelection = { model: "later-chat", effort: "low" };
  expect(reportSelectionLabel(r)).toContain("requested, unverified");
  expect(reportSelectionLabel(r)).not.toContain("later-chat");
  r.model = "";
  r.effort = "";
  expect(reportSelectionLabel(r)).toBe(
    "Model: not recorded · Effort: not recorded",
  );
  r.confirmedReviewSelection = { model: "report-model", effort: "high" };
  expect(reportSelectionLabel(r)).toBe(
    "Model: report-model (confirmed) · Effort: high (confirmed)",
  );
});

it.each(reportLabelCases)(
  "uses the shared UI and export format: $label",
  (testCase) => {
    expect(
      reportSelectionLabel({
        ...freshDemo().reviews[0],
        ...testCase,
        provider: testCase.provider as Provider | undefined,
        legacyChatSelection: { model: "later-chat", effort: "high" },
      }),
    ).toBe(testCase.label);
  },
);

describe("Antigravity reviewer", () => {
  const antigravityRepo = () => {
    const s = freshDemo();
    const pr = s.prs.find((p) => !s.reviews.some((r) => r.prId === p.id))!;
    s.settings.repositories = s.settings.repositories.map((r) =>
      r.name === pr.repo ? { ...r, provider: "antigravity" as const } : r,
    );
    return { s, pr };
  };
  it("needs a chosen model before an Antigravity review can start", () => {
    const { s, pr } = antigravityRepo();
    s.codexReady = false;
    expect(antigravitySelection(s)).toEqual({ model: "", effort: "high" });
    expect(reviewerSelectionUnavailable(s, "antigravity")).toBe(
      "Choose an Antigravity model in Settings.",
    );
    expect(canStartReview(s, pr.id)).toBe(false);
    s.settings.antigravityModel = "gemini-3.1-pro-high";
    expect(canStartReview(s, pr.id)).toBe(true);
    s.settings.antigravityModel = "gemini-gone";
    expect(reviewerSelectionUnavailable(s, "antigravity")).toMatch(
      /Antigravity model 'gemini-gone' is unavailable/,
    );
    s.settings.antigravityModel = "gemini-3.1-pro-high";
    s.antigravity = { ready: false, error: "Antigravity is not signed in." };
    expect(reviewerSelectionUnavailable(s, "antigravity")).toBe(
      "Antigravity is not signed in.",
    );
  });
  it("checks Antigravity follow-ups against Antigravity's models", () => {
    const s = freshDemo();
    const r = s.reviews.find((r) => r.threadId && r.worktree)!;
    Object.assign(r, {
      status: "ready",
      provider: "antigravity",
      model: "gemini-3.1-pro-high",
      effort: "high",
    });
    s.codexReady = false;
    expect(followUpUnavailable(s, r)).toBeNull();
    r.model = "claude-opus-5-5";
    expect(followUpUnavailable(s, r)).toMatch(/Antigravity model/);
  });
});

describe("Claude reviewer", () => {
  const claudeRepo = () => {
    const s = freshDemo();
    const pr = s.prs.find((p) => !s.reviews.some((r) => r.prId === p.id))!;
    s.settings.repositories = s.settings.repositories.map((r) =>
      r.name === pr.repo ? { ...r, provider: "claude" as const } : r,
    );
    return { s, pr };
  };
  it("starts reviews on a Claude repository without Codex", () => {
    const { s, pr } = claudeRepo();
    s.codexReady = false;
    expect(canStartReview(s, pr.id)).toBe(true);
    s.claude = { ready: false, error: "Claude is not signed in." };
    expect(canStartReview(s, pr.id)).toBe(false);
  });
  it("falls back to the default Claude model and its effort", () => {
    const s = freshDemo();
    expect(claudeSelection(s)).toEqual({
      model: "claude-opus-5-5",
      effort: "high",
    });
    s.settings.claudeModel = "claude-sonnet-5-5";
    s.settings.claudeEffort = "low";
    expect(claudeSelection(s)).toEqual({
      model: "claude-sonnet-5-5",
      effort: "low",
    });
  });
  it("checks Claude follow-ups against Claude's models", () => {
    const s = freshDemo();
    const r = s.reviews.find((r) => r.threadId && r.worktree)!;
    Object.assign(r, {
      status: "ready",
      provider: "claude",
      model: "claude-opus-5-5",
      effort: "high",
    });
    s.codexReady = false;
    expect(followUpUnavailable(s, r)).toBeNull();
    r.model = "demo-review-model";
    expect(followUpUnavailable(s, r)).toMatch(/Claude model/);
  });
});

describe("findings form", () => {
  it("knows the engine's categories", () => {
    expect(Object.keys(categoryLabels)).toEqual(findingCategories);
  });
  it("reads what wasn't checked from current and older reviews", () => {
    const r = freshDemo().reviews[0];
    expect(notChecked({ ...r, notChecked: ["a"], testsNotRun: ["b"] })).toEqual(
      ["a"],
    );
    expect(
      notChecked({ ...r, notChecked: undefined, testsNotRun: ["b"] }),
    ).toEqual(["b"]);
    expect(
      notChecked({ ...r, notChecked: undefined, testsNotRun: undefined }),
    ).toEqual([]);
  });
  it("names findings by short ID and says what blocks", () => {
    const r = freshDemo().reviews[0];
    const [first, second] = r.findings!;
    expect(shortId(first)).toBe("F1");
    expect(shortId({ ...first, number: null })).toBeNull();
    expect(isBlocking(first, { ...r, blockingPriority: 2 })).toBe(true);
    expect(isBlocking(first, { ...r, blockingPriority: 1 })).toBe(false);
    expect(isBlocking(first, { ...r, blockingPriority: undefined })).toBeNull();
    expect(countNote({ ...r, reportFindings: 2 })).toBeNull();
    expect(countNote({ ...r, reportFindings: 3, provider: "claude" })).toBe(
      "Claude’s report lists 3 findings, but Orbit’s list has 2, so some may have been left out. Check the report.",
    );
    // Earlier findings the reviewer still holds open count as listed.
    const held = {
      ...r,
      reportFindings: 3,
      priorFindings: [{ id: "x", status: "still_open" as const, note: "" }],
    };
    expect(countNote(held)).toBeNull();
    expect(countNote({ ...r, reportFindings: null })).toBeNull();
    expect(second.number).toBe(2);
  });
  it("says what the author said about an earlier finding", () => {
    const claim = {
      findingId: "r-0",
      author: "alex",
      createdAt: "",
      kind: "fixed" as const,
      commit: "abc1234def",
      commitCheck: null,
      reason: null,
    };
    expect(claimSummary(claim)).toBe("fixed in abc1234");
    expect(claimSummary({ ...claim, commit: null })).toBe("fixed");
    expect(
      claimSummary({ ...claim, kind: "declined", reason: "intended" }),
    ).toBe("declined: intended");
    expect(claimSummary({ ...claim, kind: "declined" })).toBe("declined");
  });
});

describe("Bit's fixes", () => {
  const item = (fix: Attention["fix"]): Attention => ({
    key: "loop:a/b#1:Round limit reached.",
    kind: "loop",
    prId: "a/b#1",
    reviewId: null,
    approvalId: null,
    repo: null,
    reason: "Round limit reached.",
    fix,
  });
  it("offers an open item's fix unless the rule leaves it to you", () => {
    const s = structuredClone(freshDemo());
    s.attention = [item("continue_loop")];
    expect(offeredFix(s, item("continue_loop").key)?.fix).toBe("continue_loop");
    expect(offeredFix(s, "gone")).toBeUndefined();
    expect(offeredFix(s, null)).toBeUndefined();
    s.settings = withPatch(s.settings, {
      buddy: { rules: { continue_loop: "leave_it" } },
    });
    expect(offeredFix(s, item("continue_loop").key)).toBeUndefined();
    // A settings change has no rule; it is always a button.
    s.attention = [{ ...item("turn_on_autopilot"), key: "autopilot-paused" }];
    expect(offeredFix(s, "autopilot-paused")?.fix).toBe("turn_on_autopilot");
    s.attention = [item(null)];
    expect(offeredFix(s, item(null).key)).toBeUndefined();
  });
  it("patches one rule at a time, as the engine does", () => {
    const settings = structuredClone(defaults);
    delete settings.buddy;
    const patched = withPatch(settings, {
      buddy: { rules: { publish: "do_it" } },
    });
    expect(buddyOf(patched).rules).toEqual({
      continue_loop: "ask_first",
      review_again: "ask_first",
      publish: "do_it",
      sync_now: "ask_first",
    });
    expect(
      buddyOf(withPatch(patched, { buddy: { name: "Orbie" } })).rules.publish,
    ).toBe("do_it");
  });
});

describe("repository changes from Bit", () => {
  it("applies only what is given and says it in words", () => {
    const repo: Repository = {
      name: "acme/api",
      path: "/work/api",
      automatic: true,
      setupCommand: "npm ci",
      networkAccess: true,
      roundLimit: 4,
    };
    const patch: RepositoryPatch = {
      checkCommand: "npm test",
      devContainer: true,
      reviewer: "claude",
      approvalMinutes: 0,
    };
    expect(withRepositoryPatch(repo, patch)).toEqual({
      ...repo,
      provider: "claude",
      approvalMinutes: 0,
      keepUpdated: undefined,
      devContainer: { enabled: true, checkCommand: "npm test" },
    });
    expect(describeRepositoryPatch(patch)).toBe(
      "Check command: npm test; Dev container: On; Reviewer: Claude; Approval time limit: Wait for me",
    );
    expect(
      describeRepositoryPatch({
        setupCommand: "",
        approvalMinutes: 15,
        keepCloneUpdated: false,
        autopilot: true,
        roundLimit: 6,
      }),
    ).toBe(
      "Setup command: None; Approval time limit: Decline after 15 minutes; Keep the copy up to date: Off; Automatic reviews: On; Automatic rounds per pull request: 6",
    );
  });
});

describe("editedOver", () => {
  it("keeps a change made elsewhere while you edited something else", () => {
    const opened = {
      setupCommand: "",
      roundLimit: 4,
      devContainer: { enabled: false, checkCommand: "" },
    };
    // Bit set a setup command and a check command while the dialog was open.
    const current = {
      setupCommand: "npm ci",
      roundLimit: 4,
      devContainer: { enabled: false, checkCommand: "npm test" },
    };
    const edited = {
      setupCommand: "",
      roundLimit: 6,
      devContainer: { enabled: true, checkCommand: "" },
    };
    expect(editedOver(current, opened, edited)).toEqual({
      setupCommand: "npm ci",
      roundLimit: 6,
      devContainer: { enabled: true, checkCommand: "npm test" },
    });
    // What you changed yourself wins.
    expect(
      editedOver(current, opened, { ...edited, setupCommand: "pnpm i" })
        .setupCommand,
    ).toBe("pnpm i");
  });
});
