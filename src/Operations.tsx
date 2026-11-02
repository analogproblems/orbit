import {
  Button,
  Card,
  CodeBlock,
  EmptyState,
  Heading,
  HStack,
  List,
  ListItem,
  StatusDot,
  Text,
  Timestamp,
  Token,
  VStack,
} from "@astryxdesign/core";
import { FolderGit2, Workflow } from "lucide-react";
import { openLink, useOrbit } from "./store";
import { SaveFeedback } from "./SaveFeedback";
import {
  alertNeedsYou,
  buddyOf,
  checkNames,
  readinessLine,
  sameRepo,
  shortSha,
  trustedSource,
  type ActionsStorage,
  type AlertSeverity,
  type AlertSource,
  type AttentionHandlers,
  type Broke,
  type CiFailure,
  type CiState,
  type OpsAction,
  type OpsItem,
  type OpsThread,
  type OpsWrite,
  type Problem,
  type RepoCi,
  type Runner,
  type RunnerPlace,
  type SecurityAlert,
  type Snapshot,
  type WorkflowRisk,
} from "./types";

/** How many closed problems the page lists, and how many writes. */
const RECENT = 10;
/** How many of a problem's latest events it shows. */
const EVENTS = 5;
/** How many waiting runs a problem lists, and runners a place lists. */
const RUNS = 5;
const RUNNERS = 10;
/** How many of each kind of security alert a repository lists. */
const ALERTS = 5;
/** A gigabyte as GitHub counts storage, GitHub's usual limit for a
 * repository's caches, and how near it counts as near, as the engine has
 * them (`actions_storage`). */
const GB = 2 ** 30;
const CACHE_LIMIT = 10 * GB;
const NEAR = 9 * GB;

const dots: Record<
  CiState,
  { variant: "success" | "error" | "accent" | "neutral"; label: string }
> = {
  passing: { variant: "success", label: "Passing" },
  failing: { variant: "error", label: "Failing" },
  running: { variant: "accent", label: "Running" },
  none: { variant: "neutral", label: "No checks" },
};

/** Where a repository's default branch's checks stand, in a sentence. */
export function branchSummary(ci: RepoCi) {
  const branch = ci.branch || "the default branch";
  if (!ci.head.sha) return "No commits to check yet.";
  const at = shortSha(ci.head.sha);
  if (ci.head.incomplete && ci.head.state !== "failing")
    return `Orbit couldn’t read every check on ${branch} at ${at}, so it can’t say it passes.`;
  switch (ci.head.state) {
    case "failing":
      return `Failing on ${branch} at ${at}: ${checkNames(ci.head.failing)}.`;
    case "running":
      return `Checks are running on ${branch} at ${at}.`;
    case "passing":
      return `Passing on ${branch} at ${at}.`;
    default:
      return `No checks ran on ${branch} at ${at}.`;
  }
}

/** Whether some of a repository's checks that Orbit follows are unread: it
 * wasn't read, its last read failed, or its default branch or a pull
 * request Orbit follows has more checks than Orbit read. */
function partlyRead(s: Snapshot, repo: string) {
  const ci = s.ci?.find((c) => sameRepo(c.repo, repo));
  if (!ci?.checkedAt || ci.error || ci.head.incomplete) return true;
  return ci.pulls.some(
    (pull) =>
      pull.ci.incomplete &&
      s.prs.some(
        (pr) =>
          sameRepo(pr.repo, repo) &&
          pr.number === pull.number &&
          pr.headSha === pull.ci.sha &&
          !pr.draft &&
          trustedSource(pr, s.settings.trustedAuthors),
      ),
  );
}

/** Whether a problem's agent hears of it in an issue rather than a
 * comment on a pull request. */
function opensIssue(p: Problem) {
  return (
    p.kind === "default_branch" ||
    p.kind === "security" ||
    p.kind === "workflow_risks" ||
    p.kind === "no_ci"
  );
}

/** Whether a problem is yours to see to: a failing default branch, or runs
 * waiting for a runner. */
function needsYou(p: Problem) {
  return p.kind === "default_branch" || p.kind === "waiting";
}

/** Orbit's comment or issue about a problem. */
function threadOf(s: Snapshot, p: Problem) {
  return s.ops?.find((t) => t.problemId === p.id);
}

/** The rule for posting about a problem. */
function ruleOf(s: Snapshot, p: Problem) {
  const { rules } = buddyOf(s.settings);
  if (p.kind === "no_ci") return rules.propose_ci;
  return opensIssue(p) ? rules.open_issues : rules.comment_on_prs;
}

/** Whether nothing about a problem's security alerts or risky workflow
 * settings goes to GitHub, as GitHub last said its repository isn't
 * private, as the engine has it (`withheld`). What is already on GitHub
 * stays as it is. */
function withheld(p: Problem) {
  return (p.kind === "security" || p.kind === "workflow_risks") && !p.private;
}

/** Whether Bit waits for your tap before it posts about a problem. */
function asksFirst(s: Snapshot, p: Problem, t: OpsThread | undefined) {
  const buddy = buddyOf(s.settings);
  const rule = ruleOf(s, p);
  return (
    !!t &&
    !withheld(p) &&
    !t.url &&
    !t.unsure &&
    buddy.enabled &&
    rule === "ask_first" &&
    !t.allowed
  );
}

/** "A", "A and B", "A, B and C". */
function and(parts: string[]) {
  return parts.length < 2
    ? (parts[0] ?? "")
    : `${parts.slice(0, -1).join(", ")} and ${parts.at(-1)}`;
}

/** Where a default branch broke, as Orbit's issue says it, or why Orbit
 * can't say yet. */
export function brokeText(b: Broke | null | undefined) {
  if (!b) return undefined;
  if (!b.done || !b.commits.length) return b.error ?? undefined;
  const [red, green, n] = [
    shortSha(b.red),
    shortSha(b.green),
    b.commits.length,
  ];
  if (b.pull != null)
    return n === 1
      ? `It started failing with #${b.pull}, merged as ${red}; ${green} before it passed.`
      : `It started failing with #${b.pull}, merged as ${n} commits up to ${red}; ${green} before them passed.`;
  if (n === 1)
    return `It started failing at ${red}, pushed without a pull request; ${green} before it passed.`;
  const from = b.pulls.map((p) => `#${p}`);
  if (b.direct === 1) from.push("1 pushed without a pull request");
  else if (b.direct > 1) from.push(`${b.direct} pushed without one`);
  return `It started failing in the ${n} commits after ${green}, which passed, up to ${red}: from ${and(from)}.`;
}

