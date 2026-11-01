import { useRef, useState } from "react";
import {
  AlertDialog,
  Banner,
  Button,
  Card,
  ChatComposer,
  ChatComposerInput,
  ChatLayout,
  ChatMessage,
  ChatMessageBubble,
  ChatMessageList,
  ChatMessageMetadata,
  ChatSendButton,
  ClickableCard,
  Code,
  EmptyState,
  Grid,
  Heading,
  HStack,
  IconButton,
  Kbd,
  List,
  ListItem,
  SegmentedControl,
  SegmentedControlItem,
  SelectableCard,
  Selector,
  SideNavItem,
  Spinner,
  Switch,
  Text,
  TextArea,
  TextInput,
  Timestamp,
  TimeInput,
  Token,
  VStack,
} from "@astryxdesign/core";
import { createISOTimeString } from "@astryxdesign/core/utils";
import { ExternalLink, RefreshCw, Trash2 } from "lucide-react";
import { openLink, remote, useOrbit } from "./store";
import {
  actionLabels,
  apiProviders,
  briefDay,
  buddyMood,
  buddyOf,
  buddyAnswerers,
  buddyTones,
  fixDescriptions,
  fixLabels,
  fixRuleLabels,
  maxQuestion,
  memoryChangeText,
  memoryLimits,
  moodReason,
  needsSetup,
  offeredFix,
  postDescriptions,
  questionLabels,
  ruleLabel,
  withPatch,
  type ApiProvider,
  type Buddy,
  type BuddyHat,
  type BuddyLook,
  type BuddyNote,
  type BuddyAnswerer,
  type BuddyTone,
  type FixRule,
  type Mood,
  type Proposal,
  type Question,
  type RuledAction,
  type RuledFix,
  type RuledPost,
  type SettingsPatch,
  type Snapshot,
} from "./types";
import { Bit, bitColours, bitHats, bitLooks, moodLabels } from "./Bit";
import { SaveFeedback } from "./SaveFeedback";
import { useReconnect } from "./CliReviewers";
import { SafeMarkdown } from "./SafeMarkdown";
import { describePatch, describeRepositoryPatch } from "./describePatch";
import { recentTimestamp } from "./timestamps";
import { TimeZoneSelector } from "./TimeZone";

/** The buddy as it will be once saves in progress land. */
function useBuddy(): Buddy {
  const { snapshot: s, saves } = useOrbit();
  let settings = s.settings;
  for (const save of Object.values(saves)) {
    if (save.command === "update_settings" && save.state === "saving")
      settings = withPatch(settings, save.args.patch as SettingsPatch);
  }
  return buddyOf(settings);
}

/** Bit in the sidebar: its face and latest note, or just its face when the
 * sidebar is collapsed. Opens the feed. */
export function BuddyCard({
  collapsed,
  isSelected,
  onOpen,
}: {
  collapsed: boolean;
  isSelected: boolean;
  onOpen: () => void;
}) {
  const { snapshot: s } = useOrbit();
  const buddy = useBuddy();
  if (!buddy.enabled) return null;
  const mood = buddyMood(s);
  // What Bit last noticed; answers stay on its page.
  const latest = s.buddyFeed?.notes.filter((n) => n.kind !== "answer").at(-1);
  if (collapsed)
    return (
      <SideNavItem
        label={buddy.name}
        icon={
          <Bit
            mood={mood}
            colour={buddy.colour}
            hat={buddy.hat}
            look={buddy.look}
            size="xs"
          />
        }
        isSelected={isSelected}
        onClick={onOpen}
      />
    );
  return (
    <ClickableCard
      label={`Open ${buddy.name}'s notes`}
      onClick={onOpen}
      padding={2}
      variant={isSelected ? "muted" : "default"}
    >
      <HStack gap={2} vAlign="center">
        <Bit
          mood={mood}
          colour={buddy.colour}
          hat={buddy.hat}
          look={buddy.look}
          size="sm"
        />
        <VStack gap={0.5}>
          <Text weight="semibold">{buddy.name}</Text>
          <Text type="supporting" color="secondary" maxLines={2}>
            {latest?.text ?? moodReason(s, mood)}
          </Text>
        </VStack>
      </HStack>
    </ClickableCard>
  );
}

/** A button for an open item's fix, unless you asked Bit to leave it to
 * you, with the error if it fails. */
