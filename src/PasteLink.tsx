import { useState } from "react";
import {
  Button,
  Card,
  Heading,
  Text,
  TextInput,
  VStack,
} from "@astryxdesign/core";
import { linkedToken, openWith } from "./remote";

/** In a browser without Orbit's access link, such as Orbit added to an
 * iPhone's Home Screen, which starts without what Safari kept: paste the
 * link to connect. Only a link to this Orbit is taken. */
export function PasteLink() {
  const [link, setLink] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const open = () => {
    const linked = linkedToken(link, location.origin);
    if ("problem" in linked) setProblem(linked.problem);
    else openWith(linked.token);
  };
  return (
    <Card>
      <VStack gap={3}>
        <Heading level={3}>Connect with your access link</Heading>
        <Text color="secondary">
          Paste the link you open Orbit with. Orbit on a Home Screen starts
          without it, and asks for it once.
        </Text>
        <TextInput
          label="Access link"
          placeholder="https://…/#token=…"
          value={link}
          onChange={(value) => {
            setLink(value);
            setProblem(null);
          }}
          onEnter={open}
          autoComplete="off"
          width="100%"
          status={problem ? { type: "error", message: problem } : undefined}
        />
        <Button
          label="Connect"
          variant="primary"
          isDisabled={!link.trim()}
          onClick={open}
        />
      </VStack>
    </Card>
  );
}