/** Where Orbit told the problem's agent, or why it hasn't. */
export function toldText(s: Snapshot, p: Problem, t: OpsThread | undefined) {
  if (p.kind === "waiting")
    return "Only a person can add or fix a runner, so Orbit doesn’t tell an agent.";
  if (p.kind === "security" && p.source === "secret_scanning")
    return "Only a person can revoke a secret, so Orbit doesn’t tell an agent.";
  if (withheld(p)) {
    const these = p.kind === "security" ? "these alerts" : "these settings";
    return t?.url
      ? `Orbit told the repository’s agent in issue #${t.number}, but the repository isn’t private now, so Orbit leaves that issue as it is. ${these[0].toUpperCase()}${these.slice(1)} stay with you.`
      : `Orbit keeps ${these} with you, as the repository isn’t private and an issue would show others where the weakness is.`;
  }
  const buddy = buddyOf(s.settings);
  const proposal = p.kind === "no_ci";
  const issue = opensIssue(p);
  if (t?.gone) return t.error ?? "";
  if (t?.url) {
    const where = proposal
      ? `Orbit proposed a workflow to the repository’s agent in issue #${t.number}.`
      : issue
        ? `Orbit told the repository’s agent in issue #${t.number}.`
        : "Orbit told the agent in a comment on the pull request.";
    return t.error
      ? `${where} Its latest update didn’t go through: ${t.error}`
      : where;
  }
  if (t?.error) return `Orbit couldn’t tell the agent yet: ${t.error}`;
  if (t?.unsure)
    return "Orbit stopped before GitHub answered its post, so it looks on GitHub for it before posting again.";
  const rule = ruleOf(s, p);
  if (!buddy.enabled)
    return `${buddy.name} is off, so Orbit doesn’t tell the agent.`;
  if (rule === "leave_it")
    return "Your rule is Leave it to me, so Orbit doesn’t tell the agent.";
  if (asksFirst(s, p, t))
    return proposal
      ? `${buddy.name} asks before opening an issue with a workflow for the repository’s agent.`
      : issue
        ? `${buddy.name} asks before opening an issue for the repository’s agent.`
        : `${buddy.name} asks before commenting for the pull request’s agent.`;
  return "Orbit tells the agent on GitHub after this sync.";
}

/** What an item is about: its check, "main moved on", "CI on pull
 * requests", or an alert, such as "Dependabot alert 14, lodash". */
function itemName(item: OpsItem) {
  if (item.kind === "no_ci") return "CI on pull requests";
  return item.kind === "base_moved" ? `${item.check} moved on` : item.check;
}

/** What Orbit found in a repository where no CI runs on pull requests. */
export function noCiText(p: Problem) {
  const at = shortSha(p.sha);
  const found = p.found;
  const why = found?.workflows.length
    ? `None of its workflows on ${p.branch || "the default branch"} runs on pull requests, at ${at}.`
    : `It has no workflows on ${p.branch || "the default branch"}, at ${at}.`;
  const proposal = found?.proposal
    ? ` Orbit can propose one built from ${and(found.from)}.`
    : " Orbit can’t tell how it builds, so its issue only asks for one.";
  return `No CI runs on its pull requests. ${why}${proposal}`;
}

/** Names in a sentence: "A", "A and B", "A, B and 2 more". */
function some(names: string[]) {
  return names.length > 2
    ? `${names[0]}, ${names[1]} and ${names.length - 2} more`
    : and(names);
}

/** Runs waiting for a runner, in a sentence, as Orbit's note says it. */
export function waitingText(p: Problem) {
  const runs = p.waiting ?? [];
  const names = some([...new Set(runs.map((r) => r.name).filter(Boolean))]);
  const waited = "waited more than 30 minutes for a runner";
  if (runs.length === 1)
    return names ? `A run of ${names} has ${waited}.` : `A run has ${waited}.`;
  return names
    ? `${runs.length} runs have ${waited}: ${names}.`
    : `${runs.length} runs have ${waited}.`;
}

/** A place's self-hosted runners in a sentence: "2 runners, 2 online, 1
 * busy." */
export function runnersText(p: RunnerPlace) {
  const online = p.runners.filter((r) => r.online);
  const busy = online.filter((r) => r.busy).length;
  const count = `${p.total} ${p.total === 1 ? "runner" : "runners"}`;
  const state = !online.length
    ? p.total === 1
      ? "offline"
      : "all offline"
    : busy
      ? `${online.length} online, ${busy} busy`
      : `${online.length} online`;
  const more =
    p.total > p.runners.length
      ? ` Orbit read the first ${p.runners.length}.`
      : "";
  return `${count}, ${state}.${more}`;
}

/** Bytes in plain words: "9.4 GB", "640 MB", or "under 1 MB". */
export function size(bytes: number) {
  if (bytes >= GB) return `${(bytes / GB).toFixed(1)} GB`;
  if (bytes >= 2 ** 20) return `${Math.round(bytes / 2 ** 20)} MB`;
  return "under 1 MB";
}
/** A repository's caches in a sentence: "42 caches holding 9.4 GB, near
 * GitHub's usual 10 GB limit." */
export function cachesText(s: ActionsStorage) {
  if (!s.caches && !s.cacheBytes) return "No caches.";
  const what = `${s.caches} ${s.caches === 1 ? "cache" : "caches"} holding ${size(s.cacheBytes)}`;
  return s.cacheBytes >= NEAR
    ? `${what}, near GitHub’s usual ${CACHE_LIMIT / GB} GB limit.`
    : `${what}.`;
}
/** The artifacts a repository keeps in a sentence: "52 artifacts kept,
 * holding 640 MB." */
export function artifactsText(s: ActionsStorage) {
  const more = s.moreArtifacts ? ", and more Orbit didn’t read" : "";
  if (!s.artifacts)
    return s.moreArtifacts
      ? "Orbit didn’t read all its artifacts."
      : "No artifacts kept.";
  const n = `${s.artifacts} ${s.artifacts === 1 ? "artifact" : "artifacts"}`;
  return `${n} kept, holding ${size(s.artifactBytes)}${more}.`;
}
/** What a job that couldn't upload an artifact as the account's storage is
 * full says. */
export const quotaText =
  "GitHub’s artifact storage for this account is full, so the job couldn’t upload. Only a person can fix it: let old artifacts expire, delete some, or raise the account’s storage. GitHub counts it again every 6 to 12 hours.";

