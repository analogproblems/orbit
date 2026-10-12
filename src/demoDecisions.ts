// The browser demo's Decisions page: a few pull requests in the demo's
// repositories, pieced together as the engine would from GitHub, and
// written up as a writer might.
import type {
  Decision,
  DecisionKind,
  Decisions,
  EarlierFinding,
  Flag,
  History,
  LedgerClaim,
  LedgerFinding,
  LedgerRound,
  Metrics,
  RoundVerdict,
  Ruling,
  Trajectory,
  WriteUp,
} from "./decisions";
import type { Category, PriorStatus } from "./types";

const at = (minutesAgo: number) =>
  new Date(Date.now() - minutesAgo * 60_000).toISOString();

function finding(
  review: string,
  n: number,
  title: string,
  priority: number,
  category: Category,
  problem: string,
  path: string,
  line: number,
): LedgerFinding {
  return {
    id: `${review}-${n}`,
    shortId: `F${n}`,
    number: n,
    title,
    priority,
    blocking: priority <= 1,
    category,
    locations: [{ path, startLine: line, endLine: line }],
    problem,
    confirmed: true,
    inline: true,
  };
}
function round(
  url: string,
  n: number,
  verdict: RoundVerdict,
  minutesAgo: number,
  findings: LedgerFinding[] = [],
  earlier: EarlierFinding[] = [],
  extra: Partial<LedgerRound> = {},
): LedgerRound {
  return {
    reviewId: `rev-${url.length}-${n}`,
    format: "orbit",
    round: n,
    headSha: `${n}a7c${"0".repeat(36)}`,
    verdict,
    blocking:
      verdict === "clean"
        ? 0
        : findings.filter((f) => f.blocking).length +
          earlier.filter(
            (e) =>
              (e.priority ?? 3) <= 1 &&
              (e.status === "still_open" || e.status === "declined_disputed"),
          ).length,
    blockingPriority: 1,
    reviewer: "Codex",
    summary: null,
    findings,
    earlierFindings: earlier,
    notChecked: [],
    commentId: 1000 + n,
    url: `${url}#pullrequestreview-${1000 + n}`,
    createdAt: at(minutesAgo),
    ...extra,
  };
}
function earlier(
  f: LedgerFinding,
  status: PriorStatus,
  note: string,
): EarlierFinding {
  return {
    id: f.id,
    shortId: f.shortId,
    title: f.title,
    priority: f.priority,
    status,
    note,
  };
}
function claim(
  url: string,
  minutesAgo: number,
  n: number,
  kind: "fixed" | "declined",
  detail: string,
): LedgerClaim {
  return {
    commentId: 2000 + n,
    url: `${url}#issuecomment-${2000 + n}`,
    createdAt: at(minutesAgo),
    author: "alex",
    numbers: [n],
    kind,
    commit: kind === "fixed" ? detail : null,
    reason: kind === "declined" ? detail : null,
    scope: "named",
  };
}
function path(
  f: LedgerFinding,
  opened: LedgerRound,
  rulings: Ruling[],
  said: LedgerClaim | null,
  openAtMerge = false,
): Trajectory {
  const last = rulings.at(-1)?.status;
  return {
    findingId: f.id,
    shortId: f.shortId,
    openedRound: opened.round ?? 1,
    openedAt: opened.createdAt,
    priority: f.priority,
    blocking: f.blocking,
    category: f.category,
    title: f.title,
    claim: said,
    rulings,
    finalStatus: last ?? "open",
    minutesToClaim: said
      ? Math.round(
          (Date.parse(said.createdAt) - Date.parse(opened.createdAt)) / 60_000,
        )
      : null,
    openAtMerge,
  };
}
function metrics(
  rounds: LedgerRound[],
  paths: Trajectory[],
  created: string,
  merged: string | null,
): Metrics {
  const minutes = (a: string, b: string) =>
    Math.round((Date.parse(b) - Date.parse(a)) / 60_000);
  const clean = rounds.find((r) => r.verdict === "clean");
  const count = (s: Trajectory["finalStatus"]) =>
    paths.filter((t) => t.finalStatus === s).length;
  const last = rounds.at(-1);
  return {
    rounds: rounds.length,
    findings: paths.length,
    blockingFindings: paths.filter((t) => t.blocking).length,
    fixed: count("fixed"),
    declined: count("declined_accepted") + count("declined_disputed"),
    disputed: count("declined_disputed"),
    openAtMerge: paths.filter((t) => t.openAtMerge).length,
    minutesToClean: clean ? minutes(created, clean.createdAt) : null,
    minutesToMerge: merged ? minutes(created, merged) : null,
    firstVerdict: rounds[0]?.verdict ?? null,
    finalVerdict: last?.verdict ?? null,
    mergedByLoop: merged
      ? last?.verdict === "clean" && minutes(last.createdAt, merged) <= 45
      : null,
    verificationGaps: last?.notChecked.length ?? 0,
    likelyNotVerified: 0,
  };
}
function history(
  repo: string,
  number: number,
  title: string,
  opened: number,
  merged: number | null,
  narrative: Partial<History["narrative"]>,
  rounds: LedgerRound[],
  paths: Trajectory[],
  claims: LedgerClaim[],
  flags: Omit<Flag, "url">[],
): History {
  const url = `https://github.com/${repo}/pull/${number}`;
  const createdAt = at(opened);
  const mergedAt = merged == null ? null : at(merged);
  return {
    repo,
    number,
    title,
    url,
    state: mergedAt ? "closed" : "open",
    merged: !!mergedAt,
    draft: false,
    author: "alex",
    createdAt,
    updatedAt: mergedAt ?? rounds.at(-1)?.createdAt ?? createdAt,
    closedAt: mergedAt,
    mergedAt,
    headRef: `change/${number}`,
    baseRef: "main",
    additions: 40 + (number % 50),
    deletions: 8 + (number % 9),
    changedFiles: 2 + (number % 4),
    fetchedAt: at(2),
    narrative: { closes: [], sections: [], ...narrative },
    rounds,
    claims,
    trajectories: paths,
    metrics: metrics(rounds, paths, createdAt, mergedAt),
    flags: flags.map((f) => ({ ...f, url })),
  };
}

