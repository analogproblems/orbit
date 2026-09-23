import { expect, it } from "vitest";
import { parseDiff } from "./diff";
it("tracks both file line numbers across hunks and no-newline metadata", () => {
  const lines = parseDiff(
    "@@ -40,2 +40,3 @@\n context\n-old\n+new\n+another\n\\ No newline at end of file\n@@ -90 +91 @@\n last",
  );
  expect(lines[1]).toMatchObject({ oldLine: 40, newLine: 40 });
  expect(lines[2]).toMatchObject({ oldLine: 41, kind: "removed" });
  expect(lines[3]).toMatchObject({ newLine: 41, kind: "added" });
  expect(lines[4]).toMatchObject({ newLine: 42 });
  expect(lines[5].newLine).toBeUndefined();
  expect(lines[7]).toMatchObject({ oldLine: 90, newLine: 91 });
});