/** A runner in a line: its name, system, state and labels. */
function runnerLine(r: Runner) {
  const state = !r.online ? "offline" : r.busy ? "busy" : "free";
  return [r.name, r.os, state, r.labels.join(", ")].filter(Boolean).join(" · ");
}

/** What a pull request's checks ran without, when its base moved on. */
export function movedText(p: Problem) {
  return `Its checks pass, but ${p.base || "its base"} moved on to ${shortSha(p.baseSha ?? "")} after they began. Its author’s to merge it in and let CI run again.`;
}

/** What an agent said about a failing item, and for a fix, what Orbit
 * found. The agent's words are shown as plain text. */
export function answerText(t: OpsThread, item: OpsItem) {
  const a = item.answer;
  if (!a) return undefined;
  const who = a.author || "The agent";
  const about = `I${item.number} · ${itemName(item)}:`;
  const said = (text?: string | null) => (text ? `: ${text}` : ".");
  switch (a.kind) {
    case "taking":
      return `${about} ${who} is on it.`;
    case "declined":
      return `${about} ${who} declined it${said(a.text)}`;
    case "question":
      return `${about} ${who} needs a person${said(a.text)}`;
    case "fixed": {
      // Orbit doesn't check a commit for a proposal: it closes once a
      // workflow on the default branch runs on pull requests.
      if (item.kind === "no_ci") {
        const where = a.commit ? ` in ${a.commit.slice(0, 7)}` : "";
        return `${about} ${who} says it’s added${where}. Orbit is waiting to see a workflow that runs on pull requests on ${t.branch || "the default branch"}.`;
      }
      // Nor for a risky setting: Orbit reads the files again at the next
      // commit on the default branch.
      if (item.kind === "risk") {
        const where = a.commit ? ` in ${a.commit.slice(0, 7)}` : "";
        return `${about} ${who} says it’s fixed${where}. Orbit checks again at the next commit on ${t.branch || "the default branch"}.`;
      }
      // Nor for an alert: only GitHub says when one is fixed.
      if (item.kind === "alert") {
        const where = a.commit ? ` in ${a.commit.slice(0, 7)}` : "";
        return `${about} ${who} says it’s fixed${where}. Orbit is waiting for GitHub to mark the alert fixed.`;
      }
      if (!a.commit)
        return `${about} ${who} says it’s fixed. Orbit is waiting to see it pass.`;
      const fixed = `${about} ${who} says it’s fixed in ${a.commit.slice(0, 7)}`;
      switch (a.check) {
        case "passes":
          return `${fixed}, and it passes there.`;
        case "fails":
          return `${fixed}, but it still fails there.`;
        case "not_on_branch":
          return `${fixed}, which isn’t on ${t.branch || "the branch"}.`;
        case "not_found":
          return `${fixed}, which Orbit can’t find.`;
        default:
          return `${fixed}. Orbit is waiting for its checks there.`;
      }
    }
  }
}

/** Why a failing check failed, as Orbit read its job's log. */
export function whyText(f: CiFailure) {
  if (f.held)
    return `${f.check}: Orbit held back its log, as it looks like it holds a secret.`;
  if (f.error) return `${f.check}: ${f.error}`;
  if (f.step) return `${f.check} failed at step “${f.step}”.`;
  return `${f.check} failed. Its log ends:`;
}

/** The reruns of an item's job that went through at its commit. */
function doneReruns(i: OpsItem) {
  return (i.reruns ?? []).filter((r) => r.done);
}

/** Whether Bit waits for your tap before rerunning a failing item's job:
 * it looks like bad luck, Orbit hasn't rerun this job or given up on it,
 * isn't checking whether a rerun went through, fewer than two reruns went
 * through at this commit, and the rule for the next one is Ask first. */
export function asksToRerun(s: Snapshot, p: Problem, i: OpsItem) {
  const buddy = buddyOf(s.settings);
  if (!buddy.enabled || p.closedAt || !i.failing) return false;
  if (i.why?.url !== i.url || !i.why.flaky) return false;
  if (i.reruns?.some((r) => r.unsure)) return false;
  const tried = i.reruns?.find((r) => r.url === i.url);
  if (tried?.done || tried?.lasting) return false;
  const done = doneReruns(i).length;
  if (done >= 2) return false;
  const rule = done ? buddy.rules.rerun_again : buddy.rules.rerun_flaky;
  return rule === "ask_first" && i.rerunOk !== i.url;
}

/** What Orbit did about a failing item's job looking like bad luck: that
 * it reran it, that it failed again, or why it hasn't rerun it yet. */
export function rerunText(s: Snapshot, p: Problem, i: OpsItem) {
  const name = buddyOf(s.settings).name;
  if (i.reruns?.some((r) => r.unsure))
    return "Orbit is checking on GitHub whether its rerun went through.";
  const done = doneReruns(i);
  const last = done.at(-1);
  if (last?.url === i.url) {
    if (last.already) return "It was already rerun on GitHub.";
    return done.length === 1
      ? "Orbit reran it."
      : "Orbit reran it a second time.";
  }
  if (last) {
    const real =
      done.length === 1
        ? "It failed again after a rerun, so it’s likely real."
        : "It failed again after two reruns, so it’s likely real.";
    return asksToRerun(s, p, i)
      ? `${real} ${name} asks before rerunning it again.`
      : real;
  }
  const tried = i.reruns?.find((r) => r.url === i.url);
  if (tried?.error) return tried.error;
  if (asksToRerun(s, p, i)) return `${name} asks before rerunning it.`;
  return undefined;
}

/** Why each of a problem's failing checks failed, with the end of its log
 * as plain text, and whether it looks like bad luck. */
function Why({ s, p, t }: { s: Snapshot; p: Problem; t?: OpsThread }) {
  // Named as the problem names it, as checks may share a link.
  const read = p.failing.flatMap((c) => {
    const f = c.url && s.failures?.find((f) => f.url === c.url);
    return f ? [{ ...f, check: c.name }] : [];
  });
  return read.map((f) => {
    const item = t?.items.find((i) => i.failing && i.url === f.url);
    const rerun = item && rerunText(s, p, item);
    return (
      <VStack key={`${f.check} ${f.url}`} gap={1}>
        <Text color="secondary">{whyText(f)}</Text>
        {f.flaky && (
          <Text color="secondary">Looks like bad luck: {f.flaky}</Text>
        )}
        {f.quota && <Text color="secondary">{quotaText}</Text>}
        {rerun && <Text color="secondary">{rerun}</Text>}
        {!f.held && !f.error && f.lines.length > 0 && (
          <CodeBlock
            code={f.lines.join("\n")}
            isWrapped
            width="100%"
            size="sm"
            hasCopyButton={false}
            aria-label={`The end of ${f.check}’s log`}
          />
        )}
      </VStack>
    );
  });
}

