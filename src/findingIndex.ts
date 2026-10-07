import type {
  Finding,
  PriorStatus,
  PullRequest,
  Review,
  Snapshot,
} from "./types";

/** Where a finding stands now, worked out from the review loop. */
export type FindingStatus =
  "open" | "fixed" | "declined_accepted" | "declined_disputed" | "pr_closed";
export const findingStatusLabels: Record<FindingStatus, string> = {
  open: "Open",
  declined_disputed: "Declined (disputed)",
  fixed: "Fixed",
  declined_accepted: "Declined (accepted)",
  pr_closed: "PR closed",
};
export const findingStatusColors: Record<
  FindingStatus,
  "orange" | "red" | "green" | "gray"
> = {
  open: "orange",
  declined_disputed: "red",
  fixed: "green",
  declined_accepted: "gray",
  pr_closed: "gray",
};
/** One step in a finding's life: found, then each later round's decision. */
export interface FindingEvent {
  reviewId: string;
  round: number;
  at: string;
  /** Null for the round that found it. */
  status: PriorStatus | null;
  note: string;
}
/** Where a finding came from. Repository scans will add another source. */
export type FindingSource = "review";
export interface FindingRow {
  finding: Finding;
  source: FindingSource;
  /** The review that found it. */
  review: Review;
  pr?: PullRequest;
  prId: string;
  repo: string;
  /** Whether the PR is still in the inbox. */
  prOpen: boolean;
  /** The round, counted among the PR's structured reviews, oldest first. */
  round: number;
  status: FindingStatus;
  /** The reviewer's note with the latest decision, if any. */
  note: string;
  history: FindingEvent[];
}

export function repoOfPr(prId: string) {
  const i = prId.lastIndexOf("#");
  return i < 0 ? prId : prId.slice(0, i);
}

/** A review whose findings count: structured, and a real round of its PR. */
function counted(r: Review) {
  return r.isReview !== false && !!r.findings && !r.superseded;
}

/** Every finding Orbit has collected, newest first, with its current status.
 * A finding carried into later rounds keeps the ID of the review that first
 * found it, so each later round's decision about it is matched by that ID. */
export function collectFindings(s: Snapshot): FindingRow[] {
  const known = new Map(
    [...(s.historyPrs ?? []), ...s.prs].map((p) => [p.id, p]),
  );
  const open = new Set(s.prs.map((p) => p.id));
  const byPr = new Map<string, Review[]>();
  for (const r of s.reviews.filter(counted)) {
    byPr.set(r.prId, [...(byPr.get(r.prId) ?? []), r]);
  }
  const rows: FindingRow[] = [];
  for (const [prId, reviews] of byPr) {
    reviews.sort((a, b) => a.createdAt.localeCompare(b.createdAt));
    const pr = known.get(prId);
    const prOpen = open.has(prId);
    reviews.forEach((review, index) => {
      for (const finding of review.findings ?? []) {
        const history: FindingEvent[] = [
          {
            reviewId: review.id,
            round: index + 1,
            at: review.createdAt,
            status: null,
            note: "",
          },
        ];
        reviews.slice(index + 1).forEach((later, offset) => {
          const decision = later.priorFindings?.find(
            (p) => p.id === finding.id,
          );
          if (decision)
            history.push({
              reviewId: later.id,
              round: index + offset + 2,
              at: later.createdAt,
              status: decision.status,
              note: decision.note,
            });
        });
        const last = history[history.length - 1];
        let status: FindingStatus =
          !last.status || last.status === "still_open" ? "open" : last.status;
        if (status === "open" && !prOpen) status = "pr_closed";
        rows.push({
          finding,
          source: "review",
          review,
          pr,
          prId,
          repo: pr?.repo ?? repoOfPr(prId),
          prOpen,
          round: index + 1,
          status,
          note: last.note,
          history,
        });
      }
    });
  }
  return rows.sort(
    (a, b) =>
      b.review.createdAt.localeCompare(a.review.createdAt) ||
      a.finding.priority - b.finding.priority,
  );
}

/** The finding's lines on GitHub at the commit that was reviewed. */
export function findingFileUrl(row: FindingRow): string | null {
  const f = row.finding;
  if (!f.path || !row.review.headSha) return null;
  const path = f.path.split("/").map(encodeURIComponent).join("/");
  const lines =
    f.startLine === f.endLine
      ? `#L${f.startLine}`
      : `#L${f.startLine}-L${f.endLine}`;
  return `https://github.com/${row.repo}/blob/${row.review.headSha}/${path}${lines}`;
}

/** The inline comment the finding was published as, if it was. */
export function findingCommentUrl(row: FindingRow): string | null {
  const inline = row.review.inlineComments?.find(
    (c) => c.findingId === row.finding.id,
  );
  if (inline && row.pr) return `${row.pr.url}#discussion_r${inline.commentId}`;
  return row.review.commentUrl ?? null;
}
