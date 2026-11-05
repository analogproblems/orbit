import { useState } from "react";
import { Button } from "@astryxdesign/core";
import { useOrbit } from "./store";
import { buddyOf, type Attention, type Problem, type Snapshot } from "./types";

/** The question **Ask Bit why** asks about a failing problem, in the words
 * you'd type, or null for a problem that isn't something failing. */
export function whyQuestion(p: Problem): string | null {
  if (p.kind === "default_branch")
    return `Why is CI failing on ${p.branch || "the default branch"} in ${p.repo}?`;
  if (p.kind === "pull_request" && p.prId)
    return `Why is CI failing on ${p.prId}?`;
  if (p.kind === "release_build")
    return `Why is the release build for ${p.branch} failing in ${p.repo}?`;
  return null;
}

/** The open problem a Needs you item is about, from its key
 * (`problem:<id>:<alerts>`). */
export function problemOfItem(s: Snapshot, a: Attention) {
  if (!a.key.startsWith("problem:")) return undefined;
  const id = a.key.split(":")[1];
  return s.problems?.find((p) => p.id === id && !p.closedAt);
}

/** **Ask Bit why**: asks Bit about a failing problem for you, as if you'd
 * typed the question, then opens Bit's conversation for its answer. Not
 * shown while Bit is off; greyed out while Bit answers another question. */
export function AskWhy({
  problem,
  onAsked,
}: {
  problem: Problem;
  onAsked?: () => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const [asking, setAsking] = useState(false);
  const buddy = buddyOf(s.settings);
  const question = whyQuestion(problem);
  if (!buddy.enabled || !question) return null;
  // A question Bit asked itself stops for yours.
  const answering = (s.buddyFeed?.notes ?? []).some((n) => n.pending && !n.own);
  return (
    <Button
      label={`Ask ${buddy.name} why`}
      size="sm"
      variant="secondary"
      isLoading={asking}
      isDisabled={asking || answering}
      tooltip={
        answering
          ? `${buddy.name} is still answering your last question.`
          : undefined
      }
      onClick={async () => {
        setAsking(true);
        const result = await action("ask_buddy_anything", { question });
        setAsking(false);
        if (result.ok) onAsked?.();
      }}
    />
  );
}