/** A problem's name: its repository and branch, or its pull request. */
function title(s: Snapshot, p: Problem) {
  if (p.kind === "security")
    return `${p.repo}: ${alertKinds[p.source ?? "dependabot"].many}`;
  if (p.kind === "no_ci") return `${p.repo}: CI on pull requests`;
  if (p.kind === "waiting") return `${p.repo}: runs waiting for a runner`;
  if (p.kind === "workflow_risks") return `${p.repo}: workflow settings`;
  if (p.kind === "default_branch")
    return `${p.repo}: ${p.branch || "default branch"}`;
  const pr = s.prs.find((pr) => pr.id === p.prId);
  return pr ? `${p.prId} · ${pr.title}` : (p.prId ?? p.repo);
}

/** Where a risky setting is: "ci.yml line 12, job build". */
function riskPlace(r: WorkflowRisk) {
  const file = r.path.split("/").at(-1) || r.path;
  return `${file} line ${r.line}${r.job ? `, job ${r.job}` : ""}`;
}
/** A risky workflow setting on one line, in the words of Orbit's issue. */
export function riskText(r: WorkflowRisk) {
  const at = riskPlace(r);
  switch (r.kind) {
    case "unpinned":
      return `${r.subject} isn’t pinned to a full commit, in ${at}.`;
    case "permissions":
      return r.subject === "missing"
        ? `No permissions are set, so its jobs get the token’s default, in ${at}.`
        : `It asks for ${r.subject} permissions, in ${at}.`;
    case "pull_checkout":
      return `It checks out a pull request’s code (${r.subject}) where secrets are exposed, in ${at}.`;
    case "injection":
      return `${r.subject} goes straight into a script, in ${at}.`;
  }
}
/** A workflows problem in a sentence, as the engine has it
 * (`risks_text`). */
export function risksText(p: Problem) {
  const risks = p.risks ?? [];
  const n = risks.length;
  const high = risks.filter(
    (r) => r.severity === "high" || r.severity === "critical",
  ).length;
  const many = n === 1 ? "1 risky setting" : `${n} risky settings`;
  const which = !high
    ? ""
    : n === 1
      ? ", and it is high"
      : `, ${high} of them high`;
  return `Its workflows on ${p.branch || "the default branch"} have ${many}${which}, at ${shortSha(p.sha)}.`;
}
/** A link to a risky setting's line on GitHub, at the commit Orbit read. */
function riskLink(p: Problem, r: WorkflowRisk) {
  const path = r.path.split("/").map(encodeURIComponent).join("/");
  return `https://github.com/${p.repo}/blob/${p.sha}/${path}#L${r.line}`;
}

function Repositories({ go }: { go: AttentionHandlers }) {
  const { snapshot: s, action } = useOrbit();
  const repos = s.settings.repositories;
  if (!repos.length)
    return (
      <EmptyState
        icon={<FolderGit2 />}
        title="No repositories connected"
        description="Connect a repository and Orbit watches its CI after each sync."
        actions={
          <Button
            label="Open repositories"
            variant="primary"
            onClick={go.onRepositories}
          />
        }
      />
    );
  return (
    <Card padding={0}>
      <List hasDividers aria-label="CI by repository">
        {repos.map((r) => {
          const ci = s.ci?.find((c) => sameRepo(c.repo, r.name));
          const failingPulls = (s.problems ?? []).filter(
            (p) =>
              !p.closedAt &&
              p.kind === "pull_request" &&
              sameRepo(p.repo, r.name),
          ).length;
          const dot = ci?.head.sha ? dots[ci.head.state] : dots.none;
          const noCi = (s.problems ?? []).find(
            (p) =>
              !p.closedAt && p.kind === "no_ci" && sameRepo(p.repo, r.name),
          );
          const t = noCi && threadOf(s, noCi);
          const told = noCi && toldText(s, noCi, t);
          return (
            <ListItem
              key={r.name}
              startContent={
                <StatusDot variant={dot.variant} label={dot.label} />
              }
              label={<Text weight="medium">{r.name}</Text>}
              description={
                <VStack gap={1}>
                  <Text color="secondary">
                    {ci && (ci.head.sha || !ci.error)
                      ? branchSummary(ci)
                      : "Not read yet."}
                  </Text>
                  {failingPulls > 0 && (
                    <Text color="secondary">
                      {failingPulls === 1
                        ? "1 pull request is failing."
                        : `${failingPulls} pull requests are failing.`}
                    </Text>
                  )}
                  {noCi && (
                    <HStack gap={2} vAlign="start">
                      <StatusDot variant="warning" label="No CI" />
                      <VStack gap={1}>
                        <Text>{noCiText(noCi)}</Text>
                        {told && <Text color="secondary">{told}</Text>}
                        {t?.items
                          .filter((i) => i.failing && i.answer)
                          .map((i) => (
                            <Text key={i.number} color="secondary">
                              {answerText(t, i)}
                            </Text>
                          ))}
                      </VStack>
                    </HStack>
                  )}
                  {ci?.error && (
                    <HStack gap={2} vAlign="center">
                      <StatusDot variant="warning" label="Not read" />
                      <Text color="secondary">
                        Couldn’t read CI: {ci.error}
                      </Text>
                    </HStack>
                  )}
                </VStack>
              }
              endContent={
                <VStack gap={2} hAlign="end">
                  {noCi && t && asksFirst(s, noCi, t) ? (
                    <Button
                      label="Propose CI"
                      size="sm"
                      variant="primary"
                      onClick={() => void action("tell_agent", { key: t.key })}
                    />
                  ) : (
                    t?.url && (
                      <Button
                        label="Open issue"
                        size="sm"
                        variant="secondary"
                        onClick={() => openLink(t.url)}
                      />
                    )
                  )}
                  {ci?.checkedAt && (
                    <Timestamp value={ci.checkedAt} format="relative_short" />
                  )}
                </VStack>
              }
            />
          );
        })}
      </List>
    </Card>
  );
}

function Timeline({ p }: { p: Problem }) {
  return (
    <VStack gap={1}>
      {p.timeline.slice(-EVENTS).map((e, i) => (
        <HStack key={`${e.at}-${i}`} gap={2} vAlign="start">
          <Timestamp value={e.at} format="relative_short" type="supporting" />
          <Text type="supporting" color="secondary">
            {e.text}
          </Text>
        </HStack>
      ))}
    </VStack>
  );
}

