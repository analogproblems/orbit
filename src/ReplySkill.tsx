import {
  Button,
  Card,
  Code,
  CodeBlock,
  Collapsible,
  Heading,
  HStack,
  Text,
  VStack,
} from "@astryxdesign/core";
import { useClipboard } from "@astryxdesign/core/hooks";
import { Check, Copy } from "lucide-react";
import skill from "../.claude/skills/answer-orbit-reviews/SKILL.md?raw";

export const replySkill = skill;
export const replySkillPath = ".claude/skills/answer-orbit-reviews/SKILL.md";

/** The Claude Code skill that teaches a coding agent to answer findings in
 * the form the next round recognises. Orbit shows it; you choose where it
 * goes. */
export function ReplySkill() {
  const { copy, isCopied } = useClipboard({ announce: "Skill copied" });
  return (
    <Card>
      <VStack gap={4}>
        <VStack gap={2}>
          <Heading level={3}>Replies from coding agents</Heading>
          <Text color="secondary">
            When a coding agent fixes a pull request, this Claude Code skill
            teaches it to answer each finding with a line such as “Fixed F2 in
            3f9c2a1” or “Declined F4: reason”, which the next round recognises.
          </Text>
          <Text color="secondary">
            Save it as <Code>{replySkillPath}</Code> in a repository Orbit
            reviews, then commit and push it: cloud sessions load skills from
            the repository they clone. Or save it under{" "}
            <Code>~/.claude/skills/answer-orbit-reviews/</Code> for every
            project on one computer. Orbit doesn’t add it for you.
          </Text>
        </VStack>
        <HStack>
          <Button
            label={isCopied ? "Copied" : "Copy skill"}
            icon={isCopied ? <Check /> : <Copy />}
            onClick={() => void copy(skill)}
          />
        </HStack>
        <Collapsible
          defaultIsOpen={false}
          trigger={<Text weight="semibold">Show the skill</Text>}
        >
          <CodeBlock
            code={skill}
            language="markdown"
            title="SKILL.md"
            isWrapped
            width="100%"
          />
        </Collapsible>
      </VStack>
    </Card>
  );
}
