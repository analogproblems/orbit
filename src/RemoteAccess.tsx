import { useEffect, useRef, useState } from "react";
import {
  Banner,
  Button,
  Card,
  CodeBlock,
  Heading,
  NumberInput,
  Switch,
  Text,
  VStack,
} from "@astryxdesign/core";
import { RotateCcw } from "lucide-react";
import { desktop, remote, useOrbit, windowless } from "./store";

/** Browser access as the desktop app reports it (`remote::Status`). */
export interface RemoteAccessStatus {
  enabled: boolean;
  port: number;
  running: boolean;
  error?: string | null;
  link?: string | null;
  tailnetHost?: string | null;
}

/** The link to open on another device: through Tailscale when it is known. */
export function shareLink(status: RemoteAccessStatus) {
  if (!status.link) return null;
  const token = status.link.split("#token=")[1];
  return status.tailnetHost
    ? `https://${status.tailnetHost}/#token=${token}`
    : status.link;
}

export function RemoteAccess() {
  const action = useOrbit((state) => state.action);
  const alwaysOn = useOrbit((state) => state.alwaysOn);
  // Showing an always-on Orbit, this app serves no browsers of its own.
  const own = desktop && !alwaysOn;
  const [status, setStatus] = useState<RemoteAccessStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  // Set once you change something, so the first read can't undo it.
  const changed = useRef(false);
  const run = async (command: string, args: Record<string, unknown> = {}) => {
    changed.current = true;
    setBusy(true);
    setError(null);
    const result = await action<RemoteAccessStatus>(command, args, {
      quiet: true,
    });
    setBusy(false);
    if (result.ok) setStatus(result.data);
    else setError(result.error);
  };
  useEffect(() => {
    if (!own) return;
    void action<RemoteAccessStatus>(
      "get_remote_access",
      {},
      { quiet: true },
    ).then((result) => {
      if (changed.current) return;
      if (result.ok) setStatus(result.data);
      else setLoadError(result.error);
    });
  }, [action, own]);
  const link = status && shareLink(status);
  return (
    <Card>
      <VStack gap={4}>
        <Heading level={3}>Use Orbit from other devices</Heading>
        {remote && !windowless && (
          <Text color="secondary">
            You’re using Orbit from another device. Turn this off or make a new
            link from the desktop app.
          </Text>
        )}
        {windowless && (
          <Text color="secondary">
            You’re using an Orbit that runs without a window. To make a new
            link, stop it on the computer it runs on, run orbit-server link
            --new there, then start it again.
          </Text>
        )}
        {desktop && !own && (
          <Text color="secondary">
            This app shows an always-on Orbit, so browsers open that one. Its
            link is shown on the computer it runs on: run orbit-server link
            there.
          </Text>
        )}
        {!desktop && !remote && (
          <Text color="secondary">
            Open Orbit on your phone or laptop over Tailscale. Available in the
            desktop app.
          </Text>
        )}
        {own && status && (
          <>
            <Switch
              label="Allow access from a browser"
              labelPosition="start"
              labelSpacing="spread"
              width="100%"
              value={status.enabled}
              isDisabled={busy}
              onChange={(enabled) =>
                void run("set_remote_access", { enabled, port: status.port })
              }
              description="Open this workspace on your phone or laptop through Tailscale. Anyone with the link can do everything you can here, including running repository setup commands on this computer, so only open it on your own devices. Turning this off makes a new link."
            />
            <NumberInput
              label="Port"
              value={status.port}
              min={1024}
              max={65535}
              isIntegerOnly
              isDisabled={busy}
              description="Orbit listens on this computer only. Tailscale forwards your other devices to this port."
              onChange={(port) =>
                port !== status.port &&
                void run("set_remote_access", {
                  enabled: status.enabled,
                  port,
                })
              }
            />
            {status.error && <Banner status="error" title={status.error} />}
            {status.running && link && (
              <VStack gap={3}>
                <Text weight="semibold">
                  1. Publish it on your tailnet, once, from this computer
                </Text>
                <CodeBlock
                  code={`tailscale serve --bg ${status.port}`}
                  language="bash"
                  size="sm"
                />
                <Text weight="semibold">
                  2. Open this link on your other device
                </Text>
                <CodeBlock code={link} size="sm" width="100%" isWrapped />
                {!status.tailnetHost && (
                  <Text color="secondary" type="supporting">
                    Tailscale wasn’t found on this computer, so this is the
                    local address. On another device, replace 127.0.0.1:
                    {status.port} with this computer’s Tailscale name.
                  </Text>
                )}
                <VStack gap={1}>
                  <Button
                    label="Make a new link"
                    variant="ghost"
                    size="sm"
                    icon={<RotateCcw />}
                    isLoading={busy}
                    onClick={() => void run("reset_remote_link")}
                  />
                  <Text color="secondary" type="supporting">
                    Signs out every browser that uses the current link.
                  </Text>
                </VStack>
              </VStack>
            )}
          </>
        )}
        {loadError && (
          <Text color="secondary">
            Browser access settings are unavailable: {loadError}
          </Text>
        )}
        {error && <Banner status="error" title={error} />}
      </VStack>
    </Card>
  );
}
