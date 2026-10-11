import { Banner, Button, Spinner, Text, VStack } from "@astryxdesign/core";
import { useOrbit } from "./store";
import {
  fixLabels,
  fixRuleLabels,
  type Buddy,
  type FixRule,
  type RuledFix,
  type SettingsPatch,
} from "./types";

export function SaveFeedback({ saveKey }: { saveKey?: string }) {
  const { saves, retrySave, snapshot } = useOrbit();
  const describePatch = (patch: SettingsPatch) => {
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
    const buddyEntries = (
      buddy: NonNullable<SettingsPatch["buddy"]>,
    ): [string, unknown, string][] =>
      Object.entries(buddy).flatMap(([field, v]) =>
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
              ? snapshot.models.find((m) => m.model === value)?.displayName ||
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
  };
  const entries = Object.entries(saves).filter(([key]) =>
    saveKey ? key === saveKey : !key.startsWith("repository:"),
  );
  if (saveKey && !entries.length) return null;
  return (
    <VStack gap={2} aria-live="polite">
      {entries.some(([, value]) => value.state === "saving") ? (
        <Spinner label="Saving…" />
      ) : (
        <Text type="supporting" color="secondary">
          {entries.some(([, value]) => value.state === "error")
            ? "Some changes were not saved."
            : entries.length
              ? "Saved"
              : "Changes save automatically on this device."}
        </Text>
      )}
      {entries
        .filter(([, value]) => value.state === "error")
        .map(([key, value]) => (
          <Banner
            key={key}
            status="error"
            title={value.error}
            description={
              value.command === "update_settings"
                ? `Not applied — ${describePatch(value.args.patch as SettingsPatch)}. Controls show the active settings.`
                : undefined
            }
            endContent={
              <Button
                label="Retry saving"
                size="sm"
                onClick={() => void retrySave(key)}
              />
            }
          />
        ))}
    </VStack>
  );
}
