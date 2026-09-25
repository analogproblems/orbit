import { useEffect, useState } from "react";
import {
  Banner,
  Button,
  Code,
  HStack,
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
          <details className="file-diff" open key={f.filename}>
            <summary>
              <HStack gap={2} vAlign="center">
                <FileCode2 />
                <Code>{f.filename}</Code>
                <Text className="diff-added">+{f.additions}</Text>
                <Text className="diff-removed">−{f.deletions}</Text>
              </HStack>
            </summary>
            <div className="diff-lines">
              {f.patch ? (
                parseDiff(f.patch).map((line, i) => (
                  <div key={i} className={line.kind}>
                    <span aria-label="Original line">{line.oldLine ?? ""}</span>
                    <span aria-label="Updated line">{line.newLine ?? ""}</span>
                    <code>{line.text || " "}</code>
                  </div>
                ))
              ) : (
                <Text color="secondary">
                  GitHub does not provide a preview for this file. It may be
                  binary or too large.
                </Text>
              )}
            </div>
          </details>
        ))
      )}
    </VStack>
  );
}