function FixButton({ itemKey }: { itemKey: string | null | undefined }) {
  const { snapshot: s, action } = useOrbit();
  const [error, setError] = useState<string | null>(null);
  const offered = offeredFix(s, itemKey);
  if (!offered) return null;
  return (
    <VStack gap={2}>
      <HStack>
        <Button
          label={fixLabels[offered.fix]}
          size="sm"
          clickAction={async () => {
            setError(null);
            const result = await action(
              "use_fix",
              { key: offered.item.key },
              { quiet: true },
            );
            if (!result.ok) setError(result.error);
          }}
        />
      </HStack>
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}

/** Got it, on the morning brief while it is in Needs you, to take it out. */
function ReadButton({ itemKey }: { itemKey: string | null | undefined }) {
  const { snapshot: s, action } = useOrbit();
  const item = s.attention?.find((a) => a.key === itemKey);
  if (!item) return null;
  return (
    <HStack>
      <Button
        label="Got it"
        size="sm"
        variant="secondary"
        clickAction={async () => {
          await action("close_brief", { day: briefDay(item) });
        }}
      />
    </HStack>
  );
}

/** What a button from one of Bit's answers says it does. */
function proposalLabel(p: Proposal) {
  if (p.label) return p.label;
  const pr = p.prId?.split("/").pop();
  const only = (patch: object | null | undefined, key: string) =>
    !!patch && Object.keys(patch).length === 1 && key in patch;
  if (p.action === "set_up_repository") {
    if (p.connect) return `Connect ${p.repo}`;
    if (only(p.repository, "autopilot") && p.repository?.autopilot)
      return `Review ${p.repo} automatically`;
    return `Set up ${p.repo}`;
  }
  if (only(p.settings, "automatic") && p.settings?.automatic)
    return "Turn on autopilot";
  return pr ? `${actionLabels[p.action]}: ${pr}` : actionLabels[p.action];
}

/** A line of Bit's, with any `command` in it shown as code. */
function LineText({ text }: { text: string }) {
  const parts = text.split("`");
  if (parts.length % 2 === 0) return <Text>{text}</Text>;
  return (
    <Text>
      {parts.map((part, index) =>
        index % 2 ? <Code key={index}>{part}</Code> : part,
      )}
    </Text>
  );
}

/** A button from one of Bit's answers. It runs only when you tap it, and
 * Orbit checks it against what's true then. */
function ProposalButton({
  noteId,
  proposal,
}: {
  noteId: number;
  proposal: Proposal;
}) {
  const { snapshot: s, action } = useOrbit();
  const [error, setError] = useState<string | null>(null);
  const label = proposalLabel(proposal);
  const repoChange =
    proposal.repository && describeRepositoryPatch(proposal.repository);
  return (
    <VStack gap={2}>
      <HStack gap={2} vAlign="center" wrap="wrap">
        <Button
          label={label}
          size="sm"
          isDisabled={!!proposal.done}
          clickAction={async () => {
            setError(null);
            // A page opens where you are, on this computer or your phone.
            if (proposal.action === "open_page") {
              if (proposal.link) openLink(proposal.link);
              return;
            }
            const result = await action(
              "use_proposal",
              { noteId, proposalId: proposal.id },
              { quiet: true },
            );
            if (!result.ok) setError(result.error);
          }}
        />
        {proposal.done && <Token label="Done" color="green" size="sm" />}
      </HStack>
      {proposal.settings && (
        <Text type="supporting" color="secondary">
          {describePatch(proposal.settings, s.models)}
        </Text>
      )}
      {proposal.connect && (
        <Text type="supporting" color="secondary">
          Orbit downloads its own copy of {proposal.repo} and connects it with
          automatic reviews off, no setup command and no network, unless changed
          here.
        </Text>
      )}
      {repoChange && (
        <Text type="supporting" color="secondary">
          {repoChange}
        </Text>
      )}
      {error && <Banner status="error" title={error} />}
    </VStack>
  );
}

/** Where a note leads: its pull request's decisions, its loop, the
 * repositories, or settings. Answers lead nowhere; their lines carry their
 * own buttons. */
function noteLink(note: BuddyNote) {
  if (
    note.kind === "answer" ||
    note.kind === "sign_in" ||
    note.kind === "brief"
  )
    return undefined;
  if (note.kind === "decide") return "Open decisions";
  if (note.prId) return "Open loop";
  if (note.key?.startsWith("problem:")) return "Open operations";
  if (note.repo) return "Open repositories";
  if (note.key || note.kind === "fixing" || note.kind === "fixed")
    return "Open settings";
  return undefined;
}

/** One note from Bit, with what it lists, its button and where it leads. */
function NoteMessage({
  note,
  age,
  onNote,
}: {
  note: BuddyNote;
  age: number;
  onNote: (note: BuddyNote) => void;
}) {
  const { snapshot: s } = useOrbit();
  const buddy = useBuddy();
  const link = noteLink(note);
  return (
    <ChatMessage
      sender="assistant"
      avatar={
        <Bit
          mood={buddyMood(s)}
          colour={buddy.colour}
          hat={buddy.hat}
          look={buddy.look}
          size="xs"
          still
        />
      }
    >
      <ChatMessageBubble
        variant="ghost"
        width="100%"
        name={
          <Text type="supporting" weight="semibold" color="secondary">
            {buddy.name}
          </Text>
        }
        metadata={
          <ChatMessageMetadata
            timestamp={<Timestamp value={note.at} {...recentTimestamp(age)} />}
            footer={
              link && (
                <Button
                  label={link}
                  variant="ghost"
                  size="sm"
                  onClick={() => onNote(note)}
                />
              )
            }
          />
        }
      >
        <VStack gap={2}>
          {note.pending ? (
            <HStack gap={2} vAlign="center">
              <Spinner size="sm" aria-label="Thinking…" />
              <Text color="secondary">Thinking…</Text>
            </HStack>
          ) : note.kind === "answer" ? (
            <SafeMarkdown>{note.text}</SafeMarkdown>
          ) : (
            <Text>{note.text}</Text>
          )}
          {!!note.lines?.length && (
            <List hasDividers aria-label={note.text}>
              {note.lines.map((line, index) => (
                <ListItem
                  key={index}
                  label={<LineText text={line.text} />}
                  description={<FixButton itemKey={line.key} />}
                  endContent={
                    line.new && <Token label="New" color="blue" size="sm" />
                  }
                />
              ))}
            </List>
          )}
          {note.proposals?.map((proposal) => (
            <ProposalButton
              key={proposal.id}
              noteId={note.id}
              proposal={proposal}
            />
          ))}
          {note.memory?.map((change, index) => (
            <Text key={index} type="supporting" color="secondary">
              {memoryChangeText(change)}
            </Text>
          ))}
          <FixButton itemKey={note.key} />
          {note.kind === "brief" && <ReadButton itemKey={note.key} />}
        </VStack>
      </ChatMessageBubble>
    </ChatMessage>
  );
}

const questions = Object.keys(questionLabels) as Question[];

/** Bit's page: its face and mood, then one conversation that every window
 * shares, with Bit's notes, your questions and Bit's answers. */
export function BuddyPage({
  onNote,
  onSettings,
}: {
  onNote: (note: BuddyNote) => void;
  onSettings: () => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const buddy = useBuddy();
  const mood = buddyMood(s);
  const notes = s.buddyFeed?.notes ?? [];
  const [asking, setAsking] = useState<Question | null>(null);
  const setupNeeded = needsSetup(s);
  const ask = async (question: Question) => {
    setAsking(question);
    await action("ask_buddy", { question });
    setAsking(null);
  };
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [askError, setAskError] = useState<string | null>(null);
  const inFlight = useRef(false);
  const thinking = notes.some((n) => n.pending);
  const tooLong = [...draft.trim()].length > maxQuestion;
  const askAnything = async () => {
    if (!draft.trim() || tooLong || thinking || inFlight.current) return;
    inFlight.current = true;
    setSending(true);
    setAskError(null);
    const typed = draft;
    const result = await action(
      "ask_buddy_anything",
      { question: typed },
      { quiet: true },
    );
    // Keep the draft when the question wasn't taken, or was edited since.
    if (result.ok) setDraft((now) => (now === typed ? "" : now));
    else setAskError(result.error);
    inFlight.current = false;
    setSending(false);
  };
  return (
    <VStack height="100%" className="buddy-page">
      <VStack
        gap={4}
        padding={6}
        paddingBlockEnd={2}
        maxWidth={760}
        className="content-page"
      >
        <HStack gap={4} vAlign="center">
          <Bit
            mood={mood}
            colour={buddy.colour}
            hat={buddy.hat}
            look={buddy.look}
            size="lg"
            label={`${buddy.name}, ${moodLabels[mood].toLowerCase()}`}
          />
          <VStack gap={2}>
            <Heading level={2}>
              {buddy.enabled
                ? `${buddy.name} is ${moodLabels[mood].toLowerCase()}.`
                : `${buddy.name} is turned off.`}
            </Heading>
            <Text color="secondary">
              {buddy.enabled
                ? moodReason(s, mood)
                : "Turn your buddy on in Settings to get notes about your review loops."}
            </Text>
            <HStack>
              <Button
                label={
                  buddy.enabled ? `Customize ${buddy.name}` : "Open Settings"
                }
                variant="secondary"
                onClick={onSettings}
              />
            </HStack>
          </VStack>
        </HStack>
      </VStack>
      {buddy.enabled && (
        <VStack maxWidth={760} className="content-page buddy-chat">
          <ChatLayout
            composer={
              <VStack gap={2} padding={3} className="buddy-chat-composer">
                <Text type="supporting" color="secondary">
                  Ask {buddy.name}
                </Text>
                <HStack gap={2} wrap="wrap">
                  {questions.map((question) => (
                    <Button
                      key={question}
                      label={questionLabels[question]}
                      size="sm"
                      variant={
                        question === "setup" && setupNeeded
                          ? "primary"
                          : "secondary"
                      }
                      isLoading={asking === question}
                      isDisabled={!!asking && asking !== question}
                      onClick={() => void ask(question)}
                    />
                  ))}
                </HStack>
                {askError && <Banner status="error" title={askError} />}
                <ChatComposer
                  value={draft}
                  onChange={setDraft}
                  onSubmit={() => void askAnything()}
                  elevation="none"
                  density="compact"
                  status={
                    tooLong
                      ? {
                          type: "warning",
                          message: `Keep your question under ${maxQuestion} characters.`,
                        }
                      : undefined
                  }
                  input={
                    <ChatComposerInput
                      label={`Ask ${buddy.name} anything`}
                      placeholder={
                        thinking
                          ? `${buddy.name} is thinking…`
                          : `Ask ${buddy.name} anything about your reviews…`
                      }
                      pasteAsToken={false}
                      hasHistory={false}
                      maxRows={4}
                      // Orbit clears the draft only once the question is taken.
                      onKeyDown={(e) => {
                        if (
                          e.key === "Enter" &&
                          !e.shiftKey &&
                          !e.nativeEvent.isComposing &&
                          e.keyCode !== 229
                        ) {
                          e.preventDefault();
                          void askAnything();
                        }
                      }}
                    />
                  }
                  sendButton={
                    <ChatSendButton
                      isDisabled={
                        !draft.trim() || tooLong || thinking || sending
                      }
                      onSend={() => void askAnything()}
                    />
                  }
                />
                <Text type="supporting" color="secondary">
                  {buddy.name} answers with {buddyAnswerers[buddy.answerer]} and
                  can’t change anything until you tap a button.{" "}
                  <Kbd keys="enter" /> to send · <Kbd keys="shift+enter" /> for
                  a new line
                </Text>
              </VStack>
            }
          >
            <ChatMessageList
              density="compact"
              aria-label={`${buddy.name}'s notes`}
              emptyState={
                setupNeeded ? (
                  <EmptyState
                    title="Let's get you set up"
                    description={`${buddy.name} can check your connections and walk you through connecting a repository, one tap at a time.`}
                    isCompact
                    actions={
                      <Button
                        label={questionLabels.setup}
                        variant="primary"
                        isLoading={asking === "setup"}
                        onClick={() => void ask("setup")}
                      />
                    }
                  />
                ) : (
                  <EmptyState
                    title="Nothing to report yet"
                    description={`${buddy.name} will note each round, each clean loop, and anything that needs you. Ask a question below any time.`}
                    isCompact
                  />
                )
              }
            >
              {notes.flatMap((note, index) => {
                const message = (
                  <NoteMessage
                    key={note.id}
                    note={note}
                    age={notes.length - 1 - index}
                    onNote={onNote}
                  />
                );
                return note.question
                  ? [
                      <ChatMessage key={`${note.id}-asked`} sender="user">
                        <ChatMessageBubble>{note.question}</ChatMessageBubble>
                      </ChatMessage>,
                      message,
                    ]
                  : [message];
              })}
            </ChatMessageList>
          </ChatLayout>
        </VStack>
      )}
    </VStack>
  );
}

const previewMoods: Mood[] = ["happy", "busy", "worried", "sleepy"];

/** Where Ollama is and which of its models answers Bit. */
function OllamaSettings() {
  const { snapshot: s, updateSettings, saves } = useOrbit();
  const buddy = useBuddy();
  const [connecting, reconnect] = useReconnect();
  const [url, setUrl] = useState<string | null>(null);
  const typedUrl = url ?? buddy.ollamaUrl;
  const saving = (key: string) => saves[`buddy.${key}`]?.state === "saving";
  const saveUrl = async () => {
    if (!typedUrl.trim() || typedUrl.trim() === buddy.ollamaUrl) return;
    const result = await updateSettings(
      { buddy: { ollamaUrl: typedUrl } },
      "buddy.ollamaUrl",
    );
    if (result.ok) setUrl(null);
  };
  const models = s.ollamaModels ?? [];
  return (
    <VStack gap={2}>
      <HStack gap={2} vAlign="end">
        <TextInput
          label="Ollama address"
          description={`${buddy.name} sends each question, with what Orbit knows, to this address.`}
          width="100%"
          value={typedUrl}
          onChange={setUrl}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void saveUrl();
            }
          }}
        />
        <Button
          label="Save address"
          variant="secondary"
          isDisabled={
            !typedUrl.trim() ||
            typedUrl.trim() === buddy.ollamaUrl ||
            saving("ollamaUrl")
          }
          onClick={() => void saveUrl()}
        />
      </HStack>
      <SaveFeedback saveKey="buddy.ollamaUrl" />
      {s.ollama?.error && (
        <Banner
          status="warning"
          title={s.ollama.error}
          endContent={
            <Button
              label={connecting ? "Checking…" : "Check again"}
              size="sm"
              icon={<RefreshCw />}
              isLoading={connecting}
              onClick={() => void reconnect()}
            />
          }
        />
      )}
      <Selector
        label="Ollama model"
        description={
          models.length
            ? "The models Ollama has at that address."
            : "Ollama's models show here once Orbit reaches it."
        }
        value={buddy.ollamaModel}
        isDisabled={!models.length || saving("ollamaModel")}
        options={models.map((m) => ({ value: m, label: m }))}
        onChange={(ollamaModel) =>
          void updateSettings({ buddy: { ollamaModel } }, "buddy.ollamaModel")
        }
      />
      <SaveFeedback saveKey="buddy.ollamaModel" />
    </VStack>
  );
}

