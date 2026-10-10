import { useState } from "react";
import {
  Button,
  Card,
  ClickableCard,
  EmptyState,
  Grid,
  Heading,
  HStack,
  List,
  ListItem,
  SegmentedControl,
  SegmentedControlItem,
  SelectableCard,
  Selector,
  SideNavItem,
  Switch,
  Text,
  TextArea,
  TextInput,
  Timestamp,
  VStack,
} from "@astryxdesign/core";
import { useOrbit } from "./store";
import {
  buddyMood,
  buddyOf,
  buddyTones,
  moodReason,
  withPatch,
  type Buddy,
  type BuddyHat,
  type BuddyNote,
  type BuddyTone,
  type Mood,
  type SettingsPatch,
} from "./types";
import { Bit, bitColours, bitHats, moodLabels } from "./Bit";
import { SaveFeedback } from "./SaveFeedback";
import { recentTimestamp } from "./timestamps";

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
  const latest = s.buddyFeed?.notes.at(-1);
  if (collapsed)
    return (
      <SideNavItem
        label={buddy.name}
        icon={
          <Bit mood={mood} colour={buddy.colour} hat={buddy.hat} size="xs" />
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
        <Bit mood={mood} colour={buddy.colour} hat={buddy.hat} size="sm" />
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

/** Bit's notes, newest first. */
export function BuddyPage({
  onNote,
  onSettings,
}: {
  onNote: (note: BuddyNote) => void;
  onSettings: () => void;
}) {
  const { snapshot: s } = useOrbit();
  const buddy = useBuddy();
  const mood = buddyMood(s);
  const notes = [...(s.buddyFeed?.notes ?? [])].reverse();
  return (
    <VStack gap={6} padding={6} maxWidth={760} className="content-page">
      <HStack gap={4} vAlign="center">
        <Bit
          mood={mood}
          colour={buddy.colour}
          hat={buddy.hat}
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
      {buddy.enabled &&
        (notes.length ? (
          <List hasDividers aria-label={`${buddy.name}'s notes`}>
            {notes.map((note, index) => (
              <ListItem
                key={note.id}
                label={note.text}
                description={
                  <Text type="supporting" color="secondary">
                    <Timestamp value={note.at} {...recentTimestamp(index)} />
                  </Text>
                }
                onClick={() => onNote(note)}
              />
            ))}
          </List>
        ) : (
          <EmptyState
            title="Nothing to report yet"
            description={`${buddy.name} will note each round, each clean loop, and anything that needs you.`}
          />
        ))}
    </VStack>
  );
}

const previewMoods: Mood[] = ["happy", "busy", "worried", "sleepy"];

/** The buddy's part of Settings: on or off, name, colour, hat, tone and
 * personality, with a live preview. */
export function BuddySettings() {
  const { updateSettings, saves } = useOrbit();
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
                  <Bit mood={preview} colour={id} hat={buddy.hat} size="sm" />
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
                      size="sm"
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
          <TextArea
            label="Personality"
            isOptional
            description={`Describe ${buddy.name} in your own words. It will shape how ${buddy.name} chats once that arrives.`}
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
      </VStack>
    </Card>
  );
}
