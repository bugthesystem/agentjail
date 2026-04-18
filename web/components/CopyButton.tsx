"use client";

import { useState } from "react";
import { Button } from "./Button";

/** Button that copies a string to the clipboard and briefly shows a tick. */
export function CopyButton({ value, label = "Copy" }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      variant="ghost"
      size="sm"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        } catch {
          /* ignore */
        }
      }}
      aria-label={copied ? "Copied" : label}
    >
      {copied ? "Copied" : label}
    </Button>
  );
}
