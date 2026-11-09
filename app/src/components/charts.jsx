import { useEffect, useMemo, useRef, useState } from "react";
import { DAY, HOUR, MIN, fmtClock, fmtDate, fmtDur, parts } from "../util";

export function useWidth(ref) {
  const [w, setW] = useState(640);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    setW(Math.max(120, Math.floor(el.clientWidth)));
    const ro = new ResizeObserver((es) => setW(Math.max(120, Math.floor(es[0].contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return w;
}

/** 24 bars, one per local hour. */
export function HourBars({ values, color = "var(--accent)", height = 132, compare }) {
  const [hi, setHi] = useState(null);
  const max = Math.max(1, ...values, ...(compare || []));
  const peak = values.indexOf(Math.max(...values));
  return (
    <div className="hourbars">
      <div className="readout muted small">
        {hi != null ? (
          <>
            <b>{String(hi).padStart(2, "0")}:00 to {String((hi + 1) % 24).padStart(2, "0")}:00</b> · {fmtDur(values[hi])}
          </>
        ) : Math.max(...values) > 0 ? (
          <>Busiest hour <b>{String(peak).padStart(2, "0")}:00</b> · {fmtDur(values[peak])}</>
        ) : (
          "No activity in this range"
        )}
      </div>
      <div className="bars" style={{ height }} onMouseLeave={() => setHi(null)}>
        {values.map((v, i) => (
          <div key={i} className={`bar-col ${hi === i ? "on" : ""}`} onMouseEnter={() => setHi(i)}>
            {compare && <i className="bar ghost" style={{ height: `${(compare[i] / max) * 100}%` }} />}
            <i className="bar" style={{ height: `${Math.max(v > 0 ? 2 : 0, (v / max) * 100)}%`, background: color, opacity: hi == null || hi === i ? 1 : 0.55 }} />
          </div>
        ))}
      </div>
      <div className="bar-labels">
        {values.map((_, i) => (
          <span key={i}>{i % 3 === 0 ? String(i).padStart(2, "0") : ""}</span>
        ))}
      </div>
    </div>
  );
}

/** Active time per day across a range. `days` come from the backend; gaps are filled with zero. */
export function DayBars({ days, from, to, offset, height = 132, onPick }) {
  const [hi, setHi] = useState(null);
  const series = useMemo(() => {
    const map = new Map(days.map((d) => [d.day_ms, d]));
    const out = [];
    const nowLocal = Math.floor((Date.now() + offset * MIN) / DAY) * DAY;
    const rangeStart = Math.floor((from + offset * MIN) / DAY) * DAY;
    const rangeEnd = Math.floor((to - 1 + offset * MIN) / DAY) * DAY;
    const dataStart = days.length ? days[0].day_ms : rangeStart;
    const endLocal = Math.min(rangeEnd, nowLocal);
    const startLocal = Math.max(from <= 0 ? dataStart : rangeStart, endLocal - 399 * DAY);
    for (let d = startLocal; d <= endLocal; d += DAY) out.push({ day_ms: d, ms: map.get(d)?.ms || 0 });
    return out;
  }, [days, from, to, offset]);
  const max = Math.max(1, ...series.map((s) => s.ms));
  const labelEvery = Math.max(1, Math.ceil(series.length / 8));
  const cur = hi != null ? series[hi] : null;
  return (
    <div className="hourbars">
      <div className="readout muted small">
        {cur ? (
          <>
            <b>{new Date(cur.day_ms).toLocaleDateString(undefined, { timeZone: "UTC", weekday: "short", month: "short", day: "numeric" })}</b> · {fmtDur(cur.ms)}
          </>
        ) : (
          "Hover a day for details"
        )}
      </div>
      <div className="bars" style={{ height }} onMouseLeave={() => setHi(null)}>
        {series.map((s, i) => (
          <div key={s.day_ms} className={`bar-col ${hi === i ? "on" : ""}`} onMouseEnter={() => setHi(i)} onClick={() => onPick && onPick(s.day_ms)}>
            <i className="bar" style={{ height: `${Math.max(s.ms > 0 ? 2 : 0, (s.ms / max) * 100)}%`, opacity: hi == null || hi === i ? 1 : 0.55 }} />
          </div>
        ))}
      </div>
      <div className="bar-labels" style={{ gridTemplateColumns: `repeat(${series.length}, 1fr)` }}>
        {series.map((s, i) => (
          <span key={s.day_ms}>{i % labelEvery === 0 ? new Date(s.day_ms).toLocaleDateString(undefined, { timeZone: "UTC", month: "short", day: "numeric" }) : ""}</span>
        ))}
      </div>
    </div>
  );
}

export function Donut({ items, size = 148, thickness = 16, center }) {
  const r = (size - thickness) / 2;
  const c = 2 * Math.PI * r;
  let acc = 0;
  const total = items.reduce((a, b) => a + b.value, 0) || 1;
  return (
    <div className="donut" style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--panel-2)" strokeWidth={thickness} />
        {items.map((it, i) => {
          const len = (it.value / total) * c;
          const el = (
            <circle key={i} cx={size / 2} cy={size / 2} r={r} fill="none" stroke={it.color} strokeWidth={thickness} strokeDasharray={`${Math.max(0, len - 2)} ${c - Math.max(0, len - 2)}`} strokeDashoffset={-acc} transform={`rotate(-90 ${size / 2} ${size / 2})`} />
          );
          acc += len;
          return el;
        })}
      </svg>
      <div className="donut-c">{center}</div>
    </div>
  );
}

export function BarList({ items, max, empty = "Nothing recorded in this range." }) {
  if (!items.length) return <p className="muted small">{empty}</p>;
  const m = max || Math.max(1, ...items.map((i) => i.value));
  return (
    <ul className="barlist">
      {items.map((it, i) => (
        <li key={it.key || i}>
          <button className="bl-row" onClick={it.onClick} disabled={!it.onClick}>
            <span className="bl-top">
              <span className="bl-label">
                {it.color && <i className="swatch" style={{ background: it.color }} />}
                <span className="ellipsis">{it.label}</span>
              </span>
              <span className="bl-val">{it.valueLabel}</span>
            </span>
            <span className="bl-bar">
              <i style={{ width: `${Math.max(1.5, (it.value / m) * 100)}%`, background: it.color || "var(--accent)" }} />
            </span>
            {it.sub && <span className="muted small ellipsis">{it.sub}</span>}
          </button>
        </li>
      ))}
    </ul>
  );
}

/** GitHub-style calendar of daily active time (Monday first). */
export function CalendarHeat({ days, offset, weeks = 53, onPick }) {
  const [hov, setHov] = useState(null);
  const { cells, months, levelOf } = useMemo(() => {
    const map = new Map(days.map((d) => [d.day_ms, d.ms]));
    const today = Math.floor((Date.now() + offset * MIN) / DAY) * DAY;
    const wd = (new Date(today).getUTCDay() + 6) % 7;
    const start = today - wd * DAY - (weeks - 1) * 7 * DAY;
    const vals = [...map.values()].filter((v) => v > 0).sort((a, b) => a - b);
    const q = (p) => (vals.length ? vals[Math.min(vals.length - 1, Math.floor(p * vals.length))] : 1);
    const t = [q(0.25), q(0.5), q(0.75)];
    const lvl = (v) => (v <= 0 ? 0 : v <= t[0] ? 1 : v <= t[1] ? 2 : v <= t[2] ? 3 : 4);
    const cells = [];
    const months = [];
    let lastM = -1;
    for (let i = 0; i < weeks * 7; i++) {
      const d = start + i * DAY;
      const dt = new Date(d);
      if (i % 7 === 0 && dt.getUTCMonth() !== lastM) {
        months.push({ col: i / 7, label: dt.toLocaleDateString(undefined, { timeZone: "UTC", month: "short" }) });
        lastM = dt.getUTCMonth();
      }
      cells.push({ d, ms: map.get(d) || 0, future: d > today });
    }
    return { cells, months, levelOf: lvl };
  }, [days, offset, weeks]);
  return (
    <div className="calendar">
      <div className="cal-months" style={{ gridTemplateColumns: `repeat(${weeks}, 1fr)` }}>
        {months.map((m) => (
          <span key={m.col} style={{ gridColumnStart: m.col + 1 }}>{m.label}</span>
        ))}
      </div>
      <div className="cal-grid" style={{ gridTemplateColumns: `repeat(${weeks}, 1fr)` }}>
        {cells.map((c) =>
          c.future ? (
            <i key={c.d} className="cal-cell future" />
          ) : (
            <button key={c.d} className={`cal-cell l${levelOf(c.ms)}`} onMouseEnter={() => setHov(c)} onMouseLeave={() => setHov(null)} onClick={() => onPick && onPick(c.d)} aria-label={`${new Date(c.d).toDateString()} ${fmtDur(c.ms)}`} />
          )
        )}
      </div>
      <div className="cal-foot muted small">
        <span>{hov ? `${new Date(hov.d).toLocaleDateString(undefined, { timeZone: "UTC", weekday: "short", month: "short", day: "numeric", year: "numeric" })} · ${hov.ms ? fmtDur(hov.ms) : "no activity"}` : "Hover a day. Click to open it."}</span>
        <span className="cal-legend">Less {[0, 1, 2, 3, 4].map((l) => <i key={l} className={`cal-cell l${l}`} />)} More</span>
      </div>
    </div>
  );
}

const WD = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
/** 7 x 24 grid: weekday by hour. */
export function WeekHeat({ grid }) {
  const [hov, setHov] = useState(null);
  const max = Math.max(1, ...grid.flat());
  return (
    <div className="weekheat">
      <div className="wh-cols">
        {Array.from({ length: 24 }, (_, h) => (
          <span key={h}>{h % 3 === 0 ? String(h).padStart(2, "0") : ""}</span>
        ))}
      </div>
      {grid.map((row, d) => (
        <div className="wh-row" key={d}>
          <span className="wh-label">{WD[d]}</span>
          <div className="wh-cells">
            {row.map((v, h) => (
              <i key={h} className="wh-cell" style={{ opacity: v ? 0.14 + 0.86 * (v / max) : 0.05 }} onMouseEnter={() => setHov({ d, h, v })} onMouseLeave={() => setHov(null)} />
            ))}
          </div>
        </div>
      ))}
      <div className="muted small wh-foot">{hov ? `${WD[hov.d]} ${String(hov.h).padStart(2, "0")}:00 · ${fmtDur(hov.v)} in total` : "Total active time by weekday and hour in this range."}</div>
    </div>
  );
}

/** Two 24-hour profiles drawn as lines (current solid, previous dashed). */
export function LineCompare({ a, b, labelA, labelB }) {
  const max = Math.max(1, ...a, ...b);
  const pts = (arr) => arr.map((v, i) => `${(i / 23) * 100},${100 - (v / max) * 92 - 4}`).join(" ");
  return (
    <div className="linecompare">
      <svg viewBox="0 0 100 100" preserveAspectRatio="none">
        {[25, 50, 75].map((y) => <line key={y} x1="0" x2="100" y1={y} y2={y} className="grid" vectorEffect="non-scaling-stroke" />)}
        <polyline points={pts(b)} className="prev" vectorEffect="non-scaling-stroke" fill="none" />
        <polyline points={pts(a)} className="cur" vectorEffect="non-scaling-stroke" fill="none" />
      </svg>
      <div className="bar-labels">
        {Array.from({ length: 24 }, (_, i) => <span key={i}>{i % 3 === 0 ? String(i).padStart(2, "0") : ""}</span>)}
      </div>
      <div className="legend small muted">
        <span><i className="ln cur" /> {labelA}</span>
        <span><i className="ln prev" /> {labelB}</span>
      </div>
    </div>
  );
}
