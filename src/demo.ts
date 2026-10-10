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
  type SecurityScan,
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
      body: "Saving the global review configuration replaces the repository configuration object, so per-repository overrides are lost.",
      confidence: 0.8,
      category: "data_loss",
      otherLocations: [
        { path: "src/settings/save.ts", startLine: 21, endLine: 24 },
      ],
      failureScenario:
        "Give a repository its own model, then change any global setting: the repository is back on the default model.",
      evidence:
        "Read saveDefaults in src/settings/save.ts and the repository object it rebuilds in src/config/repositories.ts:48.",
      confirmed: true,
      suggestedFix:
        "Merge the updated defaults with the existing repository entries.",
      doneWhen:
        "A test saves defaults after configuring an override and the override is still there.",
      reviewerNotes: null,
    },
    {
      id: `${reviewId}-1`,
      title: "Validate the workspace before enqueueing a review",
      priority: 2,
      path: "src/reviews/queue.ts",
      startLine: 72,
      endLine: 72,
      body: "A deleted or moved checkout still passes the configuration check and fails later during worktree creation.",
      confidence: 0.7,
      category: "bug",
      otherLocations: [],
      failureScenario:
        "Move a connected checkout, then start a review: it queues, then fails while creating the worktree with no way to fix the path.",
      evidence: "Read the enqueue check in src/reviews/queue.ts:72.",
      confirmed: false,
      suggestedFix:
        "Check that the directory is a Git repository before enqueueing, and link to repository settings when it isn't.",
      doneWhen: null,
      reviewerNotes: null,
    },
  ];
}
/** Illustrative findings of a demo security scan. */
export function demoScanFindings(scanId: string): Finding[] {
  return [
    {
      id: `${scanId}:csf_demo_0`,
      title: "Search query is built from request text",
      priority: 1,
      path: "server/db/search.ts",
      startLine: 30,
      endLine: 33,
      body: "The search handler joins the request's query string into SQL, so a crafted search can read other users' rows.\n\n**Severity:** High (CVSS:3.1 8.6) · **Weakness:** CWE-89 · **Confidence:** High\n\n**Suggested fix:** Pass the search text as a bound parameter.",
      confidence: 0.9,
      category: "security",
    },
    {
      id: `${scanId}:csf_demo_1`,
      title: "Webhook signature is compared in variable time",
      priority: 2,
      path: "server/webhooks/verify.ts",
      startLine: 18,
      endLine: 18,
      body: "The signature check uses `===`, which can leak how many leading bytes matched.\n\n**Severity:** Medium · **Weakness:** CWE-208 · **Confidence:** Medium\n\n**Suggested fix:** Compare with `crypto.timingSafeEqual`.",
      confidence: 0.6,
      category: "security",
    },
    {
      id: `${scanId}:csf_demo_2`,
      title: "Debug route is reachable in production builds",
      priority: 3,
      path: "server/routes/debug.ts",
      startLine: 4,
      endLine: 9,
      body: "The `/debug/state` route is registered whatever the environment.\n\n**Severity:** Low · **Weakness:** CWE-489 · **Confidence:** High\n\n**Suggested fix:** Register it only in development.",
      confidence: 0.9,
      category: "security",
    },
  ];
}
export const demoScanReport =
  "# Codex Security scan\n\nThis is an illustrative report in demo mode. In the desktop app, the Codex Security plugin writes this report when a scan finishes.\n\n## Findings\n\n1. **High** · Search query is built from request text\n2. **Medium** · Webhook signature is compared in variable time\n3. **Low** · Debug route is reachable in production builds\n";
