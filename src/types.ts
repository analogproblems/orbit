export type Status =
  | "queued"
  | "preparing"
  | "reviewing"
  | "awaitingApproval"
  | "ready"
  | "publishing"
  | "posted"
  | "failed"
  | "cancelled";
export interface PullRequest {
  id: string;
  repo: string;
  number: number;
  title: string;
  author: string;
  body: string;
  url: string;
  headSha: string;
  baseSha: string;
  headRef: string;
  baseRef: string;
  updatedAt: string;
  draft: boolean;
  additions: number;
  deletions: number;
  changedFiles: number;
  reviewRequested: boolean;
  labels: string[];
  headRepo?: string;
  authorAssociation?: string;
  baseRepo?: string;
  /** When Orbit first saw the current head. */
  headSeenAt?: string;
}
export interface ModelSelection {
  model: string;
  effort: string;
}
export interface Message {
  id: string;
  role: "user" | "agent" | "system";
  confirmedSelection?: ModelSelection | null;
  delivery?: "pending" | "sent" | "failed" | "interrupted" | null;
  text: string;
  createdAt: string;
}
export interface Review {
  id: string;
  prId: string;
  headSha: string;
  baseSha: string;
  status: Status;
  model: string;
  effort: string;
  confirmedReviewSelection?: ModelSelection | null;
  legacyChatSelection?: ModelSelection | null;
  threadId?: string;
  turnId?: string;
  worktree?: string;
  createdAt: string;
  updatedAt: string;
  messages: Message[];
  report: string;
  error?: string;
  commentUrl?: string;
  automatic: boolean;
  isReview: boolean;
  sensitive?: string[];
  beforeFollowUp?: { status: Status; error?: string };
  cleanupPending?: boolean;
  cleanupError?: string;
  /** Null or absent: an unstructured run (not extracted yet, or it failed). */
  findings?: Finding[] | null;
  verdict?: Verdict | null;
  summary?: string | null;
  testsNotRun?: string[];
  /** Requests Orbit declined because no one approved them in time. */
  declinedApprovals?: string[];
  /** The findings-extraction turn is running. */
  extracting?: boolean;
  /** The earlier round a re-review started from; absent for a first review. */
  previousRound?: PreviousRound | null;
  /** What the reviewer decided about each finding in `previousRound`. */
  priorFindings?: PriorFinding[];
  /** Blocking findings this review left open; null without findings. */
  blocking?: number | null;
  /** Orbit's verdict from the blocking priority; `verdict` is the model's. */
  orbitVerdict?: Verdict | null;
  /** Replaced by a newer head; never published automatically. */
  superseded?: boolean;
  /** The GitHub PR review a structured run was published as. */
  reviewId?: number | null;
  /** The review's inline comments, one per finding placed on its lines. */
  inlineComments?: InlineComment[];
}
/** Where publication will put each finding of a structured run. */
export interface PublishPlan {
  placements: Placement[];
  /** The review's own text, as it will be posted. */
  body: string;
}
export interface Placement {
  findingId: string;
  /** On its lines in the diff; otherwise in the review text. */
  inline: boolean;
  /** Why it is not inline. */
  reason: string | null;
}
export interface InlineComment {
  findingId: string;
  commentId: number;
}
export type Verdict = "clean" | "changes_requested";
export interface PreviousRound {
  reviewId: string;
  headSha: string;
  verdict: Verdict | null;
  /** Each keeps the ID of the review that first reported it. */
  findings: Finding[];
  /** The latest classification of each earlier finding that has one. */
  decisions?: PriorFinding[];
  commentsIncluded: number;
  commentsOmitted: number;
}
export type PriorStatus =
  "fixed" | "still_open" | "declined_accepted" | "declined_disputed";
