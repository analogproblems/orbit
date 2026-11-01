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
  /** Whether findings can block a clean verdict at all; missing means on. */
  blockFindings?: boolean;
  /** Queue a new head automatically only once it is a poll interval old. */
  settlePushes: boolean;
  /** Show each automatic round's result on its commit as `orbit/review`. */
  commitStatus: boolean;
  /** Show a desktop notification when something new needs you. */
  notify: boolean;
  /** Keep a security scan's bulky working files after its import. */
  keepScanArtifacts?: boolean;
  /** Watch CI on each repository's default branch and on the pull requests
   * Orbit follows; missing means on. */
  watchCi?: boolean;
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
/** How Bit looks: the classic ringed planet, or one of the orbs and shapes
 * in orbs.ts. */
export type BuddyLook =
  | "classic"
  | "pip"
  | "tide"
  | "bloom"
  | "moss"
  | "glim"
  | "nova"
  | "duo"
  | "puff"
  | "jelly"
  | "fizz"
  | "poly"
  | "prism"
  | "gem"
  | "twirl"
  | "cubby"
  | "fold";
export type BuddyTone = "cheerful" | "calm" | "dry";
/** A provider Bit can use with an API key of yours. */
export type ApiProvider = "openai" | "anthropic" | "google";
/** Who answers the questions you type to Bit. */
export type BuddyAnswerer = Provider | "ollama" | ApiProvider;
export interface Buddy {
  enabled: boolean;
  name: string;
  /** One of the colours in Bit.tsx. */
  colour: string;
  hat: BuddyHat;
  look: BuddyLook;
  tone: BuddyTone;
  /** Saved now, used once Bit can chat. */
  personality: string;
  /** What Bit does about each fix it can do on its own. */
  rules: BuddyRules;
  answerer: BuddyAnswerer;
  /** Where Ollama listens, when it answers. */
  ollamaUrl: string;
  /** The Ollama model that answers. */
  ollamaModel: string;
  /** The model that answers with your OpenAI key. */
  openaiModel: string;
  /** The model that answers with your Anthropic key. */
  anthropicModel: string;
  /** The model that answers with your Google key. */
  googleModel: string;
  /** Whether Bit sends a morning brief. */
  brief: boolean;
  /** When, by this computer's clock, as HH:MM. */
  briefAt: string;
}
/** One of Orbit's own controls, offered for something that needs you. */
export type Fix =
  | "continue_loop"
  | "review_again"
  | "publish"
  | "sync_now"
  | "turn_on_autopilot"
  | "check_connections"
  | "post_here_again";
/** The fixes that always ask first: changing settings, checking
 * connections, and posting again where another Orbit posts. */
export type AskingFix =
  "turn_on_autopilot" | "check_connections" | "post_here_again";
/** The fixes Bit can do on its own. */
export type RuledFix = Exclude<Fix, AskingFix>;
export type FixRule = "do_it" | "ask_first" | "leave_it";
/** What Bit does on GitHub about CI: telling agents, rerunning jobs that
 * failed by bad luck, and proposing CI where none runs on pull requests. */
export type RuledPost =
  | "comment_on_prs"
  | "open_issues"
  | "rerun_flaky"
  | "rerun_again"
  | "propose_ci";