/** Your key for a provider's API, kept in your system's password store,
 * and which of the provider's models answers. */
function ApiKeySettings({ provider }: { provider: ApiProvider }) {
  const { snapshot: s, updateSettings, saves, action } = useOrbit();
  const buddy = useBuddy();
  const [connecting, reconnect] = useReconnect();
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { company, page, pageName } = apiProviders[provider];
  const saved = s.apiKeys?.[provider];
  const models = saved?.models ?? [];
  const field = `${provider}Model` as const;
  const saving = saves[`buddy.${field}`]?.state === "saving";
  if (s.passwordStore) return <Banner status="warning" title={noStore} />;
  const save = async (next: string | null) => {
    if (next !== null && !next.trim()) return;
    setError(null);
    setBusy(true);
    const result = await action(
      "set_api_key",
      { provider, key: next },
      { quiet: true },
    );
    setBusy(false);
    if (result.ok) setKey("");
    else setError(result.error);
  };
  return (
    <VStack gap={2}>
      <Text color="secondary">
        Orbit keeps your key in this computer&rsquo;s password store, never in
        its settings, and only sends it to {company}. Each question you type is
        billed to your {company} account.
      </Text>
      <HStack gap={2} vAlign="end">
        <TextInput
          type="password"
          autoComplete="off"
          label={`${company} API key`}
          description={
            saved?.saved
              ? "A key is saved. Paste a new one here to replace it."
              : "Paste your key here."
          }
          width="100%"
          value={key}
          onChange={setKey}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void save(key);
            }
          }}
        />
        <Button
          label="Save key"
          variant="secondary"
          isDisabled={!key.trim() || busy}
          onClick={() => void save(key)}
        />
      </HStack>
      <HStack gap={2}>
        <Button
          label={`Get a key from ${pageName}`}
          variant="ghost"
          size="sm"
          icon={<ExternalLink />}
          onClick={() => openLink(page)}
        />
        {saved?.saved && (
          <Button
            label="Remove key"
            variant="ghost"
            size="sm"
            icon={<Trash2 />}
            isDisabled={busy}
            onClick={() => void save(null)}
          />
        )}
      </HStack>
      {error && <Banner status="error" title={error} />}
      {saved?.status.error && (
        <Banner
          status="warning"
          title={saved.status.error}
          endContent={
            <Button
              label={connecting ? "Checking…" : "Check again"}
              size="sm"
              icon={<RefreshCw />}
              isLoading={connecting}
              onClick={() => void reconnect()}
            />
          }
        />
      )}
      <Selector
        label={`${company} model`}
        description={
          models.length
            ? `The models ${company} offers your key.`
            : `${company}'s models show here once your key is saved.`
        }
        value={buddy[field]}
        isDisabled={!models.length || saving}
        options={models.map((m) => ({ value: m, label: m }))}
        onChange={(model) =>
          void updateSettings({ buddy: { [field]: model } }, `buddy.${field}`)
        }
      />
      <SaveFeedback saveKey={`buddy.${field}`} />
    </VStack>
  );
}

