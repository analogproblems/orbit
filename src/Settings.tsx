import type { ReactNode } from "react";
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
  Tab,
  TabList,
  Text,
  TextInput,
  Token,
  VStack,
} from "@astryxdesign/core";
import { Code2, Container, GitBranch, RefreshCw } from "lucide-react";
import { useOrbit, live } from "./store";
import {
  connectionState,
  defaultEffort,
  defaultWriteUps,
  reviewerSelectionUnavailable,
  withPatch,
  writerLabels,
  type Settings,
  type SettingsPatch,
  type SettingsTab,
  type Snapshot,
  type Writer,
} from "./types";
import { SaveFeedback } from "./SaveFeedback";
import { Storage } from "./Storage";
import { RemoteAccess } from "./RemoteAccess";
import {
  CliConnection,
  CliReviewSettings,
  ReviewerProblem,
  useReconnect,
  usedBy,
} from "./CliReviewers";
import { BuddySettings } from "./Buddy";
import { ReplySkill } from "./ReplySkill";

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

/** Settings' pages, in order. */
const tabs: { value: SettingsTab; label: string }[] = [
  { value: "connections", label: "Connections" },
  { value: "reviewers", label: "Reviewers" },
  { value: "automation", label: "Automation" },
  { value: "decisions", label: "Decisions" },
  { value: "buddy", label: "Buddy" },
  { value: "devices", label: "Other devices" },
  { value: "storage", label: "Storage" },
];

/** Whether something Orbit always needs, GitHub or Codex, isn't connected. */
function connectionsNeedYou(s: Snapshot): boolean {
  return !s.codexReady || !s.githubUser || !!s.githubError;
}

/** Whether a reviewer a repository uses can't start a review. */
function reviewersNeedYou(s: Snapshot, settings: Settings): boolean {
  return (["codex", "claude", "antigravity"] as const).some(
    (p) =>
      (p === "codex" || usedBy(settings, p)) &&
      !!reviewerSelectionUnavailable(s, p),
  );
}

export function SettingsPage() {
  const { snapshot: s, settingsTab: tab, setSettingsTab } = useOrbit();
  const draft = useDraft();
  const needsYou: Partial<Record<SettingsTab, boolean>> = {
    connections: connectionsNeedYou(s),
    reviewers: reviewersNeedYou(s, draft),
  };
  const pages: Record<SettingsTab, ReactNode> = {
    connections: <ConnectionSettings />,
    reviewers: (
      <>
        <CodexReviewSettings />
        <CliReviewSettings provider="claude" />
        <CliReviewSettings provider="antigravity" />
        <ReplySkill />
      </>
    ),
    automation: <AutomationSettings />,
    decisions: <DecisionSettings />,
    buddy: <BuddySettings />,
    devices: <RemoteAccess />,
    storage: <Storage />,
  };
  return (
    <VStack gap={6} padding={6} maxWidth={760} className="content-page">
      <VStack gap={2}>
        <Heading level={2}>A review flow that fits.</Heading>
        <Text color="secondary">
          Set the pace, pick the model, and choose how hands-on you want to be.
          Changes save automatically on this device.
        </Text>
      </VStack>
      <TabList
        role="tablist"
        aria-label="Settings pages"
        value={tab}
        onChange={(value) => setSettingsTab(value as SettingsTab)}
        hasDivider
      >
        {tabs.map((t) => (
          <Tab
            key={t.value}
            id={`settings-tab-${t.value}`}
            value={t.value}
            label={t.label}
            panelId={`settings-${t.value}`}
            endContent={
              needsYou[t.value] ? (
                <StatusDot label="Needs you" variant="warning" />
              ) : undefined
            }
          />
        ))}
      </TabList>
      <VStack
        gap={6}
        role="tabpanel"
        id={`settings-${tab}`}
        aria-labelledby={`settings-tab-${tab}`}
      >
        {pages[tab]}
      </VStack>
    </VStack>
  );
}

/** What Orbit connects to, and checking them again. */
function ConnectionSettings() {
  const { snapshot: s, action } = useOrbit();
  const draft = useDraft();
  const [connecting, reconnect] = useReconnect();
  return (
    <Card>
      <VStack gap={4}>
        <HStack hAlign="between" vAlign="center">
          <Heading level={3}>Connections</Heading>
          <Button
            label={connecting ? "Checking…" : "Check again"}
            variant="ghost"
            icon={<RefreshCw />}
            isLoading={connecting}
            onClick={() => void reconnect()}
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
                  {connectionState(s.codexReady, s.codexError)}
                </Text>
              </HStack>
            }
          />
          <CliConnection provider="claude" />
          <CliConnection provider="antigravity" />
          <ListItem
            label="Docker and Dev Container CLI"
            startContent={<Container />}
            description={
              <Text color="secondary">
                {s.devContainers?.error ||
                  (s.devContainers?.ready
                    ? `Runs setup and checks in dev containers · ${s.devContainers.version}.`
                    : "Install and start Docker, then run npm install -g @devcontainers/cli, to run a repository's setup and checks in its dev container.")}
              </Text>
            }
            endContent={
              <HStack gap={2} vAlign="center">
                <StatusDot
                  label="Dev container connection"
                  variant={
                    s.devContainers?.ready
                      ? "success"
                      : draft.repositories.some((r) => r.devContainer?.enabled)
                        ? "warning"
                        : "neutral"
                  }
                />
                <Text color="secondary" type="supporting">
                  {connectionState(
                    !!s.devContainers?.ready,
                    s.devContainers?.error,
                  )}
                </Text>
              </HStack>
            }
          />
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
                  {connectionState(!!s.githubUser, s.githubError)}
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
                Run <Code>npm run desktop</Code> in this project. The native app
                connects to <Code>gh</Code> and <Code>codex</Code> installed on
                your computer.
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
  );
}

