export interface DiffLine {
  text: string;
  kind: "hunk" | "added" | "removed" | "context" | "metadata";
  oldLine?: number;
  newLine?: number;
}
export function parseDiff(patch: string): DiffLine[] {
  let oldLine = 0,
    newLine = 0;
  return patch.split("\n").map((text) => {
    const hunk = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
    if (hunk) {
      oldLine = Number(hunk[1]);
      newLine = Number(hunk[2]);
      return { text, kind: "hunk" };
    }
    if (text.startsWith("+"))
      return { text, kind: "added", newLine: newLine++ };
    if (text.startsWith("-"))
      return { text, kind: "removed", oldLine: oldLine++ };
    if (text.startsWith(" "))
      return { text, kind: "context", oldLine: oldLine++, newLine: newLine++ };
    return { text, kind: "metadata" };
  });
}