function OpenProblems({ go }: { go: AttentionHandlers }) {
  const { snapshot: s, action } = useOrbit();
  // A repository with no CI is listed with the repository: nothing fails.
  // Security alerts have a section of their own.
  const open = (s.problems ?? [])
    .filter((p) => !p.closedAt && p.kind !== "no_ci" && p.kind !== "security")
    .sort(
      (a, b) =>
        Number(!needsYou(a)) - Number(!needsYou(b)) ||
        b.openedAt.localeCompare(a.openedAt),
    );
  // All clear only once every check Orbit follows was read.
  const unread = s.settings.repositories.filter((r) =>
    partlyRead(s, r.name),
  ).length;
  if (!open.length && unread)
    return (
      <Card padding={3}>
        <HStack gap={3} vAlign="center">
          <StatusDot variant="neutral" label="Not all read" />
          <Text weight="medium">
            No problems found so far, but Orbit couldn’t read every check on{" "}
            {unread} {unread === 1 ? "repository" : "repositories"}.
          </Text>
        </HStack>
      </Card>
    );
  if (!open.length)
    return (
      <Card variant="green" padding={3}>
        <HStack gap={3} vAlign="center">
          <StatusDot variant="success" label="All clear" />
          <Text weight="medium">All clear. Nothing is failing.</Text>
        </HStack>
      </Card>
    );
  return (
    <Card padding={0}>
      <List hasDividers aria-label="Open problems">
        {open.map((p) => {
          const pr = s.prs.find((pr) => pr.id === p.prId);
          const yours = needsYou(p);
          const waiting = p.kind === "waiting";
          const risky = p.kind === "workflow_risks";
          const risks = p.risks ?? [];
          const t = threadOf(s, p);
          const told = toldText(s, p, t);
          const broke =
            p.kind === "default_branch" ? brokeText(p.broke) : undefined;
          // The run that has waited longest, to open on GitHub.
          const longest = waiting ? p.waiting?.find((r) => r.url) : undefined;
          // Whether Orbit's latest read missed some queued runs.
          const more =
            waiting &&
            s.runners?.some(
              (r) =>
                r.name.toLowerCase() === p.repo.toLowerCase() && r.moreQueued,
            );
          // The jobs Bit asks before rerunning.
          const reruns = t?.items.filter((i) => asksToRerun(s, p, i)) ?? [];
          return (
            <ListItem
              key={p.id}
              startContent={
                <StatusDot
                  variant={yours ? "error" : "warning"}
                  label={
                    yours
                      ? "Needs you"
                      : risky
                        ? "Risky settings"
                        : "Author’s to fix"
                  }
                />
              }
              label={
                <Text weight="medium" maxLines={2}>
                  {title(s, p)}
                </Text>
              }
              description={
                <VStack gap={2}>
                  <Text>
                    {waiting
                      ? waitingText(p)
                      : risky
                        ? risksText(p)
                        : yours
                          ? `Failing: ${checkNames(p.failing)}.`
                          : p.kind === "base_moved"
                            ? movedText(p)
                            : `Failing: ${checkNames(p.failing)}. Its author’s to fix.`}
                  </Text>
                  {risks.slice(0, ALERTS).map((r) => (
                    <HStack
                      key={r.number ?? `${r.path}:${r.line}`}
                      gap={2}
                      vAlign="start"
                    >
                      <Token
                        label={severityLabels[r.severity]}
                        color={severityColours[r.severity]}
                        size="sm"
                      />
                      <Text color="secondary" maxLines={2}>
                        {riskText(r)}
                      </Text>
                    </HStack>
                  ))}
                  {risks.length > ALERTS && (
                    <Text color="secondary">
                      And {risks.length - ALERTS} more.
                    </Text>
                  )}
                  {waiting &&
                    p.waiting?.slice(0, RUNS).map((r) => (
                      <HStack key={r.id} gap={1} vAlign="center">
                        <Text color="secondary">
                          {r.name || "A run"} · queued
                        </Text>
                        <Timestamp value={r.since} format="relative_short" />
                      </HStack>
                    ))}
                  {more && (
                    <Text color="secondary">
                      Orbit couldn’t read every queued run, so more may wait.
                    </Text>
                  )}
                  {broke && <Text>{broke}</Text>}
                  <Why s={s} p={p} t={t} />
                  {told && <Text color="secondary">{told}</Text>}
                  {t?.items
                    .filter((i) => i.failing && i.answer)
                    .map((i) => (
                      <Text key={i.number} color="secondary">
                        {answerText(t, i)}
                      </Text>
                    ))}
                  <Timeline p={p} />
                </VStack>
              }
              onClick={pr ? () => go.onChoose(pr) : undefined}
              endContent={
                // Stacked, so a narrow window keeps room for the text.
                <VStack gap={2} hAlign="end">
                  {t && reruns.length > 0 && (
                    <Button
                      label={
                        reruns.some((i) => doneReruns(i).length)
                          ? "Rerun again"
                          : "Rerun"
                      }
                      size="sm"
                      variant="primary"
                      onClick={() => {
                        for (const i of reruns)
                          void action("rerun_job", {
                            key: t.key,
                            number: i.number,
                          });
                      }}
                    />
                  )}
                  {t && asksFirst(s, p, t) ? (
                    <Button
                      label={opensIssue(p) ? "Open issue" : "Post comment"}
                      size="sm"
                      variant="primary"
                      onClick={() => void action("tell_agent", { key: t.key })}
                    />
                  ) : t?.url ? (
                    <Button
                      label="Open on GitHub"
                      size="sm"
                      variant="secondary"
                      onClick={() => openLink(t.url)}
                    />
                  ) : longest ? (
                    <Button
                      label="Open run"
                      size="sm"
                      variant="secondary"
                      onClick={() => openLink(longest.url)}
                    />
                  ) : risks[0] ? (
                    <Button
                      label="Open file"
                      size="sm"
                      variant="secondary"
                      onClick={() => openLink(riskLink(p, risks[0]))}
                    />
                  ) : (
                    !reruns.length && (
                      <Timestamp value={p.openedAt} format="relative_short" />
                    )
                  )}
                </VStack>
              }
            />
          );
        })}
      </List>
    </Card>
  );
}

/** Each kind of security alert, as the engine names them
 * (`security.rs`). */