export interface PriorFinding {
  id: string;
  status: PriorStatus;
  note: string;
}
export const priorStatusLabels: Record<PriorStatus, string> = {
  fixed: "fixed",
  still_open: "still open",
  declined_accepted: "declined (accepted)",
  declined_disputed: "declined (disputed)",
};
/** The review that first reported a finding: its ID is `{reviewId}-{index}`. */
export function findingReviewId(f: Finding) {
  return f.id.slice(0, f.id.lastIndexOf("-"));
}
export interface Finding extends Record<string, unknown> {
  id: string;
  title: string;
  /** 0 (P0, most severe) to 3. */
  priority: number;
  /** Relative to the repository root; null when outside the worktree. */
  path: string | null;
  startLine: number;
  endLine: number;
  body: string;
  confidence: number;
}
export function findingLocation(f: Finding) {
  if (!f.path) return "Outside the repository";
  return f.startLine === f.endLine
    ? `${f.path}:${f.startLine}`
    : `${f.path}:${f.startLine}–${f.endLine}`;
}
export interface Repository {
  name: string;
  path: string;
  automatic: boolean;
  setupCommand: string;
  networkAccess: boolean;
  /** Automatic rounds per PR before the loop needs a person. */
  roundLimit: number;
  /** Minutes an approval on an automatic review may wait; 0 waits. */
  approvalMinutes?: number;
  /** Fast-forward the clone's branch to its upstream on each sync. */
  keepUpdated?: boolean;
}
export interface CloneUpdate {
  repo: string;
  state: "updated" | "current" | "skipped" | "failed";
  message: string;
  at: string;
}
export interface RepositorySummary {
  name: string;
  private: boolean;
  description: string;
  pushedAt: string;
}
export interface Settings {
  automatic: boolean;
  autoPost: boolean;
  pollSeconds: number;
  concurrency: number;
  model: string;
  effort: string;
  repositories: Repository[];
  trustedAuthors: string[];
  /** Findings at this priority or more severe (0 = P0) block a clean verdict. */
  blockingPriority: number;
  /** Queue a new head automatically only once it is a poll interval old. */
  settlePushes: boolean;
  /** Show each automatic round's result on its commit as `orbit/review`. */
  commitStatus: boolean;
  /** Show a desktop notification when something new needs you. */
  notify: boolean;
  /** Keep a security scan's bulky working files after its import. */
  keepScanArtifacts?: boolean;
}
export interface CommitStatus {
  state: "pending" | "success" | "failure" | "error";
  description: string;
  error?: string | null;
}
/** What Orbit knows of a commit's `orbit/review` status. */
export interface CommitCheck {
  repo: string;
  sha: string;
  prId: string;
  /** The last status GitHub accepted. */
  accepted?: CommitStatus | null;
  /** The last status GitHub refused, with why. */
  refused?: CommitStatus | null;
}
export type SettingsPatch = Partial<Omit<Settings, "repositories">>;
export type ActionResult<T> =
  { ok: true; data: T } | { ok: false; error: string };
export interface StorageUsage {
  stateBytes: number;
  worktreeBytes: number;
  unrecognizedBytes: number;
  unrecognized: { name: string; bytes: number; error?: string | null }[];
  worktrees: {
    reviewId: string;
    bytes: number;
    removable: boolean;
    error?: string;
  }[];
  errors: string[];
}
export interface CleanupResult {
  reviewId: string;
  error?: string;
  warning?: string;
}
export interface Model {
  id: string;
  model: string;
  displayName: string;
  defaultReasoningEffort: string;
  supportedReasoningEfforts: { reasoningEffort: string; description: string }[];
  isDefault: boolean;
}
export interface Approval {
  id: string | number;
  reviewId: string;
  method: string;
  params: Record<string, unknown>;
}
export type LoopStatus =
  "reviewing" | "waiting_for_author" | "clean" | "needs_you" | "stopped";
export interface LoopState {
  prId: string;
  status: LoopStatus;
  reason: string | null;
  rounds: number;
  roundLimit: number;
  /** The rounds' review IDs, oldest first. */
  roundIds?: string[];
  /** The PR left the inbox, so the loop is over. */
  closed?: boolean;
}
export interface LoopControl {
  prId: string;
  stopped: boolean;
  extraRounds: number;
}
export const loopStatusLabels: Record<LoopStatus, string> = {
  reviewing: "Reviewing",
  waiting_for_author: "Waiting for author",
  clean: "Clean",
  needs_you: "Needs you",
  stopped: "Stopped",
};
export type AttentionKind =
  "approval" | "held" | "failed" | "loop" | "autopilot_paused" | "sync_failing";
