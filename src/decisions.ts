// What the engine pieces together about each pull request for the Decisions
// page (`get_decisions`; see src-tauri/src/ledger), and the summaries the
// page shows. It is all read from GitHub, apart from the optional write-ups,
// which a writer makes from it.
import type { Category, PriorStatus } from "./types";

export type RoundVerdict = "clean" | "changes_requested" | "unknown";
export type FinalStatus =
  "fixed" | "still_open" | "declined_accepted" | "declined_disputed" | "open";
export type FlagKind =
  | "merged_with_open_findings"
  | "disputed_decline"
  | "repeat_blocking"
  | "no_review_before_merge"
  | "reviewer_could_not_check"
  | "likely_not_verified";

export interface Place {
  path?: string | null;
  startLine: number;
  endLine: number;
}
/** A finding as a round reported it. */
export interface LedgerFinding {
  id: string;
  shortId?: string | null;
  number?: number | null;
  title: string;
  priority: number;
  blocking?: boolean | null;
  category?: Category | null;
  locations: Place[];
  problem: string;
  failureScenario?: string | null;
  evidence?: string | null;
  confirmed?: boolean | null;
  suggestedFix?: string | null;
  doneWhen?: string | null;
  reviewerNotes?: string | null;
  confidence?: number | null;
  inline?: boolean | null;
}
/** What a round said about an earlier round's finding. */
export interface EarlierFinding {
  id: string;
  shortId?: string | null;
  title: string;
  priority?: number | null;
  status?: PriorStatus | null;
  note?: string | null;
  authorSaid?: {
    kind: "fixed" | "declined";
    commit?: string | null;
    reason?: string | null;
  } | null;
}
/** One review round, from Orbit's published review or Milo's. */
export interface LedgerRound {
  reviewId?: string | null;
  format: "orbit" | "milo";
  round?: number | null;
  headSha?: string | null;
  baseSha?: string | null;
  verdict: RoundVerdict;
  blocking?: number | null;
  blockingPriority?: number | null;
  reviewer?: string | null;
  summary?: string | null;
  findings: LedgerFinding[];
  earlierFindings: EarlierFinding[];
  notChecked: string[];
  completed?: boolean | null;
  commentId: number;
  url: string;
  createdAt: string;
}
/** A builder's reply saying a finding is fixed or declined. */
export interface LedgerClaim {
  commentId: number;
  url: string;
  createdAt: string;
  author: string;
  numbers: number[];
  kind: "fixed" | "declined";
  commit?: string | null;
  reason?: string | null;
  scope: "named" | "thread" | "all";
  threadFindingId?: string | null;
}
export interface Ruling {
  round: number;
  at: string;
  status: PriorStatus;
  note?: string | null;
}
/** Where one finding went, from the round that opened it to its end. */
export interface Trajectory {
  findingId: string;
  shortId?: string | null;
  openedRound: number;
  openedAt: string;
  priority: number;
  blocking?: boolean | null;
  category?: Category | null;
  title: string;
  claim?: LedgerClaim | null;
  rulings: Ruling[];
  finalStatus: FinalStatus;
  minutesToClaim?: number | null;
  /** Where it stood when the pull request merged, from the rounds up to
   * then. None when it didn't merge or the finding came after. */
  statusAtMerge?: FinalStatus | null;
  openAtMerge: boolean;
}
export interface Metrics {
  rounds: number;
  findings: number;
  blockingFindings: number;
  fixed: number;
  declined: number;
  disputed: number;
  openAtMerge: number;
  /** Minutes from opening to the first clean round. */
  minutesToClean?: number | null;
  minutesToMerge?: number | null;
  firstVerdict?: RoundVerdict | null;
  finalVerdict?: RoundVerdict | null;
  /** Merged soon after a clean last round, as the loop merges. Inferred. */
  mergedByLoop?: boolean | null;
  /** What the last round said it could not check. */
  verificationGaps: number;
  likelyNotVerified: number;
}
export interface Flag {
  kind: FlagKind;
  /** "act": someone should decide something; "look": good to know. */
  attention: "act" | "look";
  text: string;
  findingId?: string | null;
  round?: number | null;
  url?: string | null;
}
/** The pull request's description, by section. */
export interface Narrative {
  before?: string | null;
  after?: string | null;
  how?: string | null;
  decisions?: string | null;
  checks?: string | null;
  epic?: number | null;
  stage?: string | null;
  closes: number[];
  /** Every section in order, as [name, text]. */
  sections: [string, string][];
  /** What came before the first section. */
  lead?: string | null;
}
/** Everything Orbit works out about one pull request. */
export interface History {
  repo: string;
  number: number;
  title: string;
  url: string;
  state: string;
  merged: boolean;
  draft: boolean;
  author: string;
  createdAt: string;
  updatedAt: string;
  closedAt?: string | null;
  mergedAt?: string | null;
  headRef: string;
  baseRef: string;
  additions: number;
  deletions: number;
  changedFiles: number;
  fetchedAt: string;
  narrative: Narrative;
  rounds: LedgerRound[];
  claims: LedgerClaim[];
  trajectories: Trajectory[];
  metrics: Metrics;
  flags: Flag[];
  /** Present once write-ups are on and this one has been written up. */
  writeup?: WriteUp | null;
}
export type DecisionKind =
  | "intent"
  | "rule"
  | "tradeoff"
  | "interpretation"
  | "boundary"
  | "challenge"
  | "pushback"
  | "verification";
