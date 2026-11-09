import { useCallback, useEffect, useRef, useState } from "react";
import { Crosshair, Maximize, ZoomIn, ZoomOut } from "lucide-react";
import { useWidth } from "./charts";
import { DAY, HOUR, MIN, fmtClock, fmtClockSec, fmtDate, fmtDur } from "../util";

const MIN_SPAN = 15000; // the closest you can zoom: 15 seconds across the whole width
const GUTTER = 118; // lane-name column in the timeline lanes

function rr(g, x, y, w, h, r) {
  const rad = Math.max(0, Math.min(r, w / 2, h / 2));
  g.beginPath();
  g.moveTo(x + rad, y);
  g.arcTo(x + w, y, x + w, y + h, rad);
  g.arcTo(x + w, y + h, x, y + h, rad);
  g.arcTo(x, y + h, x, y, rad);
  g.arcTo(x, y, x + w, y, rad);
  g.closePath();
}
const cssVar = (n, fb) => getComputedStyle(document.documentElement).getPropertyValue(n).trim() || fb;

/** Index of the segment containing t (or within tol ms of it); segments must be sorted by start. */
function findSeg(segs, t, tol = 0) {
  let lo = 0, hi = segs.length - 1, best = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (segs[mid].s <= t) { best = mid; lo = mid + 1; } else hi = mid - 1;
  }
  if (best >= 0 && segs[best].e + tol > t) return best;
  if (best + 1 < segs.length && segs[best + 1].s - tol <= t) return best + 1;
  return -1;
}

const STEPS = [10000, 30000, MIN, 5 * MIN, 10 * MIN, 15 * MIN, 30 * MIN, HOUR, 2 * HOUR, 3 * HOUR, 6 * HOUR, DAY];

/** Tick labels that adapt to the zoom level: seconds when zoomed right in, hours when zoomed out. */
export function TimeAxis({ from, to, offset, left = 0 }) {
  const span = to - from;
  const step = STEPS.find((s) => span / s <= 10) || DAY;
  const first = Math.ceil((from + offset * MIN) / step) * step - offset * MIN;
  const ticks = [];
  for (let t = first; t < to && ticks.length < 40; t += step) ticks.push(t);
  return (
    <div className="axis" style={{ marginLeft: left }}>
      {ticks.map((t) => (
        <span key={t} style={{ left: `${((t - from) / span) * 100}%` }}>
          {step >= DAY ? fmtDate(t, offset, { weekday: undefined }) : step < MIN ? fmtClockSec(t, offset) : fmtClock(t, offset)}
        </span>
      ))}
    </div>
  );
}

function Tip({ x, w, children }) {
  const flip = x > w * 0.6;
  return (
    <div className="tip" style={{ left: x, transform: flip ? "translateX(calc(-100% - 12px))" : "translateX(12px)" }}>
      {children}
    </div>
  );
}

/** The visible window [a, b] inside the full range [from, to]. Resets when the range changes (another day). */
function useViewport(from, to) {
  const [view, setView] = useState({ a: from, b: to });
  useEffect(() => setView({ a: from, b: to }), [from, to]);
  const full = to - from;
  const clampView = useCallback(
    (a, s) => {
      const size = Math.max(Math.min(MIN_SPAN, full), Math.min(full, s));
      const start = Math.max(from, Math.min(a, to - size));
      return { a: start, b: start + size };
    },
    [from, to, full]
  );
  const zoomAt = useCallback(
    (factor, frac) =>
      setView((v) => {
        const s = v.b - v.a;
        const ns = s * factor;
        const anchor = v.a + s * frac; // the instant under the cursor stays under the cursor
        return clampView(anchor - ns * frac, ns);
      }),
    [clampView]
  );
  const set = useCallback((a, b) => setView(clampView(a, b - a)), [clampView]);
  const reset = useCallback(() => setView({ a: from, b: to }), [from, to]);
  return { view, zoomAt, set, reset, zoomed: view.b - view.a < full - 1000, full };
}

function ZoomBar({ vp, onFit }) {
  const span = vp.view.b - vp.view.a;
  return (
    <div className="zoom-bar">
      <span className="muted small">
        {vp.zoomed ? `Showing ${fmtDur(span)}. Drag to move, double-click to reset.` : "Scroll to zoom in, then drag to move."}
      </span>
      <div className="zoom-ctl">
        <button className="icon-btn boxed" onClick={() => vp.zoomAt(1.6, 0.5)} disabled={!vp.zoomed} aria-label="Zoom out" title="Zoom out"><ZoomOut size={15} /></button>
        <button className="icon-btn boxed" onClick={() => vp.zoomAt(0.6, 0.5)} aria-label="Zoom in" title="Zoom in"><ZoomIn size={15} /></button>
        <button className="icon-btn boxed" onClick={onFit} aria-label="Fit to the active hours" title="Fit to the active hours"><Crosshair size={15} /></button>
        <button className="icon-btn boxed" onClick={vp.reset} disabled={!vp.zoomed} aria-label="Show the whole day" title="Show the whole day"><Maximize size={15} /></button>
      </div>
    </div>
  );
}

