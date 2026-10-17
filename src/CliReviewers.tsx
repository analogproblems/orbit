import { useState } from "react";
import {
  Banner,
  Button,
  Card,
  FormLayout,
  Heading,
  HStack,
  ListItem,
  Selector,
  StatusDot,
  Text,
  VStack,
} from "@astryxdesign/core";
import { Bot, RefreshCw, Sparkles } from "lucide-react";
import { useOrbit } from "./store";
import {
  type CliProvider,
  type Settings,
  type SettingsPatch,
  type Provider,
  cliStatus,
  connectionState,
  defaultEffort,
  providerLabels,
  providerModels,
  providerSelection,
  reviewerSelectionUnavailable,
  reviewerUnavailable,
  withPatch,
} from "./types";
import { SaveFeedback } from "./SaveFeedback";

/** What Settings says about each CLI reviewer. */
const cli = {
  claude: {
    name: "Claude Code CLI",
    label: "Claude",
    icon: <Bot />,
    signIn: "Uses your Claude sign-in",
    missing:
      "Optional. Install Claude Code and sign in to choose Claude as a repository's reviewer.",
    about:
      "Used by repositories whose reviewer is Claude. Claude reviews with its own code review rules, runs only a short list of Git and text commands inside its sandbox, and has no network access.",
    modelKey: "claudeModel",
    effortKey: "claudeEffort",
  },
  antigravity: {
    name: "Antigravity CLI",
    label: "Antigravity",
    icon: <Sparkles />,
    signIn: "Uses your Antigravity sign-in and settings",
    missing:
      "Optional. Install the Antigravity CLI (agy) and sign in to choose Antigravity as a repository's reviewer.",
    about:
      "Used by repositories whose reviewer is Antigravity. Antigravity follows Orbit's review instructions inside its sandbox, with no network access. Orbit reads your Antigravity settings but never changes them, and won't start a review if they would let a pull request reach past the sandbox.",
    modelKey: "antigravityModel",
    effortKey: "antigravityEffort",
  },
} as const;

/** The draft settings: saved settings with any save in progress applied. */
function useDraft(): Settings {
  const { snapshot: s, saves } = useOrbit();
  let draft = { ...s.settings };
  for (const save of Object.values(saves)) {
    if (save.command === "update_settings" && save.state === "saving")
      draft = withPatch(draft, save.args.patch as SettingsPatch);
  }
  return draft;
}

/** Checks every connection again, and whether a check is running. */
export function useReconnect(): [boolean, () => Promise<void>] {
  const { action } = useOrbit();
  const [connecting, setConnecting] = useState(false);
  const reconnect = async () => {
    setConnecting(true);
    await action("connect_services");
    setConnecting(false);
  };
  return [connecting, reconnect];
}

/** Whether any connected repository is reviewed by `provider`. */
export function usedBy(settings: Settings, provider: Provider): boolean {
  return settings.repositories.some(
    (r) => (r.provider ?? "codex") === provider,
  );
}

/** What stops a reviewer, if anything, with a way to check again. A
 * reviewer that isn't connected says why whether or not a repository uses
 * it; a wrong model or effort only matters once one does. Codex also runs
 * Bit and write-ups, so it always counts as used. */
export function ReviewerProblem({ provider }: { provider: Provider }) {
  const { snapshot: s } = useOrbit();
  const draft = useDraft();
  const [connecting, reconnect] = useReconnect();
  const label = providerLabels[provider];
  const used = provider === "codex" || usedBy(draft, provider);
  const unavailable = reviewerUnavailable(s, provider);
  const problem = unavailable
    ? used
      ? `${label} reviews are blocked: ${unavailable}`
      : `${label} isn’t set up: ${unavailable}`
    : used && reviewerSelectionUnavailable(s, provider)
      ? `${label} reviews are blocked: ${reviewerSelectionUnavailable(s, provider)}`
      : null;
  if (!problem) return null;
  return (
    <Banner
      status={used ? "warning" : "info"}
      title={problem}
      endContent={
        unavailable ? (
          <Button
            label={connecting ? "Checking…" : "Check again"}
            size="sm"
            icon={<RefreshCw />}
            isLoading={connecting}
            onClick={() => void reconnect()}
          />
        ) : undefined
      }
    />
  );
}

