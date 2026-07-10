import type { ControlErrorEvent } from "./types";

export function controlErrorMessage(event: ControlErrorEvent): string {
  const command = event.cmd?.trim() || "runtime control";
  return `${command}: ${event.message}`;
}
