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
/** Who reviews a repository's pull requests. */
export type Provider = "codex" | "claude" | "antigravity";
/** The reviewers that run as a CLI on this computer. */
export type CliProvider = Exclude<Provider, "codex">;
export const providerLabels: Record<Provider, string> = {
  codex: "Codex",
  claude: "Claude",
  antigravity: "Antigravity",
};
export interface Review {
  id: string;
  prId: string;
  headSha: string;
  baseSha: string;
  status: Status;
  /** Who runs this review; older state is always Codex. */
  provider?: Provider;
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
  /** What the reviewer said it could not read, run or check. */
  notChecked?: string[];
  /** The same, in demos saved before it was renamed. */
  testsNotRun?: string[];
  /** False when the reviewer said it could not check the change. */
  completed?: boolean | null;
  /** Requests Orbit declined because no one approved them in time. */
  declinedApprovals?: string[];
  /** Setup and checks Orbit ran in the repository's dev container. */
  checks?: ContainerChecks | null;
  /** The findings-extraction turn is running. */
  extracting?: boolean;
  /** The earlier round a re-review started from; absent for a first review. */
  previousRound?: PreviousRound | null;
  /** What the reviewer decided about each finding in `previousRound`. */
  priorFindings?: PriorFinding[];
  /** Blocking findings this review left open; null without findings. */
  blocking?: number | null;
  /** The blocking priority `blocking` was counted with; missing in older
   * reviews. */
  blockingPriority?: number | null;
  /** How many findings the reviewer's own report lists, when Orbit can
   * count them. */
  reportFindings?: number | null;
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
  /** What the author said about each earlier finding in a reply Orbit
   * recognised, such as "Fixed F2 in abc1234". */
  claims?: AuthorClaim[];
  commentsIncluded: number;
  commentsOmitted: number;
}
export type CommitPlace =
  "in_change" | "earlier" | "not_in_branch" | "not_found";
export interface AuthorClaim {
  findingId: string;
  author: string;
  createdAt: string;
  kind: "fixed" | "declined";
  commit: string | null;
  commitCheck: CommitPlace | null;
  reason: string | null;
}
/** What the author said, as a short phrase. Matches the published review. */
export function claimSummary(c: AuthorClaim): string {
  if (c.kind === "fixed")
    return c.commit ? `fixed in ${c.commit.slice(0, 7)}` : "fixed";
  return c.reason ? `declined: ${c.reason}` : "declined";
}
/** Where a claimed fix's commit is, when that is worth saying. */
export const commitPlaceNotes: Record<CommitPlace, string | null> = {
  in_change: null,
  earlier: "That commit was already in the reviewed head.",
  not_in_branch: "That commit isn’t part of this pull request.",
  not_found: "Orbit couldn’t find that commit.",
};
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
export type Category =
  | "bug"
  | "security"
  | "data_loss"
  | "compatibility"
  | "test_gap"
  | "performance"
  | "other";
export const categoryLabels: Record<Category, string> = {
  bug: "Bug",
  security: "Security",
  data_loss: "Data loss",
  compatibility: "Compatibility",
  test_gap: "Test gap",
  performance: "Performance",
  other: "Other",
};
/** A place in the repository. */
export interface Location {
  /** Relative to the repository root; null when outside the worktree. */
  path: string | null;
  startLine: number;
  endLine: number;
}
export interface Finding extends Location, Record<string, unknown> {
  id: string;
  title: string;
  /** 0 (P0, most severe) to 3. */
  priority: number;
  /** What is wrong. Older findings also fold in the failure scenario and
   * the fix. */
  body: string;
  confidence: number;
  /** The fields below are missing or null in older findings. */
  category?: Category | null;
  /** Other places the finding involves. */
  otherLocations?: Location[];
  /** A concrete input or state and the wrong result it produces. */
  failureScenario?: string | null;
  /** How the reviewer established it. */
  evidence?: string | null;
  /** Verified by reading or running the code, or only likely. */
  confirmed?: boolean | null;
  suggestedFix?: string | null;
  /** The test or check that would show it is fixed. */
  doneWhen?: string | null;
  reviewerNotes?: string | null;
  /** The number in its short ID (F2), unique within its pull request. */
  number?: number | null;
}
/** A finding's short ID, such as F2; null for older findings and scans. */
export function shortId(f: Finding): string | null {
  return f.number ? `F${f.number}` : null;
}
/** Whether a review's finding blocks the change, or null when the review
 * predates blocking labels. */
