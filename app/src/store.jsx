import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { makeRange, previousRange } from "./util";

const Ctx = createContext(null);
export const useApp = () => useContext(Ctx);

/** Runs an async loader whenever `deps` change; keeps the previous data on screen while reloading. */
export function useAsync(fn, deps) {
  const [s, setS] = useState({ loading: true, data: null, error: null });
  useEffect(() => {
    let alive = true;
    setS((p) => ({ ...p, loading: true, error: null }));
    Promise.resolve()
      .then(fn)
      .then((d) => alive && setS({ loading: false, data: d, error: null }))
      .catch((e) => alive && setS({ loading: false, data: null, error: e.message || String(e) }));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return s;
}

export function AppProvider({ children }) {
  const [info, setInfo] = useState(null);
  const [bootError, setBootError] = useState(null);
  const [route, setRoute] = useState({ page: "overview", param: null });
  const [rangeKey, setRangeKey] = useState("7d");
  const [custom, setCustom] = useState({ from: "", to: "" });
  const [rev, setRev] = useState(0); // bump to refetch data after rules / preferences change
  const [exploreSeed, setExploreSeed] = useState(null);
  const [pendingDay, setPendingDay] = useState(null); // a day (UTC ms of local midnight) the Timeline should open
  const [toast, setToast] = useState(null);
  const lockTimer = useRef(null);
  const [lockFlash, setLockFlash] = useState(false); // lock-icon transition shown when the USB key is pulled
  const [lockReason, setLockReason] = useState(null);
  const lockingForKey = useRef(false);

  const refreshInfo = useCallback(async () => {
    try {
      const i = await api.info();
      setInfo(i);
      setBootError(null);
      return i;
    } catch (e) {
      setBootError(e.message);
      return null;
    }
  }, []);

  useEffect(() => {
    refreshInfo();
  }, [refreshInfo]);

  // Theme
  useEffect(() => {
    if (!info) return;
    const apply = () => {
      const t = info.prefs.theme;
      const dark = t === "system" ? window.matchMedia("(prefers-color-scheme: dark)").matches : t !== "light";
      document.documentElement.dataset.theme = dark ? "dark" : "light";
      document.documentElement.dataset.ui = info.prefs.ui_style || "modern";
      document.documentElement.dataset.effects = info.prefs.effects || "full";
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [info]);

  const themePref = info ? info.prefs.theme : "dark";
  const dark = themePref === "system" ? window.matchMedia("(prefers-color-scheme: dark)").matches : themePref !== "light";

  // Interface style: "modern" is the original look. "fluidity" adds glass, blobs and fluid motion.
  const ui = info ? info.prefs.ui_style || "modern" : "modern";
  const fx = info ? info.prefs.effects || "full" : "full";
  const reduceTransparency = typeof window !== "undefined" && !!window.matchMedia && window.matchMedia("(prefers-reduced-transparency: reduce)").matches;
  const fluid = ui === "fluidity";
  const liquid = fluid && fx === "full" && !reduceTransparency; // true refraction only in Full

  const offset = info ? info.offset_minutes : 0;
  const range = useMemo(() => makeRange(rangeKey, offset, custom), [rangeKey, offset, custom]);
  const prevRange = useMemo(() => (rangeKey === "all" ? null : previousRange(range)), [rangeKey, range]);

  const go = useCallback((page, param = null) => setRoute({ page, param }), []);
  const say = useCallback((text, tone = "ok") => {
    setToast({ text, tone, id: Date.now() });
  }, []);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4200);
    return () => clearTimeout(t);
  }, [toast]);

  const lockNow = useCallback(async () => {
    try {
      await api.lock();
    } catch (e) {
      /* locking only drops memory; ignore */
    }
    await refreshInfo();
    setRoute({ page: "overview", param: null });
  }, [refreshInfo]);

  // USB key removed: lock at once, show the lock icon, then land on the lock screen.
  const isUnlocked = !!(info && info.unlocked);
  useEffect(() => {
    if (!isUnlocked) return;
    const t = setInterval(async () => {
      if (lockingForKey.current) return;
      let present = true;
      try {
        present = await api.keyPresent();
      } catch (e) {
        return; // a failed check is not evidence of removal
      }
      if (present) return;
      lockingForKey.current = true;
      setLockFlash(true); // cover the app immediately, before anything else is drawn
      try {
        await api.lock();
      } catch (e) {
        /* locking only drops memory */
      }
      setLockReason("The USB key was removed, so Hermes locked itself. Plug it back in to unlock.");
      await refreshInfo();
      setRoute({ page: "overview", param: null });
      setTimeout(() => {
        setLockFlash(false);
        lockingForKey.current = false;
      }, 1100);
    }, 700);
    return () => clearInterval(t);
  }, [isUnlocked, refreshInfo]);

  // Auto-lock after idle time inside the app.
  const unlocked = !!(info && info.unlocked);
  const mins = info ? info.prefs.auto_lock_minutes : 0;
  useEffect(() => {
    if (!unlocked || !mins) return;
    const reset = () => {
      clearTimeout(lockTimer.current);
      lockTimer.current = setTimeout(() => lockNow(), mins * 60000);
    };
    const ev = ["mousemove", "mousedown", "keydown", "wheel", "touchstart"];
    ev.forEach((e) => window.addEventListener(e, reset, { passive: true }));
    reset();
    return () => {
      clearTimeout(lockTimer.current);
      ev.forEach((e) => window.removeEventListener(e, reset));
    };
  }, [unlocked, mins, lockNow]);

  const savePrefs = useCallback(
    async (patch) => {
      const next = { ...info.prefs, ...patch };
      const i = await api.savePrefs(next);
      setInfo(i);
      setRev((r) => r + 1);
    },
    [info]
  );

  const value = {
    info, bootError, refreshInfo, offset, dark, unlocked, ui, fx, fluid, liquid,
    route, go,
    rangeKey, setRangeKey, custom, setCustom, range, prevRange,
    rev, bump: () => setRev((r) => r + 1),
    savePrefs, lockNow,
    exploreSeed, setExploreSeed, pendingDay, setPendingDay,
    toast, say, lockFlash, lockReason, setLockReason,
    categories: info ? info.categories : [],
  };
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useCategoryMap() {
  const { categories } = useApp();
  return useMemo(() => Object.fromEntries(categories.map((c) => [c.id, c])), [categories]);
}