/** A finished demo scan of `repo`. */
export function demoScan(repo: string, minutesAgo: number): SecurityScan {
  const id = `scan-demo-${repo.split("/")[1]}`;
  return {
    id,
    repo,
    mode: "standard",
    status: "completed",
    createdAt: ago(minutesAgo),
    finishedAt: ago(minutesAgo - 74),
    branch: "main",
    commit: "4be2f7a91c0d3e56b7a8f9012c3d4e5f6a7b8c9d",
    model: "demo-review-model",
    effort: "medium",
    findings: demoScanFindings(id),
    hasReport: true,
    diskBytes: 3.2 * 1024 * 1024,
    warnings: [],
  };
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
          category: "bug" as const,
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
          category: "other" as const,
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
/** Two weeks of finished automatic reviews on PRs that have since merged, so
 * the Live page's charts have something to show. Deterministic. */
function demoHistory(): { prs: PullRequest[]; reviews: Review[] } {
  let seed = 7;
  const rand = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const repos = ["orbit", "api", "dashboard"];
  const prs: PullRequest[] = [];
  const reviews: Review[] = [];
  // Minutes ago, newest first; busier in recent days and in working hours.
  for (let minutes = 7 * 60; minutes < 14 * 24 * 60;) {
    const hour = new Date(Date.now() - minutes * 60000).getHours();
    const busy = hour >= 9 && hour < 19;
    minutes += Math.round((busy ? 40 : 180) + rand() * (busy ? 120 : 300));
    const repo = repos[Math.floor(rand() * rand() * repos.length * 1.6) % 3];
    const number = 900 - prs.length;
    const pr: PullRequest = {
      id: `acme/${repo}#${number}`,
      repo: `acme/${repo}`,
      number,
      title: `Earlier change #${number}`,
      author: "alex",
      body: "",
      url: `https://github.com/acme/${repo}/pull/${number}`,
      headSha: `fed${String(number % 10).repeat(37)}`,
      baseSha: "def" + "0".repeat(37),
      headRef: `change/${number}`,
      baseRef: "main",
      updatedAt: ago(minutes),
      draft: false,
      additions: 20,
      deletions: 5,
      changedFiles: 2,
      reviewRequested: false,
      labels: [],
      headRepo: `acme/${repo}`,
      authorAssociation: "MEMBER",
    };
    prs.push(pr);
    const rounds = 1 + Math.floor(rand() * 3);
    for (let i = 0; i < rounds; i++) {
      const at = minutes + (rounds - 1 - i) * 35;
      const failed = minutes < 24 * 60 && prs.length === 3 && i === 0;
      reviews.push({
        id: `demo-history-${number}-${i + 1}`,
        prId: pr.id,
        headSha: pr.headSha,
        baseSha: pr.baseSha,
        status: failed ? "failed" : "posted",
        error: failed ? "Codex stopped responding." : undefined,
        model: "demo-review-model",
        effort: "high",
        createdAt: ago(at + 4),
        updatedAt: ago(at),
        messages: [],
        report: failed ? "" : "Illustrative report in demo mode.",
        automatic: true,
        isReview: true,
        findings: failed ? null : [],
        blocking: failed ? null : 0,
        orbitVerdict: failed ? null : "clean",
      });
    }
  }
  return { prs, reviews };
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
    notChecked: ["End-to-end tests (no browser in the review sandbox)"],
    completed: true,
    automatic: false,
    isReview: true,
  };
  // Converged, waiting for its author, and out of rounds.
  const clean = demoLoop(prs[4], [4, 1, 0], 30);
  const waiting = demoLoop(prs[2], [3, 2], 20);
  const limited = demoLoop(prs[1], [3, 2, 2, 1], 15);
  const ids = (rounds: Review[]) => rounds.map((r) => r.id).reverse();
  const label = (p: PullRequest) => `${p.repo.split("/")[1]}#${p.number}`;
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
    // Bit's notes, as the engine would have written them.
    buddyFeed: {
      notes: [
        {
          id: 1,
          at: ago(30),
          kind: "clean",
          prId: prs[4].id,
          reviewId: clean[0].id,
          repo: null,
          text: `${label(prs[4])} is clean after 3 rounds! 🎉 4 → 1 → 0`,
        },
        {
          id: 2,
          at: ago(20),
          kind: "round_finished",
          prId: prs[2].id,
          reviewId: waiting[0].id,
          repo: null,
          text: `Round 2 of ${label(prs[2])} found 2 blocking findings. Over to the author!`,
        },
        {
          id: 3,
          at: ago(15),
          kind: "needs_you",
          prId: prs[1].id,
          reviewId: limited[0].id,
          repo: null,
          text: `Could you take a look at ${label(prs[1])}? Round limit reached.`,
        },
        {
          id: 4,
          at: ago(10),
          kind: "needs_you",
          prId: null,
          reviewId: null,
          repo: "acme/dashboard",
          text: "Heads up! acme/dashboard failed to sync 3 times in a row: GitHub returned 404 Not Found.",
        },
      ],
    },
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
    claude: { ready: true, version: "demo" },
    claudeModels: [
      ["claude-opus-5-5", "Claude Opus 5.5", true],
      ["claude-sonnet-5-5", "Claude Sonnet 5.5", false],
    ].map(([model, displayName, isDefault]) => ({
      id: model as string,
      model: model as string,
      displayName: displayName as string,
      defaultReasoningEffort: "high",
      supportedReasoningEfforts: ["low", "medium", "high", "xhigh", "max"].map(
        (reasoningEffort) => ({ reasoningEffort, description: "" }),
      ),
      isDefault: isDefault as boolean,
    })),
    antigravity: { ready: true },
    antigravityModels: [
      ["gemini-3.1-pro-high", "Gemini 3.1 Pro (High)"],
      ["gemini-3.8-flash-medium", "Gemini 3.8 Flash (Medium)"],
    ].map(([model, displayName]) => ({
      id: model,
      model,
      displayName,
      defaultReasoningEffort: "high",
      supportedReasoningEfforts: ["low", "medium", "high"].map(
        (reasoningEffort) => ({ reasoningEffort, description: "" }),
      ),
      isDefault: false,
    })),
    approvals: [],
    githubUser: "you",
    codexReady: true,
    modelCatalogReady: true,
    polling: false,
    lastPoll: ago(0),
    usage: {
      rateLimits: {
        limitId: "codex",
        primary: {
          usedPercent: 10,
          windowDurationMins: 300,
          resetsAt: Math.floor(Date.now() / 1000) + 3 * 3600,
        },
        secondary: {
          usedPercent: 42,
          windowDurationMins: 10080,
          resetsAt: Math.floor(Date.now() / 1000) + 4 * 86400,
        },
        planType: "pro",
      },
      otherLimits: [],
      updatedAt: ago(0),
    },
    scans: [demoScan("acme/api", 26 * 60)],
  };
}
/** The browser demo: the fresh workspace plus two weeks of history. */
export function demoWorkspace(): Snapshot {
  const s = freshDemo();
  const history = demoHistory();
  s.historyPrs = history.prs;
  s.reviews.push(...history.reviews);
  return s;
}
