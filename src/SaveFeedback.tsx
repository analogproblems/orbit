import { Banner, Button, Spinner, Text, VStack } from "@astryxdesign/core";
import { useOrbit } from "./store";
import { describePatch } from "./describePatch";
import { type SettingsPatch } from "./types";

export function SaveFeedback({ saveKey }: { saveKey?: string }) {
  const { saves, retrySave, snapshot } = useOrbit();
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
                ? `Not applied — ${describePatch(value.args.patch as SettingsPatch, snapshot.models)}. Controls show the active settings.`
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