export function isBlocking(f: Finding, r: Review): boolean | null {
  return r.blockingPriority == null ? null : f.priority <= r.blockingPriority;
}
/** A warning when the reviewer's report lists more findings than its list
 * and the earlier findings it still holds open. Matches the published note. */
export function countNote(r: Review): string | null {
  if (r.reportFindings == null || !r.findings) return null;
  const held = (r.priorFindings ?? []).filter(
    (p) => p.status === "still_open" || p.status === "declined_disputed",
  ).length;
  const listed = r.findings.length + held;
  if (r.reportFindings <= listed) return null;
  const n = r.reportFindings;
  return `${providerLabels[r.provider ?? "codex"]}’s report lists ${n} ${n === 1 ? "finding" : "findings"}, but Orbit’s list has ${listed}, so some may have been left out. Check the report.`;
}
/** What the reviewer could not check, from current or older state. */
export function notChecked(r: Review): string[] {
  return r.notChecked ?? r.testsNotRun ?? [];
}
export function findingLocation(f: Location) {
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
  /** Who reviews this repository's pull requests. */
  provider?: Provider;
  /** Run the setup and check commands in the repository's dev container. */
  devContainer?: DevContainer;
}
export interface DevContainer {
  enabled: boolean;
  /** Runs after the setup command, such as `npm test`. */
  checkCommand: string;
  /** Keeps the container for the review, so the reviewer can run its own
   * commands there. */
  reviewerTool?: boolean;
}
/** One command run in a dev container. */
export interface CheckStep {
  command: string;
  /** Null when it was stopped at its time limit. */
  exitCode: number | null;
  timedOut: boolean;
  seconds: number;
  /** The end of what it printed. */
  output: string;
}
export interface ContainerChecks {
  image: string;
  network: boolean;
  setup: CheckStep | null;
  /** Null without a check command, or after a failed setup. */
  check: CheckStep | null;
}
export function checkPassed(step: CheckStep) {
  return step.exitCode === 0 && !step.timedOut;
}
/** How a step ended, in a few words, as the reviewer is told. */
export function checkOutcome(step: CheckStep) {
  if (step.timedOut) return `stopped at its time limit after ${step.seconds}s`;
  if (step.exitCode === 0) return `passed in ${step.seconds}s`;
  if (step.exitCode === null)
    return `stopped by a signal after ${step.seconds}s`;
  return `failed with exit code ${step.exitCode} after ${step.seconds}s`;
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
  /** The model and effort of Claude reviews; empty uses Claude's default. */
  claudeModel?: string;
  claudeEffort?: string;
  /** The model and effort of Antigravity reviews; the model must be chosen,
   * and an empty effort uses high. */
  antigravityModel?: string;
  antigravityEffort?: string;
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
  /** Bit, Orbit's buddy. Older saved state has none. */
  buddy?: Buddy;
  /** Plain-language write-ups on the Decisions page. Older saved state has
   * none. */
  writeUps?: WriteUps;
}
export type Writer = "codex" | "claude";
export const writerLabels: Record<Writer, string> = {
  codex: "Codex",
  claude: "Claude",
};
/** Who writes up each merged or closed pull request in plain words, one at
 * a time in the background. Off until turned on. */
export interface WriteUps {
  enabled: boolean;
  writer: Writer;
}
export const defaultWriteUps: WriteUps = { enabled: false, writer: "codex" };
export type Mood = "happy" | "busy" | "worried" | "sleepy";
export type BuddyHat =
  | "none"
  | "party"
  | "top-hat"
  | "beanie"
  | "crown"
  | "propeller"
  | "wizard"
  | "cowboy"
  | "chef"
  | "cap"
  | "graduation"
  | "beret"
  | "flower"
  | "bow"
  | "halo"
  | "headphones"
  | "santa"
  | "viking"
  | "cat-ears"
  | "antenna"
  | "space-helmet";
export type BuddyTone = "cheerful" | "calm" | "dry";
export interface Buddy {
  enabled: boolean;
  name: string;
  /** One of the colours in Bit.tsx. */
  colour: string;
  hat: BuddyHat;
  tone: BuddyTone;
  /** Saved now, used once Bit can chat. */
  personality: string;
  /** What Bit does about each fix it can do on its own. */
  rules: BuddyRules;
}
/** One of Orbit's own controls, offered for something that needs you. */
export type Fix =
  | "continue_loop"
  | "review_again"
  | "publish"
  | "sync_now"
  | "turn_on_autopilot"
  | "check_connections";