/** Something only the developer can resolve. */
export interface Attention {
  /** Stays the same while the item is open. */
  key: string;
  kind: AttentionKind;
  prId: string | null;
  reviewId: string | null;
  /** The request to answer, for an approval. */
  approvalId: string | number | null;
  repo: string | null;
  reason: string;
}
export const attentionLabels: Record<AttentionKind, string> = {
  approval: "Approval",
  held: "Publication held",
  failed: "Review failed",
  loop: "Loop",
  autopilot_paused: "Autopilot paused",
  sync_failing: "Sync failing",
};
export interface AttentionHandlers {
  onChoose: (p: PullRequest, r?: Review) => void;
  onRepositories: () => void;
  onSettings: () => void;
  onApproval: (reviewId: string, id: string | number) => void;
  onPublish: (r: Review) => void;
}
/** Where an item is resolved: its review, its PR, or a settings page. */
export function attentionAction(
  s: Snapshot,
  a: Attention,
  go: AttentionHandlers,
): (() => void) | undefined {
  if (a.kind === "sync_failing" && a.repo) return go.onRepositories;
  if (a.kind === "autopilot_paused" || a.kind === "sync_failing")
    return go.onSettings;
  const p = [...s.prs, ...(s.historyPrs ?? [])].find((p) => p.id === a.prId);
  const r = s.reviews.find((r) => r.id === a.reviewId);
  if (a.kind === "approval" && r && a.approvalId != null) {
    const id = a.approvalId;
    return () => {
      if (p) go.onChoose(p, r);
      go.onApproval(r.id, id);
    };
  }
  if (!p) return undefined;
  return () => go.onChoose(p, r);
}
/** The loop is over: clean, or its PR left the inbox. */
export function loopFinished(l: LoopState) {
  return !!l.closed || l.status === "clean";
}
/** Loops with at least one round, and the header's summary of them. */
export function loops(s: Snapshot) {
  return (s.loopStates ?? []).filter((l) => l.rounds > 0);
}
export function autopilotSummary(s: Snapshot) {
  const running = loops(s).filter(
    (l) => !loopFinished(l) && l.status !== "stopped",
  ).length;
  const needs = s.attention?.length ?? 0;
  return [
    "Autopilot",
    `${running} ${running === 1 ? "loop" : "loops"} running`,
    needs > 0 && `${needs} ${needs === 1 ? "needs" : "need"} you`,
  ]
    .filter(Boolean)
    .join(" · ");
}
export function loopState(s: Snapshot, prId: string) {
  return s.loopStates?.find((l) => l.prId === prId);
}
export interface Snapshot {
  loops?: LoopControl[];
  /** Derived by the app for display. */
  commitChecks?: CommitCheck[];
  loopStates?: LoopState[];
  /** Derived by the app for display. */
  attention?: Attention[];
  prs: PullRequest[];
  historyPrs?: PullRequest[];
  reviews: Review[];
  settings: Settings;
  models: Model[];
  approvals: Approval[];
  githubUser?: string;
  githubError?: string;
  codexError?: string;
  lastPoll?: string;
  polling: boolean;
  codexReady: boolean;
  modelCatalogReady: boolean;
  modelConfigurationError?: string | null;
  modelSelectionNotice?: string | null;
  /** The latest update of each clone kept up to date. */
  cloneUpdates?: CloneUpdate[];
  /** The Codex account's usage limits; absent when the account has none. */
  usage?: Usage | null;
  /** Repository security scans, newest first. */
  scans?: SecurityScan[];
}
/** How far a security scan has come, as the Codex Security plugin reports it. */
export interface ScanProgress {
  phase?: string | null;
  completed?: number | null;
  total?: number | null;
  /** What `completed` and `total` count, such as files or reviews. */
  unit?: string | null;
  /** Findings worth reporting so far. */
  findings?: number | null;
  updatedAt: string;
}
export type ScanMode = "standard" | "deep";
export type ScanStatus =
  | "preparing"
  | "running"
  /** Waiting for the Codex Security plugin to confirm its scan stopped. */
  | "cancelling"
  | "importing"
  | "completed"
  | "failed"
  | "cancelled"
  | "interrupted";
/** A whole-repository security scan with the Codex Security plugin. */
export interface SecurityScan {
  id: string;
  repo: string;
  mode: ScanMode;
  status: ScanStatus;
  createdAt: string;
  finishedAt?: string | null;
  /** The default branch and commit scanned, once fetched. */
  branch?: string | null;
  commit?: string | null;
  model: string;
  effort: string;
  threadId?: string | null;
  progress?: ScanProgress | null;
  /** The deep scan's Daybreak access check: granted, not_granted or unknown. */
  daybreak?: string | null;
  findings: Finding[];
  hasReport: boolean;
  /** Bytes the scan's kept files and leftover working files use. */
  diskBytes?: number | null;
  error?: string | null;
  warnings: string[];
  /** Codex's latest message in the scan thread. */
  lastMessage?: string | null;
}
export const scanActive = (s: SecurityScan) =>
  s.status === "preparing" ||
  s.status === "running" ||
  s.status === "cancelling" ||
  s.status === "importing";
