import {
  defaults,
  type Snapshot,
  type PullRequest,
  type Review,
  type RepositorySummary,
} from "./types";
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
    prId: prs[0].id,
    headSha: prs[0].headSha,
    baseSha: prs[0].baseSha,
    status: "ready",
    model: "codex-default",
    effort: "high",
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
    automatic: false,
    isReview: true,
  };
  return {
    prs,
    reviews: [review],
    settings: {
      ...defaults,
      repositories: ["orbit", "api", "dashboard"].map((n) => ({
        name: `acme/${n}`,
        path: `~/Projects/${n}`,
        automatic: n === "orbit",
        setupCommand: "",
        networkAccess: false,
      })),
    },
    models: [
      {
        id: "codex-default",
        model: "codex-default",
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
    polling: false,
    lastPoll: ago(0),
  };
}
