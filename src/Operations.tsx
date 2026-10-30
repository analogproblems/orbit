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
  VStack,
} from "@astryxdesign/core";
import { FolderGit2, Workflow } from "lucide-react";
import { openLink, useOrbit } from "./store";
import { SaveFeedback } from "./SaveFeedback";
import {
  buddyOf,
  checkNames,
  sameRepo,
  shortSha,
  trustedSource,
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
  type Snapshot,
} from "./types";

/** How many closed problems the page lists, and how many writes. */
const RECENT = 10;
/** How many of a problem's latest events it shows. */
const EVENTS = 5;

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

/** Orbit's comment or issue about a problem. */
function threadOf(s: Snapshot, p: Problem) {
  return s.ops?.find((t) => t.problemId === p.id);
}

/** Whether Bit waits for your tap before it posts about a problem. */
function asksFirst(s: Snapshot, p: Problem, t: OpsThread | undefined) {
  const buddy = buddyOf(s.settings);
  const rule =
    p.kind === "default_branch"
      ? buddy.rules.open_issues
      : buddy.rules.comment_on_prs;
  return (
    !!t &&
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
  const buddy = buddyOf(s.settings);
  const issue = p.kind === "default_branch";
  if (t?.gone) return t.error ?? "";
  if (t?.url) {
    const where = issue
      ? `Orbit told the repository’s agent in issue #${t.number}.`
      : "Orbit told the agent in a comment on the pull request.";
    return t.error
      ? `${where} Its latest update didn’t go through: ${t.error}`
      : where;
  }
  if (t?.error) return `Orbit couldn’t tell the agent yet: ${t.error}`;
  if (t?.unsure)
    return "Orbit stopped before GitHub answered its post, so it looks on GitHub for it before posting again.";
  const rule = issue ? buddy.rules.open_issues : buddy.rules.comment_on_prs;
  if (!buddy.enabled)
    return `${buddy.name} is off, so Orbit doesn’t tell the agent.`;
  if (rule === "leave_it")
    return "Your rule is Leave it to me, so Orbit doesn’t tell the agent.";
  if (asksFirst(s, p, t))
    return issue
      ? `${buddy.name} asks before opening an issue for the repository’s agent.`
      : `${buddy.name} asks before commenting for the pull request’s agent.`;
  return "Orbit tells the agent on GitHub after this sync.";
}

/** What an item is about: its check, or "main moved on". */
function itemName(item: OpsItem) {
  return item.kind === "base_moved" ? `${item.check} moved on` : item.check;
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
  if (p.kind === "default_branch")
    return `${p.repo}: ${p.branch || "default branch"}`;
  const pr = s.prs.find((pr) => pr.id === p.prId);
  return pr ? `${p.prId} · ${pr.title}` : (p.prId ?? p.repo);
}

function Repositories({ go }: { go: AttentionHandlers }) {
  const { snapshot: s } = useOrbit();
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
                ci?.checkedAt ? (
                  <Timestamp value={ci.checkedAt} format="relative_short" />
                ) : undefined
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
  const open = (s.problems ?? [])
    .filter((p) => !p.closedAt)
    .sort(
      (a, b) =>
        Number(a.kind !== "default_branch") -
          Number(b.kind !== "default_branch") ||
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
          const yours = p.kind === "default_branch";
          const t = threadOf(s, p);
          const told = toldText(s, p, t);
          const broke = yours ? brokeText(p.broke) : undefined;
          // The jobs Bit asks before rerunning.
          const reruns = t?.items.filter((i) => asksToRerun(s, p, i)) ?? [];
          return (
            <ListItem
              key={p.id}
              startContent={
                <StatusDot
                  variant={yours ? "error" : "warning"}
                  label={yours ? "Needs you" : "Author’s to fix"}
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
                    {yours
                      ? `Failing: ${checkNames(p.failing)}.`
                      : p.kind === "base_moved"
                        ? movedText(p)
                        : `Failing: ${checkNames(p.failing)}. Its author’s to fix.`}
                  </Text>
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
                      label={yours ? "Open issue" : "Post comment"}
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
  return (
    <VStack gap={6} padding={6} maxWidth={1100} className="content-page">
      <VStack gap={2}>
        <Heading level={2}>Operations</Heading>
        <Text color="secondary">
          Orbit reads CI after each sync. A failing default branch needs you; a
          failing pull request is its author’s to fix. Either way, Orbit tells
          the project’s agent on GitHub: an issue for a default branch, one
          comment on a pull request.
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
        </>
      ) : (
        <>
          <VStack gap={3}>
            <Heading level={3}>Open problems</Heading>
            <OpenProblems go={go} />
          </VStack>
          <VStack gap={3}>
            <Heading level={3}>CI by repository</Heading>
            <Repositories go={go} />
          </VStack>
          <RecentlyFixed />
          <Posted />
        </>
      )}
    </VStack>
  );
}