/** Whether the Codex Security plugin can be used. */
export interface ScanPluginStatus {
  available: boolean;
  installed: boolean;
  version?: string | null;
}
/** One usage window, as Codex reports it. */
export interface UsageWindow {
  usedPercent: number;
  windowDurationMins?: number | null;
  /** Unix seconds. */
  resetsAt?: number | null;
}
export interface UsageLimit {
  limitId?: string | null;
  limitName?: string | null;
  /** The model this limit belongs to, for a limit tied to one model. */
  normalModelSlug?: string | null;
  /** The short window, such as 5 hours. */
  primary?: UsageWindow | null;
  /** The long window, usually a week. */
  secondary?: UsageWindow | null;
  credits?: {
    hasCredits: boolean;
    unlimited: boolean;
    balance?: string | null;
  } | null;
  planType?: string | null;
  rateLimitReachedType?: string | null;
}
export interface Usage {
  rateLimits: UsageLimit;
  otherLimits: UsageLimit[];
  updatedAt: string;
}
export interface ChangedFile {
  filename: string;
  status: string;
  additions: number;
  deletions: number;
  patch?: string;
}
export const activeStatuses: Status[] = [
  "queued",
  "preparing",
  "reviewing",
  "awaitingApproval",
];
export const statusLabels: Record<Status, string> = {
  queued: "Queued",
  preparing: "Preparing sandbox",
  reviewing: "Reviewing",
  awaitingApproval: "Needs approval",
  ready: "Ready to publish",
  publishing: "Publishing",
  posted: "Published",
  failed: "Failed",
  cancelled: "Stopped",
};
export const defaults: Settings = {
  automatic: false,
  autoPost: false,
  pollSeconds: 120,
  concurrency: 2,
  model: "",
  effort: "",
  repositories: [],
  trustedAuthors: [],
  blockingPriority: 1,
  settlePushes: true,
  commitStatus: true,
  notify: true,
  keepScanArtifacts: false,
};
export function latestReview(reviews: Review[], prId: string) {
  return reviews
    .filter((r) => r.prId === prId)
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt))[0];
}
// GitHub repository names ignore case.
export function sameRepo(a: string, b: string) {
  return a.toLowerCase() === b.toLowerCase();
}
// A PR that left the inbox may still be open if its repository was
// disconnected; one from a connected repository has closed.
export function publishTarget(s: Snapshot, pr: PullRequest) {
  if (s.prs.some((p) => p.id === pr.id)) return pr;
  const connected = s.settings.repositories.some((r) =>
    sameRepo(r.name, pr.repo),
  );
  return connected ? undefined : pr;
}
export function canPublish(review: Review, pr: PullRequest | undefined) {
  return (
    review.status === "ready" &&
    !review.beforeFollowUp &&
    !!review.report.trim() &&
    !review.cleanupPending &&
    review.headSha === pr?.headSha &&
    review.baseSha === pr?.baseSha
  );
}
export type DisplayStatus = Status | "outdated" | "saved" | "superseded";
export function reviewDisplayStatus(
  s: Snapshot,
  review: Review,
): DisplayStatus {
  if (review.superseded && ["ready", "cancelled"].includes(review.status))
    return "superseded";
  if (review.status !== "ready") return review.status;
  const pr = [...s.prs, ...(s.historyPrs ?? [])].find(
    (p) => p.id === review.prId,
  );
  if (pr && (review.headSha !== pr.headSha || review.baseSha !== pr.baseSha))
    return "outdated";
  return pr && canPublish(review, publishTarget(s, pr)) ? "ready" : "saved";
}
export const displayStatusLabels: Record<DisplayStatus, string> = {
  ...statusLabels,
  outdated: "Outdated report",
  saved: "Saved report",
  superseded: "Superseded",
};
export function selectionError(
  models: Model[],
  selection: ModelSelection,
): string | null {
  if (!selection.model || !selection.effort)
    return "Choose an explicit model and reasoning effort in Settings.";
  const matches = models.filter((m) => m.model === selection.model);
  if (matches.length !== 1)
    return `Model '${selection.model}' is unavailable. Reconnect Codex or choose an available model in Settings.`;
  if (
    !matches[0].supportedReasoningEfforts.some(
      (e) => e.reasoningEffort === selection.effort,
    )
  )
    return `Model '${selection.model}' does not support effort '${selection.effort}'. Choose a supported effort in Settings.`;
  return null;
}
export function catalogUnavailable(s: Snapshot): string | null {
  if (!s.codexReady) return "Codex is disconnected. Reconnect in Settings.";
  if (!s.modelCatalogReady)
    return (
      s.modelConfigurationError ||
      "Model discovery is not ready. Reconnect Codex in Settings."
    );
  return null;
}
export function workspaceSelectionUnavailable(s: Snapshot): string | null {
  return (
    catalogUnavailable(s) ||
    s.modelConfigurationError ||
    selectionError(s.models, s.settings)
  );
}
export function defaultEffort(model: Model): string | undefined {
  return model.supportedReasoningEfforts.some(
    (e) => e.reasoningEffort === model.defaultReasoningEffort,
  )
    ? model.defaultReasoningEffort
    : undefined;
}
export function chatSelection(r: Review): ModelSelection | undefined {
  return r.model && r.effort
    ? { model: r.model, effort: r.effort }
    : (r.legacyChatSelection ?? undefined);
}
export function reportSelectionLabel(r: Review): string {
  if (r.confirmedReviewSelection)
    return `Model: ${r.confirmedReviewSelection.model} (confirmed) · Effort: ${r.confirmedReviewSelection.effort} (confirmed)`;
  const label = (value: string) =>
    value ? `${value} (requested, unverified)` : "not recorded";
  return `Model: ${label(r.model)} · Effort: ${label(r.effort)}`;
}
export function chatModelUnavailable(s: Snapshot, r: Review): string | null {
  if (r.cleanupPending)
    return "This worktree is being removed. Your chat stays readable.";
  if (activeStatuses.includes(r.status))
    return "Wait for this run to finish, or stop it first.";
  if (r.status === "publishing") return "Wait for publication to finish.";
  if (!r.threadId) return "Start a review to create its chat.";
  if (!r.worktree)
    return "Chat workspace removed. Start a new review to continue.";
  return catalogUnavailable(s);
}
export function followUpUnavailable(s: Snapshot, r: Review): string | null {
  const unavailable = chatModelUnavailable(s, r);
  if (unavailable) return unavailable;
  const selection = chatSelection(r);
  if (!selection)
    return "Choose a chat model and effort before sending a follow-up.";
  const error = selectionError(s.models, selection);
  if (error)
    return `${error} This chat's selection is pinned; start a new review to use different settings.`;
  if (
    s.reviews.filter(
      (r) => activeStatuses.includes(r.status) && r.status !== "queued",
    ).length >= s.settings.concurrency
  )
    return "All review slots are in use. Wait for a run to finish.";
  return null;
}
export function canStartReview(s: Snapshot, prId: string) {
  const pr = s.prs.find((p) => p.id === prId);
  return (
    !!pr &&
    !workspaceSelectionUnavailable(s) &&
    s.settings.repositories.some((r) => sameRepo(r.name, pr.repo)) &&
    !s.reviews.some((r) => r.prId === prId && activeStatuses.includes(r.status))
  );
}
export function canExport(r: Review) {
  return (
    !activeStatuses.includes(r.status) &&
    r.status !== "publishing" &&
    !r.beforeFollowUp &&
    !!r.report.trim()
  );
}
// Compares against the base repository as GitHub reports it now, which
// differs from the configured name after a rename or transfer.
export function sameRepository(pr: PullRequest) {
  const base = pr.baseRepo || pr.repo;
  return pr.headRepo?.toLowerCase() === base.toLowerCase();
}
// Mirrors trusted_source in src-tauri/src/types.rs.
export function trustedSource(
  pr: PullRequest | undefined,
  trustedAuthors: string[] = [],
) {
  return (
    !!pr &&
    sameRepository(pr) &&
    (["OWNER", "MEMBER", "COLLABORATOR"].includes(pr.authorAssociation ?? "") ||
      trustedAuthors.some((a) => a.toLowerCase() === pr.author.toLowerCase()))
  );
}
