import {
  activeStatuses,
  checkNames,
  defaults,
  loopStatusLabels,
  sameRepo,
  scanActive,
  selectionError,
  type BuddyNote,
  type BuddyNoteLine,
  type CiFailure,
  type Fix,
  type LoopStatus,
  type Proposal,
  type Question,
  type Finding,
  type Model,
  type ModelSelection,
  type Snapshot,
  type OpsThread,
  type OpsWrite,
  type Problem,
  type PullRequest,
  type RepoCi,
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
      number: 1,
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
      number: 2,
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
          number: i + 1,
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
          number: blocking + 1,
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
    blockingPriority: 1,
    summary: report,
    previousRound: previous
      ? {
          reviewId: previous.id,
          headSha: previous.headSha,
          verdict: previous.orbitVerdict ?? null,
          findings: earlier,
          // The author answered each one by its short ID.
          claims: priorFindings.map((p) => ({
            findingId: p.id,
            author: pr.author,
            createdAt: ago(minutesAgo + 10),
            kind:
              p.status === "declined_accepted"
                ? ("declined" as const)
                : ("fixed" as const),
            commit: p.status === "declined_accepted" ? null : head,
            commitCheck:
              p.status === "declined_accepted" ? null : ("in_change" as const),
            reason:
              p.status === "declined_accepted"
                ? "The name matches the other queue constants."
                : null,
          })),
          commentsIncluded: priorFindings.length,
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
        id: "demo-msg-checks",
        role: "system",
        text: "Dev container results:\nSetup (`npm ci`): passed in 38s\nOutput (the end of it):\n  added 412 packages in 37s\nChecks (`npm test`): failed with exit code 1 after 64s\nOutput (the end of it):\n  FAIL src/settings.test.ts > keeps the saved repository\n  expected 'acme/api' to be 'acme/orbit'\n  Tests  1 failed | 174 passed (175)\n",
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
    checks: {
      image: "orbit-devcontainer:acme.orbit-3f2a91c0d4e5",
      network: true,
      setup: {
        command: "npm ci",
        exitCode: 0,
        timedOut: false,
        seconds: 38,
        output: "added 412 packages in 37s",
      },
      check: {
        command: "npm test",
        exitCode: 1,
        timedOut: false,
        seconds: 64,
        output:
          "FAIL src/settings.test.ts > keeps the saved repository\nexpected 'acme/api' to be 'acme/orbit'\nTests  1 failed | 174 passed (175)",
      },
    },
    completed: true,
    blockingPriority: 2,
    automatic: false,
    isReview: true,
  };
  // Converged, waiting for its author, and out of rounds.
  const clean = demoLoop(prs[4], [4, 1, 0], 30);
  const waiting = demoLoop(prs[2], [3, 2], 20);
  const limited = demoLoop(prs[1], [3, 2, 2, 1], 15);
  const ids = (rounds: Review[]) => rounds.map((r) => r.id).reverse();
  const label = (p: PullRequest) => `${p.repo.split("/")[1]}#${p.number}`;
  const ci = demoCi(prs);
  const red = ci.problems![0];
  const redReason = `CI is failing on main in acme/orbit since ${red.sinceSha.slice(0, 7)}: ${checkNames(red.failing)}.`;
  return {
    prs,
    ...ci,
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
        fix: "continue_loop",
      },
      {
        key: "sync:acme/dashboard",
        kind: "sync_failing",
        prId: null,
        reviewId: null,
        approvalId: null,
        repo: "acme/dashboard",
        reason:
          "acme/dashboard can't sync. GitHub returned 404 Not Found. Orbit checks again every 2 minutes.",
        fix: "sync_now",
      },
      {
        key: `problem:${red.id}:${red.alerts}`,
        kind: "ci_failing",
        prId: null,
        reviewId: null,
        approvalId: null,
        repo: red.repo,
        reason: `${redReason} It still fails at ${red.sha.slice(0, 7)}.`,
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
          id: 6,
          at: ago(45),
          kind: "needs_you",
          prId: null,
          reviewId: null,
          repo: red.repo,
          text: `Heads up! ${redReason}`,
          key: `problem:${red.id}:${red.alerts}`,
        },
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
          id: 5,
          at: ago(25),
          kind: "decide",
          prId: "acme/api#318",
          reviewId: null,
          repo: "acme/api",
          text: "Something to decide on api#318: The builder relies on the nightly job to retire the old secret; the reviewer says nothing retires it if that job is off. Decide whether an old secret may stay valid when the nightly job is off.",
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
          key: `loop:${prs[1].id}:Round limit reached.`,
        },
        {
          id: 4,
          at: ago(10),
          kind: "needs_you",
          prId: null,
          reviewId: null,
          repo: "acme/dashboard",
          text: "Heads up! acme/dashboard can't sync. GitHub returned 404 Not Found. Orbit checks again every 2 minutes.",
          key: "sync:acme/dashboard",
        },
      ],
      memory: {
        general: ["You like short answers, with a button when there is one."],
        repositories: {
          "acme/api": ["Its reviews often flag missing tests for new routes."],
        },
      },
    },
    settings: {
      ...defaults,
      model: "demo-review-model",
      effort: "medium",
      writeUps: { enabled: true, writer: "codex" },
      repositories: ["orbit", "api", "dashboard"].map((n) => ({
        name: `acme/${n}`,
        path: `~/Projects/${n}`,
        automatic: n === "orbit",
        setupCommand: n === "api" ? "npm ci" : "",
        networkAccess: n === "api",
        roundLimit: 4,
        devContainer: { enabled: n === "api", checkCommand: "npm test" },
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
    devContainers: {
      ready: true,
      version: "Docker demo · Dev Container CLI demo",
    },
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
const sha = (start: string) => start + "0".repeat(40 - start.length);
/** CI as Orbit last read it: orbit's main fails, api's passes after failing
 * yesterday, dashboard can't be read, and one pull request fails its own
 * checks. */
export function demoCi(
  prs: PullRequest[],
): Pick<Snapshot, "ci" | "problems" | "ops" | "journal" | "failures"> {
  const failingPr = prs[2];
  const check = (name: string, url = "") => ({ name, url });
  const rustJob = "https://github.com/acme/orbit/actions/runs/1001/job/2001";
  const frontendJob = `https://github.com/${failingPr.repo}/actions/runs/1002/job/2002`;
  const ci: RepoCi[] = [
    {
      repo: "acme/orbit",
      branch: "main",
      head: {
        sha: sha("9f3c2a1"),
        state: "failing",
        failing: [check("CI / Rust tests", rustJob)],
        running: 0,
        passing: 3,
      },
      title: "Merge pull request #146 from acme/bump-tokio",
      pulls: [],
      complete: true,
      checkedAt: ago(1),
    },
    {
      repo: "acme/api",
      branch: "main",
      head: {
        sha: sha("4be81aa"),
        state: "passing",
        failing: [],
        running: 0,
        passing: 5,
      },
      title: "Rotate the signing keys",
      pulls: [],
      complete: true,
      checkedAt: ago(1),
    },
    {
      repo: "acme/dashboard",
      branch: "",
      head: { sha: "", state: "none", failing: [], running: 0, passing: 0 },
      title: "",
      pulls: [],
      complete: false,
      checkedAt: "",
      error:
        "Could not resolve to a Repository with the name 'acme/dashboard'.",
    },
  ];
  ci.find((c) => sameRepo(c.repo, failingPr.repo))?.pulls.push({
    number: failingPr.number,
    draft: false,
    ci: {
      sha: failingPr.headSha,
      state: "failing",
      failing: [check("CI / Frontend", frontendJob)],
      running: 0,
      passing: 2,
    },
  });
  const problems: Problem[] = [
    {
      id: "demo-problem-1",
      key: "ci:acme/orbit",
      kind: "default_branch",
      repo: "acme/orbit",
      branch: "main",
      sha: sha("9f3c2a1"),
      sinceSha: sha("7d41e09"),
      failing: [check("CI / Rust tests", rustJob)],
      openedAt: ago(45),
      updatedAt: ago(18),
      timeline: [
        { at: ago(45), text: "Failing on main at 7d41e09: CI / Rust tests." },
        { at: ago(18), text: "Still failing at 9f3c2a1." },
      ],
      alerts: 1,
    },
    {
      id: "demo-problem-2",
      key: `ci:${failingPr.id}`,
      kind: "pull_request",
      repo: failingPr.repo,
      prId: failingPr.id,
      branch: failingPr.headRef,
      sha: failingPr.headSha,
      sinceSha: failingPr.headSha,
      failing: [check("CI / Frontend", frontendJob)],
      openedAt: ago(20),
      updatedAt: ago(20),
      timeline: [
        {
          at: ago(20),
          text: `Failing on ${failingPr.headRef} at ${failingPr.headSha.slice(0, 7)}: CI / Frontend.`,
        },
      ],
      alerts: 0,
    },
    {
      id: "demo-problem-3",
      key: "ci:acme/api",
      kind: "default_branch",
      repo: "acme/api",
      branch: "main",
      sha: sha("c19e4d2"),
      sinceSha: sha("c19e4d2"),
      failing: [check("CI / Lint")],
      openedAt: ago(26 * 60 + 40),
      updatedAt: ago(26 * 60),
      closedAt: ago(26 * 60),
      closedReason: "Passed at 4be81aa.",
      timeline: [
        {
          at: ago(26 * 60 + 40),
          text: "Failing on main at c19e4d2: CI / Lint.",
        },
        { at: ago(26 * 60), text: "Passed at 4be81aa." },
      ],
      alerts: 1,
    },
  ];
  const item = (
    number: number,
    check: string,
    failing: boolean,
    since: string,
    at: string,
    url = "",
  ) => ({ number, check, url, failing, since, at });
  const thread = (
    t: Partial<OpsThread> & Pick<OpsThread, "key" | "kind" | "repo">,
  ): OpsThread => ({
    url: "",
    branch: "main",
    problemId: "",
    items: [],
    posted: "",
    closed: false,
    allowed: false,
    gone: false,
    tries: 0,
    updatedAt: ago(18),
    ...t,
  });
  const ops: OpsThread[] = [
    thread({
      key: "issue:demo-problem-1",
      kind: "issue",
      repo: "acme/orbit",
      number: 212,
      url: "https://github.com/acme/orbit/issues/212",
      problemId: "demo-problem-1",
      items: [
        {
          ...item(
            1,
            "CI / Rust tests",
            true,
            sha("7d41e09"),
            sha("9f3c2a1"),
            rustJob,
          ),
          answer: {
            kind: "taking",
            author: "acme-bot",
            at: ago(12),
            url: "https://github.com/acme/orbit/issues/212#issuecomment-930",
            replyId: 930,
            head: "",
          },
        },
      ],
    }),
    thread({
      key: `pr:${failingPr.id}`,
      kind: "comment",
      repo: failingPr.repo,
      number: failingPr.number,
      prId: failingPr.id,
      commentId: 901,
      url: `${failingPr.url}#issuecomment-901`,
      branch: failingPr.headRef,
      problemId: "demo-problem-2",
      items: [
        {
          ...item(
            1,
            "CI / Frontend",
            true,
            failingPr.headSha,
            failingPr.headSha,
            frontendJob,
          ),
          answer: {
            kind: "fixed",
            author: failingPr.author,
            at: ago(19),
            url: `${failingPr.url}#issuecomment-905`,
            replyId: 905,
            commit: failingPr.headSha.slice(0, 7),
            check: "fails",
            head: failingPr.headSha,
          },
        },
      ],
      updatedAt: ago(20),
    }),
    thread({
      key: "issue:demo-problem-3",
      kind: "issue",
      repo: "acme/api",
      number: 88,
      url: "https://github.com/acme/api/issues/88",
      problemId: "demo-problem-3",
      items: [item(1, "CI / Lint", false, sha("c19e4d2"), sha("4be81aa"))],
      closed: true,
      updatedAt: ago(25 * 60),
    }),
  ];
  const write = (
    id: string,
    action: OpsWrite["action"],
    t: OpsThread,
    at: string,
  ): OpsWrite => ({
    id,
    thread: t.key,
    action,
    repo: t.repo,
    number: t.number,
    state: "done",
    at,
    doneAt: at,
    url: t.url,
  });
  const journal: OpsWrite[] = [
    write("demo-write-1", "open_issue", ops[2], ago(26 * 60 + 39)),
    write("demo-write-2", "edit_issue", ops[2], ago(26 * 60)),
    write("demo-write-3", "note_on_issue", ops[2], ago(25 * 60)),
    write("demo-write-4", "close_issue", ops[2], ago(25 * 60)),
    write("demo-write-5", "open_issue", ops[0], ago(44)),
    write("demo-write-6", "post_comment", ops[1], ago(19)),
    write("demo-write-7", "edit_issue", ops[0], ago(18)),
  ];
  const failures: CiFailure[] = [
    {
      url: rustJob,
      check: "CI / Rust tests",
      step: "Run cargo test",
      lines: [
        "running 214 tests",
        "test review_loop::tests::stops_when_clear ... ok",
        "test engine::tests::sync_reads_new_heads ... FAILED",
        "failures:",
        "---- engine::tests::sync_reads_new_heads stdout ----",
        "thread 'engine::tests::sync_reads_new_heads' panicked at src/engine/sync_tests.rs:88:5:",
        "assertion `left == right` failed",
        "  left: 2",
        " right: 3",
        "test result: FAILED. 213 passed; 1 failed; 0 ignored",
        "Error: Process completed with exit code 101.",
      ],
      held: false,
      readAt: ago(17),
    },
    {
      url: frontendJob,
      check: "CI / Frontend",
      step: "Run npm test",
      lines: [
        " FAIL  src/Inbox.test.tsx > shows the newest pull request first",
        "AssertionError: expected 'Fix the login page' to be 'Add dark mode'",
        " Test Files  1 failed | 14 passed (15)",
        "Error: Process completed with exit code 1.",
      ],
      held: false,
      readAt: ago(19),
    },
  ];
  for (const t of ops)
    for (const i of t.items)
      if (i.failing) i.why = failures.find((f) => f.url === i.url) ?? null;
  return { ci, problems, ops, journal, failures };
}
/** The browser demo: the fresh workspace plus two weeks of history. */
export function demoWorkspace(): Snapshot {
  const s = freshDemo();
  const history = demoHistory();
  s.historyPrs = history.prs;
  s.reviews.push(...history.reviews);
  return s;
}

const plural = (n: number, one: string, many: string) =>
  `${n} ${n === 1 ? one : many}`;
/** `orbit#41` for a PR the demo knows. */
function prLabel(s: Snapshot, prId: string | null) {
  const p = [...s.prs, ...(s.historyPrs ?? [])].find((p) => p.id === prId);
  return p ? `${p.repo.split("/")[1]}#${p.number}` : (prId ?? "");
}
const line = (
  text: string,
  prId: string | null = null,
  key: string | null = null,
): BuddyNoteLine => ({ text, prId, key });
/** What Bit says when a fix goes through, as the engine words it in the
 * cheerful tone. */
export function demoFixed(s: Snapshot, fix: Fix, prId: string | null) {
  const pr = prLabel(s, prId) || "this pull request";
  return {
    continue_loop: `Gave ${pr} two more rounds!`,
    review_again: `Queued a new review of ${pr}. Fingers crossed!`,
    publish: `Published the report on ${pr}! 📬`,
    sync_now: "Synced with GitHub. All good again!",
    turn_on_autopilot: "Autopilot is back on!",
    check_connections: "It's connected again. All good!",
  }[fix];
}
/** Bit's answer to a quick question, from the demo's state, as the engine
 * words it in the cheerful tone. */
export function demoAnswer(
  s: Snapshot,
  question: Question,
): Pick<BuddyNote, "text" | "lines" | "proposals"> {
  if (question === "setup") return demoSetup(s);
  const items = s.attention ?? [];
  const autopilot = (text: string) =>
    s.settings.automatic
      ? text
      : `Autopilot is off, so nothing is reviewed automatically. ${text}`;
  if (question === "needs_you") {
    const lines = items.map((a) =>
      line(
        a.prId ? `${prLabel(s, a.prId)}: ${a.reason}` : a.reason,
        a.prId,
        a.key,
      ),
    );
    return {
      text: lines.length
        ? `${plural(lines.length, "thing needs you", "things need you")}. Here's the list:`
        : "Nothing needs you right now. 🎉",
      lines,
    };
  }
  if (question === "running") {
    const lines: BuddyNoteLine[] = [];
    if (s.polling) lines.push(line("Syncing with GitHub."));
    for (const r of s.reviews.filter((r) =>
      activeStatuses.includes(r.status),
    )) {
      const state =
        {
          queued: "is queued",
          preparing: "is getting ready",
          awaitingApproval: "is waiting for your approval",
        }[r.status as string] ?? "is running";
      lines.push(
        line(
          `${prLabel(s, r.prId)}: ${r.automatic ? "a round" : "a review"} ${state}.`,
          r.prId,
        ),
      );
    }
    for (const scan of (s.scans ?? []).filter(scanActive))
      lines.push(line(`${scan.repo}: a security scan is running.`));
    return {
      text: autopilot(
        lines.length
          ? `${plural(lines.length, "thing is running", "things are running")}. Here's what:`
          : "Nothing is running. All quiet!",
      ),
      lines,
    };
  }
  if (question === "loops") {
    const open = (s.loopStates ?? []).filter((l) => !l.closed && l.rounds > 0);
    const status = (l: { status: LoopStatus }) =>
      l.status === "waiting_for_author"
        ? "waiting for the author"
        : l.status === "stopped"
          ? "paused"
          : loopStatusLabels[l.status].toLowerCase();
    const lines = open.map((l) => {
      const reason = l.status === "needs_you" && l.reason ? ` ${l.reason}` : "";
      const about = items.filter((a) => a.prId === l.prId);
      const item = about.find((a) => a.fix) ?? about[0];
      return line(
        `${prLabel(s, l.prId)}: round ${l.rounds} of ${l.roundLimit}, ${status(l)}.${reason}`,
        l.prId,
        item?.key ?? null,
      );
    });
    const order: LoopStatus[] = [
      "needs_you",
      "reviewing",
      "waiting_for_author",
      "clean",
      "stopped",
    ];
    const counts = order.flatMap((kind) => {
      const n = open.filter((l) => l.status === kind).length;
      return n ? [`${n} ${status({ status: kind })}`] : [];
    });
    return {
      text: open.length
        ? `${plural(open.length, "loop is open", "loops are open")}: ${counts.join(", ")}. Here's each one:`
        : "No loops are open right now.",
      lines,
    };
  }
  const repositories = s.settings.repositories;
  const lines = repositories.map((repo) => {
    const open = s.prs.filter((p) => sameRepo(p.repo, repo.name)).length;
    const about = (kind: string) =>
      items.find(
        (a) => a.kind === kind && !!a.repo && sameRepo(a.repo, repo.name),
      );
    const sync = about("sync_failing");
    const item = sync ?? about("ci_failing");
    const failing = sync ? ` ${sync.reason}` : "";
    const ci =
      (s.settings.watchCi ?? true)
        ? s.ci?.find((c) => sameRepo(c.repo, repo.name))
        : undefined;
    const branch = ci?.branch || "the default branch";
    const checks = !ci?.head.sha
      ? ""
      : {
          failing: ` CI is failing on ${branch}: ${checkNames(ci.head.failing)}.`,
          running: ` CI is running on ${branch}.`,
          passing: ` CI is passing on ${branch}.`,
          none: "",
        }[ci.head.state];
    return line(
      `${repo.name}: ${plural(open, "open pull request", "open pull requests")}, autopilot ${repo.automatic ? "on" : "off"}.${failing}${checks}`,
      null,
      item?.key ?? null,
    );
  });
  return {
    text: autopilot(
      repositories.length
        ? `You have ${plural(repositories.length, "repository", "repositories")}. Here's how they're doing:`
        : "No repositories yet. Add one under Repositories and I'll keep an eye on it!",
    ),
    lines,
  };
}
/** Bit's setup help in the demo, where everything is connected and no
 * copy suggests a setup command, as the engine words it in the cheerful
 * tone. */
function demoSetup(
  s: Snapshot,
): Pick<BuddyNote, "text" | "lines" | "proposals"> {
  const lines = [
    line(`GitHub: connected as @${s.githubUser ?? "demo"}.`),
    line("Codex: connected."),
  ];
  const proposals: Proposal[] = [];
  const repositories = s.settings.repositories;
  if (!repositories.length)
    lines.push(
      line(
        "No repositories are connected yet, and GitHub lists none of yours to connect. You can add one under Repositories.",
      ),
    );
  for (const repo of repositories) {
    const command = repo.setupCommand.trim();
    const setup = command
      ? `${repo.name} sets up with \`${command}\`.`
      : `${repo.name} has no setup command, and nothing in its copy suggests one, so reviews use only what's already installed on the computer running Orbit.`;
    if (repo.automatic) {
      lines.push(
        line(`${setup} Its pull requests are reviewed automatically.`),
      );
      continue;
    }
    proposals.push({
      id: proposals.length,
      action: "set_up_repository",
      repo: repo.name,
      repository: { autopilot: true },
    });
    lines.push(line(`${setup} Automatic reviews are off.`));
  }
  if (!s.settings.automatic && repositories.length) {
    proposals.push({
      id: proposals.length,
      action: "change_settings",
      settings: { automatic: true },
    });
    lines.push(
      line(
        "Autopilot is off for all of Orbit, so nothing is reviewed automatically.",
      ),
    );
  }
  const toDo = proposals.length + (repositories.length ? 0 : 1);
  return {
    text: toDo
      ? `Let's get you set up! ${plural(toDo, "thing", "things")} to do. Tap a button when you're ready:`
      : "You're all set! Everything's connected and ready. 🎉",
    lines,
    proposals,
  };
}
/** Bit's answer to a typed question in the demo, which has no Codex to
 * think with. Its buttons show how answers offer Orbit's controls. */
export function demoTypedAnswer(): Pick<
  BuddyNote,
  "text" | "proposals" | "memory"
> {
  return {
    text: "This is the demo, so I can't think about your question for real. In Orbit, Codex answers here using what Orbit knows, and I can offer buttons for Orbit's own controls. They only run when you tap them, like these:",
    proposals: [
      { id: 0, action: "sync_now" },
      { id: 1, action: "change_settings", settings: { pollSeconds: 300 } },
      {
        id: 2,
        action: "set_up_repository",
        repo: "acme/dashboard",
        repository: { setupCommand: "npm ci", networkAccess: true },
      },
    ],
    memory: [
      {
        repo: null,
        line: "You've tried asking me something in the demo.",
        kind: "remember",
      },
    ],
  };
}