export type RuledAction = RuledFix | RuledPost;
export type BuddyRules = Record<RuledAction, FixRule>;
export const fixLabels: Record<Fix, string> = {
  continue_loop: "Continue loop",
  review_again: "Review again",
  publish: "Publish report",
  sync_now: "Sync now",
  turn_on_autopilot: "Turn autopilot back on",
  check_connections: "Check again",
  post_here_again: "Post here again",
};
export const fixDescriptions: Record<RuledFix, string> = {
  continue_loop: "When a loop reaches its round limit, allow two more rounds.",
  review_again:
    "When an automatic review fails or is interrupted, start a new review. It doesn't count as a round, and its report waits for you.",
  publish:
    "When a report is waiting for you, publish it. Never a report with text that looks like credentials, or one from a pull request automatic publication wouldn't trust.",
  sync_now: "When syncing keeps failing, sync again.",
};
export const postLabels: Record<RuledPost, string> = {
  comment_on_prs: "Comment on pull requests",
  open_issues: "Open issues",
  rerun_flaky: "Rerun a failed job once when it looks flaky",
  rerun_again: "Rerun the same job a second time",
  propose_ci: "Propose CI where none runs on pull requests",
};
export const postDescriptions: Record<RuledPost, string> = {
  comment_on_prs:
    "When a pull request’s checks fail, post one comment on it for its agent, and keep it up to date.",
  open_issues:
    "When a default branch’s checks fail, open an issue labelled orbit for the repository’s agent, and close it once the checks have passed for an hour.",
  rerun_flaky:
    "When a job fails before any test ran, or because its runner went away or the network failed, rerun it once and say so on Orbit’s comment or issue.",
  rerun_again:
    "When a rerun job fails again and still looks like bad luck, rerun it one more time. Orbit never reruns a job a third time on the same commit.",
  propose_ci:
    "When no workflow runs on a repository’s pull requests, open an issue labelled orbit for its agent, with a workflow to start from, and close it once one runs.",
};
/** A rule's name, for a fix or a post. */
export const ruleLabel = (rule: RuledAction) =>
  rule in postLabels
    ? postLabels[rule as RuledPost]
    : fixLabels[rule as RuledFix];
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
  look: "glim",
  tone: "cheerful",
  personality: "",
  rules: {
    continue_loop: "ask_first",
    review_again: "ask_first",
    publish: "ask_first",
    sync_now: "ask_first",
    comment_on_prs: "do_it",
    open_issues: "do_it",
    rerun_flaky: "do_it",
    rerun_again: "ask_first",
    propose_ci: "ask_first",
  },
  answerer: "codex",
  ollamaUrl: "http://localhost:11434",
  ollamaModel: "",
  openaiModel: "",
  anthropicModel: "",
  googleModel: "",
  brief: true,
  briefAt: "09:00",
};
export const buddyTones: Record<BuddyTone, string> = {
  cheerful: "Cheerful",
  calm: "Calm",
  dry: "Dry",
};
export const buddyAnswerers: Record<BuddyAnswerer, string> = {
  codex: "Codex",
  claude: "Claude Code",
  antigravity: "Antigravity",
  ollama: "Ollama",
  openai: "OpenAI (API key)",
  anthropic: "Anthropic (API key)",
  google: "Google Gemini (API key)",
};
export const apiProviders: Record<
  ApiProvider,
  { company: string; page: string; pageName: string }
> = {
  openai: {
    company: "OpenAI",
    page: "https://platform.openai.com/api-keys",
    pageName: "OpenAI's API keys page",
  },
  anthropic: {
    company: "Anthropic",
    page: "https://console.anthropic.com/settings/keys",
    pageName: "Anthropic's API keys page",
  },
  google: {
    company: "Google",
    page: "https://aistudio.google.com/apikey",
    pageName: "Google AI Studio",
  },
};
/** One API key: never the key, only whether it is saved and, once checked,
 * whether the provider takes it and its models. */
export interface ApiKeyStatus {
  saved: boolean;
  status: CliStatus;
  models: string[];
}
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
  | "connected"
  | "decide"
  | "sign_in"
  | "ci"
  | "brief";
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
  /** For a line of the morning brief, that it is new since the last one. */
  new?: boolean;
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
  | "check_connections"
  | "sign_in"
  | "open_page";
