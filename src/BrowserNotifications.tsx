import { useState } from "react";
import {
  Button,
  List,
  ListItem,
  Switch,
  Text,
  VStack,
} from "@astryxdesign/core";
import {
  browserNotices,
  turnOnBrowserNotices,
  type BrowserNotices,
} from "./notices";
import {
  forget,
  pushSupported,
  turnOffPush,
  turnOnPush,
  useSavedDevice,
} from "./push";
import { accessLink, remote, useOrbit } from "./store";

const said: Record<
  Exclude<BrowserNotices, "off" | "needsHomeScreen">,
  string
> = {
  on: "This browser shows them too, while Orbit’s page is open in it.",
  blocked:
    "This browser blocks Orbit’s notifications. Allow them in its settings for this site, then reload the page.",
  needsHttps:
    "To show them in this browser too, open Orbit’s https link through Tailscale.",
  unsupported: "This browser can’t show notifications.",
};

/** Under Settings' notifications switch, in a browser using an Orbit:
 * whether this browser shows them, and the button that asks it to. */
export function BrowserNotifications() {
  const [state, setState] = useState(browserNotices);
  const [asking, setAsking] = useState(false);
  if (state === "needsHomeScreen") return <HomeScreen />;
  if (state === "on") {
    return (
      <VStack gap={2}>
        <Text color="secondary" type="supporting">
          {said.on}
        </Text>
        <WhenClosed />
      </VStack>
    );
  }
  if (state !== "off") {
    return (
      <Text color="secondary" type="supporting">
        {said[state]}
      </Text>
    );
  }
  const ask = async () => {
    setAsking(true);
    try {
      setState(await turnOnBrowserNotices());
    } finally {
      setAsking(false);
    }
  };
  return (
    <VStack gap={1}>
      <Button
        label="Show them in this browser"
        isLoading={asking}
        onClick={() => void ask()}
      />
      <Text color="secondary" type="supporting">
        Your browser asks you first. They show while Orbit’s page is open in it.
      </Text>
    </VStack>
  );
}

/** Commands that say what went wrong here rather than at the top of the
 * page. */
function useQuietAction() {
  const action = useOrbit((s) => s.action);
  return <T,>(command: string, args?: Record<string, unknown>) =>
    action<T>(command, args, { quiet: true });
}

/** Once this browser shows them: the switch that has Orbit wake it when
 * its page is closed too (push.ts). */
function WhenClosed() {
  const devices = useOrbit((s) => s.snapshot.pushDevices);
  const action = useQuietAction();
  const device = useSavedDevice();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  if (!pushSupported()) {
    return (
      <Text color="secondary" type="supporting">
        This browser can’t get them while Orbit’s page is closed.
      </Text>
    );
  }
  const on = device !== null && (devices ?? []).some((d) => d.id === device);
  const turn = async (next: boolean) => {
    setBusy(true);
    setProblem(null);
    try {
      await (next ? turnOnPush(action) : turnOffPush(action));
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <VStack gap={1}>
      <Switch
        label="Also when Orbit’s page is closed"
        labelPosition="start"
        labelSpacing="spread"
        width="100%"
        value={on}
        isDisabled={busy}
        onChange={(next) => void turn(next)}
        description="Orbit wakes this browser through its push service (Apple’s, Google’s, Mozilla’s or Microsoft’s), which learns only that something is waiting, never what. This browser then reads it from Orbit over your link."
      />
      {problem && (
        <Text role="alert" type="supporting">
          {problem}
        </Text>
      )}
    </VStack>
  );
}

/** The browsers Orbit wakes with its page closed, each with Remove. */
export function PushDevices() {
  const devices = useOrbit((s) => s.snapshot.pushDevices) ?? [];
  const action = useQuietAction();
  const [removing, setRemoving] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const saved = useSavedDevice();
  if (devices.length === 0) return null;
  const here = remote ? saved : null;
  const remove = async (id: string) => {
    setRemoving(id);
    setProblem(null);
    try {
      const removed = await action("remove_push_device", { id });
      if (!removed.ok) setProblem(removed.error);
      else if (id === here) await forget();
    } finally {
      setRemoving(null);
    }
  };
  return (
    <VStack gap={1}>
      <List
        hasDividers
        density="compact"
        header={
          <Text color="secondary" type="supporting">
            Woken with Orbit’s page closed
          </Text>
        }
      >
        {devices.map((d) => (
          <ListItem
            key={d.id}
            label={d.id === here ? `${d.name} (this browser)` : d.name}
            description={
              <Text color="secondary" type="supporting">
                {d.error ?? `Added ${new Date(d.added).toLocaleString()}`}
              </Text>
            }
            endContent={
              <Button
                label="Remove"
                variant="ghost"
                isLoading={removing === d.id}
                onClick={() => void remove(d.id)}
              />
            }
          />
        ))}
      </List>
      {problem && (
        <Text role="alert" type="supporting">
          {problem}
        </Text>
      )}
    </VStack>
  );
}

/** On an iPhone or iPad, in Safari: add Orbit to the Home Screen. Orbit
 * there starts without what Safari kept, so it asks once for the access
 * link (`PasteLink`), which this copies. */
function HomeScreen() {
  const [copied, setCopied] = useState<string | null>(null);
  const link = accessLink();
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(link ?? "");
      setCopied("Copied. Paste it when Orbit on the Home Screen asks for it.");
    } catch {
      setCopied(
        "This browser wouldn’t copy it. Paste the link you opened Orbit with instead.",
      );
    }
  };
  return (
    <VStack gap={1}>
      <Text color="secondary" type="supporting">
        To show them on an iPhone or iPad, add this page to the Home Screen
        (Share, then Add to Home Screen), then open Orbit from there. It asks
        once for Orbit’s access link{link ? ", which you can copy here" : ""}.
      </Text>
      {link && (
        <Button label="Copy the access link" onClick={() => void copy()} />
      )}
      {copied && (
        <Text color="secondary" type="supporting">
          {copied}
        </Text>
      )}
    </VStack>
  );
}
