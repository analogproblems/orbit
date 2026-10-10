import { useState } from "react";
import {
  Banner,
  Button,
  Card,
  Code,
  FormLayout,
  Heading,
  HStack,
  List,
  ListItem,
  Selector,
  StatusDot,
  Switch,
  Text,
  TextInput,
  Token,
  VStack,
} from "@astryxdesign/core";
import { Code2, GitBranch, RefreshCw } from "lucide-react";
import { useOrbit, live } from "./store";
import {
  defaultEffort,
  withPatch,
  workspaceSelectionUnavailable,
  type SettingsPatch,
} from "./types";
import { SaveFeedback } from "./SaveFeedback";
import { Storage } from "./Storage";
import { RemoteAccess } from "./RemoteAccess";
import { CliConnection, CliReviewSettings } from "./CliReviewers";
import { BuddySettings } from "./Buddy";
import { ReplySkill } from "./ReplySkill";

export function SettingsPage() {
  const {
    snapshot: s,
    action,
    updateSettings,
    saves,
    trustedInput: newLogin,
    setTrustedInput: setNewLogin,
  } = useOrbit();
  let draft = { ...s.settings };
  for (const save of Object.values(saves)) {
    if (save.command === "update_settings" && save.state === "saving")
      draft = withPatch(draft, save.args.patch as SettingsPatch);
  }
  const [connecting, setConnecting] = useState(false);
  const addLogin = async () => {
    const login = newLogin.trim();
    if (!login || saves.trustedAuthors?.state === "saving") return;
    if (
      draft.trustedAuthors.some((a) => a.toLowerCase() === login.toLowerCase())
    )
      return;
    const result = await updateSettings({
      trustedAuthors: [...draft.trustedAuthors, login],
    });
    if (result.ok && useOrbit.getState().trustedInput === newLogin)
      setNewLogin("");
  };
  const model = s.models.find((m) => m.model === draft.model);
  const pending = (key: string) => saves[key]?.state === "saving";
  return (
    <VStack gap={6} padding={6} maxWidth={760} className="content-page">
      <VStack gap={2}>
        <Heading level={2}>A review flow that fits.</Heading>
        <Text color="secondary">
          Set the pace, pick the model, and choose how hands-on you want to be.
        </Text>
      </VStack>
      <Card>
        <VStack gap={4}>
          <HStack hAlign="between" vAlign="center">
            <Heading level={3}>Connections</Heading>
            <Button
              label={connecting ? "Connecting…" : "Reconnect"}
              variant="ghost"
              icon={<RefreshCw />}
              isLoading={connecting}
              onClick={async () => {
                setConnecting(true);
                await action("connect_services");
                setConnecting(false);
              }}
            />
          </HStack>
          <List hasDividers>
            <ListItem
              label="Codex CLI"
              startContent={<Code2 />}
              description={
                <Text color="secondary">
                  {s.codexError ||
                    "Uses your existing Codex login and available models."}
                </Text>
              }
              endContent={
                <HStack gap={2} vAlign="center">
                  <StatusDot
                    label="Codex connection"
                    variant={s.codexReady ? "success" : "warning"}
                  />
                  <Text color="secondary" type="supporting">
                    {s.codexReady ? "Connected" : "Not connected"}
                  </Text>
                </HStack>
              }
            />
            <CliConnection provider="claude" />
            <CliConnection provider="antigravity" />
            <ListItem
              label="GitHub CLI"
              startContent={<GitBranch />}
              description={
                <Text color="secondary">
                  {s.githubError ||
                    (s.githubUser
                      ? `Connected as @${s.githubUser}`
                      : "Run gh auth login in your terminal to connect.")}
                </Text>
              }
              endContent={
                <HStack gap={2} vAlign="center">
                  <StatusDot
                    label="GitHub connection"
                    variant={
                      s.githubUser && !s.githubError ? "success" : "warning"
                    }
                  />
                  <Text color="secondary" type="supporting">
                    {s.githubUser && !s.githubError
                      ? "Connected"
                      : "Not connected"}
                  </Text>
                </HStack>
              }
            />
          </List>
          {!live && (
            <Banner
              status="info"
              title="Ready for your real repositories?"
              description={
                <Text>
                  Run <Code>npm run desktop</Code> in this project. The native
                  app connects to <Code>gh</Code> and <Code>codex</Code>{" "}
                  installed on your computer.
                </Text>
              }
              endContent={
                <Button
                  label="Reset sample workspace"
                  size="sm"
                  onClick={() => void action("reset_demo")}
                />
              }
            />
          )}
        </VStack>
      </Card>
      <Card>
        <VStack gap={4}>
          <Heading level={3}>Codex reviews</Heading>
          {workspaceSelectionUnavailable(s) && (
            <Banner
              status="warning"
              title={`Reviews are blocked: ${workspaceSelectionUnavailable(s)}`}
            />
          )}
          {s.modelSelectionNotice && (
            <Text role="status" color="secondary">
              {s.modelSelectionNotice}
            </Text>
          )}
          <FormLayout>
            <VStack gap={2}>
              <Selector
                label="Model"
                value={draft.model}
                placeholder="Choose a model"
                description="Available models are loaded from your Codex account."
                isDisabled={pending("model") || !s.modelCatalogReady}
                options={[
                  ...(draft.model && !model
                    ? [
                        {
                          value: draft.model,
                          label: `${draft.model} (unavailable)`,
                          disabled: true,
                        },
                      ]
                    : []),
                  ...s.models.map((m) => ({
                    value: m.model,
                    label:
                      m.displayName +
                      (!defaultEffort(m)
                        ? " (no supported default effort)"
                        : ""),
                    disabled: !defaultEffort(m),
                  })),
                ]}
                onChange={(value) => {
                  const selected = s.models.find((m) => m.model === value);
                  if (selected && defaultEffort(selected))
                    void updateSettings(
                      { model: value, effort: defaultEffort(selected)! },
                      "model",
                    );
                }}
              />
              <SaveFeedback saveKey="model" />
            </VStack>
            <VStack gap={2}>
              <Selector
                label="Reasoning effort"
                value={draft.effort}
                placeholder="Choose an effort"
                isDisabled={
                  !s.modelCatalogReady ||
                  !model ||
                  pending("effort") ||
                  (!!saves.model && saves.model.state !== "saved")
                }
                description={
                  saves.model?.state === "error"
                    ? "Retry the failed model change or choose a model again before changing effort."
                    : "More effort gives the agent more time to reason."
                }
                options={[
                  ...(draft.effort &&
                  !model?.supportedReasoningEfforts.some(
                    (e) => e.reasoningEffort === draft.effort,
                  )
                    ? [
                        {
                          value: draft.effort,
                          label: `${draft.effort} (unsupported)`,
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
                onChange={(effort) => void updateSettings({ effort })}
              />
              <SaveFeedback saveKey="effort" />
            </VStack>
          </FormLayout>
        </VStack>
      </Card>
      <CliReviewSettings provider="claude" />
      <CliReviewSettings provider="antigravity" />
      <Card>
        <VStack gap={6}>
          <Heading level={3}>Automation</Heading>
          <VStack gap={2}>
            <Switch
              label="Automatically review new pull requests"
              labelPosition="start"
              labelSpacing="spread"
              width="100%"
              value={draft.automatic}
              isDisabled={pending("automatic")}
              onChange={(automatic) => void updateSettings({ automatic })}
              description="Create a sandbox and chat for each new commit in enabled repositories. Drafts are skipped, as are pull requests from forks or from authors who aren’t owners, members, collaborators, or trusted accounts."
            />
            <SaveFeedback saveKey="automatic" />
          </VStack>
          <VStack gap={2}>
            <Switch
              label="Automatically publish findings"
              labelPosition="start"
              labelSpacing="spread"
              width="100%"
              value={draft.autoPost}
              isDisabled={pending("autoPost")}
              onChange={(autoPost) => void updateSettings({ autoPost })}
              description="Post completed automatic reviews as PR comments using your GitHub account. Only pull requests opened from the repository itself by its owners, members, collaborators, or trusted accounts are published automatically, and never when a report looks like it contains credentials. Everything else waits for you to publish it."
            />
            <SaveFeedback saveKey="autoPost" />
          </VStack>
          <VStack gap={3}>
            <Text weight="semibold">Trusted accounts</Text>
            <Text color="secondary">
              Pull requests these accounts open from a branch in the repository
              are reviewed and published automatically, like your own. Only add
              bots that act for you: anyone who can direct a trusted bot can get
              its pull requests reviewed automatically.
            </Text>
            <HStack gap={2} wrap="wrap">
              {draft.trustedAuthors.map((login) => (
                <Token
                  key={login}
                  label={login}
                  isDisabled={pending("trustedAuthors")}
                  onRemove={() =>
                    void updateSettings({
                      trustedAuthors: draft.trustedAuthors.filter(
                        (a) => a !== login,
                      ),
                    })
                  }
                />
              ))}
            </HStack>
            <HStack gap={2} vAlign="end">
              <TextInput
                label="Add a trusted account"
                placeholder="e.g. claude[bot]"
                width="100%"
                value={newLogin}
                onChange={setNewLogin}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void addLogin();
                  }
                }}
              />
              <Button
                label="Add"
                isDisabled={!newLogin.trim() || pending("trustedAuthors")}
                onClick={() => void addLogin()}
              />
            </HStack>
            <SaveFeedback saveKey="trustedAuthors" />
          </VStack>
          <VStack gap={2}>
            <Selector
              label="Blocking findings"
              value={String(draft.blockingPriority ?? 1)}
              isDisabled={pending("blockingPriority")}
              onChange={(v) =>
                void updateSettings({ blockingPriority: Number(v) })
              }
              options={[0, 1, 2, 3].map((n) => ({
                value: String(n),
                label: n === 0 ? "P0 only" : `P0 to P${n}`,
              }))}
              description="A review with a finding at these priorities asks for changes; with none, it is clean. Applies to reviews that finish after you change it."
            />
            <SaveFeedback saveKey="blockingPriority" />
          </VStack>
          <VStack gap={2}>
            <Switch
              label="Wait for pushes to settle"
              labelPosition="start"
              labelSpacing="spread"
              width="100%"
              value={draft.settlePushes ?? true}
              isDisabled={pending("settlePushes")}
              onChange={(settlePushes) => void updateSettings({ settlePushes })}
              description="Review a new commit automatically only once it has stayed the latest for one poll interval, so a quick series of pushes gets one review."
            />
            <SaveFeedback saveKey="settlePushes" />
          </VStack>
          <VStack gap={2}>
            <Switch
              label="GitHub status check"
              labelPosition="start"
              labelSpacing="spread"
              width="100%"
              value={draft.commitStatus ?? true}
              isDisabled={pending("commitStatus")}
              onChange={(commitStatus) => void updateSettings({ commitStatus })}
              description="Show each automatic review’s result on its commit as the orbit/review check. Only while automatic publication is on."
            />
            <SaveFeedback saveKey="commitStatus" />
          </VStack>
          <VStack gap={2}>
            <Switch
              label="Notify me when a review needs attention"
              labelPosition="start"
              labelSpacing="spread"
              width="100%"
              value={draft.notify ?? true}
              isDisabled={pending("notify")}
              onChange={(notify) => void updateSettings({ notify })}
              description="Show a desktop notification once when something new needs you, such as an approval or a held report. The Needs you list shows every item either way."
            />
            <SaveFeedback saveKey="notify" />
          </VStack>
          <VStack gap={2}>
            <Switch
              label="Keep security scan working files"
              labelPosition="start"
              labelSpacing="spread"
              width="100%"
              value={draft.keepScanArtifacts ?? false}
              isDisabled={pending("keepScanArtifacts")}
              onChange={(keepScanArtifacts) =>
                void updateSettings({ keepScanArtifacts })
              }
              description="A scan’s working files can take gigabytes. When off, Orbit deletes them once the findings are imported and keeps only the findings and report."
            />
            <SaveFeedback saveKey="keepScanArtifacts" />
          </VStack>
          <FormLayout>
            <VStack gap={2}>
              <Selector
                label="Concurrent reviews"
                value={String(draft.concurrency)}
                isDisabled={pending("concurrency")}
                onChange={(v) =>
                  void updateSettings({ concurrency: Number(v) })
                }
                options={[1, 2, 3, 4].map((n) => ({
                  value: String(n),
                  label: `${n} ${n === 1 ? "review" : "reviews"} at a time`,
                }))}
              />
              <SaveFeedback saveKey="concurrency" />
            </VStack>
            <VStack gap={2}>
              <Selector
                label="Poll interval"
                value={String(draft.pollSeconds)}
                isDisabled={pending("pollSeconds")}
                onChange={(v) =>
                  void updateSettings({ pollSeconds: Number(v) })
                }
                options={[30, 60, 120, 300, 600, 1800, 3600].map((n) => ({
                  value: String(n),
                  label:
                    n < 60
                      ? `${n} seconds`
                      : `${n / 60} ${n === 60 ? "minute" : "minutes"}`,
                }))}
              />
              <SaveFeedback saveKey="pollSeconds" />
            </VStack>
          </FormLayout>
        </VStack>
      </Card>
      <ReplySkill />
      <BuddySettings />
      <RemoteAccess />
      <Text type="supporting" color="secondary">
        Changes save automatically on this device.
      </Text>
      <Storage />
    </VStack>
  );
}