/** One decision a write-up records. */
export interface Decision {
  kind: DecisionKind;
  statement: string;
  detail?: string | null;
  madeBy: "builder" | "reviewer" | "both" | "human";
  /** The part of the app it is about. */
  area: string;
  /** A short excerpt Orbit found in the pull request. */
  quote?: string | null;
  /** Where to read it: the comment the quote is from, else the pull
   * request. */
  url: string;
  /** None when nobody needs to look. */
  attention?: "act" | "look" | null;
  attentionReason?: string | null;
  /** An earlier rule this one changes, in the writer's words. */
  supersedes?: string | null;
}
/** What Codex or Claude made of a merged or closed pull request. */
export interface WriteUp {
  repo: string;
  number: number;
  writer: string;
  model: string;
  writtenAt: string;
  /** What changed for the user, then how the review went. */
  summary: string;
  areas: string[];
  decisions: Decision[];
  /** Each finding in plain words, by finding. */
  findingNotes: { findingId: string; plain: string }[];
}
export interface Decisions {
  prs: History[];
  /** Stored files that couldn't be read. */
  problems: string[];
}

export const historyKey = (h: History) => `${h.repo}#${h.number}`;
/** `orbit#41`. */
export const shortRef = (h: History) =>
  `${h.repo.split("/")[1] ?? h.repo}#${h.number}`;

export const verdictLabels: Record<RoundVerdict, string> = {
  clean: "Clean",
  changes_requested: "Changes requested",
  unknown: "No verdict",
};
export const verdictColors: Record<RoundVerdict, "green" | "orange" | "gray"> =
  { clean: "green", changes_requested: "orange", unknown: "gray" };
export const finalStatusLabels: Record<FinalStatus, string> = {
  fixed: "Fixed",
  still_open: "Still open",
  declined_accepted: "Declined (accepted)",
  declined_disputed: "Declined (disputed)",
  open: "No ruling",
};
export const finalStatusColors: Record<
  FinalStatus,
  "green" | "orange" | "blue" | "red" | "gray"
> = {
  fixed: "green",
  still_open: "orange",
  declined_accepted: "blue",
  declined_disputed: "red",
  open: "gray",
};
export const flagLabels: Record<FlagKind, string> = {
  merged_with_open_findings: "Merged with a finding open",
  disputed_decline: "Disputed decline",
  repeat_blocking: "Still open after two rounds",
  no_review_before_merge: "Merged before a review",
  reviewer_could_not_check: "Not checked",
  likely_not_verified: "Not verified",
};

/** The order a write-up's decisions are shown in, and what each kind is
 * called. */
export const decisionKinds: DecisionKind[] = [
  "intent",
  "rule",
  "tradeoff",
  "interpretation",
  "boundary",
  "challenge",
  "pushback",
  "verification",
];
export const decisionKindLabels: Record<DecisionKind, string> = {
  intent: "What it set out to do",
  rule: "Rules it set",
  tradeoff: "Trade-offs",
  interpretation: "How a request was read",
  boundary: "What was left out",
  challenge: "What the reviewer challenged",
  pushback: "Pushback",
  verification: "What was checked",
};
export const madeByLabels: Record<Decision["madeBy"], string> = {
  builder: "The builder",
  reviewer: "The reviewer",
  both: "Builder and reviewer",
  human: "A person",
};

/** Open, Merged, Closed or Draft. */
export function stateLabel(h: History) {
  if (h.merged) return "Merged";
  if (h.state === "open") return h.draft ? "Draft" : "Open";
  return "Closed";
}
export function stateColor(h: History): "purple" | "green" | "gray" {
  if (h.merged) return "purple";
  return h.state === "open" ? "green" : "gray";
}

/** "3 hours", "2 days", from minutes. */
export function duration(minutes: number) {
  if (minutes < 60) return `${Math.max(0, Math.round(minutes))} min`;
  const hours = minutes / 60;
  if (hours < 48) return `${Math.round(hours)} h`;
  return `${Math.round(hours / 24)} days`;
}

