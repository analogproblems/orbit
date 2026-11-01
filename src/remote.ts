// Browser mode: Orbit opened from another device through the desktop app's
// built-in server (see src-tauri/src/remote.rs). The access link carries the
// token in its fragment; it is kept in this browser and sent as a header, never
// in a URL.
import type { Snapshot } from "./types";

const tokenKey = "orbit-remote-token";
/** Where an access link carries its token: `#token=` and 64 hex digits. */
const tokenPattern = /(?:^#|&)token=([0-9a-f]{64})(?:&|$)/;
export const expiredLink =
  "This access link no longer works. Open Orbit's current link to reconnect.";
export const lostConnection = "Lost connection to Orbit. Reconnecting…";
/** How long the event stream may stay silent; the server writes every 15 s. */
const silence = 45_000;

/** Takes the token from the access link, or the one this browser kept. */
export function readToken(): string | null {
  // Outside a browser (tests run under Node) there is no address to read.
  if (typeof location === "undefined") return null;
  const match = tokenPattern.exec(location.hash);
  if (match) {
    try {
      localStorage.setItem(tokenKey, match[1]);
    } catch {
      // Private browsing can refuse storage; the token still works for this visit.
    }
    // Keep the token out of history, bookmarks and screenshots of the address bar.
    history.replaceState(null, "", location.pathname + location.search);
    return match[1];
  }
  try {
    return localStorage.getItem(tokenKey);
  } catch {
    return null;
  }
}
/** The token in a pasted access link, if it is a link to this Orbit, at
 * `origin`, or else what is wrong with it. */
export function linkedToken(
  link: string,
  origin: string,
): { token: string } | { problem: string } {
  let url: URL;
  try {
    url = new URL(link.trim());
  } catch {
    return { problem: notALink };
  }
  const match = tokenPattern.exec(url.hash);
  if (!match) return { problem: notALink };
  if (url.origin !== origin)
    return {
      problem: `That link opens the Orbit at ${url.host}. Open it there.`,
    };
  return { token: match[1] };
}
const notALink =
  "That isn’t Orbit’s access link. Copy the whole link, with #token= in it.";
/** Opens Orbit again as from its access link with `token`, so a page that
 * started without one keeps it, such as one added to the Home Screen,
 * which starts without what Safari kept. The token goes in the fragment,
 * as in the link, never in a URL sent to the server. */
export function openWith(token: string) {
  history.replaceState(null, "", `${location.pathname}#token=${token}`);
  location.reload();
}
function forgetToken() {
  try {
    localStorage.removeItem(tokenKey);
  } catch {
    // Nothing was kept.
  }
}

export async function call<T>(
  token: string,
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  const response = await fetch(`/api/${command}`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
    },
    body: JSON.stringify(args),
  });
  const body = await response.json().catch(() => null);
  if (response.status === 401) {
    forgetToken();
    throw Error(expiredLink);
  }
  if (!response.ok)
    throw Error(body?.error ?? `Orbit could not do that (${response.status}).`);
  return body as T;
}

/**
 * Follows the live snapshot, reconnecting when the stream drops. `onProblem`
 * gets a message while disconnected and `null` once connected again.
 * Returns a function that stops following.
 */
export function follow(
  token: string,
  onSnapshot: (snapshot: Snapshot) => void,
  onProblem: (message: string | null) => void,
): () => void {
  let stopped = false;
  let controller: AbortController | undefined;
  let wake: (() => void) | undefined;
  const pause = (ms: number) =>
    new Promise<void>((resolve) => {
      wake = resolve;
      setTimeout(resolve, ms);
    });
  async function run() {
    let delay = 1000;
    while (!stopped) {
      controller = new AbortController();
      const current = controller;
      let watchdog: ReturnType<typeof setTimeout> | undefined;
      const quiet = () => {
        clearTimeout(watchdog);
        watchdog = setTimeout(() => current.abort(), silence);
      };
      try {
        quiet();
        const response = await fetch("/api/events", {
          headers: { authorization: `Bearer ${token}` },
          signal: current.signal,
        });
        if (response.status === 401) {
          forgetToken();
          onProblem(expiredLink);
          return;
        }
        if (!response.ok || !response.body) throw Error();
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffered = "";
        let connected = false;
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          quiet();
          buffered += decoder.decode(value, { stream: true });
          let end;
          while ((end = buffered.indexOf("\n")) >= 0) {
            const line = buffered.slice(0, end);
            buffered = buffered.slice(end + 1);
            if (!line.trim()) continue;
            onSnapshot(JSON.parse(line) as Snapshot);
            if (!connected) {
              connected = true;
              delay = 1000;
              onProblem(null);
            }
          }
        }
      } catch {
        // Retried below, unless following was stopped.
      } finally {
        clearTimeout(watchdog);
      }
      if (stopped) return;
      onProblem(lostConnection);
      await pause(delay);
      delay = Math.min(delay * 2, 15_000);
    }
  }
  void run();
  return () => {
    stopped = true;
    controller?.abort();
    wake?.();
  };
}

/** Saves text as a file from the browser. */
export function download(name: string, text: string) {
  const url = URL.createObjectURL(
    new Blob([text], { type: "text/markdown;charset=utf-8" }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