/** Six pull requests: a loop that took three rounds, a disputed decline, a
 * clean first round, one still open, one merged with a finding open, and one
 * merged without a review. */
export function demoDecisions(): Decisions {
  const prs: History[] = [];

  {
    const url = "https://github.com/acme/orbit/pull/142";
    const f1 = finding(
      "rev142a",
      1,
      "Saving defaults drops each repository's overrides",
      1,
      "bug",
      "`saveConfig` replaces the whole config with the defaults, so a repository's own model and setup command are lost.",
      "src/config.ts",
      44,
    );
    const f2 = finding(
      "rev142a",
      2,
      "No test for a repository without overrides",
      2,
      "test_gap",
      "The new merge is only tested with overrides present.",
      "src/config.test.ts",
      12,
    );
    const r1 = round(url, 1, "changes_requested", 3 * 1440, [f1, f2]);
    const r2 = round(
      url,
      2,
      "changes_requested",
      3 * 1440 - 120,
      [],
      [
        earlier(
          f1,
          "still_open",
          "Only the model is kept; the setup command is still dropped.",
        ),
        earlier(
          f2,
          "declined_accepted",
          "The settings round-trip test covers it.",
        ),
      ],
    );
    const r3 = round(
      url,
      3,
      "clean",
      3 * 1440 - 200,
      [],
      [earlier(f1, "fixed", "Both overrides are kept now.")],
    );
    const c1 = claim(url, 3 * 1440 - 60, 1, "fixed", "4c1d2e9");
    const c2 = claim(
      url,
      3 * 1440 - 58,
      2,
      "declined",
      "the settings round-trip test already saves a repository with no overrides",
    );
    prs.push(
      history(
        "acme/orbit",
        142,
        "Keep repository overrides when saving defaults",
        3 * 1440 + 30,
        3 * 1440 - 220,
        {
          before:
            "Saving the default settings replaced each repository's own model and setup command.",
          after:
            "Saving defaults keeps every repository's overrides; only the defaults change.",
          how: "`saveConfig` merges the defaults into the current config instead of replacing it.",
          decisions:
            "Overrides win over defaults everywhere, including for repositories added later.",
          checks:
            "`npm test`, and saving defaults by hand with two repositories.",
          epic: 30,
          stage: "2",
        },
        [r1, r2, r3],
        [
          path(
            f1,
            r1,
            [
              {
                round: 2,
                at: r2.createdAt,
                status: "still_open",
                note: "Only the model is kept.",
              },
              {
                round: 3,
                at: r3.createdAt,
                status: "fixed",
                note: "Both overrides are kept now.",
              },
            ],
            c1,
          ),
          path(
            f2,
            r1,
            [
              {
                round: 2,
                at: r2.createdAt,
                status: "declined_accepted",
                note: "The settings round-trip test covers it.",
              },
            ],
            c2,
          ),
        ],
        [c1, c2],
        [],
      ),
    );
  }

  {
    const url = "https://github.com/acme/api/pull/318";
    const f1 = finding(
      "rev318a",
      1,
      "Signature compared in variable time",
      0,
      "security",
      "`verify` compares the signature with `==`, which leaks how many bytes matched.",
      "src/webhooks/verify.ts",
      22,
    );
    const f2 = finding(
      "rev318a",
      2,
      "The old secret is accepted forever",
      1,
      "security",
      "After a rotation the previous secret stays valid with no end date.",
      "src/webhooks/secrets.ts",
      58,
    );
    const r1 = round(url, 1, "changes_requested", 2 * 1440, [f1, f2]);
    const r2 = round(
      url,
      2,
      "changes_requested",
      2 * 1440 - 150,
      [],
      [
        earlier(f1, "fixed", "Uses a constant-time comparison now."),
        earlier(
          f2,
          "declined_disputed",
          "The rotation job can be switched off, and then nothing expires the old secret.",
        ),
      ],
      {
        notChecked: [
          "the webhook integration tests, which need a live endpoint",
        ],
      },
    );
    const c1 = claim(url, 2 * 1440 - 90, 1, "fixed", "9be03f1");
    const c2 = claim(
      url,
      2 * 1440 - 88,
      2,
      "declined",
      "the nightly rotation job expires the old secret after a day",
    );
    prs.push(
      history(
        "acme/api",
        318,
        "Sign webhook payloads with a rotating secret",
        2 * 1440 + 20,
        2 * 1440 - 170,
        {
          before: "Webhooks were signed with one secret that never changed.",
          after:
            "Webhooks are signed with a secret that rotates every night; receivers accept the previous one for a day.",
          how: "A `secrets` table keeps the current and previous secret; `verify` tries both in constant time.",
          checks: "`npm test` and a local webhook round trip.",
        },
        [r1, r2],
        [
          path(
            f1,
            r1,
            [
              {
                round: 2,
                at: r2.createdAt,
                status: "fixed",
                note: "Uses a constant-time comparison now.",
              },
            ],
            c1,
          ),
          path(
            f2,
            r1,
            [
              {
                round: 2,
                at: r2.createdAt,
                status: "declined_disputed",
                note: "The rotation job can be switched off, and then nothing expires the old secret.",
              },
            ],
            c2,
            true,
          ),
        ],
        [c1, c2],
        [
          {
            kind: "disputed_decline",
            attention: "act",
            text: "The builder declined F2 (The old secret is accepted forever) and the reviewer still thinks it is a real problem.",
            findingId: f2.id,
            round: 1,
          },
          {
            kind: "reviewer_could_not_check",
            attention: "look",
            text: "The reviewer could not check: the webhook integration tests, which need a live endpoint",
            round: 2,
          },
        ],
      ),
    );
  }

  {
    const url = "https://github.com/acme/dashboard/pull/77";
    const r1 = round(url, 1, "clean", 1440 - 25, [], [], {
      summary:
        "The new column reads from the existing build table; nothing to fix.",
    });
    prs.push(
      history(
        "acme/dashboard",
        77,
        "Show build times per branch",
        1440,
        1440 - 40,
        {
          after:
            "The builds table shows how long each branch's latest build took.",
          how: "A `duration` column computed from the build's start and end.",
        },
        [r1],
        [],
        [],
        [],
      ),
    );
  }

  {
    const url = "https://github.com/acme/api/pull/321";
    const f1 = finding(
      "rev321a",
      1,
      "The last page drops one entry",
      2,
      "bug",
      "`offset + limit` is used as an exclusive end, so the final entry on the last page is skipped.",
      "src/audit/list.ts",
      31,
    );
    const r1 = round(url, 1, "clean", 300, [f1]);
    const c1 = claim(url, 240, 1, "fixed", "b72e4a0");
    prs.push(
      history(
        "acme/api",
        321,
        "Paginate the audit log",
        330,
        null,
        {
          before: "The audit log endpoint returned every entry at once.",
          after:
            "It returns 100 entries a page with a cursor for the next one.",
        },
        [r1],
        [path(f1, r1, [], c1)],
        [c1],
        [],
      ),
    );
  }

  {
    const url = "https://github.com/acme/orbit/pull/139";
    const f1 = finding(
      "rev139a",
      1,
      "The retry loop never gives up",
      1,
      "bug",
      "A clone that always fails is retried forever, holding the queue.",
      "src/clone.ts",
      88,
    );
    const r1 = round(url, 1, "changes_requested", 5 * 1440, [f1]);
    prs.push(
      history(
        "acme/orbit",
        139,
        "Retry flaky clone fetches",
        5 * 1440 + 15,
        5 * 1440 - 120,
        {
          after: "A clone fetch that fails with a network error is retried.",
        },
        [r1],
        [path(f1, r1, [], null, true)],
        [],
        [
          {
            kind: "merged_with_open_findings",
            attention: "act",
            text: "Merged while F1 (The retry loop never gives up) was still open.",
            findingId: f1.id,
            round: 1,
          },
        ],
      ),
    );
  }

  prs.push(
    history(
      "acme/dashboard",
      75,
      "Update the chart library",
      6 * 1440,
      6 * 1440 - 30,
      {
        lead: "Routine update of the chart library to its latest minor version.",
      },
      [],
      [],
      [],
      [
        {
          kind: "no_review_before_merge",
          attention: "look",
          text: "Merged before any reviewer round Orbit can see.",
        },
      ],
    ),
  );
  for (const pr of prs) pr.writeup = demoWriteUp(pr);
  prs.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
  return { prs, problems: [] };
}