/** What each pull request set out to do, in a line: its After, else its lead. */
export function gist(h: History) {
  const n = h.narrative;
  const text = n.after ?? n.lead ?? n.before ?? "";
  const first =
    text
      .split(/\n\s*\n/)[0]
      ?.replace(/\s+/g, " ")
      .trim() ?? "";
  return first.length > 240 ? `${first.slice(0, 239).trimEnd()}…` : first;
}

/** Something worth a person's glance: a flag from the reviews, or a
 * decision the pull request's write-up marked. */
export interface WorthRow {
  pr: History;
  key: string;
  attention: "act" | "look";
  label: string;
  text: string;
  /** Why a person should look, from the write-up. */
  reason: string | null;
  fromWriteUp: boolean;
}
/** Every flag and marked decision, those that need a decision first, newest
 * pull request first within each. With an area, only its decisions. */
export function worthALook(prs: History[], area = "all"): WorthRow[] {
  const rows = prs.flatMap((pr): WorthRow[] => [
    ...pr.flags.map((flag, i) => ({
      pr,
      key: `${historyKey(pr)}-${flag.kind}-${flag.findingId ?? i}`,
      attention: flag.attention,
      label: flagLabels[flag.kind],
      text: flag.text,
      reason: null,
      fromWriteUp: false,
    })),
    ...(pr.writeup?.decisions ?? []).flatMap((d, i) =>
      d.attention && (area === "all" || sameArea(d.area, area))
        ? [
            {
              pr,
              key: `${historyKey(pr)}-writeup-${i}`,
              attention: d.attention,
              label: decisionKindLabels[d.kind],
              text: d.statement,
              reason: d.attentionReason ?? null,
              fromWriteUp: true,
            },
          ]
        : [],
    ),
  ]);
  const rank = (r: WorthRow) => (r.attention === "act" ? 0 : 1);
  return rows.sort(
    (a, b) => rank(a) - rank(b) || b.pr.updatedAt.localeCompare(a.pr.updatedAt),
  );
}

/** Areas are the same whatever their case. */
export const sameArea = (a: string, b: string) =>
  a.trim().toLowerCase() === b.trim().toLowerCase();
/** The pull requests whose write-up has a decision about an area. */
export function inArea(prs: History[], area: string) {
  return prs.filter((pr) =>
    pr.writeup?.decisions.some((d) => sameArea(d.area, area)),
  );
}
/** When a pull request's decisions took effect. */
const decidedAt = (pr: History) => pr.mergedAt ?? pr.updatedAt;

export interface RuleRow {
  pr: History;
  rule: Decision;
}
export interface RulebookArea {
  area: string;
  rules: RuleRow[];
}
/** Every rule the write-ups found, by area, the area with most rules first
 * and the oldest rule first within each. With an area, only its rules. */
export function rulebook(prs: History[], area = "all"): RulebookArea[] {
  const groups = new Map<string, RulebookArea>();
  for (const pr of prs)
    for (const rule of pr.writeup?.decisions ?? []) {
      if (rule.kind !== "rule") continue;
      if (area !== "all" && !sameArea(rule.area, area)) continue;
      const key = rule.area.trim().toLowerCase();
      const group = groups.get(key) ?? { area: rule.area, rules: [] };
      groups.set(key, group);
      group.rules.push({ pr, rule });
    }
  const out = [...groups.values()];
  for (const g of out)
    g.rules.sort((a, b) => decidedAt(a.pr).localeCompare(decidedAt(b.pr)));
  return out.sort(
    (a, b) => b.rules.length - a.rules.length || a.area.localeCompare(b.area),
  );
}

export interface AreaRow {
  name: string;
  repos: string[];
  prs: number;
  decisions: number;
  rules: number;
  challenges: number;
  pushbacks: number;
  /** Decisions marked for a person's glance. */
  attention: number;
  lastAt: string;
}
/** Each area the write-ups name, with what was decided there, the most
 * decided first. */
export function areas(prs: History[]): AreaRow[] {
  const rows = new Map<string, AreaRow>();
  const seen = new Set<string>();
  for (const pr of prs)
    for (const d of pr.writeup?.decisions ?? []) {
      const key = d.area.trim().toLowerCase();
      const row = rows.get(key) ?? {
        name: d.area,
        repos: [],
        prs: 0,
        decisions: 0,
        rules: 0,
        challenges: 0,
        pushbacks: 0,
        attention: 0,
        lastAt: "",
      };
      rows.set(key, row);
      if (!seen.has(`${key}\n${historyKey(pr)}`)) {
        seen.add(`${key}\n${historyKey(pr)}`);
        row.prs++;
        if (!row.repos.includes(pr.repo)) row.repos.push(pr.repo);
      }
      row.decisions++;
      if (d.kind === "rule") row.rules++;
      if (d.kind === "challenge") row.challenges++;
      if (d.kind === "pushback") row.pushbacks++;
      if (d.attention) row.attention++;
      if (decidedAt(pr) > row.lastAt) row.lastAt = decidedAt(pr);
    }
  return [...rows.values()].sort(
    (a, b) => b.decisions - a.decisions || a.name.localeCompare(b.name),
  );
}