const alertKinds: Record<AlertSource, { many: string; title: string }> = {
  dependabot: { many: "Dependabot alerts", title: "Dependabot" },
  code_scanning: { many: "code scanning alerts", title: "Code scanning" },
  secret_scanning: { many: "secret scanning alerts", title: "Secret scanning" },
};
const sources: AlertSource[] = [
  "dependabot",
  "code_scanning",
  "secret_scanning",
];
const severityColours: Record<
  AlertSeverity,
  "red" | "orange" | "yellow" | "gray"
> = {
  critical: "red",
  high: "orange",
  medium: "yellow",
  low: "gray",
};
const severityLabels: Record<AlertSeverity, string> = {
  critical: "Critical",
  high: "High",
  medium: "Medium",
  low: "Low",
};

/** Where an alert is: "package.json", "src/db.ts line 12", or nothing. */
export function alertPlace(a: SecurityAlert) {
  if (!a.path) return "";
  return a.line ? `${a.path} line ${a.line}` : a.path;
}
/** An alert on one line, such as "lodash in package-lock.json: Prototype
 * pollution in lodash. Fixed in 4.17.21." */
export function alertText(a: SecurityAlert) {
  const place = alertPlace(a);
  if (a.source === "secret_scanning") {
    const parts = [
      `${a.title || a.subject || "A secret"}${place ? ` in ${place}` : ""}.`,
    ];
    if (a.validity === "active") parts.push("GitHub says it still works.");
    if (a.validity === "inactive")
      parts.push("GitHub says it no longer works.");
    if (a.bypassed) parts.push("It was pushed past push protection.");
    return parts.join(" ");
  }
  const what = a.source === "dependabot" ? a.subject : a.title || a.subject;
  const about = a.source === "dependabot" && a.title ? `: ${a.title}` : "";
  const parts = [`${what}${place ? ` in ${place}` : ""}${about}.`];
  if (a.fixedIn) parts.push(`Fixed in ${a.fixedIn}.`);
  return parts.join(" ");
}
/** How many of a kind are open, and how many need you, to follow its
 * name: "2 open alerts, 1 high or critical." */
export function alertCount(source: AlertSource, alerts: SecurityAlert[]) {
  const n = alerts.length;
  if (source === "secret_scanning")
    return n
      ? `${n} ${n === 1 ? "secret" : "secrets"} found.`
      : "no secrets found.";
  if (!n) return "no open alerts.";
  const open = `${n} open ${n === 1 ? "alert" : "alerts"}`;
  const urgent = alerts.filter(alertNeedsYou).length;
  if (!urgent) return `${open}.`;
  return urgent === n
    ? `${open}, ${n === 1 ? "high or critical" : "all high or critical"}.`
    : `${open}, ${urgent} high or critical.`;
}

/** Each connected repository's open security alerts, kind by kind, and
 * what Orbit can't see. */
function Security() {
  const { snapshot: s, action } = useOrbit();
  const read = (s.security ?? []).filter((r) => r.readAt || r.error);
  return (
    <VStack gap={3}>
      <Heading level={3}>Security alerts</Heading>
      <Text color="secondary">
        Orbit reads each repository’s Dependabot, code scanning and secret
        scanning alerts every half hour. A secret GitHub found, or a high or
        critical alert, needs you. In a private repository, Orbit tells the
        agent about Dependabot and code scanning alerts in an issue; in any
        other, an issue would show others where the weakness is, so it doesn’t.
        Orbit never keeps a secret’s value.
      </Text>
      {!read.length ? (
        <Card padding={3}>
          <HStack gap={3} vAlign="center">
            <StatusDot variant="neutral" label="Not read yet" />
            <Text>Orbit hasn’t read any security alerts yet.</Text>
          </HStack>
        </Card>
      ) : (
        <Card padding={0}>
          <List hasDividers aria-label="Security alerts">
            {read.map((r) => {
              const open = (s.problems ?? []).filter(
                (p) =>
                  !p.closedAt &&
                  p.kind === "security" &&
                  sameRepo(p.repo, r.repo),
              );
              const problemOf = (source: AlertSource) =>
                open.find((p) => p.source === source);
              const alertsOf = (source: AlertSource) =>
                problemOf(source)?.securityAlerts ?? [];
              const all = sources.flatMap(alertsOf);
              const urgent = all.filter(alertNeedsYou).length;
              const unseen = r.kinds.some((k) => k.unseen && !k.off);
              const dot = urgent
                ? { variant: "error" as const, label: "Needs you" }
                : all.length
                  ? { variant: "warning" as const, label: "Open alerts" }
                  : unseen || !r.readAt
                    ? { variant: "neutral" as const, label: "Not all seen" }
                    : { variant: "success" as const, label: "None open" };
              return (
                <ListItem
                  key={r.repo}
                  startContent={
                    <StatusDot variant={dot.variant} label={dot.label} />
                  }
                  label={<Text weight="medium">{r.repo}</Text>}
                  description={
                    <VStack gap={2}>
                      {r.readAt &&
                        sources.map((source) => {
                          const kind = r.kinds.find((k) => k.source === source);
                          if (!kind) return null;
                          const alerts = alertsOf(source);
                          const p = problemOf(source);
                          const t = p && threadOf(s, p);
                          const told = p && !kind.unseen && toldText(s, p, t);
                          return (
                            <VStack key={source} gap={1}>
                              <Text>
                                {kind.unseen ??
                                  `${alertKinds[source].title}: ${alertCount(source, alerts)}`}
                              </Text>
                              {!kind.unseen &&
                                alerts.slice(0, ALERTS).map((a) => (
                                  <HStack key={a.number} gap={2} vAlign="start">
                                    {source === "secret_scanning" ? (
                                      <Token
                                        label="Secret"
                                        color="red"
                                        size="sm"
                                      />
                                    ) : (
                                      <Token
                                        label={severityLabels[a.severity]}
                                        color={severityColours[a.severity]}
                                        size="sm"
                                      />
                                    )}
                                    <Text color="secondary" maxLines={2}>
                                      {alertText(a)}
                                    </Text>
                                  </HStack>
                                ))}
                              {!kind.unseen && alerts.length > ALERTS && (
                                <Text color="secondary">
                                  And {alerts.length - ALERTS} more.
                                </Text>
                              )}
                              {kind.more && (
                                <Text color="secondary">
                                  GitHub has more open than the newest 100 Orbit
                                  reads.
                                </Text>
                              )}
                              {told && <Text color="secondary">{told}</Text>}
                              {told &&
                                t?.items
                                  .filter((i) => i.failing && i.answer)
                                  .map((i) => (
                                    <Text key={i.number} color="secondary">
                                      {answerText(t, i)}
                                    </Text>
                                  ))}
                              {told && p && t && asksFirst(s, p, t) ? (
                                <HStack gap={2}>
                                  <Button
                                    label="Open issue"
                                    size="sm"
                                    variant="primary"
                                    onClick={() =>
                                      void action("tell_agent", { key: t.key })
                                    }
                                  />
                                </HStack>
                              ) : (
                                told &&
                                t?.url && (
                                  <HStack gap={2}>
                                    <Button
                                      label={`See issue #${t.number}`}
                                      size="sm"
                                      variant="secondary"
                                      onClick={() => openLink(t.url)}
                                    />
                                  </HStack>
                                )
                              )}
                            </VStack>
                          );
                        })}
                      {r.error && (
                        <HStack gap={2} vAlign="center">
                          <StatusDot variant="warning" label="Not read" />
                          <Text color="secondary">{r.error}</Text>
                        </HStack>
                      )}
                    </VStack>
                  }
                  endContent={
                    <VStack gap={2} hAlign="end">
                      <Button
                        label="Open on GitHub"
                        size="sm"
                        variant="secondary"
                        onClick={() =>
                          openLink(`https://github.com/${r.repo}/security`)
                        }
                      />
                      {r.readAt && (
                        <Timestamp value={r.readAt} format="relative_short" />
                      )}
                    </VStack>
                  }
                />
              );
            })}
          </List>
        </Card>
      )}
    </VStack>
  );
}