/** Overview strip shown while zoomed: the whole range with the visible window outlined; click or drag to move. */
function MiniMap({ segments, from, to, vp, colorOf, w, left = 0 }) {
  const cv = useRef(null);
  const down = useRef(false);
  const plotW = Math.max(10, w - left);
  const full = to - from;
  const H = 24;
  useEffect(() => {
    const c = cv.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = w * dpr;
    c.height = H * dpr;
    const g = c.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, H);
    g.fillStyle = cssVar("--panel-2", "#181430");
    rr(g, left, 0, plotW, H, 6);
    g.fill();
    for (const sg of segments) {
      const x = left + ((sg.s - from) / full) * plotW;
      const x2 = left + ((sg.e - from) / full) * plotW;
      g.fillStyle = colorOf(sg);
      g.globalAlpha = 0.8;
      g.fillRect(x, 5, Math.max(x2 - x, 1), H - 10);
    }
    g.globalAlpha = 1;
    const vx = left + ((vp.view.a - from) / full) * plotW;
    const vw = Math.max(6, ((vp.view.b - vp.view.a) / full) * plotW);
    g.fillStyle = cssVar("--accent", "#8f73f5");
    g.globalAlpha = 0.2;
    g.fillRect(vx, 0, vw, H);
    g.globalAlpha = 1;
    g.strokeStyle = cssVar("--accent-2", "#c4b0ff");
    g.lineWidth = 2;
    rr(g, vx + 1, 1, Math.max(4, vw - 2), H - 2, 5);
    g.stroke();
  }, [segments, from, full, w, left, plotW, colorOf, vp.view.a, vp.view.b]);
  const center = (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    const t = from + ((e.clientX - r.left - left) / plotW) * full;
    const s = vp.view.b - vp.view.a;
    vp.set(t - s / 2, t + s / 2);
  };
  return (
    <canvas
      ref={cv}
      className="minimap"
      style={{ width: w, height: H }}
      onPointerDown={(e) => { down.current = true; e.currentTarget.setPointerCapture(e.pointerId); center(e); }}
      onPointerMove={(e) => down.current && center(e)}
      onPointerUp={() => (down.current = false)}
      onPointerCancel={() => (down.current = false)}
    />
  );
}

/** Wheel zoom (anchored at the cursor, page scroll is not trapped elsewhere) and drag-to-pan for a chart stage. */
function useStage(vp, left, plotW) {
  const stage = useRef(null);
  const drag = useRef(null);
  useEffect(() => {
    const el = stage.current;
    if (!el) return;
    const h = (e) => {
      e.preventDefault();
      const r = el.getBoundingClientRect();
      const frac = Math.max(0, Math.min(1, (e.clientX - r.left - left) / plotW));
      vp.zoomAt(e.deltaY < 0 ? 0.78 : 1.28, frac);
    };
    el.addEventListener("wheel", h, { passive: false });
    return () => el.removeEventListener("wheel", h);
  }, [vp.zoomAt, left, plotW]);
  return { stage, drag };
}

function SegTip({ seg, offset, x, w }) {
  return (
    <Tip x={x} w={w}>
      <b>{seg.app.replace(/\.exe$/i, "")}</b>
      {seg.title && <span className="tip-t">{seg.title}</span>}
      <span className="muted">
        {fmtClockSec(seg.s, offset)} to {fmtClockSec(seg.e, offset)} · {fmtDur(seg.e - seg.s)}
      </span>
    </Tip>
  );
}

