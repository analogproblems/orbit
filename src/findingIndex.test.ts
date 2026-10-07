import { describe, expect, it } from "vitest";
import {
  collectFindings,
  findingCommentUrl,
  findingFileUrl,
} from "./findingIndex";
import { freshDemo } from "./demo";
import type { Finding, PriorFinding, Review, Snapshot } from "./types";

const finding = (id: string, priority = 1): Finding => ({
  id,
  title: `Finding ${id}`,
  priority,
  path: "src/a b.ts",
  startLine: 10,
  endLine: 12,
  body: "Details",
  confidence: 0.9,
});
function snapshot(): Snapshot {
  const s = freshDemo();
  s.reviews = [];
  s.historyPrs = [];
  return s;
}
function round(
  s: Snapshot,
  id: string,
  minute: number,
  findings: Finding[],
  priorFindings: PriorFinding[] = [],
  changes: Partial<Review> = {},
): Review {
  const r: Review = {
    ...freshDemo().reviews[0],
    id,
    prId: s.prs[0].id,
    headSha: `${id}-sha`,
    createdAt: `2026-10-07T00:${String(minute).padStart(2, "0")}:00.000Z`,
    findings,
    priorFindings,
    inlineComments: [],
    superseded: false,
    isReview: true,
    ...changes,
  };
  s.reviews.push(r);
  return r;
}

describe("collecting findings", () => {
  it("follows each finding through later rounds", () => {
    const s = snapshot();
    round(s, "r1", 1, [finding("r1-0"), finding("r1-1"), finding("r1-2", 3)]);
    round(
      s,
      "r2",
      2,
      [finding("r2-0", 2)],
      [
        { id: "r1-0", status: "still_open", note: "Not yet" },
        { id: "r1-1", status: "fixed", note: "Fixed in abc" },
        { id: "r1-2", status: "declined_accepted", note: "Intended" },
      ],
    );
    round(
      s,
      "r3",
      3,
      [],
      [{ id: "r1-0", status: "declined_disputed", note: "Still wrong" }],
    );
    const rows = collectFindings(s);
    const by = (id: string) => rows.find((r) => r.finding.id === id)!;
    expect(rows).toHaveLength(4);
    // Newest round first.
    expect(rows[0].finding.id).toBe("r2-0");
    expect(by("r1-0").status).toBe("declined_disputed");
    expect(by("r1-0").note).toBe("Still wrong");
    expect(by("r1-0").history.map((h) => [h.round, h.status])).toEqual([
      [1, null],
      [2, "still_open"],
      [3, "declined_disputed"],
    ]);
    expect(by("r1-1").status).toBe("fixed");
    expect(by("r1-2").status).toBe("declined_accepted");
    expect(by("r2-0")).toMatchObject({ status: "open", round: 2 });
  });
  it("closes open findings once the PR leaves the inbox", () => {
    const s = snapshot();
    round(s, "r1", 1, [finding("r1-0"), finding("r1-1")]);
    round(s, "r2", 2, [], [{ id: "r1-1", status: "fixed", note: "" }]);
    s.historyPrs = [s.prs[0]];
    s.prs = s.prs.slice(1);
    const rows = collectFindings(s);
    expect(rows.find((r) => r.finding.id === "r1-0")!.status).toBe("pr_closed");
    expect(rows.find((r) => r.finding.id === "r1-1")!.status).toBe("fixed");
    expect(rows.every((r) => !r.prOpen && r.pr)).toBe(true);
  });
  it("leaves out superseded runs, chats and unstructured reviews", () => {
    const s = snapshot();
    round(s, "r1", 1, [finding("r1-0")], [], { superseded: true });
    round(s, "r2", 2, [finding("r2-0")], [], { isReview: false });
    round(s, "r3", 3, [], [], { findings: null });
    round(s, "r4", 4, [finding("r4-0")]);
    const rows = collectFindings(s);
    expect(rows.map((r) => [r.finding.id, r.round])).toEqual([["r4-0", 1]]);
  });
  it("links the lines at the reviewed commit and the published comment", () => {
    const s = snapshot();
    const r = round(s, "r1", 1, [finding("r1-0"), finding("r1-1")], [], {
      commentUrl: "https://github.com/acme/orbit/pull/1#pullrequestreview-9",
      inlineComments: [{ findingId: "r1-0", commentId: 77 }],
    });
    const [first, second] = collectFindings(s).sort((a, b) =>
      a.finding.id.localeCompare(b.finding.id),
    );
    expect(findingFileUrl(first)).toBe(
      `https://github.com/${s.prs[0].repo}/blob/${r.headSha}/src/a%20b.ts#L10-L12`,
    );
    expect(findingCommentUrl(first)).toBe(`${s.prs[0].url}#discussion_r77`);
    expect(findingCommentUrl(second)).toBe(r.commentUrl);
    first.finding = { ...first.finding, path: null };
    expect(findingFileUrl(first)).toBeNull();
  });
});
