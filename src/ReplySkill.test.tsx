// @vitest-environment jsdom
import "./test-setup";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { ReplySkill, replySkill, replySkillPath } from "./ReplySkill";
afterEach(cleanup);

describe("ReplySkill", () => {
  it("copies the skill and says where it goes", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    render(<ReplySkill />);
    expect(screen.getByText(replySkillPath)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Copy skill" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(replySkill));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();
  });

  it("is the skill file, named as Claude Code expects", () => {
    expect(replySkill).toMatch(/^---\nname: answer-orbit-reviews\n/);
    expect(replySkill).toContain("Fixed F2 in 3f9c2a1");
  });
});