export interface PushbackRow {
  pr: History;
  trajectory: Trajectory;
  /** What the builder gave as the reason. */
  reason: string | null;
  /** The reviewer's latest word on it. */
  ruling: Ruling | null;
}
/** Every finding the builder declined, newest first. */
export function pushback(prs: History[]): PushbackRow[] {
  return prs
    .flatMap((pr) =>
      pr.trajectories
        .filter(
          (t) =>
            t.claim?.kind === "declined" ||
            t.finalStatus === "declined_accepted" ||
            t.finalStatus === "declined_disputed",
        )
        .map((trajectory) => ({
          pr,
          trajectory,
          reason: trajectory.claim?.reason ?? null,
          ruling: trajectory.rulings.at(-1) ?? null,
        })),
    )
    .sort((a, b) =>
      (b.trajectory.claim?.createdAt ?? b.trajectory.openedAt).localeCompare(
        a.trajectory.claim?.createdAt ?? a.trajectory.openedAt,
      ),
    );
}

export interface Health {
  /** Pull requests with at least one review round. */
  reviewed: number;
  rounds: number;
  /** Reviewed pull requests whose first round was clean. */
  cleanFirst: number;
  findings: number;
  fixed: number;
  declined: number;
  disputed: number;
  /** Findings still open when their pull request merged. */
  openAtMerge: number;
  /** The middle time from opening to the first clean round, in minutes. */
  medianToClean: number | null;
  /** Reviewed pull requests by how many rounds they took: 1, 2, 3, 4, 5+. */
  roundsPerPr: { label: string; count: number }[];
  /** Findings by priority, P0 to P3, and where they ended up. */
  byPriority: {
    priority: number;
    total: number;
    counts: Record<FinalStatus, number>;
  }[];
}
export const finalStatuses: FinalStatus[] = [
  "fixed",
  "declined_accepted",
  "declined_disputed",
  "still_open",
  "open",
];
export function health(prs: History[]): Health {
  const reviewed = prs.filter((p) => p.rounds.length > 0);
  const findings = prs.flatMap((p) => p.trajectories);
  const count = (s: FinalStatus) =>
    findings.filter((t) => t.finalStatus === s).length;
  const toClean = reviewed
    .map((p) => p.metrics.minutesToClean)
    .filter((m): m is number => m != null)
    .sort((a, b) => a - b);
  const middle = toClean.length >> 1;
  const medianToClean = !toClean.length
    ? null
    : toClean.length % 2
      ? toClean[middle]
      : (toClean[middle - 1] + toClean[middle]) / 2;
  const buckets = ["1", "2", "3", "4", "5+"];
  return {
    reviewed: reviewed.length,
    rounds: reviewed.reduce((n, p) => n + p.rounds.length, 0),
    cleanFirst: reviewed.filter((p) => p.rounds[0]?.verdict === "clean").length,
    findings: findings.length,
    fixed: count("fixed"),
    declined: count("declined_accepted") + count("declined_disputed"),
    disputed: count("declined_disputed"),
    openAtMerge: findings.filter((t) => t.openAtMerge).length,
    medianToClean,
    roundsPerPr: buckets.map((label, i) => ({
      label,
      count: reviewed.filter((p) => Math.min(p.rounds.length, 5) === i + 1)
        .length,
    })),
    byPriority: [0, 1, 2, 3].map((priority) => {
      const mine = findings.filter((t) => t.priority === priority);
      const counts = Object.fromEntries(
        finalStatuses.map((s) => [
          s,
          mine.filter((t) => t.finalStatus === s).length,
        ]),
      ) as Record<FinalStatus, number>;
      return { priority, total: mine.length, counts };
    }),
  };
}

/** Pull requests matching a search across their title, number, repository,
 * description, finding titles and write-up. */
export function searchHistories(prs: History[], search: string) {
  const words = search.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return prs;
  return prs.filter((h) => {
    const text = [
      h.title,
      shortRef(h),
      h.repo,
      `#${h.number}`,
      h.narrative.before ?? "",
      h.narrative.after ?? "",
      h.narrative.lead ?? "",
      ...h.trajectories.map((t) => t.title),
      h.writeup?.summary ?? "",
      ...(h.writeup?.decisions ?? []).map((d) => d.statement),
    ]
      .join(" ")
      .toLowerCase();
    return words.every((w) => text.includes(w));
  });
}
