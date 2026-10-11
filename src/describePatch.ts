import {
  fixLabels,
  fixRuleLabels,
  type Buddy,
  type FixRule,
  type Model,
  type RuledFix,
  type SettingsPatch,
} from "./types";

const labels: Record<keyof SettingsPatch, string> = {
  automatic: "Automatic reviews",
  autoPost: "Automatic publication",
  model: "Model",
  effort: "Reasoning effort",
  claudeModel: "Claude model",
  claudeEffort: "Claude effort",
  antigravityModel: "Antigravity model",
  antigravityEffort: "Antigravity effort",
  concurrency: "Concurrent reviews",
  pollSeconds: "Poll interval",
  trustedAuthors: "Trusted accounts",
  blockingPriority: "Blocking findings",
  settlePushes: "Wait for pushes to settle",
  commitStatus: "GitHub status check",
  notify: "Notifications",
  keepScanArtifacts: "Keep scan working files",
  buddy: "Buddy",
};
const buddyLabels: Record<keyof Buddy, string> = {
  enabled: "Show your buddy",
  name: "Buddy's name",
  colour: "Buddy's colour",
  hat: "Buddy's hat",
  tone: "Buddy's tone",
  personality: "Buddy's personality",
  rules: "Buddy's rules",
};

function buddyEntries(
  buddy: NonNullable<SettingsPatch["buddy"]>,
): [string, unknown, string][] {
  return Object.entries(buddy).flatMap(([field, v]) =>
    field === "rules"
      ? Object.entries(v as Record<RuledFix, FixRule>).map(
          ([fix, rule]): [string, unknown, string] => [
            fix,
            fixRuleLabels[rule],
            `${fixLabels[fix as RuledFix]} rule`,
          ],
        )
      : [[field, v, buddyLabels[field as keyof Buddy]]],
  );
}

/** A settings change in words, such as "Poll interval: 600 seconds". */
export function describePatch(patch: SettingsPatch, models: Model[]): string {
  return Object.entries(patch)
    .flatMap(([key, value]): [string, unknown, string][] =>
      key === "buddy"
        ? buddyEntries(value as NonNullable<SettingsPatch["buddy"]>)
        : [[key, value, labels[key as keyof SettingsPatch]]],
    )
    .map(([key, value, label]) => {
      const display =
        typeof value === "boolean"
          ? value
            ? "On"
            : "Off"
          : key === "model"
            ? models.find((m) => m.model === value)?.displayName ||
              value ||
              "Unresolved model"
            : key === "effort"
              ? value || "Unresolved effort"
              : key === "pollSeconds"
                ? `${value} seconds`
                : key === "blockingPriority"
                  ? value === 0
                    ? "P0 only"
                    : `P0 to P${value}`
                  : Array.isArray(value)
                    ? value.join(", ") || "None"
                    : value;
      return `${label}: ${display}`;
    })
    .join("; ");
}