/** Said when the computer running Orbit has nowhere to keep API keys. */
const noStore =
  "The computer running Orbit has no password store to keep API keys in, so they can’t be used there. Choose Codex, Claude, Antigravity or Ollama to answer instead.";

/** What answering with the chosen answerer means, and whether it can yet. */
function answererDescription(s: Snapshot, buddy: Buddy): string {
  const ready = {
    codex: s.codexReady,
    claude: !!s.claude?.ready,
    antigravity: !!s.antigravity?.ready,
    ollama: !!s.ollama?.ready,
    openai: !!s.apiKeys?.openai.status.ready,
    anthropic: !!s.apiKeys?.anthropic.status.ready,
    google: !!s.apiKeys?.google.status.ready,
  }[buddy.answerer];
  if (s.passwordStore && buddy.answerer in apiProviders) return noStore;
  const only = `${buddy.name} only answers and offers buttons; nothing changes until you tap one.`;
  const uses =
    buddy.answerer === "ollama"
      ? `Ollama runs on this computer or another one on your network, with the model you choose below. ${only}`
      : buddy.answerer in apiProviders
        ? `${apiProviders[buddy.answerer as ApiProvider].company} answers with your API key and the model you choose below. ${only}`
        : `Each uses its own sign-in, with the model and effort you chose for it as a reviewer. ${only}`;
  return ready
    ? uses
    : `${buddyAnswerers[buddy.answerer]} isn't connected yet, so ${buddy.name} can't answer until it is. ${uses}`;
}

