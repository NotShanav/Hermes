import { save } from "@tauri-apps/plugin-dialog";
import { api } from "./api";

/** Asks where to save, then writes the matching events as PLAINTEXT. Returns the number of events or null if cancelled. */
export async function exportFlow(filter, format) {
  const stamp = new Date().toISOString().slice(0, 10);
  const path = await save({ defaultPath: `hermes-export-${stamp}.${format}`, filters: [{ name: format.toUpperCase(), extensions: [format] }] });
  if (!path) return null;
  const n = await api.exportEvents(filter, path, format);
  return { n, path };
}

export const KINDS = [
  { v: "FOCUS", l: "Focus started" },
  { v: "FOCUS END", l: "Focus ended" },
  { v: "TITLE", l: "Title changed" },
  { v: "SESSION START", l: "Session start" },
  { v: "SESSION END", l: "Session end" },
  { v: "SYSTEM", l: "System notes" },
];
