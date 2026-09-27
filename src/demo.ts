import {
  defaults,
  selectionError,
  type Finding,
  type Model,
  type ModelSelection,
  type Snapshot,
  type PullRequest,
  type Review,
  type RepositorySummary,
} from "./types";

// Simulated config/read values, independent of the demo workspace's settings.
export const demoCodexConfig = {
  model: "demo-review-model",
  model_reasoning_effort: "high",
};
export function resolveDemoModelSelection(
  settings: ModelSelection,
  models: Model[],
  config:
    | { model?: string | null; model_reasoning_effort?: string | null }
    | Error = demoCodexConfig,
) {
  const selection = { model: settings.model, effort: settings.effort };
  let modelSource = "your saved Orbit selection";
  let effortSource = "your saved Orbit selection";
  if (!selection.model) {
    if (config instanceof Error)
      throw Error(
        `Could not read effective Codex settings: ${config.message}. Reconnect or select a model in Settings.`,
      );
    if (config.model && models.some((m) => m.model === config.model)) {
      selection.model = config.model;
      modelSource = "your Codex configuration";
    } else {
      const defaults = models.filter((m) => m.isDefault);
      if (defaults.length !== 1)
        throw Error(
          "Codex has no unique default model. Choose a model and effort in Settings.",
        );
      selection.model = defaults[0].model;
      modelSource = config.model
        ? `catalog default (your configured model '${config.model}' is unavailable)`
        : "catalog default (no configured model)";
    }
  }
  if (!selection.effort) {
    const model = models.find((m) => m.model === selection.model);
    const configured =
      config instanceof Error ? undefined : config.model_reasoning_effort;
    if (
      configured &&
      model?.supportedReasoningEfforts.some(
        (e) => e.reasoningEffort === configured,
      )
    ) {
      selection.effort = configured;
      effortSource = "your Codex configuration";
    } else {
      selection.effort = model?.defaultReasoningEffort ?? "";
      if (!selection.effort)
        throw Error(
          "The selected model or its default effort is unavailable. Choose a model and effort in Settings.",
        );
      effortSource = configured
        ? `model default (your configured effort '${configured}' isn't supported by this model)`
        : config instanceof Error
          ? "model default (Codex configuration could not be read)"
          : "model default (no configured effort)";
    }
  }
  const error = selectionError(models, selection);
  if (error) throw Error(error);
  const notice =
    selection.model !== settings.model || selection.effort !== settings.effort
      ? `Resolved review selection to ${selection.model} · ${selection.effort}. Model: ${modelSource}. Effort: ${effortSource}. This explicit selection is saved for new reviews.`
      : undefined;
  return { selection, notice };
}

const ago = (minutes: number) =>
  new Date(Date.now() - minutes * 60000).toISOString();
const data = [
  [
    "orbit",
    "148",
    "Add repository-level review configuration",
    "claude[bot]",
    "feat/repository-config",
    214,
    38,
    6,
    ["feature", "review requested"],
  ],
  [
    "orbit",
    "147",
    "Keep review threads in sync with new commits",
    "alex",
    "fix/thread-sync",
    86,
    21,
    3,
    ["bug"],
  ],
  [
    "api",
    "82",
    "Introduce cursor pagination for the activity feed",
    "sam",
    "feat/activity-pagination",
    156,
    42,
    5,
    ["api"],
  ],
  [
    "dashboard",
    "36",
    "A little more room for the details",
    "taylor",
    "ui/detail-panel",
    64,
    28,
    4,
    ["design"],
  ],
  [
    "orbit",
    "145",
    "Handle interrupted reviews on restart",
    "morgan",
    "fix/review-recovery",
    112,
    16,
    3,
    ["reliability"],
  ],
  [
    "api",
    "79",
    "Update request validation dependencies",
    "ren",
    "chore/dependencies",
    24,
    19,
    2,
    ["dependencies"],
  ],
] as const;
export const demoRepositories: RepositorySummary[] = (
  [
    ["acme/orbit", true, "Pull request review workspace", 12],
    ["acme/api", true, "HTTP API and background jobs", 40],
    ["acme/dashboard", true, "Customer dashboard", 90],
    ["acme/docs", false, "Public documentation site", 300],
    ["acme/infra", true, "Terraform and deployment configuration", 1200],
    ["you/dotfiles", false, "Shell and editor configuration", 4000],
  ] as const
).map(([name, isPrivate, description, minutes]) => ({
  name,
  private: isPrivate,
  description,
  pushedAt: ago(minutes),
}));
export const demoReport =
  "## Review findings\n\n### [P2] Preserve repository overrides when saving defaults\n\n`src/config/repositories.ts:48`\n\nSaving the global review configuration replaces the repository configuration object. A repository with its own model or environment setup loses those overrides the next time global settings are saved.\n\nMerge the updated defaults with the existing repository entries, and add a regression test that saves defaults after configuring an override.\n\n### [P2] Validate the workspace before enqueueing a review\n\n`src/reviews/queue.ts:72`\n\nA deleted or moved checkout still passes the current configuration check. The job enters the queue and fails during worktree creation without a useful recovery action.\n\nCheck that the configured directory is a Git repository before enqueueing, then show a link to repository settings when validation fails.\n\n---\nReviewed the configuration flow, queue behavior, and affected tests. These are illustrative findings in demo mode.";