/** The buddy's part of Settings: on or off, name, colour, hat, tone, who
 * answers and personality, with a live preview. */
export function BuddySettings() {
  const { snapshot: s, updateSettings, saves, alwaysOn } = useOrbit();
  const buddy = useBuddy();
  const [preview, setPreview] = useState<Mood>("happy");
  const [name, setName] = useState<string | null>(null);
  const [personality, setPersonality] = useState<string | null>(null);
  const save = (patch: Partial<Buddy>, key: keyof Buddy) =>
    updateSettings({ buddy: patch }, `buddy.${key}`);
  const pending = (key: keyof Buddy) =>
    saves[`buddy.${key}`]?.state === "saving";
  const typedName = name ?? buddy.name;
  const typedPersonality = personality ?? buddy.personality;
  const saveName = async () => {
    if (!typedName.trim() || typedName.trim() === buddy.name) return;
    const result = await save({ name: typedName }, "name");
    if (result.ok) setName(null);
  };
  return (
    <Card>
      <VStack gap={4}>
        <HStack gap={4} vAlign="center">
          <Bit
            mood={preview}
            colour={buddy.colour}
            hat={buddy.hat}
            look={buddy.look}
            size="md"
            label={`${buddy.name}, ${moodLabels[preview].toLowerCase()}`}
          />
          <VStack gap={2}>
            <Heading level={3}>Buddy</Heading>
            <Text color="secondary">
              {buddy.name} keeps an eye on your review loops and tells you what
              happened, in its own voice. Its face shows how things are going.
            </Text>
            <SegmentedControl
              label="Preview a mood"
              size="sm"
              value={preview}
              onChange={(value) => setPreview(value as Mood)}
            >
              {previewMoods.map((mood) => (
                <SegmentedControlItem
                  key={mood}
                  value={mood}
                  label={moodLabels[mood]}
                />
              ))}
            </SegmentedControl>
          </VStack>
        </HStack>
        <VStack gap={2}>
          <Switch
            label={`Show ${buddy.name}`}
            description="Off hides the buddy and stops new notes."
            value={buddy.enabled}
            isDisabled={pending("enabled")}
            onChange={(enabled) => void save({ enabled }, "enabled")}
          />
          <SaveFeedback saveKey="buddy.enabled" />
        </VStack>
        <VStack gap={2}>
          <HStack gap={2} vAlign="end">
            <TextInput
              label="Name"
              width="100%"
              value={typedName}
              onChange={setName}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void saveName();
                }
              }}
            />
            <Button
              label="Rename"
              variant="secondary"
              isDisabled={
                !typedName.trim() ||
                typedName.trim() === buddy.name ||
                pending("name")
              }
              onClick={() => void saveName()}
            />
          </HStack>
          <SaveFeedback saveKey="buddy.name" />
        </VStack>
        <VStack gap={2}>
          <Text weight="semibold">Look</Text>
          <Grid columns={{ minWidth: 88 }} gap={2}>
            {(
              Object.entries(bitLooks) as [
                BuddyLook,
                (typeof bitLooks)[BuddyLook],
              ][]
            ).map(([id, look]) => (
              <SelectableCard
                key={id}
                label={`${look.label}, ${look.kind}`}
                padding={2}
                isSelected={buddy.look === id}
                isDisabled={pending("look")}
                onChange={() => void save({ look: id }, "look")}
              >
                <VStack gap={1} hAlign="center">
                  <Bit
                    mood={preview}
                    colour={buddy.colour}
                    hat={buddy.hat}
                    look={id}
                    size="sm"
                    still
                  />
                  <Text type="supporting">{look.label}</Text>
                </VStack>
              </SelectableCard>
            ))}
          </Grid>
          <SaveFeedback saveKey="buddy.look" />
        </VStack>
        <VStack gap={2}>
          <Text weight="semibold">Colour</Text>
          <Grid columns={{ minWidth: 88 }} gap={2}>
            {Object.entries(bitColours).map(([id, colour]) => (
              <SelectableCard
                key={id}
                label={colour.label}
                padding={2}
                isSelected={buddy.colour === id}
                isDisabled={pending("colour")}
                onChange={() => void save({ colour: id }, "colour")}
              >
                <VStack gap={1} hAlign="center">
                  <Bit
                    mood={preview}
                    colour={id}
                    hat={buddy.hat}
                    look={buddy.look}
                    size="sm"
                    still
                  />
                  <Text type="supporting">{colour.label}</Text>
                </VStack>
              </SelectableCard>
            ))}
          </Grid>
          <SaveFeedback saveKey="buddy.colour" />
        </VStack>
        <VStack gap={2}>
          <Text weight="semibold">Hat</Text>
          <Grid columns={{ minWidth: 88 }} gap={2}>
            {(Object.entries(bitHats) as [BuddyHat, string][]).map(
              ([id, label]) => (
                <SelectableCard
                  key={id}
                  label={label}
                  padding={2}
                  isSelected={buddy.hat === id}
                  isDisabled={pending("hat")}
                  onChange={() => void save({ hat: id }, "hat")}
                >
                  <VStack gap={1} hAlign="center">
                    <Bit
                      mood={preview}
                      colour={buddy.colour}
                      hat={id}
                      look={buddy.look}
                      size="sm"
                      still
                    />
                    <Text type="supporting">{label}</Text>
                  </VStack>
                </SelectableCard>
              ),
            )}
          </Grid>
          <SaveFeedback saveKey="buddy.hat" />
        </VStack>
        <VStack gap={2}>
          <Selector
            label="Tone"
            description="How the notes sound."
            value={buddy.tone}
            isDisabled={pending("tone")}
            options={(Object.entries(buddyTones) as [BuddyTone, string][]).map(
              ([value, label]) => ({ value, label }),
            )}
            onChange={(tone) => void save({ tone: tone as BuddyTone }, "tone")}
          />
          <SaveFeedback saveKey="buddy.tone" />
        </VStack>
        <VStack gap={2}>
          <Switch
            label="Morning brief"
            description={`Once a day, ${buddy.name} sums up what's clear to merge, what's stuck and on whom, open CI problems, and notes on runners and storage. Nothing comes on a day when nothing changed.`}
            value={buddy.brief}
            isDisabled={pending("brief")}
            onChange={(brief) => void save({ brief }, "brief")}
          />
          <SaveFeedback saveKey="buddy.brief" />
          {buddy.brief && (
            <>
              <TimeInput
                label="Brief time"
                description="If Orbit isn't running then, the brief comes when it next is."
                hourFormat="24h"
                value={createISOTimeString(buddy.briefAt) ?? undefined}
                isDisabled={pending("briefAt")}
                onChange={(briefAt) => {
                  if (briefAt)
                    void save({ briefAt: briefAt.slice(0, 5) }, "briefAt");
                }}
              />
              <SaveFeedback saveKey="buddy.briefAt" />
              <TimeZoneSelector
                value={buddy.timeZone ?? ""}
                clockZone={s.clockZone ?? ""}
                elsewhere={remote || Boolean(alwaysOn)}
                isDisabled={pending("timeZone")}
                onChange={(timeZone) => void save({ timeZone }, "timeZone")}
              />
              <SaveFeedback saveKey="buddy.timeZone" />
            </>
          )}
        </VStack>
        <VStack gap={2}>
          <Selector
            label="Who answers your questions"
            description={answererDescription(s, buddy)}
            value={buddy.answerer}
            isDisabled={pending("answerer")}
            options={(
              Object.entries(buddyAnswerers) as [BuddyAnswerer, string][]
            ).map(([value, label]) =>
              // No key can be kept on a computer without a password store.
              s.passwordStore && value in apiProviders
                ? {
                    value,
                    label,
                    description: "Needs a password store, which isn’t there",
                    disabled: true,
                  }
                : { value, label },
            )}
            onChange={(answerer) =>
              void save({ answerer: answerer as BuddyAnswerer }, "answerer")
            }
          />
          <SaveFeedback saveKey="buddy.answerer" />
        </VStack>
        {buddy.answerer === "ollama" && <OllamaSettings />}
        {buddy.answerer in apiProviders && (
          <ApiKeySettings
            key={buddy.answerer}
            provider={buddy.answerer as ApiProvider}
          />
        )}
        <VStack gap={2}>
          <TextArea
            label="Personality"
            isOptional
            description={`Describe ${buddy.name} in your own words. It shapes how ${buddy.name} answers the questions you type.`}
            rows={3}
            maxLength={4000}
            value={typedPersonality}
            onChange={setPersonality}
          />
          <HStack>
            <Button
              label="Save personality"
              variant="secondary"
              isDisabled={
                typedPersonality === buddy.personality || pending("personality")
              }
              onClick={async () => {
                const result = await save(
                  { personality: typedPersonality },
                  "personality",
                );
                if (result.ok) setPersonality(null);
              }}
            />
          </HStack>
          <SaveFeedback saveKey="buddy.personality" />
        </VStack>
        <BuddyRules />
        <BuddyMemory />
      </VStack>
    </Card>
  );
}

