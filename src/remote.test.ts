// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { call, expiredLink, follow, lostConnection, readToken } from "./remote";

const token = "ab".repeat(32);
const fetchMock = vi.fn();
function stream(...lines: string[]) {
  const encoder = new TextEncoder();
  return new ReadableStream<Uint8Array>({
    start(controller) {
      for (const line of lines) controller.enqueue(encoder.encode(line));
      controller.close();
    },
  });
}

beforeEach(() => {
  localStorage.clear();
  history.replaceState(null, "", "/");
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
});
afterEach(() => {
  vi.useRealTimers();
});

describe("access link", () => {
  it("keeps the token from the link and removes it from the address", () => {
    history.replaceState(null, "", `/?x=1#token=${token}`);
    expect(readToken()).toBe(token);
    expect(location.hash).toBe("");
    expect(location.search).toBe("?x=1");
    history.replaceState(null, "", "/");
    expect(readToken()).toBe(token);
  });
  it("ignores pages without a token", () => {
    history.replaceState(null, "", "/#token=short");
    expect(readToken()).toBeNull();
  });
});

describe("commands", () => {
  it("sends the token and arguments", async () => {
    fetchMock.mockResolvedValue(Response.json({ concurrency: 3 }));
    await expect(
      call(token, "update_settings", { patch: { concurrency: 3 } }),
    ).resolves.toEqual({ concurrency: 3 });
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/api/update_settings");
    expect(init.method).toBe("POST");
    expect(init.headers.authorization).toBe(`Bearer ${token}`);
    expect(JSON.parse(init.body)).toEqual({ patch: { concurrency: 3 } });
  });
  it("shows the server's error", async () => {
    fetchMock.mockResolvedValue(
      Response.json({ error: "Unknown review." }, { status: 422 }),
    );
    await expect(call(token, "export_report", {})).rejects.toThrow(
      "Unknown review.",
    );
  });
  it("forgets a rejected link", async () => {
    localStorage.setItem("orbit-remote-token", token);
    fetchMock.mockResolvedValue(Response.json({}, { status: 401 }));
    await expect(call(token, "get_snapshot", {})).rejects.toThrow(expiredLink);
    expect(localStorage.getItem("orbit-remote-token")).toBeNull();
  });
});

describe("live updates", () => {
  it("passes on each snapshot and skips keepalive lines", async () => {
    fetchMock.mockResolvedValueOnce(
      new Response(stream('{"polling":false}\n\n{"poll', 'ing":true}\n')),
    );
    fetchMock.mockReturnValue(new Promise(() => {}));
    const snapshots: unknown[] = [];
    const problems: (string | null)[] = [];
    const stop = follow(
      token,
      (s) => snapshots.push(s),
      (p) => problems.push(p),
    );
    await vi.waitFor(() => expect(problems).toContain(lostConnection));
    expect(snapshots).toEqual([{ polling: false }, { polling: true }]);
    expect(problems[0]).toBeNull();
    expect(fetchMock.mock.calls[0][1].headers.authorization).toBe(
      `Bearer ${token}`,
    );
    stop();
  });
  it("reconnects after the stream drops", async () => {
    vi.useFakeTimers();
    fetchMock
      .mockRejectedValueOnce(Error("offline"))
      .mockResolvedValueOnce(new Response(stream('{"polling":true}\n')))
      .mockReturnValue(new Promise(() => {}));
    const problems: (string | null)[] = [];
    const stop = follow(
      token,
      () => {},
      (p) => problems.push(p),
    );
    await vi.advanceTimersByTimeAsync(0);
    expect(problems).toEqual([lostConnection]);
    await vi.advanceTimersByTimeAsync(1000);
    expect(problems.slice(0, 2)).toEqual([lostConnection, null]);
    expect(fetchMock.mock.calls.length).toBeGreaterThanOrEqual(2);
    stop();
  });
  it("stops on a rejected link", async () => {
    fetchMock.mockResolvedValue(new Response(null, { status: 401 }));
    const problems: (string | null)[] = [];
    follow(
      token,
      () => {},
      (p) => problems.push(p),
    );
    await vi.waitFor(() => expect(problems).toEqual([expiredLink]));
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
