import { useState } from "react";
import {
  Button,
  HStack,
  Heading,
  Text,
  Token,
  VStack,
} from "@astryxdesign/core";
import { useOrbit } from "./store";
import {
  loopState,
  loopStatusLabels,
  readinessLine,
  type LoopState,
  type LoopStatus,
  type Snapshot,
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
  return (
    <VStack gap={2}>
      <Heading level={3}>Merging</Heading>
      <HStack gap={2} vAlign="center" wrap="wrap">
        <Token
          size="sm"
          color={r.clear ? "green" : "orange"}
          label={r.clear ? "Clear to merge" : "Not clear yet"}
        />
      </HStack>
      {r.clear ? (
        <Text>{readinessLine(r)}</Text>
      ) : (
        r.waits.map((w) => <Text key={`${w.kind}:${w.text}`}>{w.text}</Text>)
      )}
      <Text type="supporting" color="secondary">
        Clear means Orbit’s review of this commit is clean, every check passed
        with the base branch’s latest commit, and GitHub says it merges. Orbit
        never merges it itself.
      </Text>
    </VStack>
  );
}