export const actionLabels: Record<Action, string> = {
  continue_loop: "Continue loop",
  stop_loop: "Stop loop",
  review_again: "Review again",
  publish: "Publish report",
  sync_now: "Sync now",
  change_settings: "Change settings",
  set_up_repository: "Set up repository",
  check_connections: "Check again",
  sign_in: "Sign in",
  open_page: "Open page",
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
  /** For Sign in, what to sign in to: github or codex. */
  service?: string | null;
  /** For Open page, the page, which the app opens where you are. */
  link?: string | null;
  /** What the button says, when Orbit words it. */
  label?: string | null;
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
/** Whether a fix always waits for your tap, whatever Bit's rules. */
export const asksFirst = (fix: Fix): fix is AskingFix =>
  fix === "turn_on_autopilot" ||
  fix === "check_connections" ||
  fix === "post_here_again";
/** The fix to offer as a button for an item that is still open, unless
 * you asked Bit to leave it to you. */
export function offeredFix(
  s: Snapshot,
  key: string | null | undefined,
): { item: Attention; fix: Fix } | undefined {
  const item = s.attention?.find((a) => a.key === key);
  const fix = item?.fix;
  if (!item || !fix) return undefined;
  if (!asksFirst(fix) && buddyOf(s.settings).rules[fix] === "leave_it")
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
  // The morning brief is there to read, so it worries nobody.
  if (s.attention?.some((a) => a.kind !== "brief")) return "worried";
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
/** A page of Settings. */
export type SettingsTab =
  | "connections"
  | "reviewers"
  | "automation"
  | "decisions"
  | "buddy"
  | "devices"
  | "storage";
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
  | "disconnected"
  | "ci_failing"
  | "ci_question"
  | "runs_waiting"
  | "stuck"
  | "brief"
  | "another_orbit";
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
  ci_failing: "CI failing",
  ci_question: "An agent needs a person",
  runs_waiting: "Runs waiting",
  stuck: "Stuck",
  brief: "Morning brief",
  another_orbit: "Another Orbit posts here",
};
/** The day a morning brief's item is for, from its key. */
export const briefDay = (a: Attention) => a.key.replace(/^brief:/, "");
export interface AttentionHandlers {
  onChoose: (p: PullRequest, r?: Review) => void;
  onRepositories: () => void;
  onSettings: () => void;
  onApproval: (reviewId: string, id: string | number) => void;
  onPublish: (r: Review) => void;
  onOperations: () => void;
  /** Bit's conversation, where the morning brief is. */
  onBuddy?: () => void;
}
/** Where an item is resolved: its review, its PR, or a settings page. */
export function attentionAction(
  s: Snapshot,
  a: Attention,
  go: AttentionHandlers,
): (() => void) | undefined {
  if (a.kind === "sync_failing" && a.repo) return go.onRepositories;
  if (
    a.kind === "ci_failing" ||
    a.kind === "ci_question" ||
    a.kind === "runs_waiting"
  )
    return go.onOperations;
  if (
    a.kind === "autopilot_paused" ||
    a.kind === "sync_failing" ||
    a.kind === "disconnected"
  )
    return go.onSettings;
  if (a.kind === "brief") return go.onBuddy;
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
  /** Ollama at Bit's address, as last checked while it answers Bit. */
  ollama?: CliStatus;
  /** The models Ollama had when last checked. */
  ollamaModels?: string[];
  /** Your API keys, by provider; the keys themselves are only in your
   * system's password store. */
  apiKeys?: Record<ApiProvider, ApiKeyStatus>;
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
  /** Each connected repository's CI, as last read. */
  ci?: RepoCi[];
  /** What is wrong with CI, open or closed lately. */
  problems?: Problem[];
  /** Orbit's comments and issues that tell agents about failing CI. */
  ops?: OpsThread[];
  /** Each write Orbit made on GitHub for them; the newest last. */
  journal?: OpsWrite[];
  /** Why the jobs still failing on open problems failed, as Orbit read
   * them. */
  failures?: CiFailure[];
  /** What each connected repository's files say about its CI. */
  setups?: CiSetup[];
  /** The self-hosted runners of each connected repository, and of each
   * organization that owns one, as Orbit last read them. */
  runners?: RunnerPlace[];
  /** Each connected repository's GitHub Actions caches and artifacts, as
   * Orbit last read them. */
  actionsStorage?: ActionsStorage[];
  /** What GitHub last said about whether each pull request Orbit reviews
   * can merge. */
  merges?: Merge[];
  /** Whether each pull request Orbit reviews is clear to merge, and what
   * is in the way. Derived like `loopStates`. */
  readiness?: Readiness[];
  /** Bit's morning briefs: the latest, and the day Orbit last looked. */
  brief?: Briefs;
  /** Who this Orbit is, by the name that signs what it posts on pull
   * requests. */
  instance?: OrbitInstance;
  /** Each pull request where Orbit read whether another Orbit posts. */
  posting?: Posting[];
  /** The latest notifications Orbit sent, newest last, so a desktop app
   * showing this Orbit from another computer shows them too. */
  notices?: SentNotice[];
}
/** A notification an Orbit sent. */
export interface SentNotice {
  id: string;
  title: string;
  body: string;
  item: Attention;
}
/** The always-on Orbit the desktop app shows instead of running its own.
 * `chosen` when you gave its link in Settings; otherwise it's Orbit without
 * a window using the app's data folder on this computer. */
export interface AlwaysOn {
  host: string;
  chosen: boolean;
  /** This app's Orbit moved there, unless the always-on Orbit says it
   * didn't: the app can't tell yet. */
  pending?: boolean;
}
/** Who an Orbit is. Its name goes with its data folder on one computer. */
export interface OrbitInstance {
  name: string;
  /** A fingerprint of where it runs. */
  home: string;
  /** Its earlier names, newest last. */
  former: FormerName[];
}
/** One of an Orbit's earlier names, and when it stopped using it. What the
 * name posted later is the Orbit it was copied from. */
export interface FormerName {
  name: string;
  until: string;
}
/** A signed post on GitHub, when GitHub last took it, and the name that
 * signed it. */
export interface OrbitPost {
  url: string;
  at: string;
  by: string;
}
/** Where Orbit stands on one pull request: it steps back once another
 * Orbit posted there after it, and after the post you said to post after
 * anyway. */
export interface Posting {
  prId: string;
  /** This Orbit's newest post there. */
  mine?: OrbitPost | null;
  /** The newest post there from another Orbit. */
  other?: OrbitPost | null;
  /** The other Orbit's post you said to post after anyway. */
  resumed?: OrbitPost | null;
}
/** One thing a morning brief lists. */
export interface BriefLine {
  key: string;
  text: string;
  prId?: string;
  /** Not in the brief before. */
  new?: boolean;
}
/** A morning brief Orbit sent. */
export interface Brief {
  /** The day it is for, by this computer's clock, such as 2026-10-31. */
  day: string;
  at: string;
  text: string;
  lines: BriefLine[];
  /** How many lines of the brief before are gone. */
  settled: number;
  /** In Needs you, until you mark it read or the next brief's time comes. */
  open: boolean;
}
export interface Briefs {
  /** The latest day Orbit looked at whether to send a brief. */
  looked?: string;
  latest?: Brief;
}
/** Where a commit's checks stand. "none" is no checks, or only ones that
 * were skipped or cancelled. */
export type CiState = "none" | "running" | "passing" | "failing";
/** A check that didn't pass. */
export interface CiCheck {
  /** Such as "CI / Rust": the workflow, then the job. */
  name: string;
  url: string;
}
export interface CommitCi {
  sha: string;
  state: CiState;
  failing: CiCheck[];
  running: number;
  passing: number;
  /** Orbit couldn't read all of its checks, so it can fail but not pass. */
  incomplete?: boolean;
  /** When the earliest of its passing checks began, by GitHub's clock;
   * empty when one of them doesn't say. */
  ranAt?: string;
  /** When the commit was made, for a default branch's latest. */
  committedAt?: string;
}
export interface PullCi {
  number: number;
  draft: boolean;
  ci: CommitCi;
}
/** A repository's CI, as Orbit last read it. */
export interface RepoCi {
  repo: string;
  /** The default branch. */
  branch: string;
  head: CommitCi;
  /** The default branch's latest commit's first line. Someone else's words. */
  title: string;
  pulls: PullCi[];
  /** Whether every open pull request was read. */
  complete: boolean;
  /** When CI was last read without an error. */
  checkedAt: string;
  /** Why the latest read failed; what was read before is kept. */
  error?: string | null;
}
export type ProblemKind =
  "default_branch" | "pull_request" | "base_moved" | "no_ci" | "waiting";
export interface ProblemEvent {
  at: string;
  text: string;
}
/** One thing wrong with CI, from when Orbit first saw it until it saw it
 * fixed. A default branch's problem needs you, as do runs waiting a long
 * time for a runner (`waiting`); a pull request's is its author's to fix,
 * as is a pull request whose checks passed before its base moved on
 * (`base_moved`). A repository where no CI runs on pull requests (`no_ci`)
 * is listed with the repository. */
export interface Problem {
  id: string;
  key: string;
  kind: ProblemKind;
  repo: string;
  prId?: string | null;
  /** The default branch, or the pull request's branch. */
  branch: string;
  /** The latest commit seen failing, and the first. */
  sha: string;
  sinceSha: string;
  failing: CiCheck[];
  openedAt: string;
  updatedAt: string;
  closedAt?: string | null;
  closedReason?: string | null;
  /** The commit it passed at, when it closed by passing. */
  closedSha?: string;
  /** Oldest first. */
  timeline: ProblemEvent[];
  /** How many times it has told you. */
  alerts: number;
  /** Where a default branch broke, once Orbit has read its history. */
  broke?: Broke | null;
  /** For `base_moved`: the base branch, and its latest commit, which the
   * checks ran without. */
  base?: string;
  baseSha?: string;
  /** For `no_ci`: what Orbit found at `sha`, and the workflow it proposes. */
  found?: CiFound | null;
  /** For `waiting`: the runs waiting when Orbit last read them, the longest
   * first. */
  waiting?: WaitingRun[];
}
/** A run that has waited for a runner since `since`. */
export interface WaitingRun {
  id: number;
  /** Its workflow's name; empty when GitHub gives none. */
  name: string;
  /** The run on GitHub; empty unless it is a github.com link. */
  url: string;
  since: string;
}
/** A self-hosted runner. */
export interface Runner {
  name: string;
  os: string;
  online: boolean;
  busy: boolean;
  labels: string[];
}
/** What Orbit read about one place's self-hosted runners: a repository's,
 * or an account's that owns connected repositories. A person's account
 * shares none; `owner` is one Orbit hasn't read yet. */
export interface RunnerPlace {
  name: string;
  kind: "repository" | "owner" | "organization" | "person";
  readAt: string;
  runners: Runner[];
  /** How many GitHub says there are, more than `runners` when there are
   * more than Orbit reads. */
  total: number;
  /** Why Orbit can't see its runners. */
  unseen?: string | null;
  /** For a repository, its runs waiting for a runner. */
  waiting: WaitingRun[];
  /** GitHub had more queued runs than Orbit read, so more may wait than
   * `waiting` lists. */
  moreQueued?: boolean;
  /** Why the last read failed; what was read before is kept. */
  error?: string | null;
  tries: number;
  retryAt?: string | null;
}
/** What Orbit read of one repository's GitHub Actions storage. */
export interface ActionsStorage {
  repo: string;
  readAt: string;
  /** Its caches: how many, and the bytes they hold. */
  caches: number;
  cacheBytes: number;
  /** The artifacts it keeps, that haven't expired, of those Orbit read. */
  artifacts: number;
  artifactBytes: number;
  /** GitHub has more artifacts than Orbit read, so it may keep more. */
  moreArtifacts?: boolean;
  /** Why Orbit can't see its storage. */
  unseen?: string | null;
  /** Why the last read failed; what was read before is kept. */
  error?: string | null;
  tries: number;
  retryAt?: string | null;
}
/** What is in the way of merging a pull request. */
export type WaitKind =
  "review" | "findings" | "loop" | "checks" | "base" | "merge" | "reviews";
/** Whose move it is: the author's (or their agent's), a person's (you, or
 * whoever a rule on GitHub waits for), or nobody's yet while Orbit, CI or
 * GitHub is still at work. */
export type Turn = "author" | "person" | "working";
export interface Wait {
  kind: WaitKind;
  turn: Turn;
  text: string;
}
/** Whether a pull request Orbit reviews is clear to merge at its head. */
export interface Readiness {
  prId: string;
  repo: string;
  number: number;
  headSha: string;
  /** Its base branch, and the latest commit on it the checks ran with,
   * once Orbit knows it. */
  base: string;
  baseSha: string;
  clear: boolean;
  /** What is in the way, first things first. */
  waits: Wait[];
  /** Who was asked to review it and where each stands, and what its base
   * branch's rules need, once Orbit has asked GitHub at this head and
   * base. */
  reviews?: Reviews;
}
/** Where a reviewer stands on a pull request's latest commit. An approval
 * of an older commit doesn't count, and a request for changes stands until
 * they review again. */
export type Stand =
  "asked" | "approved" | "approved_older" | "changes_requested";
/** Someone asked to review a pull request, or who reviewed it. */
export interface Reviewer {
  /** A login, or `owner/slug` for a team. Someone else's words. */
  name: string;
  team: boolean;
  stand: Stand;
  /** The commit their latest approval or request for changes was of. */
  commit: string;
  /** For an approval of the latest commit or a request for changes,
   * whether they have write access, without which GitHub's rules don't
   * count it; null when Orbit couldn't tell. */
  mayApprove?: boolean | null;
}
/** What a base branch's rules need before a pull request merges. */
export interface ReviewRule {
  /** Approvals, which Orbit counts only of the latest commit. */
  approvals: number;
  /** A code owner's approval, which Orbit leaves to GitHub. */
  codeOwners: boolean;
}
export interface Reviews {
  reviewers: Reviewer[];
  /** What the rules need, or null when Orbit can't see them, with why in
   * `unseen`. */
  rule?: ReviewRule | null;
  unseen?: string | null;
  /** Why Orbit couldn't read the reviews, so `reviewers` is empty. */
  unread?: string | null;
  /** It has more reviews than Orbit reads, so `reviewers` is empty. */
  tooMany?: boolean;
}
/** What GitHub last said about whether a pull request can merge. */
export interface Merge {
  prId: string;
  headSha: string;
  baseSha: string;
  readAt: string;
  /** GitHub's answer, or null while it is still working it out. */
  mergeable?: boolean | null;
  /** GitHub's `mergeable_state`, such as clean, dirty, blocked or behind. */
  state: string;
  error?: string | null;
  tries: number;
  retryAt?: string | null;
  /** Who was asked to review it and their reviews, as GitHub said then. */
  reviews?: Reviews | null;
}
/** A readiness answer in one line, as Orbit says it. */
export function readinessLine(r: Readiness) {
  const first = r.waits[0];
  if (first) return `Not clear to merge yet. ${first.text}`;
  const line = `Clear to merge at ${r.headSha.slice(0, 7)} against ${r.base} at ${r.baseSha.slice(0, 7)}.`;
  // What Orbit couldn't check.
  return r.reviews && !r.reviews.rule
    ? `${line} Orbit can't see ${r.base}'s rules, so it left its reviews to GitHub.`
    : line;
}
/** What Orbit found in a repository where no CI runs on pull requests. */
export interface CiFound {
  /** The workflows there, none of which run on pull requests; empty when
   * there are none. */
  workflows: string[];
  /** The workflow Orbit proposes, empty when it can't tell how the
   * repository builds, and the files it is built from. */
  proposal: string;
  from: string[];
}
/** What a repository's files say about its CI, read at its default
 * branch's latest commit. */
export interface CiSetup {
  repo: string;
  branch: string;
  sha: string;
  readAt: string;
  workflows: { path: string; onPulls: "yes" | "no" | "unknown" }[];
  /** Another CI service's files at the top, such as `.circleci`. */
  others: string[];
  proposal: string;
  from: string[];
  error?: string | null;
  tries: number;
  retryAt?: string | null;
  /** Why Orbit knows something runs checks here, from checks it saw. */
  ran?: string | null;
}
/** Where a default branch broke: the commits after the last one whose
 * checks passed, up to the first that failed. */
export interface Broke {
  /** Whether Orbit has its answer; until then it reads again later. */
  done: boolean;
  /** The last commit that passed, empty when Orbit found none. */
  green: string;
  /** The first that failed after it. */
  red: string;
  /** The commits after `green` up to `red`, newest first. */
  commits: string[];
  /** The pull requests merged as those commits, and how many came from
   * none. */
  pulls: number[];
  direct: number;
  /** The pull request when the commits are its merge alone. */
  pull?: number | null;
  /** Why Orbit couldn't read it, or found nothing. */
  error?: string | null;
  tries: number;
  retryAt?: string | null;
}
/** One failing check, as Orbit's comment or issue lists it. */
export interface OpsItem {
  /** 1 for I1. Never reused within its comment or issue. */
  number: number;
  /** A failing check, a pull request's base moving on since its checks
   * began (then `check` is the base branch), or adding CI that runs on pull
   * requests (then it is the default branch). */
  kind?: "check" | "base_moved" | "no_ci";
  check: string;
  url: string;
  /** Whether it failed at `at`, the latest commit Orbit saw. */
  failing: boolean;
  since: string;
  at: string;
  /** What an agent last said about it since it started failing. */
  answer?: OpsAnswer | null;
  /** When GitHub first showed it failing this time: only replies from
   * then on are about it. */
  heardFrom?: string;
  /** Why its job failed, once Orbit has read its log. */
  why?: CiFailure | null;
  /** Orbit's reruns of its job at `at`, and its tries at them. */
  reruns?: OpsRerun[];
  /** The job you said to rerun, while Bit asks first. */
  rerunOk?: string;
}
/** Why one failing job failed, as Orbit read it from GitHub Actions. */
export interface CiFailure {
  /** The job's page on GitHub, its check's link. */
  url: string;
  check: string;
  /** The step that failed the job, when GitHub says. */
  step: string;
  /** The last lines of that step's log up to its last error. */
  lines: string[];
  /** The step's log looked like it held a secret, so Orbit dropped it. */
  held: boolean;
  /** Why Orbit can't say, such as a check that isn't a GitHub Actions
   * job, or one GitHub didn't answer for yet. */
  error?: string | null;
  readAt: string;
  /** Reads in a row GitHub didn't answer for. */
  tries?: number;
  /** When Orbit tries again. */
  retryAt?: string | null;
  /** GitHub's rate limit stopped the last try, so Orbit reads no log until
   * `retryAt`. */
  limited?: boolean;
  /** Why it looks like bad luck rather than the code, such as a lost
   * runner or a network error, when it does. */
  flaky?: string | null;
  /** The step couldn't upload an artifact as the account's artifact
   * storage is full, which only a person can change. */
  quota?: boolean;
}
/** One rerun of an item's job, or Orbit's tries at one. */
export interface OpsRerun {
  /** The job Orbit reran, by its link. */
  url: string;
  /** Why it looked like bad luck. */
  reason: string;
  at: string;
  done: boolean;
  /** Orbit found it already rerun on GitHub. */
  already: boolean;
  /** Why the latest try didn't go through. */
  error?: string | null;
  tries: number;
  retryAt?: string | null;
  /** GitHub won't rerun it, so Orbit doesn't ask again. */
  lasting: boolean;
  /** Orbit saw the job fail again after this rerun. */
  failedAgain: boolean;
  /** Orbit may have sent it, so it looks on GitHub before anything else. */
  unsure: boolean;
}
/** What an agent says about an item: `Taking I2`, `Fixed I2 in <commit>`,
 * `Declined I2: <reason>` or `Need a person on I2: <question>`. */
export type AnswerKind = "taking" | "fixed" | "declined" | "question";
/** What CI says at the commit an agent says fixes an item. */
export type FixCheck =
  "waiting" | "passes" | "fails" | "not_on_branch" | "not_found";
export interface OpsAnswer {
  kind: AnswerKind;
  author: string;
  at: string;
  /** The reply on GitHub. */
  url: string;
  replyId: number;
  commit?: string | null;
  /** The reason or question, in the agent's words. */
  text?: string | null;
  check?: FixCheck | null;
  head: string;
  /** When Orbit last tried to check the fix. */
  checkedAt?: string;
}
/** Where Orbit tells an agent about failing CI: its one comment on a pull
 * request, or its issue about a default branch. */
export interface OpsThread {
  key: string;
  kind: "comment" | "issue";
  repo: string;
  /** The pull request, or the issue once it is open. */
  number?: number | null;
  prId?: string | null;
  commentId?: number | null;
  /** Where it is on GitHub, once posted. */
  url: string;
  branch: string;
  problemId: string;
  items: OpsItem[];
  posted: string;
  closed: boolean;
  /** You said to post it, while Bit asks first. */
  allowed: boolean;
  /** Bit told you it can post it, for a proposal it asks about first. */
  offered?: boolean;
  /** It is gone from GitHub, so Orbit leaves it be. */
  gone: boolean;
  /** A post may have reached GitHub without Orbit hearing back, so Orbit
   * looks for it before anything else. */
  unsure?: boolean;
  error?: string | null;
  tries: number;
  retryAt?: string | null;
  updatedAt: string;
  /** The newest reply Orbit has read, by GitHub comment ID. */
  readTo?: number;
  /** Orbit reads the comments posted or edited from this time on, going
   * on from `readPage` while part way through them. */
  readSince?: string;
  readPage?: number;
  readNewest?: string;
  /** When GitHub last took Orbit's text. */
  shownAt?: string;
  /** Whether the pull request is clear to merge, as the top of its comment
   * says it, for one Orbit reviews automatically. */
  ready?: OpsReady;
}
/** The answer at the top of Orbit's comment on a pull request. */
export interface OpsReady {
  clear: boolean;
  /** The head and base it was worked out for, and once it is clear, the
   * base's latest commit. */
  headSha: string;
  base: string;
  baseSha: string;
  /** Whose move it is while it isn't clear; null once Orbit stopped
   * working it out. */
  turn?: Turn | null;
  text: string;
  /** When Orbit first gave this answer, words aside. */
  since?: string;
  /** When Orbit nudged whoever's move it is, once: the agent on the
   * comment, or you in Needs you. */
  nudged?: string | null;
  /** When GitHub took the comment reminding the agent. */
  reminded?: string | null;
}
export type OpsAction =
  | "post_comment"
  | "edit_comment"
  | "open_issue"
  | "edit_issue"
  | "note_on_issue"
  | "close_issue"
  | "reopen_issue"
  | "rerun_job";
/** One write Orbit made on GitHub, recorded before it was sent. */
export interface OpsWrite {
  id: string;
  thread: string;
  action: OpsAction;
  repo: string;
  number?: number | null;
  state: "sending" | "done" | "failed";
  at: string;
  doneAt?: string | null;
  url: string;
  error?: string | null;
}
/** A commit's first seven characters, as GitHub shows them. */
export function shortSha(sha: string) {
  return sha.slice(0, 7);
}
/** Check names in a sentence: "A", "A and B", "A, B and 2 more". */
export function checkNames(checks: CiCheck[]) {
  const [a, b, ...rest] = checks.map((c) => c.name);
  if (!a) return "no checks";
  if (!b) return a;
  if (!rest.length) return `${a} and ${b}`;
  return `${a}, ${b} and ${rest.length} more`;
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
  blockFindings: true,
  settlePushes: true,
  commitStatus: true,
  notify: true,
  keepScanArtifacts: false,
  watchCi: true,
  buddy: defaultBuddy,
  writeUps: defaultWriteUps,
};
export function latestReview(reviews: Review[], prId: string) {
  return reviews
    .filter((r) => r.prId === prId)
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt))[0];
}
const plain = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
/** What was edited since `opened`, applied over `current`, so a change made
 * elsewhere meanwhile (such as by Bit) is kept rather than undone. Objects
 * inside are merged the same way. */
export function editedOver<T extends object>(
  current: T,
  opened: T,
  edited: T,
): T {
  const was = opened as Record<string, unknown>;
  const now = current as Record<string, unknown>;
  const out: Record<string, unknown> = { ...now };
  for (const [key, value] of Object.entries(edited)) {
    if (JSON.stringify(value) === JSON.stringify(was[key])) continue;
    out[key] =
      plain(value) && plain(was[key]) && plain(now[key])
        ? editedOver(now[key], was[key], value)
        : value;
  }
  return out as T;
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
  if (!s.codexReady)
    return s.codexError || "Codex is disconnected. Reconnect in Settings.";
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
/** A connection's state in a few words. Orbit's own errors start with
 * "Cannot start" when a program is missing and say "not signed in" when it
 * needs a sign-in. */
export function connectionState(ready: boolean, error?: string | null) {
  if (ready && !error) return "Connected";
  if (error?.startsWith("Cannot start")) return "Not installed";
  if (error && /not signed in|auth login/i.test(error)) return "Not signed in";
  return "Not connected";
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
