import { useState } from "react";
import {
  Button,
  HStack,
  Heading,
  List,
  ListItem,
  StatusDot,
  Text,
  Token,
  VStack,
} from "@astryxdesign/core";
import { openLink, useOrbit } from "./store";
import {
  loopState,
  loopStatusLabels,
  readinessLine,
  type LoopState,
  type LoopStatus,
  type Readiness,
  type Reviewer,
  type Reviews,
  type Snapshot,
  type Stand,
} from "./types";

const loopColors: Record<
  LoopStatus,
  "blue" | "orange" | "green" | "red" | "gray"
> = {
  reviewing: "blue",
  waiting_for_author: "orange",
  clean: "green",
  needs_you: "red",
  stopped: "gray",
};

/** The automatic review loop's state for a PR. */
export function LoopToken({ state }: { state: LoopState }) {
  return (
    <Token
      size="sm"
      color={state.closed ? "gray" : loopColors[state.status]}
      label={`Loop: ${state.closed ? "Left the inbox" : loopStatusLabels[state.status]}`}
    />
  );
}

/** Why GitHub refused the status check on the PR's current head, if it did. */
function latestStatusError(s: Snapshot, prId: string) {
  const pr = s.prs.find((p) => p.id === prId);
  const check = s.commitChecks?.find(
    (c) =>
      c.sha === pr?.headSha && c.repo.toLowerCase() === pr.repo.toLowerCase(),
  );
  return check?.refused?.error ?? undefined;
}

/** The loop's state, why it needs a person, and its controls. */
export function LoopPanel({ prId }: { prId: string }) {
  const { snapshot: s, action } = useOrbit();
  const [busy, setBusy] = useState(false);
  const state = loopState(s, prId);
  if (!state) return null;
  const statusError = latestStatusError(s, prId);
  const run = async (command: string, args: Record<string, unknown>) => {
    setBusy(true);
    try {
      await action(command, { prId, ...args });
    } finally {
      setBusy(false);
    }
  };
  const paused =
    state.status === "stopped" ||
    (state.status === "needs_you" && state.rounds >= state.roundLimit);
  return (
    <VStack gap={2}>
      <Heading level={3}>Review loop</Heading>
      <HStack gap={2} vAlign="center" wrap="wrap">
        <LoopToken state={state} />
        <Text type="supporting" color="secondary">
          Round {state.rounds} of {state.roundLimit}
        </Text>
      </HStack>
      {state.reason && !state.closed && <Text>{state.reason}</Text>}
      {statusError && (
        <Text type="supporting" color="secondary">
          GitHub did not accept the status check: {statusError}
        </Text>
      )}
      {!state.closed && (
        <HStack gap={2}>
          {paused ? (
            <Button
              label="Continue loop"
              size="sm"
              isLoading={busy}
              isDisabled={busy}
              onClick={() => void run("continue_loop", { rounds: 2 })}
            />
          ) : (
            <Button
              label="Stop loop"
              size="sm"
              isLoading={busy}
              isDisabled={busy}
              onClick={() => void run("stop_loop", {})}
            />
          )}
        </HStack>
      )}
      {!state.closed && (
        <Text type="supporting" color="secondary">
          {paused
            ? "Continue allows two more automatic rounds. Review again always works and doesn’t count toward the limit."
            : "Stop pauses automatic reviews of this pull request. Review again still works."}
        </Text>
      )}
    </VStack>
  );
}

/** Whether the pull request is clear to merge, and what is in the way. */
export function ReadinessPanel({ prId }: { prId: string }) {
  const { snapshot: s } = useOrbit();
  const r = s.readiness?.find((x) => x.prId === prId);
  if (!r) return null;
  // Where the agent reads it.
  const told = s.ops?.find((t) => t.prId === prId && t.ready && t.url);
  // Whom Orbit nudged, once, about an answer that has stood for a while:
  // the agent only once GitHub has the reminder.
  const first = r.waits[0];
  const reminded =
    first?.turn === "author" &&
    told?.ready?.turn === "author" &&
    !!told.ready.reminded;
  const stuck = (s.attention ?? []).some(
    (a) => a.kind === "stuck" && a.prId === prId,
  );
  return (
    <VStack gap={2}>
      <Heading level={3}>Merging</Heading>
      <HStack gap={2} vAlign="center" wrap="wrap">
        <Token
          size="sm"
          color={r.clear ? "green" : "orange"}
          label={r.clear ? "Clear to merge" : "Not clear yet"}
        />
        {told && (
          <Button
            label="Open Orbit's comment"
            size="sm"
            variant="secondary"
            onClick={() => openLink(told.url)}
          />
        )}
      </HStack>
      {r.clear ? (
        <Text>{readinessLine(r)}</Text>
      ) : (
        r.waits.map((w) => <Text key={`${w.kind}:${w.text}`}>{w.text}</Text>)
      )}
      {reminded && (
        <Text>
          Orbit reminded the author once on its comment: this has waited on them
          for more than 2 hours.
        </Text>
      )}
      {stuck && (
        <Text>
          This has waited on a person for more than a day, so it is in Needs
          you.
        </Text>
      )}
      {r.reviews && <ReviewsList r={r} reviews={r.reviews} />}
      <Text type="supporting" color="secondary">
        Clear means Orbit’s review of this commit is clean, every check passed
        with the base branch’s latest commit, GitHub says it merges, and where
        the branch’s rules need approvals, enough people approved this commit.
        Orbit tells the agent at the top of its comment on the pull request once
        it is clear or waits on someone, but never merges it itself.
      </Text>
    </VStack>
  );
}

