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
}
export interface Message {
  id: string;
  role: "user" | "agent" | "system";
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
}
export interface Repository {
  name: string;
  path: string;
  automatic: boolean;
  setupCommand: string;
  networkAccess: boolean;
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
export interface Snapshot {
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
export type DisplayStatus = Status | "outdated" | "saved";
export function reviewDisplayStatus(
  s: Snapshot,
  review: Review,
): DisplayStatus {
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
};
export function followUpUnavailable(s: Snapshot, r: Review): string | null {
  if (r.cleanupPending)
    return "This worktree is being removed. Your chat stays readable.";
  if (activeStatuses.includes(r.status))
    return "Wait for this run to finish, or stop it first.";
  if (r.status === "publishing") return "Wait for publication to finish.";
  if (!r.threadId) return "Start a review to create its chat.";
  if (!r.worktree)
    return "Chat workspace removed. Start a new review to continue.";
  if (!s.codexReady) return "Codex is disconnected. Reconnect in Settings.";
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
    s.codexReady &&
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