export function demoFindings(reviewId: string): Finding[] {
  return [
    {
      id: `${reviewId}-0`,
      title: "Preserve repository overrides when saving defaults",
      priority: 2,
      path: "src/config/repositories.ts",
      startLine: 48,
      endLine: 48,
      body: "Saving the global review configuration replaces the repository configuration object, so per-repository overrides are lost. Merge the updated defaults with the existing entries and add a regression test.",
      confidence: 0.8,
    },
    {
      id: `${reviewId}-1`,
      title: "Validate the workspace before enqueueing a review",
      priority: 2,
      path: "src/reviews/queue.ts",
      startLine: 72,
      endLine: 72,
      body: "A deleted or moved checkout still passes the configuration check and fails later during worktree creation. Check the directory before enqueueing and link to repository settings on failure.",
      confidence: 0.7,
    },
  ];
}
/** One published automatic round with `blocking` findings left open. Later
 * rounds find nothing new and carry the earlier findings forward. */
function demoRound(
  pr: PullRequest,
  round: number,
  blocking: number,
  minutesAgo: number,
  last: boolean,
  previous?: Review,
): Review {
  const id = `demo-${pr.number}-round-${round}`;
  const head = last ? pr.headSha : `${round}`.repeat(7) + pr.headSha.slice(7);
  // What the previous round found, and what it left open from before.
  const earlier = previous
    ? [
        ...(previous.findings ?? []),
        ...(previous.previousRound?.findings ?? []).filter((f) =>
          previous.priorFindings?.some(
            (p) => p.id === f.id && p.status === "still_open",
          ),
        ),
      ]
    : [];
  const findings: Finding[] = previous
    ? []
    : [
        ...Array.from({ length: blocking }, (_, i) => ({
          id: `${id}-${i}`,
          title: `Blocking issue ${i + 1}`,
          priority: 1,
          path: "src/reviews/queue.ts",
          startLine: 40 + i * 10,
          endLine: 40 + i * 10,
          body: "An illustrative finding in demo mode.",
          confidence: 0.8,
        })),
        {
          id: `${id}-${blocking}`,
          title: "Name the retry constant",
          priority: 2,
          path: "src/reviews/queue.ts",
          startLine: 12,
          endLine: 12,
          body: "An illustrative nit in demo mode.",
          confidence: 0.6,
        },
      ];
  // The first `blocking` earlier blocking findings stay open; the rest are fixed.
  let open = 0;
  const priorFindings = earlier.map((f) => ({
    id: f.id,
    status:
      f.priority > 1
        ? ("declined_accepted" as const)
        : open++ < blocking
          ? ("still_open" as const)
          : ("fixed" as const),
    note: "",
  }));
  const verdict = blocking ? "changes_requested" : "clean";
  const report = blocking
    ? `${blocking} blocking finding${blocking === 1 ? "" : "s"} remain. Illustrative report in demo mode.`
    : "No blocking findings. Illustrative report in demo mode.";
  return {
    id,
    prId: pr.id,
    headSha: head,
    baseSha: pr.baseSha,
    status: "posted",
    model: "demo-review-model",
    effort: "high",
    createdAt: ago(minutesAgo + 4),
    updatedAt: ago(minutesAgo),
    messages: [
      {
        id: `${id}-msg`,
        role: "agent",
        text: report,
        createdAt: ago(minutesAgo),
      },
    ],
    report,
    commentUrl: `${pr.url}#pullrequestreview-${pr.number}${round}`,
    automatic: true,
    isReview: true,
    findings,
    verdict,
    orbitVerdict: verdict,
    blocking,
    summary: report,
    previousRound: previous
      ? {
          reviewId: previous.id,
          headSha: previous.headSha,
          verdict: previous.orbitVerdict ?? null,
          findings: earlier,
          commentsIncluded: 0,
          commentsOmitted: 0,
        }
      : null,
    priorFindings,
  };
}
/** A loop's rounds, newest first as the engine keeps them. */
function demoLoop(pr: PullRequest, blocking: number[], minutesAgo: number) {
  const rounds: Review[] = [];
  blocking.forEach((n, i) =>
    rounds.push(
      demoRound(
        pr,
        i + 1,
        n,
        minutesAgo + (blocking.length - 1 - i) * 90,
        i === blocking.length - 1,
        rounds[i - 1],
      ),
    ),
  );
  return rounds.reverse();
}
export function freshDemo(): Snapshot {
  const prs: PullRequest[] = data.map((d, i) => ({
    id: `acme/${d[0]}#${d[1]}`,
    repo: `acme/${d[0]}`,
    number: Number(d[1]),
    title: d[2],
    author: d[3],
    headRef: d[4],
    baseRef: "main",
    headSha: `abc${String(i).repeat(37)}`,
    baseSha: "def" + "0".repeat(37),
    additions: d[5],
    deletions: d[6],
    changedFiles: d[7],
    labels: [...d[8]],
    url: `https://github.com/acme/${d[0]}/pull/${d[1]}`,
    updatedAt: ago(12 + i * 27),
    draft: i === 3,
    reviewRequested: i < 3,
    // api#79 comes from an outside contributor's fork, so autopilot skips it.
    // orbit#148 is from a coding agent's bot, trusted only once added in Settings.
    headRepo: i === 5 ? `${d[3]}/${d[0]}` : `acme/${d[0]}`,
    authorAssociation: i === 5 ? "CONTRIBUTOR" : i === 0 ? "NONE" : "MEMBER",
    body:
      i === 0
        ? "## What changed\n\nAdds a dedicated configuration layer for each connected repository, so teams can tailor how their pull requests are reviewed.\n\n- Configure a local checkout and environment setup\n- Set automatic review behavior per repository\n- Keep review settings together in one place\n\n## Testing\n\nAdded coverage for config persistence and repository validation."
        : "## What changed\n\n" +
          d[2] +
          ". This pull request includes the implementation and focused regression coverage.\n\n## Testing\n\nUnit tests and type checks pass.",
  }));
  const review: Review = {
    id: "demo-completed",
    threadId: "demo-thread",
    prId: prs[0].id,
    headSha: prs[0].headSha,
    baseSha: prs[0].baseSha,
    status: "ready",
    model: "demo-review-model",
    effort: "high",
    confirmedReviewSelection: { model: "demo-review-model", effort: "high" },
    createdAt: ago(9),
    updatedAt: ago(6),
    worktree: "~/Library/Application Support/Orbit/worktrees/pr-148",
    messages: [
      {
        id: "demo-msg-1",
        role: "system",
        text: "Created an isolated worktree at abc0000. Repository environment is ready.",
        createdAt: ago(9),
      },
      {
        id: "demo-msg-2",
        role: "agent",
        text: "I’ll trace the configuration changes through persistence and the review queue, then check the regression coverage.",
        createdAt: ago(8),
      },
      { id: "demo-msg-3", role: "agent", text: demoReport, createdAt: ago(6) },
    ],
    report: demoReport,
    findings: demoFindings("demo-completed"),
    verdict: "changes_requested",
    summary:
      "Two defects in the new repository configuration flow; the rest of the change looks sound.",
    testsNotRun: ["End-to-end tests (no browser in the review sandbox)"],
    automatic: false,
    isReview: true,
  };
  // Converged, waiting for its author, and out of rounds.
  const clean = demoLoop(prs[4], [4, 1, 0], 30);
  const waiting = demoLoop(prs[2], [3, 2], 20);
  const limited = demoLoop(prs[1], [3, 2, 2, 1], 15);
  const ids = (rounds: Review[]) => rounds.map((r) => r.id).reverse();
  return {
    prs,
    reviews: [review, ...limited, ...waiting, ...clean],
    // The browser demo has no engine to derive these, so they are fixed.
    attention: [
      {
        key: `loop:${prs[1].id}:Round limit reached.`,
        kind: "loop",
        prId: prs[1].id,
        reviewId: limited[0].id,
        approvalId: null,
        repo: null,
        reason: "Round limit reached.",
      },
      {
        key: "sync:acme/dashboard",
        kind: "sync_failing",
        prId: null,
        reviewId: null,
        approvalId: null,
        repo: "acme/dashboard",
        reason:
          "acme/dashboard failed to sync 3 times in a row: GitHub returned 404 Not Found.",
      },
    ],
    loopStates: [
      {
        prId: prs[1].id,
        status: "needs_you",
        reason: "Round limit reached.",
        rounds: 4,
        roundLimit: 4,
        roundIds: ids(limited),
      },
      {
        prId: prs[2].id,
        status: "waiting_for_author",
        reason: null,
        rounds: 2,
        roundLimit: 4,
        roundIds: ids(waiting),
      },
      {
        prId: prs[4].id,
        status: "clean",
        reason: null,
        rounds: 3,
        roundLimit: 4,
        roundIds: ids(clean),
      },
    ],
    settings: {
      ...defaults,
      model: "demo-review-model",
      effort: "medium",
      repositories: ["orbit", "api", "dashboard"].map((n) => ({
        name: `acme/${n}`,
        path: `~/Projects/${n}`,
        automatic: n === "orbit",
        setupCommand: "",
        networkAccess: false,
        roundLimit: 4,
      })),
    },
    models: [
      {
        id: "demo-review-model",
        model: "demo-review-model",
        displayName: "Codex · demo",
        defaultReasoningEffort: "medium",
        supportedReasoningEfforts: ["low", "medium", "high"].map(
          (reasoningEffort) => ({ reasoningEffort, description: "" }),
        ),
        isDefault: true,
      },
    ],
    approvals: [],
    githubUser: "you",
    codexReady: true,
    modelCatalogReady: true,
    polling: false,
    lastPoll: ago(0),
  };
}