/** What a writer might make of three of the merged pull requests. */
function demoWriteUp(pr: History): WriteUp | null {
  const reply = (n: number) =>
    pr.claims.find((c) => c.numbers.includes(n))?.url ?? pr.url;
  const decision = (
    kind: DecisionKind,
    statement: string,
    more: Partial<Decision> = {},
  ): Decision => ({
    kind,
    statement,
    madeBy: "builder",
    area: "Settings",
    url: pr.url,
    ...more,
  });
  const writeup = (
    summary: string,
    decisions: Decision[],
    findingNotes: WriteUp["findingNotes"] = [],
  ): WriteUp => ({
    repo: pr.repo,
    number: pr.number,
    writer: "Codex",
    model: "Codex · demo",
    writtenAt: pr.mergedAt ?? pr.updatedAt,
    summary,
    areas: [...new Set(decisions.map((d) => d.area))],
    decisions,
    findingNotes,
  });
  const finding = (shortId: string) =>
    pr.trajectories.find((t) => t.shortId === shortId)!.findingId;
  switch (`${pr.repo}#${pr.number}`) {
    case "acme/orbit#142":
      return writeup(
        "Saving your default settings no longer wipes the model and setup command you chose for each repository. The reviewer caught that the first fix only kept the model, and it took a third round to keep both.",
        [
          decision(
            "intent",
            "Saving defaults should change only the defaults, never a repository's own choices.",
            {
              quote:
                "Saving defaults keeps every repository's overrides; only the defaults change.",
            },
          ),
          decision(
            "rule",
            "A repository's own settings always win over the defaults, including for repositories added later.",
            {
              quote:
                "Overrides win over defaults everywhere, including for repositories added later.",
              madeBy: "both",
            },
          ),
          decision(
            "challenge",
            "The first fix still lost each repository's setup command when you saved defaults.",
            {
              madeBy: "reviewer",
              quote:
                "Only the model is kept; the setup command is still dropped.",
              url: pr.rounds[1]?.url ?? pr.url,
            },
          ),
          decision(
            "pushback",
            "No separate test was added for a repository without its own settings; the reviewer accepted that an existing test covers it.",
            { url: reply(2), madeBy: "both" },
          ),
          decision(
            "verification",
            "The tests were run, and saving defaults was tried by hand with two repositories.",
            {
              quote:
                "`npm test`, and saving defaults by hand with two repositories.",
            },
          ),
        ],
        [
          {
            findingId: finding("F1"),
            plain:
              "Saving your defaults would have silently undone every repository's own settings.",
          },
          {
            findingId: finding("F2"),
            plain:
              "A repository with no settings of its own wasn't tested on its own.",
          },
        ],
      );
    case "acme/api#318":
      return writeup(
        "Webhooks are now signed with a secret that changes every night, and receivers accept yesterday's for a day. The reviewer and the builder still disagree about what happens to the old secret if the nightly job is switched off.",
        [
          decision(
            "intent",
            "Webhooks should be signed with a secret that changes regularly.",
            { area: "Webhooks" },
          ),
          decision(
            "rule",
            "Receivers accept the previous secret for one day after it changes.",
            {
              area: "Webhooks",
              quote: "receivers accept the previous one for a day",
            },
          ),
          decision(
            "rule",
            "A webhook's signature is checked in a way that doesn't reveal how much of it matched.",
            { area: "Webhooks", madeBy: "both", url: reply(1) },
          ),
          decision(
            "pushback",
            "The builder relies on the nightly job to retire the old secret; the reviewer says nothing retires it if that job is off.",
            {
              area: "Webhooks",
              madeBy: "both",
              url: reply(2),
              quote:
                "the nightly rotation job expires the old secret after a day",
              attention: "act",
              attentionReason:
                "Decide whether an old secret may stay valid when the nightly job is off.",
            },
          ),
          decision(
            "verification",
            "The webhook tests that need a live endpoint weren't run.",
            {
              area: "Webhooks",
              madeBy: "reviewer",
              attention: "look",
              attentionReason: "Nobody checked a real delivery.",
            },
          ),
        ],
        [
          {
            findingId: finding("F1"),
            plain:
              "Someone could have guessed a valid signature a little at a time.",
          },
          {
            findingId: finding("F2"),
            plain:
              "An old secret could keep working forever if the nightly job stopped.",
          },
        ],
      );
    case "acme/dashboard#77":
      return writeup(
        "The builds table now shows how long each branch's latest build took. The reviewer found nothing to fix.",
        [
          decision("intent", "Show how long each branch's latest build took.", {
            area: "Builds Table",
          }),
          decision(
            "tradeoff",
            "The time is worked out from each build's start and end rather than stored, so older builds show it too.",
            {
              area: "Builds Table",
              attention: "look",
              attentionReason: "A build still running has no end yet.",
            },
          ),
        ],
      );
    default:
      return null;
  }
}