/** The model and effort of Codex reviews. */
function CodexReviewSettings() {
  const { snapshot: s, updateSettings, saves } = useOrbit();
  const draft = useDraft();
  const pending = (key: string) => saves[key]?.state === "saving";
  const model = s.models.find((m) => m.model === draft.model);
  return (
    <Card>
      <VStack gap={4}>
        <Heading level={3}>Codex reviews</Heading>
        <ReviewerProblem provider="codex" />
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
              isDisabled={
                pending("model") || !s.codexReady || !s.modelCatalogReady
              }
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
                    (!defaultEffort(m) ? " (no supported default effort)" : ""),
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
                !s.codexReady ||
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
  );
}

/** Automatic reviews, publication and what blocks a pull request. */
function AutomationSettings() {
  const {
    updateSettings,
    saves,
    trustedInput: newLogin,
    setTrustedInput: setNewLogin,
  } = useOrbit();
  const draft = useDraft();
  const pending = (key: string) => saves[key]?.state === "saving";
  const blockFindings = draft.blockFindings ?? true;
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
  return (
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
          <Switch
            label="Block on findings"
            labelPosition="start"
            labelSpacing="spread"
            width="100%"
            value={blockFindings}
            isDisabled={pending("blockFindings")}
            onChange={(blockFindings) => void updateSettings({ blockFindings })}
            description="When on, a review with a finding at the priorities below asks for changes. When off, findings never block: every review with findings is clean, and its findings are listed without a blocking label. Applies to reviews that finish after you change it."
          />
          <SaveFeedback saveKey="blockFindings" />
        </VStack>
        <VStack gap={2}>
          <Selector
            label="Blocking priorities"
            value={String(draft.blockingPriority ?? 1)}
            isDisabled={!blockFindings || pending("blockingPriority")}
            onChange={(v) =>
              void updateSettings({ blockingPriority: Number(v) })
            }
            options={[0, 1, 2, 3].map((n) => ({
              value: String(n),
              label: n === 0 ? "P0 only" : `P0 to P${n}`,
            }))}
            description="A review with a finding at these priorities asks for changes; with none, it is clean."
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
            label="Watch CI"
            labelPosition="start"
            labelSpacing="spread"
            width="100%"
            value={draft.watchCi ?? true}
            isDisabled={pending("watchCi")}
            onChange={(watchCi) => void updateSettings({ watchCi })}
            description="After each sync, read the checks on each repository’s default branch and on the pull requests Orbit follows, and list what fails on the Operations page. A failing default branch needs you; a failing pull request is its author’s to fix."
          />
          <SaveFeedback saveKey="watchCi" />
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
              onChange={(v) => void updateSettings({ concurrency: Number(v) })}
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
              onChange={(v) => void updateSettings({ pollSeconds: Number(v) })}
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
  );
}

/** Plain-language write-ups for the Decisions page. */
function DecisionSettings() {
  const { updateSettings, saves } = useOrbit();
  const draft = useDraft();
  const pending = (key: string) => saves[key]?.state === "saving";
  const writeUps = draft.writeUps ?? defaultWriteUps;
  return (
    <Card>
      <VStack gap={6}>
        <Heading level={3}>Decisions</Heading>
        <VStack gap={2}>
          <Switch
            label="Write up each pull request in plain words"
            labelPosition="start"
            labelSpacing="spread"
            width="100%"
            value={writeUps.enabled}
            isDisabled={pending("writeUps")}
            onChange={(enabled) =>
              void updateSettings({ writeUps: { ...writeUps, enabled } })
            }
            description="Once a pull request is merged or closed, the writer sums up for the Decisions page what it decided: the rules it set, trade-offs, what the reviewer challenged, what was pushed back on and what was checked, each with a quote and a link. One at a time in the background, with no tools, and again only when the pull request changes. Each one uses the writer, so it is off until you turn it on; Codex write-ups wait while its usage is over 90%."
          />
        </VStack>
        <VStack gap={2}>
          <Selector
            label="Writer"
            value={writeUps.writer}
            isDisabled={pending("writeUps")}
            onChange={(writer) =>
              void updateSettings({
                writeUps: { ...writeUps, writer: writer as Writer },
              })
            }
            options={(["codex", "claude"] as const).map((w) => ({
              value: w,
              label: writerLabels[w],
            }))}
            description="Uses the model and effort you chose for that reviewer above."
          />
          <SaveFeedback saveKey="writeUps" />
        </VStack>
      </VStack>
    </Card>
  );
}