/** The day ribbon: one continuous band coloured by application or category. Zoom, pan, minimap. */
export function Ribbon({ segments, from, to, offset, colorOf, height = 92, onPick }) {
  const wrap = useRef(null);
  const cv = useRef(null);
  const w = useWidth(wrap);
  const vp = useViewport(from, to);
  const { stage, drag } = useStage(vp, 0, w);
  const [hov, setHov] = useState(null);
  const { a, b } = vp.view;
  const span = b - a;

  useEffect(() => {
    const c = cv.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = w * dpr;
    c.height = height * dpr;
    const g = c.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, height);
    g.fillStyle = cssVar("--panel-2", "#181430");
    rr(g, 0, 0, w, height, 12);
    g.fill();
    g.save();
    rr(g, 0, 0, w, height, 12);
    g.clip();
    const ink = document.documentElement.dataset.theme === "light" ? "#ffffff" : "#0a0813";
    g.font = '700 12px "Manrope Variable", "Segoe UI", sans-serif';
    g.textBaseline = "middle";
    for (const sg of segments) {
      if (sg.e < a || sg.s > b) continue;
      const x = ((sg.s - a) / span) * w;
      const x2 = ((sg.e - a) / span) * w;
      const ww = Math.max(x2 - x, 1);
      g.fillStyle = colorOf(sg);
      g.fillRect(x, 8, ww > 3 ? ww - 0.75 : ww, height - 16);
      if (ww > 74) {
        // Name inside blocks that are wide enough, clipped to the block so text never spills out.
        g.save();
        g.beginPath();
        g.rect(Math.max(x, 0) + 6, 8, Math.min(x2, w) - Math.max(x, 0) - 10, height - 16);
        g.clip();
        g.fillStyle = ink;
        g.fillText(sg.app.replace(/\.exe$/i, ""), Math.max(x, 0) + 8, height / 2);
        g.restore();
      }
    }
    g.restore();
  }, [segments, a, b, span, w, height, colorOf]);

  const fit = () => {
    if (!segments.length) return;
    const s = segments[0].s;
    const e = segments[segments.length - 1].e;
    const pad = Math.max(5 * MIN, (e - s) * 0.04);
    vp.set(s - pad, e + pad);
  };

  const onDown = (e) => {
    if (e.button !== 0) return;
    drag.current = { x: e.clientX, a: vp.view.a, s: vp.view.b - vp.view.a, moved: false };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onMove = (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - r.left;
    const d = drag.current;
    if (d) {
      const dx = e.clientX - d.x;
      if (Math.abs(dx) > 3) d.moved = true;
      if (d.moved && vp.zoomed) {
        const na = d.a - (dx / w) * d.s;
        vp.set(na, na + d.s);
        setHov(null);
        return;
      }
    }
    const t = a + (x / w) * span;
    const i = findSeg(segments, t, (4 / w) * span);
    setHov({ x, seg: i >= 0 ? segments[i] : null });
  };
  const onUp = () => {
    const d = drag.current;
    drag.current = null;
    if (d && !d.moved && hov && hov.seg && onPick) onPick(hov.seg);
  };

  return (
    <div className="chart-wrap" ref={wrap}>
      <ZoomBar vp={vp} onFit={fit} />
      <div ref={stage} className={`ribbon ${vp.zoomed ? "zoomed" : ""}`} style={{ height }} onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onPointerCancel={onUp} onPointerLeave={() => setHov(null)} onDoubleClick={vp.reset}>
        <canvas ref={cv} style={{ width: w, height }} />
        {hov && <i className="cursor-line" style={{ left: hov.x }} />}
        {hov && hov.seg && <SegTip seg={hov.seg} offset={offset} x={hov.x} w={w} />}
      </div>
      <TimeAxis from={a} to={b} offset={offset} />
      {vp.zoomed && <MiniMap segments={segments} from={from} to={to} vp={vp} colorOf={colorOf} w={w} />}
    </div>
  );
}

