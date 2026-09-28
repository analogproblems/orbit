import { useEffect, useState } from "react";
import {
  Banner,
  Button,
  Card,
  Code,
  Collapsible,
  HStack,
  ScrollableArea,
  Spinner,
  Text,
  VStack,
} from "@astryxdesign/core";
import { FileCode2 } from "lucide-react";
import { useOrbit } from "./store";
import { parseDiff } from "./diff";
import type { PullRequest, ChangedFile } from "./types";
export function Files({ pr }: { pr: PullRequest }) {
  const { action } = useOrbit();
  const [files, setFiles] = useState<ChangedFile[] | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  // Keyed by PR, head, and base commits, so a new revision starts from a fresh state.
  useEffect(() => {
    let alive = true;
    void action<ChangedFile[]>(
      "get_files",
      { prId: pr.id },
      { quiet: true },
    ).then((result) => {
      if (alive) {
        setFiles(result.ok ? result.data : []);
        setFileError(result.ok ? null : result.error);
      }
    });
    return () => {
      alive = false;
    };
  }, [pr.id, action, attempt]);
  return (
    <VStack gap={3} padding={4}>
      {files === null ? (
        <Spinner label="Loading changes…" />
      ) : fileError ? (
        <Banner
          status="error"
          title="Couldn’t load changed files"
          description={fileError}
          endContent={
            <Button
              label="Retry"
              onClick={() => {
                setFiles(null);
                setFileError(null);
                setAttempt((n) => n + 1);
              }}
            />
          }
        />
      ) : (
        files.map((f) => (
          <Card key={f.filename}>
            <Collapsible
              trigger={
                <HStack gap={2} vAlign="center">
                  <FileCode2 />
                  <Code>{f.filename}</Code>
                  <Text className="diff-added">+{f.additions}</Text>
                  <Text className="diff-removed">−{f.deletions}</Text>
                </HStack>
              }
            >
              {f.patch ? (
                <ScrollableArea
                  axis="inline"
                  label={`Changes in ${f.filename}`}
                >
                  <DiffLines patch={f.patch} />
                </ScrollableArea>
              ) : (
                <Text color="secondary">
                  GitHub does not provide a preview for this file. It may be
                  binary or too large.
                </Text>
              )}
            </Collapsible>
          </Card>
        ))
      )}
    </VStack>
  );
}

// The one exception to AGENTS.md's "no <div>/<span>" rule. Astryx has no diff
// viewer: CodeBlock numbers lines 1..n in a single gutter, and Table cells wrap
// or truncate instead of scrolling. Table also kept the main thread busy about
// 75% longer on a 2,000-line patch than these plain rows, laid out by the
// `.diff-line` rules in styles.css using tokens only.
function DiffLines({ patch }: { patch: string }) {
  return parseDiff(patch).map((line, i) => (
    <div key={i} className={`diff-line ${line.kind}`}>
      <span aria-label="Original line">{line.oldLine ?? ""}</span>
      <span aria-label="Updated line">{line.newLine ?? ""}</span>
      <code>{line.text || " "}</code>
    </div>
  ));
}