type Dot = {
  variant: "success" | "warning" | "error" | "neutral";
  label: string;
};
const stands: Record<Stand, Dot> = {
  asked: { variant: "neutral", label: "Asked" },
  approved: { variant: "success", label: "Approved" },
  approved_older: { variant: "warning", label: "Approved an older commit" },
  changes_requested: { variant: "error", label: "Asked for changes" },
};
/** A reviewer's dot: an approval or request for changes from someone
 * without write access, or whose access Orbit can't tell, doesn't count
 * as one. */
function dotOf(x: Reviewer): Dot {
  if (x.stand === "approved" && x.mayApprove !== true)
    return { variant: "warning", label: "Approval doesn’t count" };
  if (x.stand === "changes_requested" && x.mayApprove === false)
    return { variant: "neutral", label: "Request doesn’t count" };
  return stands[x.stand];
}
const short = (sha: string) => sha.slice(0, 7);
/** Where a reviewer stands on the latest commit, `head`. */
function standText(x: Reviewer, head: string) {
  switch (x.stand) {
    case "asked":
      return `Hasn’t reviewed ${short(head)} yet.`;
    case "approved":
      if (x.mayApprove === false)
        return `Approved ${short(head)}, but without write access that doesn’t count.`;
      if (x.mayApprove !== true)
        return `Approved ${short(head)}, but Orbit can’t tell whether they have write access.`;
      return `Approved ${short(head)}.`;
    case "approved_older":
      return `Approved ${short(x.commit)}, but not ${short(head)}.`;
    case "changes_requested":
      if (x.mayApprove === false)
        return `Asked for changes on ${short(x.commit)}, but without write access that doesn’t hold it back.`;
      return x.commit === head
        ? `Asked for changes on ${short(head)}.`
        : `Asked for changes on ${short(x.commit)}, and hasn’t reviewed ${short(head)} since.`;
  }
}
/** What the base branch's rules need, or why Orbit can't tell. */
function ruleText(r: Readiness, reviews: Reviews) {
  const rule = reviews.rule;
  if (!rule) return reviews.unseen ?? `Orbit can’t see ${r.base}’s rules.`;
  const owners = "a code owner’s approval, which GitHub checks";
  if (rule.approvals === 0)
    return rule.codeOwners
      ? `${r.base}’s rules need ${owners}.`
      : `${r.base}’s rules need no approvals.`;
  const count =
    rule.approvals === 1 ? "1 approval" : `${rule.approvals} approvals`;
  return `${r.base}’s rules need ${count}${rule.codeOwners ? `, and ${owners}` : ""}. Only approvals of ${short(r.headSha)}, its latest commit, count.`;
}
/** Who was asked to review the pull request, where each stands, and what
 * its base branch's rules need. */
function ReviewsList({ r, reviews }: { r: Readiness; reviews: Reviews }) {
  return (
    <VStack gap={1}>
      <Heading level={4}>Reviews</Heading>
      {reviews.tooMany ? (
        <Text color="secondary">
          It has more reviews than Orbit reads, so Orbit can’t list them.
        </Text>
      ) : reviews.unread ? (
        <Text color="secondary">
          Orbit couldn’t read its reviews: {reviews.unread}.
        </Text>
      ) : reviews.reviewers.length > 0 ? (
        <List hasDividers density="compact" aria-label="Reviewers">
          {reviews.reviewers.map((x) => {
            const stand = dotOf(x);
            return (
              <ListItem
                key={`${x.team ? "team" : "user"}:${x.name}`}
                startContent={
                  <StatusDot variant={stand.variant} label={stand.label} />
                }
                label={
                  <Text weight="medium">
                    {x.team ? `${x.name} team` : x.name}
                  </Text>
                }
                description={
                  <Text color="secondary">{standText(x, r.headSha)}</Text>
                }
              />
            );
          })}
        </List>
      ) : (
        <Text color="secondary">Nobody is asked to review it.</Text>
      )}
      <Text type="supporting" color="secondary">
        {ruleText(r, reviews)}
      </Text>
    </VStack>
  );
}