/** The fixes Bit can do on its own. Changing settings and checking
 * connections always ask first. */
export type RuledFix = Exclude<Fix, "turn_on_autopilot" | "check_connections">;
export type FixRule = "do_it" | "ask_first" | "leave_it";
export type BuddyRules = Record<RuledFix, FixRule>;
export const fixLabels: Record<Fix, string> = {
  continue_loop: "Continue loop",
  review_again: "Review again",
  publish: "Publish report",
  sync_now: "Sync now",
  turn_on_autopilot: "Turn autopilot back on",
  check_connections: "Check again",
};
export const fixDescriptions: Record<RuledFix, string> = {
  continue_loop: "When a loop reaches its round limit, allow two more rounds.",
  review_again:
    "When an automatic review fails or is interrupted, start a new review. It doesn't count as a round, and its report waits for you.",
  publish:
    "When a report is waiting for you, publish it. Never a report with text that looks like credentials, or one from a pull request automatic publication wouldn't trust.",
  sync_now: "When syncing keeps failing, sync again.",
};
export const fixRuleLabels: Record<FixRule, string> = {
  do_it: "Do it",
  ask_first: "Ask first",
  leave_it: "Leave it to me",
};
export const defaultBuddy: Buddy = {
  enabled: true,
  name: "Bit",
  colour: "lime",
  hat: "none",
  tone: "cheerful",
  personality: "",
  rules: {
    continue_loop: "ask_first",
    review_again: "ask_first",
    publish: "ask_first",
    sync_now: "ask_first",
  },
};
export const buddyTones: Record<BuddyTone, string> = {
  cheerful: "Cheerful",
  calm: "Calm",
  dry: "Dry",
};
export type NoteKind =
  | "round_started"
  | "round_finished"
  | "clean"
  | "needs_you"
  | "stopped"
  | "continued"
  | "fixing"
  | "fixed"
  | "answer"
  | "connected";
/** A question Bit answers straight from Orbit's state. */
export type Question =
  "needs_you" | "running" | "loops" | "repositories" | "setup";
export const questionLabels: Record<Question, string> = {
  needs_you: "What needs me?",
  running: "What's running?",
  loops: "How are my loops?",
  repositories: "How are my repositories?",
  setup: "Help me set up",
};
/** A line of an answer. */
export interface BuddyNoteLine {
  text: string;
  prId: string | null;
  /** The Needs you item the line is about. */
  key: string | null;
}
/** A line in Bit's feed, in its own voice. */
export interface BuddyNote {
  id: number;
  at: string;
  kind: NoteKind;
  prId: string | null;
  reviewId: string | null;
  /** The repository a workspace item is about. */
  repo: string | null;
  text: string;
  /** The Needs you item the note is about, whose fix it can offer. */
  key?: string | null;
  /** For an answer, the question you asked. */
  question?: string | null;
  /** For an answer, a line for each thing it lists. */
  lines?: BuddyNoteLine[];
  /** For an answer to a question you typed, the buttons it offers. */
  proposals?: Proposal[];
  /** For an answer to a question you typed, what it changed in Bit's
   * memory. */
  memory?: MemoryChange[];
  /** A question you typed that Bit is still answering. */
  pending?: boolean;
}
/** What a button in a typed question's answer does: one of Orbit's own
 * controls. */
export type Action =
  | "continue_loop"
  | "stop_loop"
  | "review_again"
  | "publish"
  | "sync_now"
  | "change_settings"
  | "set_up_repository"
  | "check_connections";
export const actionLabels: Record<Action, string> = {
  continue_loop: "Continue loop",
  stop_loop: "Stop loop",
  review_again: "Review again",
  publish: "Publish report",
  sync_now: "Sync now",
  change_settings: "Change settings",
  set_up_repository: "Set up repository",
  check_connections: "Check again",
};
/** A change to a repository's settings from one of Bit's answers. Only
 * what is given changes. */