/** One lane per application, with the application names in a left column. Zoom, pan, minimap. */
export function Lanes({ lanes, from, to, offset, laneH = 32, selected, onPick }) {
  const wrap = useRef(null);
  const cv = useRef(null);
  const w = useWidth(wrap);
  const plotW = Math.max(40, w - GUTTER);
  const vp = useViewport(from, to);
  const { stage, drag } = useStage(vp, GUTTER, plotW);
  const [hov, setHov] = useState(null);
  const { a, b } = vp.view;
  const span = b - a;
  const h = Math.max(1, lanes.length) * laneH;

  useEffect(() => {
    const c = cv.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = w * dpr;
    c.height = h * dpr;
    const g = c.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    const line = cssVar("--line", "#262040");
    const lane2 = cssVar("--panel-2", "#181430");
    const muted = cssVar("--text-2", "#a8a2ca");
    lanes.forEach((ln, i) => {
      if (i % 2 === 0) {
        g.globalAlpha = 0.55;
        g.fillStyle = lane2;
        g.fillRect(0, i * laneH, w, laneH);
        g.globalAlpha = 1;
      }
    });
    // grid lines follow the zoom level
    const step = STEPS.find((s) => span / s <= 10) || DAY;
    g.strokeStyle = line;
    g.lineWidth = 1;
    const first = Math.ceil((a + offset * MIN) / step) * step - offset * MIN;
    for (let t = first; t < b; t += step) {
      const x = Math.round(GUTTER + ((t - a) / span) * plotW) + 0.5;
      g.beginPath(); g.moveTo(x, 0); g.lineTo(x, h); g.stroke();
    }
    // segments, clipped to the plot area so they never run under the names
    g.save();
    g.beginPath();
    g.rect(GUTTER, 0, plotW, h);
    g.clip();
    lanes.forEach((ln, i) => {
      for (const sg of ln.segs) {
        if (sg.e < a || sg.s > b) continue;
        const x = GUTTER + ((sg.s - a) / span) * plotW;
        const x2 = GUTTER + ((sg.e - a) / span) * plotW;
        const ww = Math.max(x2 - x, 1.5);
        const y = i * laneH + 5;
        if (selected && selected.s === sg.s && selected.app === sg.app) {
          g.fillStyle = "#ffffff";
          g.fillRect(x - 1, y - 2, ww + 2, laneH - 6);
        }
        g.fillStyle = ln.color;
        rr(g, x, y, ww, laneH - 10, ww > 6 ? 3 : 0);
        g.fill();
      }
    });
    g.restore();
    // lane names, truncated to the column
    g.font = '600 12px "Manrope Variable", "Segoe UI", sans-serif';
    g.textBaseline = "middle";
    lanes.forEach((ln, i) => {
      g.save();
      g.beginPath();
      g.rect(0, i * laneH, GUTTER - 10, laneH);
      g.clip();
      g.fillStyle = ln.color;
      g.fillRect(6, i * laneH + laneH / 2 - 4, 8, 8);
      g.fillStyle = muted;
      g.fillText(ln.app, 22, i * laneH + laneH / 2 + 1);
      g.restore();
    });
  }, [lanes, a, b, span, w, h, laneH, offset, selected, plotW]);

  const find = (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - r.left;
    const y = e.clientY - r.top;
    const ln = lanes[Math.floor(y / laneH)];
    if (!ln || x < GUTTER) return { x, seg: null };
    const t = a + ((x - GUTTER) / plotW) * span;
    const i = findSeg(ln.segs, t, (4 / plotW) * span);
    return { x, seg: i >= 0 ? ln.segs[i] : null };
  };
  const fit = () => {
    let s = Infinity, e = -Infinity;
    for (const ln of lanes) for (const sg of ln.segs) { s = Math.min(s, sg.s); e = Math.max(e, sg.e); }
    if (s === Infinity) return;
    const pad = Math.max(5 * MIN, (e - s) * 0.04);
    vp.set(s - pad, e + pad);
  };
  const allSegs = useRefAll(lanes);

  const onDown = (e) => {
    if (e.button !== 0) return;
    drag.current = { x: e.clientX, a: vp.view.a, s: vp.view.b - vp.view.a, moved: false };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onMove = (e) => {
    const d = drag.current;
    if (d) {
      const dx = e.clientX - d.x;
      if (Math.abs(dx) > 3) d.moved = true;
      if (d.moved && vp.zoomed) {
        const na = d.a - (dx / plotW) * d.s;
        vp.set(na, na + d.s);
        setHov(null);
        return;
      }
    }
    setHov(find(e));
  };
  const onUp = () => {
    const d = drag.current;
    drag.current = null;
    if (d && !d.moved && hov && hov.seg && onPick) onPick(hov.seg);
  };

  return (
    <div className="chart-wrap" ref={wrap}>
      <ZoomBar vp={vp} onFit={fit} />
      <div ref={stage} className={`lanes ${vp.zoomed ? "zoomed" : ""}`} style={{ height: h }} onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onPointerCancel={onUp} onPointerLeave={() => setHov(null)} onDoubleClick={vp.reset}>
        <canvas ref={cv} style={{ width: w, height: h }} />
        {hov && hov.seg && <SegTip seg={hov.seg} offset={offset} x={hov.x} w={w} />}
      </div>
      <TimeAxis from={a} to={b} offset={offset} left={GUTTER} />
      {vp.zoomed && <MiniMap segments={allSegs} from={from} to={to} vp={vp} colorOf={() => cssVar("--text-2", "#a8a2ca")} w={w} left={GUTTER} />}
    </div>
  );
}

// All lane segments in one time-ordered list, for the minimap.
function useRefAll(lanes) {
  const ref = useRef({ lanes: null, list: [] });
  if (ref.current.lanes !== lanes) {
    ref.current = { lanes, list: lanes.flatMap((l) => l.segs).sort((x, y) => x.s - y.s) };
  }
  return ref.current.list;
}
