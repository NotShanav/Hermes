import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";

/** Polls the recorder state (process, tray state file, archive counters). No keys are needed. */
export function useRecorder(intervalMs = 5000) {
  const [rec, setRec] = useState(null);
  const [error, setError] = useState(null);
  const alive = useRef(true);
  const refresh = useCallback(async () => {
    try {
      const r = await api.collectorInfo();
      if (alive.current) {
        setRec(r);
        setError(null);
      }
      return r;
    } catch (e) {
      if (alive.current) setError(e.message);
      return null;
    }
  }, []);
  useEffect(() => {
    alive.current = true;
    refresh();
    const t = setInterval(refresh, intervalMs);
    return () => {
      alive.current = false;
      clearInterval(t);
    };
  }, [refresh, intervalMs]);
  return { rec, error, refresh };
}

export const STATE_LABEL = {
  recording: "Recording",
  paused: "Paused",
  locked: "Screen locked",
  asleep: "Computer asleep",
  disconnected: "Session disconnected",
  stopped: "Not running",
  unknown: "Running",
};
export const STATE_TONE = { recording: "rec", paused: "warn", locked: "muted", asleep: "muted", disconnected: "muted", stopped: "off", unknown: "ok" };