/** What Bit remembers between questions, with a button to forget each line
 * and one to forget everything. */
function BuddyMemory() {
  const { snapshot: s, action } = useOrbit();
  const buddy = useBuddy();
  const [error, setError] = useState<string | null>(null);
  const [clearing, setClearing] = useState(false);
  const [busy, setBusy] = useState(false);
  const memory = s.buddyFeed?.memory;
  const places: [string, string | null, string[], number][] = [
    ["General", null, memory?.general ?? [], memoryLimits.general],
    ...Object.entries(memory?.repositories ?? {}).map(
      ([repo, lines]): [string, string | null, string[], number] => [
        repo,
        repo,
        lines,
        memoryLimits.perRepository,
      ],
    ),
  ];
  const kept = places.filter(([, , lines]) => lines.length);
  const run = async (command: string, args: Record<string, unknown>) => {
    setError(null);
    setBusy(true);
    const result = await action(command, args, { quiet: true });
    setBusy(false);
    if (!result.ok) setError(result.error);
    return result.ok;
  };
  return (
    <VStack gap={4}>
      <VStack gap={2}>
        <Heading level={4}>What {buddy.name} remembers</Heading>
        <Text color="secondary">
          {buddy.name} keeps short notes between the questions you type: up to{" "}
          {memoryLimits.general} lines in general and{" "}
          {memoryLimits.perRepository} about each repository. It adds and
          forgets lines itself as it answers, and each answer shows what
          changed. Forget anything you’d rather it didn’t keep.
        </Text>
      </VStack>
      {kept.length ? (
        kept.map(([title, repo, lines, limit]) => (
          <VStack key={title} gap={2}>
            <HStack gap={2} vAlign="center">
              <Text weight="semibold">{title}</Text>
              <Token label={`${lines.length} of ${limit}`} size="sm" />
            </HStack>
            <List hasDividers aria-label={`${buddy.name}'s memory: ${title}`}>
              {lines.map((line) => (
                <ListItem
                  key={line}
                  label={<Text>{line}</Text>}
                  endContent={
                    <IconButton
                      label={`Forget “${line}”`}
                      tooltip="Forget"
                      size="sm"
                      variant="ghost"
                      icon={<Trash2 />}
                      isDisabled={busy}
                      onClick={() => void run("forget_memory", { repo, line })}
                    />
                  }
                />
              ))}
            </List>
          </VStack>
        ))
      ) : (
        <Text color="secondary">Nothing yet.</Text>
      )}
      {error && <Banner status="error" title={error} />}
      {!!kept.length && (
        <HStack>
          <Button
            label="Forget everything"
            variant="secondary"
            isDisabled={busy}
            onClick={() => setClearing(true)}
          />
        </HStack>
      )}
      <AlertDialog
        isOpen={clearing}
        onOpenChange={setClearing}
        title="Forget everything"
        description={`${buddy.name} forgets every line it remembers, in general and about each repository.`}
        actionLabel="Forget everything"
        isActionLoading={busy}
        onAction={async () => {
          if (busy) return;
          await run("clear_memory", {});
          setClearing(false);
        }}
      />
    </VStack>
  );
}

