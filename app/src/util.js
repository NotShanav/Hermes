export const MIN = 60000;
export const HOUR = 3600000;
export const DAY = 86400000;
export const FAR_FUTURE = 4102444800000; // 2100-01-01

export function fmtDur(ms) {
  if (!ms || ms < 1000) return "0s";
  const s = Math.round(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return m ? `${h}h ${m}m` : `${h}h`;
  if (m >= 10) return `${m}m`;
  if (m > 0) return s % 60 ? `${m}m ${s % 60}s` : `${m}m`;
  return `${s}s`;
}

export function fmtHours(ms) {
  const h = ms / HOUR;
  return h >= 10 ? `${Math.round(h)}h` : `${h.toFixed(1)}h`;
}

export const fmtNum = (n) => (n ?? 0).toLocaleString();

export function fmtBytes(n) {
  if (!n) return "0 B";
  if (n < 1024) return `${n} B`;
  if (n < 1048576) return `${(n / 1024).toFixed(1)} KiB`;
  if (n < 1073741824) return `${(n / 1048576).toFixed(1)} MiB`;
  return `${(n / 1073741824).toFixed(2)} GiB`;
}

const shift = (ms, off) => new Date(ms + off * MIN);
const p2 = (n) => String(n).padStart(2, "0");

/** Local wall-clock parts for a UTC millisecond timestamp and a UTC offset in minutes. */
export function parts(ms, off) {
  const d = shift(ms, off);
  return { y: d.getUTCFullYear(), mo: d.getUTCMonth(), d: d.getUTCDate(), h: d.getUTCHours(), mi: d.getUTCMinutes(), s: d.getUTCSeconds(), wd: (d.getUTCDay() + 6) % 7 };
}
export const fmtClock = (ms, off) => {
  const p = parts(ms, off);
  return `${p2(p.h)}:${p2(p.mi)}`;
};
export const fmtClockSec = (ms, off) => {
  const p = parts(ms, off);
  return `${p2(p.h)}:${p2(p.mi)}:${p2(p.s)}`;
};
export const fmtDate = (ms, off, o) => shift(ms, off).toLocaleDateString(undefined, { timeZone: "UTC", weekday: "short", month: "short", day: "numeric", ...(o || {}) });
export const fmtDateTime = (ms, off) => `${fmtDate(ms, off, { year: "numeric" })}, ${fmtClock(ms, off)}`;
export const isoDay = (ms, off) => {
  const p = parts(ms, off);
  return `${p.y}-${p2(p.mo + 1)}-${p2(p.d)}`;
};
export const dayStart = (ms, off) => Math.floor((ms + off * MIN) / DAY) * DAY - off * MIN;
export const fromIsoDay = (s, off) => {
  const [y, m, d] = s.split("-").map(Number);
  return Date.UTC(y, m - 1, d) - off * MIN;
};

export function ago(ms, now) {
  const d = Math.max(0, now - ms);
  if (d < 45000) return "just now";
  if (d < HOUR) return `${Math.round(d / MIN)} min ago`;
  if (d < DAY) return `${Math.round(d / HOUR)} h ago`;
  return `${Math.round(d / DAY)} d ago`;
}

export const RANGES = [
  { key: "today", label: "Today" },
  { key: "yesterday", label: "Yesterday" },
  { key: "7d", label: "7 days" },
  { key: "30d", label: "30 days" },
  { key: "90d", label: "90 days" },
  { key: "year", label: "1 year" },
  { key: "all", label: "All time" },
];

/** Range as {from, to} in UTC ms, `to` exclusive. Days follow the app's UTC offset. */
export function makeRange(key, off, custom, now = Date.now()) {
  const t = dayStart(now, off);
  switch (key) {
    case "today": return { from: t, to: t + DAY };
    case "yesterday": return { from: t - DAY, to: t };
    case "7d": return { from: t - 6 * DAY, to: t + DAY };
    case "30d": return { from: t - 29 * DAY, to: t + DAY };
    case "90d": return { from: t - 89 * DAY, to: t + DAY };
    case "year": return { from: t - 364 * DAY, to: t + DAY };
    case "all": return { from: 0, to: FAR_FUTURE };
    case "custom": {
      if (custom && custom.from && custom.to) {
        const a = fromIsoDay(custom.from, off);
        const b = fromIsoDay(custom.to, off) + DAY;
        if (b > a) return { from: a, to: b };
      }
      return { from: t - 6 * DAY, to: t + DAY };
    }
    default: return { from: t, to: t + DAY };
  }
}

export function rangeLabel(key, r, off, custom) {
  const hit = RANGES.find((x) => x.key === key);
  if (hit) return hit.label;
  return `${fmtDate(r.from, off, { year: "numeric" })} to ${fmtDate(r.to - DAY, off, { year: "numeric" })}`;
}

/** The period of the same length directly before `r` (null for "all time"). */
export const previousRange = (r) => (r.from <= 0 ? null : { from: r.from - (r.to - r.from), to: r.from });

export function delta(cur, prev) {
  if (prev == null || prev <= 0) return null;
  return (cur - prev) / prev;
}

// ---------------------------------------------------------------- colours

function hash(s) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}
/** Stable colour per application: same saturation and lightness, hue from the name. */
export function appColor(app, dark = true) {
  if (!app || app.startsWith("<")) return dark ? "#5A5670" : "#A9A4C2";
  const h = (hash(app.toLowerCase()) * 137.508) % 360;
  return `hsl(${Math.round(h)} ${dark ? 62 : 58}% ${dark ? 64 : 50}%)`;
}

export const BROWSERS = ["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe", "arc.exe", "zen.exe", "iexplore.exe"];
export const isBrowser = (app) => BROWSERS.includes((app || "").toLowerCase());

const SUFFIX = /\s+[-–—|]\s+(Google Chrome|Microsoft.{0,3}Edge|Mozilla Firefox|Brave|Opera|Vivaldi|Arc|Zen Browser|Internet Explorer)(\s+[-–—]\s+.+)?$/i;
export const cleanTitle = (t) => (t || "").replace(SUFFIX, "").trim();

export function cleanAppName(app) {
  return (app || "").replace(/\.exe$/i, "");
}

export function kindTone(kind) {
  switch (kind) {
    case "FOCUS": return "ok";
    case "TAMPERED": return "bad";
    case "LOCKED": case "UNKNOWN": case "LEGACY": case "SYSTEM": return "warn";
    default: return "muted";
  }
}