export interface RepositoryPatch {
  setupCommand?: string;
  checkCommand?: string;
  networkAccess?: boolean;
  devContainer?: boolean;
  reviewer?: Provider;
  roundLimit?: number;
  approvalMinutes?: number;
  keepCloneUpdated?: boolean;
  autopilot?: boolean;
}
/** A repository with a change from one of Bit's answers. */
export function withRepositoryPatch(
  repo: Repository,
  patch: RepositoryPatch,
): Repository {
  const devContainer = repo.devContainer ?? {
    enabled: false,
    checkCommand: "",
  };
  return {
    ...repo,
    setupCommand: patch.setupCommand ?? repo.setupCommand,
    networkAccess: patch.networkAccess ?? repo.networkAccess,
    provider: patch.reviewer ?? repo.provider,
    roundLimit: patch.roundLimit ?? repo.roundLimit,
    approvalMinutes: patch.approvalMinutes ?? repo.approvalMinutes,
    keepUpdated: patch.keepCloneUpdated ?? repo.keepUpdated,
    automatic: patch.autopilot ?? repo.automatic,
    devContainer: {
      enabled: patch.devContainer ?? devContainer.enabled,
      checkCommand: patch.checkCommand ?? devContainer.checkCommand,
      reviewerTool: devContainer.reviewerTool,
    },
  };
}
/** A button in an answer. It runs only when you tap it, and Orbit checks
 * it again then. */
export interface Proposal {
  /** Its place in the answer. */
  id: number;
  action: Action;
  prId?: string | null;
  /** For Publish, the report. */
  reviewId?: string | null;
  /** For Change settings, the change. */
  settings?: SettingsPatch | null;
  /** For Set up repository, the repository, as owner/name. */
  repo?: string | null;
  /** For Set up repository, the change to its settings. */
  repository?: RepositoryPatch | null;
  /** For Set up repository, whether it connects the repository. */
  connect?: boolean;
  /** You tapped it and it worked. */
  done?: boolean;
}
/** The longest question you can type to Bit. */
export const maxQuestion = 2000;
export interface BuddyFeed {
  /** Oldest first, at most 50. */
  notes: BuddyNote[];
  memory?: BuddyMemory;
}
/** What Bit remembers between questions, in its own words. Bit adds and
 * forgets lines as it answers what you type; you can forget any line. */
export interface BuddyMemory {
  general: string[];
  /** By repository, as owner/name. */
  repositories: Record<string, string[]>;
}
/** The most lines Bit keeps in general and about each repository. */
export const memoryLimits = { general: 20, perRepository: 10 };
/** A change an answer made to Bit's memory, or couldn't make because there
 * was no room. */
export interface MemoryChange {
  /** The repository, or null for general memory. */
  repo: string | null;
  line: string;
  kind: "remember" | "forget" | "full";
}
const memoryChangeLabels: Record<MemoryChange["kind"], string> = {
  remember: "Remembered",
  forget: "Forgot",
  full: "No room to remember",
};
/** How an answer shows a change to Bit's memory. */
export function memoryChangeText(change: MemoryChange): string {
  const about = change.repo ? ` (${change.repo})` : "";
  return `${memoryChangeLabels[change.kind]}${about}: ${change.line}`;
}
/** The buddy's settings, with defaults for anything missing. */
export function buddyOf(settings: Settings): Buddy {
  return {
    ...defaultBuddy,
    ...settings.buddy,
    rules: { ...defaultBuddy.rules, ...settings.buddy?.rules },
  };
}
/** The fix to offer as a button for an item that is still open, unless
 * you asked Bit to leave it to you. */
export function offeredFix(
  s: Snapshot,
  key: string | null | undefined,
): { item: Attention; fix: Fix } | undefined {
  const item = s.attention?.find((a) => a.key === key);
  const fix = item?.fix;
  if (!item || !fix) return undefined;
  if (
    fix !== "turn_on_autopilot" &&
    fix !== "check_connections" &&
    buddyOf(s.settings).rules[fix] === "leave_it"
  )
    return undefined;
  return { item, fix };
}
/** Whether Orbit still needs setting up: GitHub or Codex isn't connected,
 * or no repository is. Bit's setup help stands out until it doesn't. */
export function needsSetup(s: Snapshot) {
  return !s.githubUser || !s.codexReady || !s.settings.repositories.length;
}
/** Bit's mood as the engine worked it out, or the same rules applied here
 * when it didn't (the demo). */
