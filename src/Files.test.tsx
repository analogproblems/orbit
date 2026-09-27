// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { Files } from "./Files";
import { freshDemo } from "./demo";
import { useOrbit } from "./store";
import type { ChangedFile } from "./types";
afterEach(cleanup);
function show(files: ChangedFile[]) {
  const action = vi.fn().mockResolvedValue({ ok: true, data: files });
  useOrbit.setState({ action });
  render(<Files pr={freshDemo().prs[0]} />);
  return action;
}
const file = (patch?: string): ChangedFile => ({
  filename: "src/app.ts",
  status: "modified",
  additions: 1,
  deletions: 1,
  patch,
});

describe("changed files", () => {
  it("shows each line with its original and updated line numbers", async () => {
    const action = show([file("@@ -7,2 +7,2 @@ intro\n same\n-old\n+new")]);
    expect(action).toHaveBeenCalledWith(
      "get_files",
      { prId: freshDemo().prs[0].id },
      { quiet: true },
    );
    const diff = await screen.findByRole("group", {
      name: "Changes in src/app.ts",
    });
    const numbers = (label: string) =>
      within(diff)
        .getAllByLabelText(label)
        .map((e) => e.textContent);
    expect(numbers("Original line")).toEqual(["", "7", "8", ""]);
    expect(numbers("Updated line")).toEqual(["", "7", "", "8"]);
    expect(within(diff).getByText("-old")).toBeTruthy();
    expect(within(diff).getByText("+new")).toBeTruthy();
  });
  it("folds a file away and opens it again", async () => {
    show([file("@@ -1 +1 @@\n+new")]);
    const toggle = await screen.findByRole("button", { name: /src\/app\.ts/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
  });
  it("explains a file GitHub sends without a preview", async () => {
    show([file()]);
    expect(
      await screen.findByText(/GitHub does not provide a preview/),
    ).toBeTruthy();
    expect(screen.queryByRole("group", { name: /Changes in/ })).toBeNull();
  });
});
