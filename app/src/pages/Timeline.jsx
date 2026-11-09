import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { api } from "../api";
import { useApp, useAsync, useCategoryMap } from "../store";
import { Card, Empty, ErrorNote, KV, Loading, Segmented, Stat } from "../components/ui";
import { Lanes, Ribbon } from "../components/zoomcharts";
import { DAY, appColor, cleanAppName, dayStart, fmtClock, fmtClockSec, fmtDate, fmtDur, fromIsoDay, isoDay } from "../util";

export default function Timeline() {
  const { offset, dark, rev, go, pendingDay, setPendingDay } = useApp();
  const cats = useCategoryMap();
  const [day, setDay] = useState(() => dayStart(pendingDay != null ? pendingDay : Date.now(), offset));
  useEffect(() => { if (pendingDay != null) setPendingDay(null); }, [pendingDay, setPendingDay]);
  const [view, setView] = useState("lanes");
  const [sel, setSel] = useState(null);
  const from = day;
  const to = day + DAY;

  const tl = useAsync(() => api.timeline(from, to), [from, rev]);
  const sessions = useAsync(() => api.focusList(from, to, null, 400), [from, rev]);
  const sum = useAsync(() => api.summary(from, to), [from, rev]);

  useEffect(() => setSel(null), [day]);

  const colorOf = useCallback((sg) => appColor(sg.app, dark), [dark]);
  const isToday = day === dayStart(Date.now(), offset);

  // Lanes: the 10 apps with the most time get their own lane, everything else shares one.
  const lanes = useMemo(() => {
    if (!tl.data || !sum.data) return [];
    const top = sum.data.apps.slice(0, 10).map((a) => a.app);
    const map = new Map(top.map((a) => [a, { app: cleanAppName(a), color: appColor(a, dark), segs: [] }]));
    const rest = { app: "Other", color: dark ? "#8E8AA8" : "#A9A4C2", segs: [] };
    for (const sg of tl.data.segments) (map.get(sg.app) || rest).segs.push(sg);
    const out = top.map((a) => map.get(a)).filter((l) => l.segs.length);
    if (rest.segs.length) out.push(rest);
    return out;
  }, [tl.data, sum.data, dark]);

  const move = (n) => setDay((d) => d + n * DAY);
  const total = sum.data ? sum.data.totals : null;

  return (
    <div className="stack">
      <div className="row between center wrap">
        <div className="row gap center">
          <button className="icon-btn boxed" onClick={() => move(-1)} aria-label="Previous day"><ChevronLeft size={18} /></button>
          <input type="date" className="input date" value={isoDay(day, offset)} max={isoDay(Date.now(), offset)} onChange={(e) => e.target.value && setDay(fromIsoDay(e.target.value, offset))} aria-label="Day" />
          <button className="icon-btn boxed" onClick={() => move(1)} disabled={isToday} aria-label="Next day"><ChevronRight size={18} /></button>
          <button className="btn small" onClick={() => setDay(dayStart(Date.now(), offset))} disabled={isToday}>Today</button>
          <span className="muted">{fmtDate(day, offset, { weekday: "long", year: "numeric" })}</span>
        </div>
        <Segmented small value={view} onChange={setView} options={[{ value: "lanes", label: "Lanes" }, { value: "ribbon", label: "Ribbon" }]} />
      </div>

      {tl.error && <ErrorNote error={tl.error} />}
      {total && (
        <div className="grid-4">
          <Stat label="Active" value={fmtDur(total.active_ms)} />
          <Stat label="First activity" value={total.first_ms ? fmtClock(total.first_ms, offset) : "none"} />
          <Stat label="Last activity" value={total.last_ms ? fmtClock(total.last_ms, offset) : "none"} />
          <Stat label="Focus periods" value={total.focus_periods} sub={`${fmtDur(total.avg_focus_ms)} average`} />
        </div>
      )}

      <Card title={view === "lanes" ? "Day by application" : "Day ribbon"} sub="Gaps are times Hermes was not recording: locked, asleep, paused or off.">
        {!tl.data ? <Loading /> : tl.data.segments.length === 0 ? <Empty title="Nothing recorded on this day" body="Use the arrows to move to another day." /> : view === "lanes" ? (
          <Lanes lanes={lanes} from={from} to={to} offset={offset} selected={sel} onPick={setSel} />
        ) : (
          <Ribbon segments={tl.data.segments} from={from} to={to} offset={offset} colorOf={colorOf} height={96} onPick={setSel} />
        )}
        {tl.data && tl.data.truncated && <p className="muted small">Showing the first 60,000 segments of this day.</p>}
      </Card>

      {sel && (
        <Card title="Selected" actions={<button className="link" onClick={() => setSel(null)}>Clear</button>}>
          <dl className="kv-grid">
            <KV k="Application" v={<button className="link" onClick={() => go("apps", sel.app)}>{cleanAppName(sel.app)}</button>} />
            <KV k="Window title" v={sel.title} />
            <KV k="From" v={fmtClockSec(sel.s, offset)} />
            <KV k="To" v={fmtClockSec(sel.e, offset)} />
            <KV k="Duration" v={fmtDur(sel.e - sel.s)} />
            <KV k="Category" v={cats[sel.category] ? cats[sel.category].name : sel.category} />
          </dl>
        </Card>
      )}

      <Card title="Sessions" sub="Each row is one uninterrupted stay in one window, newest first" pad={false}>
        {sessions.data && sessions.data.length ? (
          <div className="table-scroll">
            <table className="tbl">
              <thead><tr><th>Start</th><th>Application</th><th>Window</th><th className="num">Duration</th></tr></thead>
              <tbody>
                {sessions.data.map((r, i) => (
                  <tr key={i} className="row-click" onClick={() => setSel({ s: r.start_ms, e: r.end_ms, app: r.app, title: r.title, category: r.category })}>
                    <td className="w-time">{fmtClock(r.start_ms, offset)}</td>
                    <td><span className="cell-app"><i className="swatch" style={{ background: appColor(r.app, dark) }} />{cleanAppName(r.app)}</span></td>
                    <td className="ellipsis wide">{r.title || <span className="muted">no title</span>}{r.titles > 1 && <span className="muted small"> · {r.titles} titles</span>}</td>
                    <td className="num">{fmtDur(r.ms)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : <div className="card-b"><p className="muted small">No sessions on this day.</p></div>}
      </Card>
    </div>
  );
}
