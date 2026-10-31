import { useState, type ReactNode } from "react";
import {
  Button,
  Collapsible,
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
import { ArrowLeft, GitPullRequest, Repeat, Square } from "lucide-react";
import { openLink, useOrbit } from "./store";
import {
  activeStatuses,
  attentionAction,
  attentionLabels,
  canPublish,
  loopFinished,
  loopStatusLabels,
  loops,
  priorStatusLabels,
  publishTarget,
  reportSelectionLabel,
  reviewDisplayStatus,
  type Attention,
  type AttentionHandlers,
  type LoopState,
  type LoopStatus,
  type PriorStatus,
  type PullRequest,
  type Review,
  type Snapshot,
} from "./types";
import { ReviewActions } from "./ReviewActions";
import { ReviewStatus } from "./ReviewStatus";
import { recentTimestamp } from "./timestamps";

const dots: Record<
  LoopStatus,
  "accent" | "warning" | "success" | "error" | "neutral"
> = {
  reviewing: "accent",
  waiting_for_author: "warning",
  clean: "success",
  needs_you: "error",
  stopped: "neutral",
};
const short = (sha: string) => sha.slice(0, 7);
function knownPr(s: Snapshot, prId: string | null) {
  return [...s.prs, ...(s.historyPrs ?? [])].find((p) => p.id === prId);
}
/** The loop's rounds, oldest first. */
function roundsOf(s: Snapshot, l: LoopState) {
  return (l.roundIds ?? []).flatMap((id) => {
    const r = s.reviews.find((r) => r.id === id);
    return r ? [r] : [];
  });
}
/** Blocking findings per round, e.g. "4 → 1 → 0". */
export function blockingTrend(rounds: Review[]) {
  return rounds.map((r) => r.blocking ?? "–").join(" → ");
}
function lastChanged(rounds: Review[]) {
  return rounds
    .map((r) => r.updatedAt)
    .sort()
    .at(-1);
}
function loopLabel(l: LoopState) {
  return l.closed ? "Left the inbox" : loopStatusLabels[l.status];
}

/** The one action that resolves an item, where there is one. */
export function mainAction(
  s: Snapshot,
  a: Attention,
  go: AttentionHandlers,
  run: (command: string, args: Record<string, unknown>) => void,
): { label: string; onClick: () => void } | undefined {
  const open = attentionAction(s, a, go);
  const p = knownPr(s, a.prId);
  const r = s.reviews.find((r) => r.id === a.reviewId);
  const publishable = !!r && !!p && canPublish(r, publishTarget(s, p));
  if (a.kind === "approval" && open)
    return { label: "Approve or deny", onClick: open };
  if (a.kind === "sync_failing" && a.repo)
    return { label: "Open repositories", onClick: go.onRepositories };
  if (
    a.kind === "ci_failing" ||
    a.kind === "ci_question" ||
    a.kind === "runs_waiting"
  )
    return { label: "Open operations", onClick: go.onOperations };
  // A reviewer or a rule on GitHub holds it back.
  if (a.kind === "stuck" && p)
    return { label: "Open on GitHub", onClick: () => openLink(p.url) };
  if (a.kind === "sync_failing" || a.kind === "autopilot_paused")
    return { label: "Open settings", onClick: go.onSettings };
  if (a.kind === "disconnected")
    return {
      label: "Check again",
      onClick: () => run("use_fix", { key: a.key }),
    };
  if (
    r &&
    publishable &&
    (a.kind === "held" ||
      a.reason === "The report is waiting for you to publish it.")
  )
    return { label: "Publish", onClick: () => go.onPublish(r) };
  if (a.fix === "continue_loop")
    return {
      label: "Continue loop",
      onClick: () => run("continue_loop", { prId: a.prId, rounds: 2 }),
    };
  if (open) return { label: "Open chat", onClick: open };
  return undefined;
}

function NeedsYou(go: AttentionHandlers) {
  const { snapshot: s, action } = useOrbit();
  const [busy, setBusy] = useState<string | null>(null);
  const attention = s.attention ?? [];
  if (!attention.length) return null;
  const run =
    (key: string) => (command: string, args: Record<string, unknown>) => {
      setBusy(key);
      void action(command, args).finally(() => setBusy(null));
    };
  return (
    <List hasDividers header={<Heading level={3}>Needs you</Heading>}>
      {attention.map((a) => {
        const p = knownPr(s, a.prId);
        const main = mainAction(s, a, go, run(a.key));
        return (
          <ListItem
            key={a.key}
            label={
              <Text weight="medium" maxLines={2}>
                {p?.title ?? a.repo ?? attentionLabels[a.kind]}
              </Text>
            }
            onClick={attentionAction(s, a, go)}
            startContent={
              <StatusDot variant="warning" label={attentionLabels[a.kind]} />
            }
            description={
              <VStack gap={1}>
                <Text color="secondary">
                  {[attentionLabels[a.kind], a.prId]
                    .filter(Boolean)
                    .join(" · ")}
                </Text>
                <Text maxLines={3}>{a.reason}</Text>
              </VStack>
            }
            endContent={
              main && (
                <Button
                  label={main.label}
                  size="sm"
                  variant="primary"
                  isLoading={busy === a.key}
                  isDisabled={busy === a.key}
                  onClick={main.onClick}
                />
              )
            }
          />
        );
      })}
    </List>
  );
}

function LoopRows({
  items,
  onSelect,
  header,
  label,
}: {
  items: LoopState[];
  onSelect: (prId: string) => void;
  header?: ReactNode;
  label?: string;
}) {
  const { snapshot: s } = useOrbit();
  return (
    <List hasDividers header={header} aria-label={label}>
      {items.map((l, index) => {
        const p = knownPr(s, l.prId);
        const rounds = roundsOf(s, l);
        const changed = lastChanged(rounds);
        return (
          <ListItem
            key={l.prId}
            onClick={() => onSelect(l.prId)}
            startContent={
              <StatusDot
                variant={l.closed ? "neutral" : dots[l.status]}
                label={loopLabel(l)}
                isPulsing={!l.closed && l.status === "reviewing"}
              />
            }
            label={
              <Text weight="medium" maxLines={2}>
                {p?.title ?? l.prId}
              </Text>
            }
            description={
              <VStack gap={1}>
                <Text color="secondary">
                  {l.prId} · {loopLabel(l)} · Round {l.rounds} of {l.roundLimit}
                </Text>
                <HStack gap={3} vAlign="center" wrap="wrap">
                  <Text type="supporting">
                    Blocking findings {blockingTrend(rounds)}
                  </Text>
                  {changed && (
                    <Timestamp value={changed} {...recentTimestamp(index)} />
                  )}
                </HStack>
              </VStack>
            }
          />
        );
      })}
    </List>
  );
}

function sinceLastRound(r: Review) {
  const counts = new Map<PriorStatus, number>();
  for (const p of r.priorFindings ?? [])
    counts.set(p.status, (counts.get(p.status) ?? 0) + 1);
  return [...counts]
    .map(([status, n]) => `${n} ${priorStatusLabels[status]}`)
    .join(" · ");
}
function byPriority(r: Review) {
  const counts = [0, 1, 2, 3].map(
    (p) => r.findings?.filter((f) => f.priority === p).length ?? 0,
  );
  return counts
    .map((n, p) => (n ? `P${p} ${n}` : ""))
    .filter(Boolean)
    .join(" · ");
}
function verdictText(r: Review) {
  if (r.orbitVerdict === "clean") return "No blocking findings";
  if (r.orbitVerdict === "changes_requested")
    return `${r.blocking} blocking ${r.blocking === 1 ? "finding" : "findings"}`;
  if (r.status === "failed") return "The review failed";
  if (r.findings === null || r.findings === undefined)
    return r.status === "posted" || r.status === "ready"
      ? "No structured findings"
      : null;
  return null;
}

/** A loop's rounds in order; opening one goes to its chat. */
function Timeline({
  loop,
  pr,
  onBack,
  onChoose,
}: {
  loop: LoopState;
  pr: PullRequest | undefined;
  onBack: () => void;
  onChoose: AttentionHandlers["onChoose"];
}) {
  const { snapshot: s } = useOrbit();
  const rounds = roundsOf(s, loop);
  return (
    <VStack gap={4}>
      <HStack>
        <Button
          label="All loops"
          size="sm"
          variant="ghost"
          icon={<ArrowLeft />}
          onClick={onBack}
        />
      </HStack>
      <VStack gap={2}>
        <Heading level={2}>{pr?.title ?? loop.prId}</Heading>
        <HStack gap={3} vAlign="center" wrap="wrap">
          <StatusDot
            variant={loop.closed ? "neutral" : dots[loop.status]}
            label={loopLabel(loop)}
          />
          <Text color="secondary">
            {loop.prId} · {loopLabel(loop)} · Round {loop.rounds} of{" "}
            {loop.roundLimit}
          </Text>
        </HStack>
        {loop.reason && <Text>{loop.reason}</Text>}
        <Text type="supporting">Blocking findings {blockingTrend(rounds)}</Text>
      </VStack>
      <List hasDividers header={<Heading level={3}>Rounds</Heading>}>
        {rounds.map((r, i) => {
          const previous = r.previousRound?.headSha ?? rounds[i - 1]?.headSha;
          const verdict = verdictText(r);
          const priorities = byPriority(r);
          const since = sinceLastRound(r);
          return (
            <ListItem
              key={r.id}
              label={
                <Text weight="medium">
                  Round {i + 1}
                  {verdict ? ` · ${verdict}` : ""}
                </Text>
              }
              description={
                <VStack gap={2} paddingBlock={1}>
                  <Text color="secondary">
                    {previous && previous !== r.headSha
                      ? `Commits ${short(previous)} → ${short(r.headSha)}`
                      : `Head ${short(r.headSha)}`}
                  </Text>
                  {priorities && <Text>Findings {priorities}</Text>}
                  {since && <Text>Since the last round: {since}</Text>}
                  <HStack gap={3} vAlign="center" wrap="wrap">
                    <ReviewStatus status={reviewDisplayStatus(s, r)} />
                    <Timestamp value={r.updatedAt} {...recentTimestamp(i)} />
                  </HStack>
                  {/* Below the details, so narrow windows keep their width. */}
                  <HStack gap={2} vAlign="center" wrap="wrap">
                    {pr && (
                      <Button
                        label={`Open round ${i + 1} chat`}
                        size="sm"
                        onClick={() => onChoose(pr, r)}
                      />
                    )}
                    <ReviewActions review={r} />
                  </HStack>
                </VStack>
              }
            />
          );
        })}
      </List>
    </VStack>
  );
}

/** Every run, manual ones and those whose PR Orbit no longer knows too. */
function AllReviews({ onChoose }: Pick<AttentionHandlers, "onChoose">) {
  const { snapshot: s, action } = useOrbit();
  return (
    <List hasDividers density="spacious" aria-label="All reviews">
      {s.reviews.map((r, index) => {
        const p = knownPr(s, r.prId);
        return (
          <ListItem
            key={r.id}
            label={
              <Text weight="medium" maxLines={3}>
                {p?.title ?? r.prId}
              </Text>
            }
            onClick={p ? () => onChoose(p, r) : undefined}
            startContent={<GitPullRequest />}
            endContent={
              <VStack gap={2}>
                <ReviewActions review={r} />
                {activeStatuses.includes(r.status) && (
                  <Button
                    label={`Stop review ${r.prId}`}
                    size="sm"
                    icon={<Square />}
                    onClick={() =>
                      void action("cancel_review", { reviewId: r.id })
                    }
                  />
                )}
              </VStack>
            }
            description={
              <VStack gap={3} paddingBlock={2}>
                <Text color="secondary">
                  {r.prId} · {reportSelectionLabel(r)} ·{" "}
                  {r.automatic ? "Automatic" : "Manual"}
                </Text>
                <HStack gap={3} vAlign="center" wrap="wrap">
                  <ReviewStatus status={reviewDisplayStatus(s, r)} />
                  <Timestamp value={r.updatedAt} {...recentTimestamp(index)} />
                </HStack>
              </VStack>
            }
          />
        );
      })}
    </List>
  );
}

/** Which loops are running, which are finished, and which need you. */
export function Loops({
  selected,
  onSelect,
  ...go
}: AttentionHandlers & {
  /** The loop whose rounds are shown; the app clears it to show the overview. */
  selected: string | null;
  onSelect: (prId: string | null) => void;
}) {
  const { snapshot: s } = useOrbit();
  const all = loops(s);
  const loop = all.find((l) => l.prId === selected);
  if (loop)
    return (
      <VStack gap={6} padding={6} maxWidth={960} className="content-page">
        <Timeline
          loop={loop}
          pr={knownPr(s, loop.prId)}
          onBack={() => onSelect(null)}
          onChoose={go.onChoose}
        />
      </VStack>
    );
  const running = all.filter((l) => !loopFinished(l));
  const finished = all.filter(loopFinished);
  return (
    <VStack gap={6} padding={6} maxWidth={960} className="content-page">
      <VStack gap={2}>
        <Heading level={2}>Review loops</Heading>
        <Text color="secondary">
          Each pull request Orbit reviews on its own, round by round.
        </Text>
      </VStack>
      <NeedsYou {...go} />
      {running.length > 0 && (
        <LoopRows
          items={running}
          onSelect={onSelect}
          header={<Heading level={3}>Loops</Heading>}
        />
      )}
      {finished.length > 0 && (
        <Collapsible
          defaultIsOpen={false}
          trigger={<Heading level={3}>Finished ({finished.length})</Heading>}
        >
          <LoopRows
            items={finished}
            onSelect={onSelect}
            label="Finished loops"
          />
        </Collapsible>
      )}
      {s.reviews.length > 0 && (
        <Collapsible
          defaultIsOpen={false}
          trigger={
            <Heading level={3}>All reviews ({s.reviews.length})</Heading>
          }
        >
          <AllReviews onChoose={go.onChoose} />
        </Collapsible>
      )}
      {!all.length && (
        <EmptyState
          icon={<Repeat />}
          title="No review loops yet"
          description="Turn on automatic reviews for a repository and Orbit will review its pull requests round by round."
        />
      )}
    </VStack>
  );
}