export function buddyMood(s: Snapshot): Mood {
  if (s.mood) return s.mood;
  if (s.attention?.length) return "worried";
  if (
    s.reviews.some((r) => activeStatuses.includes(r.status)) ||
    s.loopStates?.some((l) => !l.closed && l.status === "reviewing")
  )
    return "busy";
  if (!s.settings.automatic || !s.prs.length) return "sleepy";
  return "happy";
}
/** What Bit's mood means, in a sentence. */
export function moodReason(s: Snapshot, mood: Mood): string {
  switch (mood) {
    case "worried":
      return "Something needs you.";
    case "busy":
      return "A review is running.";
    case "sleepy":
      return s.settings.automatic
        ? "Nothing is open right now."
        : "Autopilot is off.";
    default:
      return "Everything is clean or waiting on the author.";
  }
}
/** Settings with a patch applied, as the engine applies it: only supplied
 * fields change, including inside the buddy. */
export function withPatch(settings: Settings, patch: SettingsPatch): Settings {
  const { buddy, ...rest } = patch;
  if (!buddy) return { ...settings, ...rest };
  const current = buddyOf(settings);
  return {
    ...settings,
    ...rest,
    buddy: {
      ...current,
      ...buddy,
      rules: { ...current.rules, ...buddy.rules },
    },
  };
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
export type SettingsPatch = Partial<
  Omit<Settings, "repositories" | "buddy">
> & {
  buddy?: Partial<Omit<Buddy, "rules">> & { rules?: Partial<BuddyRules> };
};
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
  | "approval"
  | "held"
  | "failed"
  | "loop"
  | "autopilot_paused"
  | "sync_failing"
  | "disconnected";
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
  /** The control that resolves it, which Bit offers. */
  fix?: Fix | null;
}
export const attentionLabels: Record<AttentionKind, string> = {
  approval: "Approval",
  held: "Publication held",
  failed: "Review failed",
  loop: "Loop",
  autopilot_paused: "Autopilot paused",
  sync_failing: "Sync failing",
  disconnected: "Not connected",
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
  if (
    a.kind === "autopilot_paused" ||
    a.kind === "sync_failing" ||
    a.kind === "disconnected"
  )
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
export interface CliStatus {
  ready: boolean;
  error?: string | null;
  version?: string | null;
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
  /** The Claude CLI, as last checked. */
  claude?: CliStatus;
  /** The Claude models Orbit offers. */
  claudeModels?: Model[];
  /** The Antigravity CLI, as last checked. */
  antigravity?: CliStatus;
  /** Docker and the Dev Container CLI, as last checked. */
  devContainers?: CliStatus;
  /** The models Antigravity offered when last checked. */
  antigravityModels?: Model[];
  modelCatalogReady: boolean;
  modelConfigurationError?: string | null;
  modelSelectionNotice?: string | null;
  /** The latest update of each clone kept up to date. */
  cloneUpdates?: CloneUpdate[];
  /** The Codex account's usage limits; absent when the account has none. */
  usage?: Usage | null;
  /** Repository security scans, newest first. */
  scans?: SecurityScan[];
  /** Bit's notes about the loops. */
  buddyFeed?: BuddyFeed;
  /** How Bit feels. Derived by the engine for display. */
  mood?: Mood | null;
  /** Reading each repository's pull-request history for the Decisions page. */
  ledger?: LedgerStatus;
}
export interface LedgerProgress {
  repo: string;
  /** The pull request being written up. */
  number?: number | null;
  done: number;
  total: number;
}
/** How reading the pull-request history, and writing it up, is going. */
export interface LedgerStatus {
  refreshing: boolean;
  /** The repository being read, while a read runs. */
  progress?: LedgerProgress | null;
  /** Each connected repository's latest read. */
  repos: { repo: string; refreshedAt?: string | null; error?: string | null }[];
  /** Goes up whenever stored history or a write-up changes. */
  version: number;
  /** The pull request being written up, while write-ups run. */
  writing?: LedgerProgress | null;
  /** Why write-ups stopped or are waiting. */
  writeError?: string | null;
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
  /** While stopping, the reason the scan is being stopped, if not a cancel. */
  stopReason?: string | null;
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
  claudeModel: "",
  claudeEffort: "",
  antigravityModel: "",
  antigravityEffort: "",
  repositories: [],
  trustedAuthors: [],
  blockingPriority: 1,
  settlePushes: true,
  commitStatus: true,
  notify: true,
  keepScanArtifacts: false,
  buddy: defaultBuddy,
  writeUps: defaultWriteUps,
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
  provider: Provider = "codex",
): string | null {
  if (provider === "antigravity" && !selection.model)
    return "Choose an Antigravity model in Settings.";
  if (!selection.model || !selection.effort)
    return "Choose an explicit model and reasoning effort in Settings.";
  const matches = models.filter((m) => m.model === selection.model);
  if (matches.length !== 1)
    return provider !== "codex"
      ? `${providerLabels[provider]} model '${selection.model}' is unavailable. Choose an available model in Settings.`
      : `Model '${selection.model}' is unavailable. Reconnect Codex or choose an available model in Settings.`;
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
/** Who reviews a repository's pull requests; unconfigured ones use Codex. */
export function repoProvider(s: Snapshot, repo: string): Provider {
  return (
    s.settings.repositories.find((r) => sameRepo(r.name, repo))?.provider ??
    "codex"
  );
}
// Mirrors models::claude_selection in src-tauri/src/models.rs.
export function claudeSelection(s: Snapshot): ModelSelection {
  const models = s.claudeModels ?? [];
  const model =
    s.settings.claudeModel || models.find((m) => m.isDefault)?.model || "";
  const effort =
    s.settings.claudeEffort ||
    models.find((m) => m.model === model)?.defaultReasoningEffort ||
    "";
  return { model, effort };
}
// Mirrors models::antigravity_selection in src-tauri/src/models.rs.
export function antigravitySelection(s: Snapshot): ModelSelection {
  return {
    model: s.settings.antigravityModel || "",
    effort: s.settings.antigravityEffort || "high",
  };
}
/** A CLI reviewer's status, as last checked. */
export function cliStatus(s: Snapshot, provider: CliProvider) {
  return provider === "claude" ? s.claude : s.antigravity;
}
/** The models a reviewer offers. */
export function providerModels(s: Snapshot, provider: Provider): Model[] {
  if (provider === "claude") return s.claudeModels ?? [];
  if (provider === "antigravity") return s.antigravityModels ?? [];
  return s.models;
}
/** The model and effort new reviews by `provider` use. */
export function providerSelection(
  s: Snapshot,
  provider: Provider,
): ModelSelection {
  if (provider === "claude") return claudeSelection(s);
  if (provider === "antigravity") return antigravitySelection(s);
  return s.settings;
}
/** Why a reviewer can't take new work now, if it can't. */
export function reviewerUnavailable(
  s: Snapshot,
  provider: Provider,
): string | null {
  if (provider === "codex") return catalogUnavailable(s);
  const status = cliStatus(s, provider);
  if (!status?.ready)
    return (
      status?.error ||
      `${providerLabels[provider]} is not connected. Check it again in Settings.`
    );
  return null;
}
/** Why a new review with this reviewer can't start, if it can't. */
export function reviewerSelectionUnavailable(
  s: Snapshot,
  provider: Provider,
): string | null {
  if (provider === "codex") return workspaceSelectionUnavailable(s);
  return (
    reviewerUnavailable(s, provider) ||
    selectionError(
      providerModels(s, provider),
      providerSelection(s, provider),
      provider,
    )
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
// Mirrors models::report_selection_label in src-tauri/src/models.rs.
export function reportSelectionLabel(r: Review): string {
  // Codex reviews keep the original label so earlier reports read the same.
  const reviewer =
    r.provider && r.provider !== "codex"
      ? `Reviewer: ${providerLabels[r.provider]} · `
      : "";
  if (r.confirmedReviewSelection)
    return `${reviewer}Model: ${r.confirmedReviewSelection.model} (confirmed) · Effort: ${r.confirmedReviewSelection.effort} (confirmed)`;
  const label = (value: string) =>
    value ? `${value} (requested, unverified)` : "not recorded";
  return `${reviewer}Model: ${label(r.model)} · Effort: ${label(r.effort)}`;
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
  return reviewerUnavailable(s, r.provider ?? "codex");
}
export function followUpUnavailable(s: Snapshot, r: Review): string | null {
  const unavailable = chatModelUnavailable(s, r);
  if (unavailable) return unavailable;
  const selection = chatSelection(r);
  if (!selection)
    return "Choose a chat model and effort before sending a follow-up.";
  const provider = r.provider ?? "codex";
  const error = selectionError(
    providerModels(s, provider),
    selection,
    provider,
  );
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
    !reviewerSelectionUnavailable(s, repoProvider(s, pr.repo)) &&
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
