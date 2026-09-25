import { useRef, useState } from "react";
import {
  Banner,
  Button,
  ChatComposer,
  ChatComposerInput,
  ChatLayout,
  ChatMessage,
  ChatMessageBubble,
  ChatMessageList,
  ChatSendButton,
  ChatSystemMessage,
  ChatToolCalls,
  CodeBlock,
  EmptyState,
  HStack,
  Kbd,
  Selector,
  Spinner,
  Text,
  Timestamp,
  Token,
  VStack,
} from "@astryxdesign/core";
import { Play, Sparkles } from "lucide-react";
import { useOrbit } from "./store";
import {
  activeStatuses,
  canPublish,
  canStartReview,
  chatModelUnavailable,
  chatSelection,
  displayStatusLabels,
  followUpUnavailable,
  publishTarget,
  reportSelectionLabel,
  reviewDisplayStatus,
  statusLabels,
  type PullRequest,
  type Review,
} from "./types";
import { LegacyChatModel } from "./LegacyChatModel";
import { ReviewActions } from "./ReviewActions";
import { SafeMarkdown } from "./SafeMarkdown";
import { recentTimestamp } from "./timestamps";

export function Chat({
  pr,
  review,
  onStart,
  onPublish,
  onHistory,
}: {
  pr: PullRequest;
  review?: Review;
  onStart: () => void;
  onPublish: (r: Review) => void;
  onHistory: (id: string) => void;
}) {
  const { snapshot: s, action } = useOrbit();
  const text = useOrbit((state) => state.chatDrafts[review?.id ?? ""] ?? "");
  const setChatDraft = useOrbit((state) => state.setChatDraft);
  const setText = (value: string) => {
    if (review) setChatDraft(review.id, value);
  };
  const [messageError, setMessageError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const inFlight = useRef(false);
  const [chooseModel, setChooseModel] = useState(false);
  const running = !!review && activeStatuses.includes(review.status);
  const unavailable = review ? followUpUnavailable(s, review) : null;
  const history = s.reviews.filter((r) => r.prId === pr.id);
  if (!review)
    return (
      <ChatLayout
        composer={null}
        emptyState={
          <EmptyState
            icon={<Sparkles />}
            title="Meet your review partner"
            description="Start a review to give Codex a dedicated sandbox and a conversation about this pull request."
            actions={
              <Button
                label="Start review"
                variant="primary"
                icon={<Play />}
                onClick={onStart}
                isDisabled={!canStartReview(s, pr.id)}
              />
            }
          />
        }
      >
        {null}
      </ChatLayout>
    );
  const submit = async () => {
    if (!text.trim() || inFlight.current || unavailable) return;
    inFlight.current = true;
    setSending(true);
    setMessageError(null);
    const result = await action(
      "send_message",
      { reviewId: review.id, text },
      { quiet: true },
    );
    if (result.ok) {
      if (useOrbit.getState().chatDrafts[review.id] === text) setText("");
    } else setMessageError(result.error);
    inFlight.current = false;
    setSending(false);
  };
  const step =
    review.status === "posted"
      ? 3
      : review.status === "ready" || review.status === "publishing"
        ? 2
        : ["reviewing", "awaitingApproval"].includes(review.status)
          ? 1
          : 0;
  const selection = chatSelection(review);
  const inputDisabled =
    !review.threadId || !review.worktree || review.cleanupPending;
  return (
    <VStack className="review-chat" height="100%">
      <ChatLayout
        composer={
          <VStack gap={2}>
            {unavailable && (
              <Text role="status" type="supporting" color="secondary">
                {unavailable}
                {!review.worktree && canStartReview(s, pr.id) && (
                  <Button
                    label="Review again"
                    variant="ghost"
                    size="sm"
                    onClick={onStart}
                  />
                )}
              </Text>
            )}
            {messageError && <Banner status="error" title={messageError} />}
            {!selection && (
              <HStack>
                <Button
                  label="Choose chat model"
                  isDisabled={!!chatModelUnavailable(s, review)}
                  onClick={() => setChooseModel(true)}
                />
              </HStack>
            )}
            <ChatComposer
              value={text}
              onChange={setText}
              onSubmit={() => void submit()}
              isDisabled={inputDisabled}
              elevation="none"
              density="compact"
              input={
                <ChatComposerInput
                  label="Message Codex"
                  placeholder={
                    running
                      ? "Codex is working. You can follow up when it finishes."
                      : "Ask a follow-up about this pull request…"
                  }
                  pasteAsToken={false}
                  hasHistory={false}
                  // Astryx clears on submission by default. Orbit owns clearing only
                  // after backend acceptance, so blocked or failed sends keep the draft.
                  onKeyDown={(e) => {
                    if (
                      e.key === "Enter" &&
                      !e.shiftKey &&
                      !e.nativeEvent.isComposing &&
                      e.keyCode !== 229
                    ) {
                      e.preventDefault();
                      void submit();
                    }
                  }}
                />
              }
              footerActions={
                <Text type="supporting" color="secondary">
                  Chat: {selection?.model || "Choose a model"} ·{" "}
                  {selection?.effort || "Choose an effort"}
                  {selection && " (pinned)"}
                </Text>
              }
              sendButton={
                <ChatSendButton
                  isDisabled={!text.trim() || !!unavailable || sending}
                  onSend={() => void submit()}
                />
              }
            />
            <Text type="supporting" color="secondary">
              Your chat stays with this review. <Kbd keys="enter" /> to send ·{" "}
              <Kbd keys="shift+enter" /> for a new line
            </Text>
          </VStack>
        }
      >
        <VStack gap={3} padding={4}>
          <HStack gap={2} wrap="wrap">
            {["Sandbox", "Review", "Publish"].map((name, i) => (
              <Token
                key={name}
                label={name}
                color={step > i ? "green" : "default"}
                size="sm"
              />
            ))}
          </HStack>
          {history.length > 1 && (
            <Selector
              label="Review history"
              value={review.id}
              onChange={onHistory}
              options={history.map((r, i) => ({
                value: r.id,
                label: `Review ${history.length - i} · ${displayStatusLabels[reviewDisplayStatus(s, r)]}`,
                description: new Date(r.createdAt).toLocaleString(),
              }))}
              renderOption={(option) => {
                const run = history.find((r) => r.id === option.value)!;
                return (
                  <VStack gap={1}>
                    <Text>{option.label}</Text>
                    <Timestamp value={run.createdAt} format="date_time" />
                  </VStack>
                );
              }}
            />
          )}
          {review.error && <Banner status="error" title={review.error} />}
          {(review.headSha !== pr.headSha || review.baseSha !== pr.baseSha) && (
            <Banner
              status="warning"
              title="New commits are available. Start a new review before publishing."
            />
          )}
          <Text type="supporting" color="secondary">
            Report selection: {reportSelectionLabel(review)}
          </Text>
        </VStack>
        <ChatMessageList
          isStreaming={running}
          align="top"
          density="compact"
          emptyState={
            <EmptyState
              title="Your review is starting"
              description="Messages will appear here as Codex works."
              isCompact
            />
          }
        >
          {review.messages.map((m, index) =>
            m.role === "system" ? (
              m.text.includes("\n") ? (
                <ChatToolCalls
                  key={m.id}
                  label="Execution log"
                  calls={[
                    {
                      key: m.id,
                      name: m.text.split("\n")[0],
                      status: "complete",
                      resultDetail: (
                        <CodeBlock code={m.text} isWrapped width="100%" />
                      ),
                    },
                  ]}
                />
              ) : (
                <ChatSystemMessage key={m.id}>{m.text}</ChatSystemMessage>
              )
            ) : (
              <ChatMessage
                key={m.id}
                sender={m.role === "agent" ? "assistant" : "user"}
                name={m.role === "agent" ? "Codex" : "You"}
                metadata={
                  <HStack gap={2} wrap="wrap">
                    {m.delivery && m.delivery !== "sent" && (
                      <Text role="status" type="supporting">
                        {m.delivery === "pending"
                          ? "Sending…"
                          : m.delivery === "failed"
                            ? "Send failed"
                            : "Sending interrupted"}
                      </Text>
                    )}
                    {m.confirmedSelection && (
                      <Text type="supporting">
                        {m.confirmedSelection.model} ·{" "}
                        {m.confirmedSelection.effort} (confirmed)
                      </Text>
                    )}
                    <Timestamp
                      value={m.createdAt}
                      {...recentTimestamp(review.messages.length - index - 1)}
                    />
                  </HStack>
                }
              >
                <ChatMessageBubble
                  variant={m.role === "agent" ? "ghost" : "filled"}
                >
                  <SafeMarkdown
                    isStreaming={
                      running &&
                      m.role === "agent" &&
                      m === review.messages.at(-1)
                    }
                  >
                    {m.text}
                  </SafeMarkdown>
                </ChatMessageBubble>
              </ChatMessage>
            ),
          )}
          {running && (
            <ChatSystemMessage>
              <Spinner label={statusLabels[review.status]} />
            </ChatSystemMessage>
          )}
          {review.status === "ready" && (
            <ChatSystemMessage>
              <HStack gap={2} wrap="wrap" vAlign="center">
                <Text>Review complete. Your call from here.</Text>
                <Button
                  label="Publish findings"
                  size="sm"
                  isDisabled={!canPublish(review, publishTarget(s, pr))}
                  onClick={() => onPublish(review)}
                />
              </HStack>
            </ChatSystemMessage>
          )}
        </ChatMessageList>
        <VStack paddingInline={4} paddingBlockEnd={2}>
          <ReviewActions review={review} />
        </VStack>
      </ChatLayout>
      <LegacyChatModel
        isOpen={chooseModel}
        review={review}
        onClose={() => setChooseModel(false)}
      />
    </VStack>
  );
}