const ruledFixes = Object.keys(fixDescriptions) as RuledFix[];
const ruledPosts = Object.keys(postDescriptions) as RuledPost[];

/** A rule's selector, saved on its own. */
function RuleSelector({
  rule,
  description,
}: {
  rule: RuledAction;
  description: string;
}) {
  const { updateSettings, saves } = useOrbit();
  const buddy = useBuddy();
  const key = `buddy.rules.${rule}`;
  return (
    <VStack gap={2}>
      <Selector
        label={ruleLabel(rule)}
        description={description}
        value={buddy.rules[rule]}
        isDisabled={saves[key]?.state === "saving"}
        options={(Object.entries(fixRuleLabels) as [FixRule, string][]).map(
          ([value, label]) => ({ value, label }),
        )}
        onChange={(value) =>
          void updateSettings(
            { buddy: { rules: { [rule]: value as FixRule } } },
            key,
          )
        }
      />
      <SaveFeedback saveKey={key} />
    </VStack>
  );
}

/** What the buddy does about each fix it can do on its own, and what it
 * does on GitHub about failing CI. */
function BuddyRules() {
  const buddy = useBuddy();
  return (
    <VStack gap={4}>
      <VStack gap={2}>
        <Heading level={4}>What {buddy.name} can do</Heading>
        <Text color="secondary">
          When something needs you, {buddy.name} can use Orbit’s own controls.
          With <Text weight="semibold">Do it</Text>, {buddy.name} does it and
          tells you. With <Text weight="semibold">Ask first</Text>, a button
          waits for your tap. With <Text weight="semibold">Leave it to me</Text>
          , there’s no button.
        </Text>
        <Text color="secondary">
          {buddy.name} never answers a reviewer’s request to run a command or
          change files, and never touches the code under review. Turning
          autopilot back on, changing settings, and connecting or setting up a
          repository from an answer always wait for your tap.
        </Text>
      </VStack>
      {ruledFixes.map((fix) => (
        <RuleSelector key={fix} rule={fix} description={fixDescriptions[fix]} />
      ))}
      <VStack gap={2}>
        <Heading level={4}>Failing CI</Heading>
        <Text color="secondary">
          While Watch CI is on, {buddy.name} tells each project’s agent on
          GitHub when its checks fail, so it can fix them, and reruns a job that
          failed by bad luck, such as a lost runner or a network error. With{" "}
          <Text weight="semibold">Ask first</Text>, Operations shows a button
          for each one. Once something is posted, {buddy.name} keeps it up to
          date whatever the rule, so it never says something that stopped being
          true.
        </Text>
      </VStack>
      {ruledPosts.map((post) => (
        <RuleSelector
          key={post}
          rule={post}
          description={postDescriptions[post]}
        />
      ))}
    </VStack>
  );
}
