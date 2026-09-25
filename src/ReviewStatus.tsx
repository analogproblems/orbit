import { Token } from "@astryxdesign/core";
import { displayStatusLabels, type DisplayStatus } from "./types";

export function ReviewStatus({
  status,
}: {
  status: DisplayStatus | "unreviewed" | "draft";
}) {
  return (
    <Token
      size="sm"
      color={
        status === "failed"
          ? "red"
          : status === "posted"
            ? "green"
            : status === "ready"
              ? "blue"
              : status === "outdated" || status === "awaitingApproval"
                ? "orange"
                : "default"
      }
      label={
        status === "unreviewed"
          ? "Not reviewed"
          : status === "draft"
            ? "Draft"
            : displayStatusLabels[status]
      }
    />
  );
}
