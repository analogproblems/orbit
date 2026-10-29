import { bitColours, bitHats, bitLooks } from "./Bit";
import {
  buddyAnswerers,
  fixLabels,
  fixRuleLabels,
  providerLabels,
  type Buddy,
  type BuddyAnswerer,
  type BuddyHat,
  type BuddyLook,
  type FixRule,
  type Model,
  type RepositoryPatch,
  type RuledFix,
  type SettingsPatch,
  type WriteUps,
  writerLabels,
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
  blockingPriority: "Blocking priorities",
  blockFindings: "Block on findings",
  settlePushes: "Wait for pushes to settle",
  commitStatus: "GitHub status check",
  notify: "Notifications",
  keepScanArtifacts: "Keep scan working files",
  buddy: "Buddy",
  writeUps: "Write-ups",
};
const buddyLabels: Record<keyof Buddy, string> = {
  enabled: "Show your buddy",
  name: "Buddy's name",
  colour: "Buddy's colour",
  hat: "Buddy's hat",
  look: "Buddy's look",
  tone: "Buddy's tone",
  personality: "Buddy's personality",
  rules: "Buddy's rules",
  answerer: "Who answers your questions",
  ollamaUrl: "Ollama address",
  ollamaModel: "Ollama model",
  openaiModel: "OpenAI model",
  anthropicModel: "Anthropic model",
  googleModel: "Gemini model",
};

/** A buddy setting's value as the settings page names it, such as Pink. */
function buddyValue(field: string, v: unknown): unknown {
  const name =
    field === "answerer"
      ? buddyAnswerers[v as BuddyAnswerer]
      : field === "colour"
        ? bitColours[v as string]?.label
        : field === "hat"
          ? bitHats[v as BuddyHat]
          : field === "look"
            ? bitLooks[v as BuddyLook]?.label
            : undefined;
  return name ?? v;
}

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
      : [[field, buddyValue(field, v), buddyLabels[field as keyof Buddy]]],
  );
}

const writeUpsLabel = (w: WriteUps) =>
  w.enabled ? `On, by ${writerLabels[w.writer]}` : "Off";

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
                  : key === "writeUps"
                    ? writeUpsLabel(value as WriteUps)
                    : Array.isArray(value)
                      ? value.join(", ") || "None"
                      : value;
      return `${label}: ${display}`;
    })
    .join("; ");
}

const repositoryLabels: Record<keyof RepositoryPatch, string> = {
  setupCommand: "Setup command",
  checkCommand: "Check command",
  networkAccess: "Network access",
  devContainer: "Dev container",
  reviewer: "Reviewer",
  roundLimit: "Automatic rounds per pull request",
  approvalMinutes: "Approval time limit",
  keepCloneUpdated: "Keep the copy up to date",
  autopilot: "Automatic reviews",
};

/** A repository change in words, such as "Setup command: npm ci; Network
 * access: On". Commands are shown exactly as they would be saved. */
export function describeRepositoryPatch(patch: RepositoryPatch): string {
  return (Object.entries(patch) as [keyof RepositoryPatch, unknown][])
    .map(([key, value]) => {
      const display =
        typeof value === "boolean"
          ? value
            ? "On"
            : "Off"
          : key === "reviewer"
            ? providerLabels[value as keyof typeof providerLabels]
            : key === "approvalMinutes"
              ? value
                ? `Decline after ${value} minutes`
                : "Wait for me"
              : key === "setupCommand" || key === "checkCommand"
                ? value || "None"
                : value;
      return `${repositoryLabels[key]}: ${display}`;
    })
    .join("; ");
}
