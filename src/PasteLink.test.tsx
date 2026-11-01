// @vitest-environment jsdom
import "./test-setup";
import { expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const token = "9a".repeat(32);
vi.hoisted(() => {
  // Orbit opened from the Home Screen: its own page, served by Orbit, with
  // nothing kept from Safari and no link in the address.
  const mark = document.createElement("meta");
  mark.name = "orbit-served";
  document.head.append(mark);
  localStorage.clear();
  history.replaceState(null, "", "/");
  vi.stubGlobal("fetch", vi.fn());
});
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
import { Theme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral/built";
import App from "./App";
import { expiredLink, linkedToken, readToken } from "./remote";
import { remote } from "./store";

it("asks a page that starts without the access link for it once", async () => {
  expect(remote).toBe(true);
  render(
    <Theme theme={neutralTheme} mode="dark">
      <App />
    </Theme>,
  );
  expect(await screen.findByText(expiredLink)).toBeTruthy();
  const field = screen.getByRole("textbox", { name: /^Access link/ });
  const connect = screen.getByRole("button", { name: "Connect" });
  // Not a link, or a link to another Orbit: said, and nothing kept.
  await userEvent.type(field, "hello");
  await userEvent.click(connect);
  // (Said in the field and to screen readers.)
  expect(await screen.findAllByText(/isn’t Orbit’s access link/)).not.toEqual(
    [],
  );
  await userEvent.clear(field);
  await userEvent.type(field, `https://elsewhere.example/#token=${token}`);
  await userEvent.click(connect);
  expect(
    await screen.findAllByText(
      "That link opens the Orbit at elsewhere.example. Open it there.",
    ),
  ).not.toEqual([]);
  expect(location.hash).toBe("");
  // This Orbit's link: Orbit opens again as from it, which keeps it.
  await userEvent.clear(field);
  await userEvent.type(field, `${location.origin}/#token=${token}`);
  await userEvent.click(connect);
  await waitFor(() => expect(location.hash).toBe(`#token=${token}`));
  expect(readToken()).toBe(token);
  expect(localStorage.getItem("orbit-remote-token")).toBe(token);
  expect(location.hash).toBe("");
});

it("takes the token only from the link's fragment", () => {
  const here = "https://orbit.tail.ts.net";
  expect(linkedToken(` ${here}/#token=${token} `, here)).toEqual({ token });
  for (const link of [
    `${here}/?token=${token}`,
    `${here}/#token=${token.slice(2)}`,
    `${here}/#token=${token.toUpperCase()}`,
    token,
  ]) {
    expect(linkedToken(link, here)).toHaveProperty("problem");
  }
  expect(linkedToken(`http://orbit.tail.ts.net/#token=${token}`, here)).toEqual(
    {
      problem: "That link opens the Orbit at orbit.tail.ts.net. Open it there.",
    },
  );
});
