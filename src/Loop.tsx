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
  type LoopState,
  type LoopStatus,
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
      color={loopColors[state.status]}
      label={`Loop: ${loopStatusLabels[state.status]}`}
    />
  );
}

/** The loop's state, why it needs a person, and its controls. */
export function LoopPanel({ prId }: { prId: string }) {
  const { snapshot: s, action } = useOrbit();
  const [busy, setBusy] = useState(false);
  const state = loopState(s, prId);
  if (!state) return null;
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
      {state.reason && <Text>{state.reason}</Text>}
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
      <Text type="supporting" color="secondary">
        {paused
          ? "Continue allows two more automatic rounds. Review again always works and doesn’t count toward the limit."
          : "Stop pauses automatic reviews of this pull request. Review again still works."}
      </Text>
    </VStack>
  );
}