/** A CLI reviewer's line in Connections. */
export function CliConnection({ provider }: { provider: CliProvider }) {
  const { snapshot: s } = useOrbit();
  const draft = useDraft();
  const about = cli[provider];
  const status = cliStatus(s, provider);
  const used = draft.repositories.some((r) => r.provider === provider);
  return (
    <ListItem
      label={about.name}
      startContent={about.icon}
      description={
        <Text color="secondary">
          {status?.error ||
            (status?.ready
              ? `${about.signIn}${status.version ? ` · ${status.version}` : ""}.`
              : about.missing)}
        </Text>
      }
      endContent={
        <HStack gap={2} vAlign="center">
          <StatusDot
            label={`${about.label} connection`}
            variant={status?.ready ? "success" : used ? "warning" : "neutral"}
          />
          <Text color="secondary" type="supporting">
            {connectionState(!!status?.ready, status?.error)}
          </Text>
        </HStack>
      }
    />
  );
}

/** A CLI reviewer's model and effort. */
export function CliReviewSettings({ provider }: { provider: CliProvider }) {
  const { snapshot: s, updateSettings, saves } = useOrbit();
  const draft = useDraft();
  const about = cli[provider];
  const models = providerModels(s, provider);
  const selection = providerSelection({ ...s, settings: draft }, provider);
  const model = models.find((m) => m.model === selection.model);
  // Settings for a reviewer that isn't connected would only be guesses.
  const off = !!reviewerUnavailable(s, provider);
  const { modelKey, effortKey } = about;
  const pending = (key: string) => saves[key]?.state === "saving";
  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={2}>
          <Heading level={3}>{about.label} reviews</Heading>
          <Text color="secondary">{about.about}</Text>
        </VStack>
        <ReviewerProblem provider={provider} />
        <FormLayout>
          <VStack gap={2}>
            <Selector
              label={`${about.label} model`}
              value={selection.model}
              placeholder="Choose a model"
              isDisabled={off || pending(modelKey) || !models.length}
              options={[
                ...(selection.model && !model
                  ? [
                      {
                        value: selection.model,
                        label: `${selection.model} (unavailable)`,
                        disabled: true,
                      },
                    ]
                  : []),
                ...models.map((m) => ({
                  value: m.model,
                  label: m.displayName,
                })),
              ]}
              onChange={(value) => {
                const selected = models.find((m) => m.model === value);
                if (selected && defaultEffort(selected))
                  void updateSettings(
                    {
                      [modelKey]: value,
                      [effortKey]: defaultEffort(selected)!,
                    },
                    modelKey,
                  );
              }}
            />
            <SaveFeedback saveKey={modelKey} />
          </VStack>
          <VStack gap={2}>
            <Selector
              label={`${about.label} effort`}
              value={selection.effort}
              placeholder="Choose an effort"
              isDisabled={
                off ||
                !model ||
                pending(effortKey) ||
                (!!saves[modelKey] && saves[modelKey].state !== "saved")
              }
              description="Higher effort reviews more carefully and takes longer."
              options={[
                ...(selection.effort &&
                !model?.supportedReasoningEfforts.some(
                  (e) => e.reasoningEffort === selection.effort,
                )
                  ? [
                      {
                        value: selection.effort,
                        label: `${selection.effort} (unsupported)`,
                        disabled: true,
                      },
                    ]
                  : []),
                ...(model?.supportedReasoningEfforts.map((e) => ({
                  value: e.reasoningEffort,
                  label:
                    e.reasoningEffort[0].toUpperCase() +
                    e.reasoningEffort.slice(1),
                })) ?? []),
              ]}
              onChange={(effort) =>
                void updateSettings(
                  { [modelKey]: selection.model, [effortKey]: effort },
                  effortKey,
                )
              }
            />
            <SaveFeedback saveKey={effortKey} />
          </VStack>
        </FormLayout>
      </VStack>
    </Card>
  );
}
