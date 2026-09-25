import { describe, it, expect } from "vitest";
import { freshDemo } from "./demo";
import {
  reviewDisplayStatus,
  followUpUnavailable,
  canStartReview,
  canPublish,
  publishTarget,
  reportSelectionLabel,
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
  expect(reportSelectionLabel(r)).toBe("report-model · high (confirmed)");
});