/** The self-hosted runners Orbit sees for each repository, and for each
 * organization that owns one, and where it can't see them. Places with
 * none aren't listed. */
function Runners() {
  const { snapshot: s } = useOrbit();
  const read = (s.runners ?? []).filter(
    (p) =>
      (p.kind === "repository" || p.kind === "organization") &&
      (p.readAt || p.error),
  );
  if (!read.length) return null;
  const listed = read.filter((p) => p.runners.length || p.unseen || p.error);
  return (
    <VStack gap={3}>
      <Heading level={3}>Self-hosted runners</Heading>
      {!listed.length ? (
        <Card padding={3}>
          <HStack gap={3} vAlign="center">
            <StatusDot variant="neutral" label="None" />
            <Text>
              Orbit sees no self-hosted runners on your repositories or the
              organizations that own them, so their runs use GitHub’s own
              runners.
            </Text>
          </HStack>
        </Card>
      ) : (
        <Card padding={0}>
          <List hasDividers aria-label="Self-hosted runners">
            {listed.map((p) => {
              const online = p.runners.some((r) => r.online);
              const dot = !p.runners.length
                ? { variant: "neutral" as const, label: "None seen" }
                : online
                  ? { variant: "success" as const, label: "Online" }
                  : { variant: "warning" as const, label: "Offline" };
              return (
                <ListItem
                  key={`${p.kind} ${p.name}`}
                  startContent={
                    <StatusDot variant={dot.variant} label={dot.label} />
                  }
                  label={
                    <Text weight="medium">
                      {p.kind === "organization"
                        ? `${p.name} · organization`
                        : p.name}
                    </Text>
                  }
                  description={
                    <VStack gap={1}>
                      {p.runners.length > 0 && <Text>{runnersText(p)}</Text>}
                      {p.kind === "organization" && p.runners.length > 0 && (
                        <Text color="secondary">
                          Orbit can’t tell which of its repositories may use
                          them, as a runner group can leave some out.
                        </Text>
                      )}
                      {p.runners.slice(0, RUNNERS).map((r, i) => (
                        <Text key={`${r.name}-${i}`} color="secondary">
                          {runnerLine(r)}
                        </Text>
                      ))}
                      {p.runners.length > RUNNERS && (
                        <Text color="secondary">
                          And {p.runners.length - RUNNERS} more.
                        </Text>
                      )}
                      {p.unseen && <Text color="secondary">{p.unseen}</Text>}
                      {p.error && (
                        <HStack gap={2} vAlign="center">
                          <StatusDot variant="warning" label="Not read" />
                          <Text color="secondary">{p.error}</Text>
                        </HStack>
                      )}
                    </VStack>
                  }
                  endContent={
                    p.readAt ? (
                      <Timestamp value={p.readAt} format="relative_short" />
                    ) : undefined
                  }
                />
              );
            })}
          </List>
        </Card>
      )}
    </VStack>
  );
}

/** Each repository's GitHub Actions caches and the artifacts it keeps,
 * and where Orbit can't see them. */
function StorageUse() {
  const { snapshot: s } = useOrbit();
  const read = (s.actionsStorage ?? []).filter((r) => r.readAt || r.error);
  if (!read.length) return null;
  return (
    <VStack gap={3}>
      <Heading level={3}>Actions storage</Heading>
      <Text color="secondary">
        Artifacts in private repositories count toward the account’s storage,
        which Orbit can’t see.
      </Text>
      <Card padding={0}>
        <List hasDividers aria-label="Actions storage">
          {read.map((r) => {
            // A job of it failed for want of artifact storage.
            const full = (s.failures ?? []).some(
              (f) =>
                f.quota &&
                f.url
                  .toLowerCase()
                  .startsWith(`https://github.com/${r.repo.toLowerCase()}/`),
            );
            const near = !r.unseen && r.cacheBytes >= NEAR;
            const dot = full
              ? { variant: "error" as const, label: "Full" }
              : near
                ? { variant: "warning" as const, label: "Near the limit" }
                : r.unseen || !r.readAt
                  ? { variant: "neutral" as const, label: "Not seen" }
                  : { variant: "success" as const, label: "Has room" };
            return (
              <ListItem
                key={r.repo}
                startContent={
                  <StatusDot variant={dot.variant} label={dot.label} />
                }
                label={<Text weight="medium">{r.repo}</Text>}
                description={
                  <VStack gap={1}>
                    {r.readAt && !r.unseen && (
                      <>
                        <Text>{cachesText(r)}</Text>
                        <Text>{artifactsText(r)}</Text>
                      </>
                    )}
                    {near && (
                      <Text color="secondary">
                        Past its limit, GitHub drops the caches used least,
                        which can slow builds. A repository can have a higher
                        limit, which Orbit can’t see.
                      </Text>
                    )}
                    {full && (
                      <Text color="secondary">
                        A job failed as the account’s artifact storage is full.
                      </Text>
                    )}
                    {r.unseen && <Text color="secondary">{r.unseen}</Text>}
                    {r.error && (
                      <HStack gap={2} vAlign="center">
                        <StatusDot variant="warning" label="Not read" />
                        <Text color="secondary">{r.error}</Text>
                      </HStack>
                    )}
                  </VStack>
                }
                endContent={
                  r.readAt ? (
                    <Timestamp value={r.readAt} format="relative_short" />
                  ) : undefined
                }
              />
            );
          })}
        </List>
      </Card>
    </VStack>
  );
}

/** Each pull request Orbit reviews: clear to merge, or what is in the
 * way. */
function ReadyToMerge({ go }: { go: AttentionHandlers }) {
  const { snapshot: s } = useOrbit();
  const all = s.readiness ?? [];
  if (!all.length) return null;
  const clear = all.filter((r) => r.clear).length;
  return (
    <VStack gap={3}>
      <Heading level={3}>Ready to merge</Heading>
      <Text color="secondary">
        {clear === 1
          ? "1 pull request is clear to merge."
          : `${clear} pull requests are clear to merge.`}{" "}
        Clear means Orbit’s review of its latest commit is clean, every check
        passed with the base branch’s latest commit, and GitHub says it merges.
      </Text>
      <Card padding={0}>
        <List hasDividers aria-label="Ready to merge">
          {all.map((r) => {
            const pr = s.prs.find((p) => p.id === r.prId);
            return (
              <ListItem
                key={r.prId}
                startContent={
                  <StatusDot
                    variant={r.clear ? "success" : "warning"}
                    label={r.clear ? "Clear to merge" : "Not clear yet"}
                  />
                }
                label={
                  <Text weight="medium" maxLines={2}>
                    {r.repo}#{r.number}
                    {pr?.title ? ` · ${pr.title}` : ""}
                  </Text>
                }
                description={<Text color="secondary">{readinessLine(r)}</Text>}
                onClick={pr ? () => go.onChoose(pr) : undefined}
              />
            );
          })}
        </List>
      </Card>
    </VStack>
  );
}

function RecentlyFixed() {
  const { snapshot: s } = useOrbit();
  const closed = (s.problems ?? [])
    .filter((p) => p.closedAt)
    .sort((a, b) => (b.closedAt ?? "").localeCompare(a.closedAt ?? ""))
    .slice(0, RECENT);
  if (!closed.length) return null;
  return (
    <VStack gap={3}>
      <Heading level={3}>Recently closed</Heading>
      <Card padding={0}>
        <List hasDividers aria-label="Recently closed">
          {closed.map((p) => (
            <ListItem
              key={p.id}
              startContent={<StatusDot variant="neutral" label="Closed" />}
              label={
                <Text weight="medium" maxLines={2}>
                  {title(s, p)}
                </Text>
              }
              description={<Text color="secondary">{p.closedReason}</Text>}
              endContent={
                p.closedAt ? (
                  <Timestamp value={p.closedAt} format="relative_short" />
                ) : undefined
              }
            />
          ))}
        </List>
      </Card>
    </VStack>
  );
}

const writeText: Record<OpsAction, string> = {
  post_comment: "Commented on",
  edit_comment: "Updated its comment on",
  open_issue: "Opened an issue on",
  edit_issue: "Updated its issue on",
  note_on_issue: "Left a closing note on",
  close_issue: "Closed its issue on",
  reopen_issue: "Reopened its issue on",
  rerun_job: "Reran a job on",
};
/** A write in words, such as "Opened an issue on acme/api #45". */
export function writeSummary(w: OpsWrite) {
  const target = w.number ? `${w.repo} #${w.number}` : w.repo;
  return `${writeText[w.action]} ${target}.`;
}

/** The latest writes Orbit made on GitHub, newest first. */
function Posted() {
  const { snapshot: s } = useOrbit();
  const writes = (s.journal ?? []).slice(-RECENT).reverse();
  if (!writes.length) return null;
  return (
    <VStack gap={3}>
      <Heading level={3}>What Orbit did on GitHub</Heading>
      <Card padding={0}>
        <List hasDividers aria-label="What Orbit did on GitHub">
          {writes.map((w) => (
            <ListItem
              key={w.id}
              startContent={
                <StatusDot
                  variant={
                    w.state === "done"
                      ? "success"
                      : w.state === "failed"
                        ? "warning"
                        : "accent"
                  }
                  label={
                    w.state === "done"
                      ? "Done"
                      : w.state === "failed"
                        ? "Didn’t go through"
                        : "Sending"
                  }
                />
              }
              label={<Text weight="medium">{writeSummary(w)}</Text>}
              description={
                w.error ? <Text color="secondary">{w.error}</Text> : undefined
              }
              onClick={w.url ? () => openLink(w.url) : undefined}
              endContent={<Timestamp value={w.at} format="relative_short" />}
            />
          ))}
        </List>
      </Card>
    </VStack>
  );
}

/** CI on every connected repository, and what is failing. */
export function Operations(go: AttentionHandlers) {
  const { snapshot: s, updateSettings } = useOrbit();
  const watching = s.settings.watchCi ?? true;
  const security = s.settings.watchSecurity ?? true;
  return (
    <VStack gap={6} padding={6} maxWidth={1100} className="content-page">
      <VStack gap={2}>
        <Heading level={2}>Operations</Heading>
        <Text color="secondary">
          Orbit reads CI after each sync. A failing default branch needs you; a
          failing pull request is its author’s to fix. Either way, Orbit tells
          the project’s agent on GitHub: an issue for a default branch, one
          comment on a pull request. Runs that wait more than half an hour for a
          runner need you too, as only a person can add or fix a runner, and so
          do secrets GitHub finds and high or critical security alerts.
        </Text>
      </VStack>
      {!watching ? (
        <>
          <EmptyState
            icon={<Workflow />}
            title="Watching CI is off"
            description="Turn it on to see each repository’s checks here and hear when a default branch fails."
            actions={
              <Button
                label="Turn on"
                variant="primary"
                onClick={() => void updateSettings({ watchCi: true })}
              />
            }
          />
          <SaveFeedback saveKey="watchCi" />
          {security && <Security />}
        </>
      ) : (
        <>
          <VStack gap={3}>
            <Heading level={3}>Open problems</Heading>
            <OpenProblems go={go} />
          </VStack>
          <ReadyToMerge go={go} />
          <VStack gap={3}>
            <Heading level={3}>CI by repository</Heading>
            <Repositories go={go} />
          </VStack>
          {security && <Security />}
          <Runners />
          <StorageUse />
          <RecentlyFixed />
          <Posted />
        </>
      )}
    </VStack>
  );
}
