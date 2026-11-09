import { useCallback, useMemo, useState } from "react";
import { api } from "../api";
import { useApp, useAsync, useCategoryMap } from "../store";
import { Card, Empty, ErrorNote, Loading, Pill, Segmented, Stat } from "../components/ui";
import { BarList, Donut, HourBars } from "../components/charts";
import { Ribbon } from "../components/zoomcharts";
import { DAY, MIN, ago, appColor, cleanAppName, cleanTitle, delta, dayStart, fmtClock, fmtDate, fmtDur, fmtHours, fmtNum, rangeLabel } from "../util";
import { STATE_LABEL, STATE_TONE, useRecorder } from "../hooks";

export default function Overview() {
  const { range, prevRange, rangeKey, custom, offset, dark, rev, go, setRangeKey, info } = useApp();
  const cats = useCategoryMap();
  const [colorBy, setColorBy] = useState("app");

  const cur = useAsync(() => api.summary(range.from, range.to), [range.from, range.to, rev]);
  const prev = useAsync(() => (prevRange ? api.summary(prevRange.from, prevRange.to) : Promise.resolve(null)), [prevRange && prevRange.from, prevRange && prevRange.to, rev]);

  // The ribbon shows the last day of the range that has already started.
  const dayFrom = useMemo(() => dayStart(Math.min(range.to - 1, Date.now()), offset), [range, offset]);
  const dayTo = dayFrom + DAY;
  const tl = useAsync(() => api.timeline(dayFrom, dayTo), [dayFrom, rev]);
  const recent = useAsync(() => api.focusList(range.from, range.to, null, 8), [range.from, range.to, rev]);
  const { rec } = useRecorder(5000);

  const colorOf = useCallback(
    (sg) => (colorBy === "category" ? (cats[sg.category] ? cats[sg.category].color : "#C4B5FD") : appColor(sg.app, dark)),
    [colorBy, cats, dark]
  );

  const s = cur.data;
  const p = prev.data;
  if (cur.error) return <ErrorNote error={cur.error} />;
  if (!s) return <Loading label="Reading your archive" />;
  if (s.totals.active_ms === 0 && rangeKey !== "all") {
    return (
      <div className="stack">
        <Empty
          title={`No activity recorded for ${rangeLabel(rangeKey, range, offset, custom).toLowerCase()}`}
          body="Pick a wider range, or check that the recorder is running."
          action={<div className="row gap center"><button className="btn primary" onClick={() => setRangeKey("30d")}>Show 30 days</button><button className="btn" onClick={() => go("privacy")}>Recording controls</button></div>}
        />
      </div>
    );
  }

  const isDay = range.to - range.from <= DAY;
  const dayLabel = dayFrom === dayStart(Date.now(), offset) ? "Today" : fmtDate(dayFrom, offset, { year: "numeric" });
  const topApps = s.apps.slice(0, 7).map((a) => ({
    key: a.app, label: cleanAppName(a.app), value: a.ms, valueLabel: fmtDur(a.ms), color: appColor(a.app, dark),
    sub: `${fmtNum(a.focus_periods)} focus periods · ${(a.share * 100).toFixed(0)}% of active time`,
    onClick: () => go("apps", a.app),
  }));
  const topTitles = s.titles.slice(0, 6).map((t, i) => ({
    key: i, label: cleanTitle(t.title) || t.title, value: t.ms, valueLabel: fmtDur(t.ms), color: appColor(t.app, dark), sub: cleanAppName(t.app),
  }));
  const catItems = s.categories.map((c) => ({ value: c.ms, color: cats[c.category] ? cats[c.category].color : "#C4B5FD", label: cats[c.category] ? cats[c.category].name : c.category, share: c.share, ms: c.ms }));
  const d = (k) => (p ? delta(s.totals[k], p.totals[k]) : null);
  const legend = s.apps.slice(0, 5);
  const perDay = s.totals.days_active ? s.totals.active_ms / s.totals.days_active : 0;

  return (
    <div className="stack">
      <Card className="hero" pad={false}>
        <div className="hero-h">
          <div>
            <h2>{dayLabel}</h2>
            <p className="muted small">
              {tl.data && tl.data.segments.length ? `${fmtClock(tl.data.segments[0].s, offset)} to ${fmtClock(tl.data.segments[tl.data.segments.length - 1].e, offset)} · hover the ribbon to see what was in front of you` : "No activity on this day"}
            </p>
          </div>
          <div className="row gap center">
            <div className="legend-inline">
              {colorBy === "app"
                ? legend.map((a) => <span key={a.app}><i className="swatch" style={{ background: appColor(a.app, dark) }} />{cleanAppName(a.app)}</span>)
                : s.categories.slice(0, 5).map((c) => <span key={c.category}><i className="swatch" style={{ background: cats[c.category] ? cats[c.category].color : "#C4B5FD" }} />{cats[c.category] ? cats[c.category].name : c.category}</span>)}
            </div>
            <Segmented small value={colorBy} onChange={setColorBy} options={[{ value: "app", label: "By app" }, { value: "category", label: "By category" }]} />
          </div>
        </div>
        <div className="hero-b">
          {tl.data ? <Ribbon segments={tl.data.segments} from={dayFrom} to={dayTo} offset={offset} colorOf={colorOf} height={104} onPick={(sg) => go("apps", sg.app)} /> : <Loading />}
        </div>
      </Card>

      <div className="grid-4">
        <Stat label="Active time" value={fmtDur(s.totals.active_ms)} delta={d("active_ms")} sub={p ? "vs previous period" : isDay ? "" : `${fmtDur(perDay)} per active day`} onClick={() => go("timeline")} hint="Open the timeline" />
        <Stat label="Focus periods" value={fmtNum(s.totals.focus_periods)} delta={d("focus_periods")} sub={`${fmtDur(s.totals.avg_focus_ms)} average`} onClick={() => go("explore")} hint="Browse every event" />
        <Stat label="Longest focus" value={fmtDur(s.totals.longest_ms)} sub="in one window" />
        <Stat label="Applications" value={fmtNum(s.totals.apps)} delta={d("apps")} sub={`${fmtNum(s.totals.days_active)} active ${s.totals.days_active === 1 ? "day" : "days"}`} onClick={() => go("apps")} hint="Open applications" />
      </div>

      <div className="grid-3-2">
        <Card title={isDay ? "Activity by hour" : "When you are active"} sub={isDay ? "Active time in each hour of the day" : "Total active time per hour of day across the range"}>
          <HourBars values={s.hourly} />
        </Card>
        <Card title="Where the time went" sub="By category. Change rules in Categories & rules.">
          {catItems.length ? (
            <div className="donut-row">
              <Donut items={catItems} center={<><b>{fmtHours(s.totals.active_ms)}</b><span className="muted small">active</span></>} />
              <ul className="legend-list">
                {catItems.slice(0, 6).map((c) => (
                  <li key={c.label}><i className="swatch" style={{ background: c.color }} /><span className="ellipsis">{c.label}</span><b>{(c.share * 100).toFixed(0)}%</b></li>
                ))}
              </ul>
            </div>
          ) : <p className="muted small">Nothing to show.</p>}
        </Card>
      </div>

      <div className="grid-3">
        <Card title="Top applications" actions={<button className="link" onClick={() => go("apps")}>See all</button>}>
          <BarList items={topApps} />
        </Card>
        <Card title="Top windows and pages" actions={<button className="link" onClick={() => go("pages")}>See all</button>} sub="Window titles, not web addresses">
          <BarList items={topTitles} empty="No window titles recorded." />
        </Card>
        <Card title="Recorder" actions={<button className="link" onClick={() => go("privacy")}>Controls</button>}>
          {rec ? (
            <div className="recbox">
              <Pill tone={STATE_TONE[rec.state] === "rec" ? "rec" : STATE_TONE[rec.state] === "warn" ? "warn" : STATE_TONE[rec.state] === "off" ? "bad" : "ok"} dot>{STATE_LABEL[rec.state]}</Pill>
              <dl className="mini-kv">
                <div><dt>Events in archive</dt><dd>{fmtNum(rec.stats.events)}</dd></div>
                <div><dt>Last event</dt><dd>{rec.stats.last_ms ? ago(rec.stats.last_ms, Date.now()) : "none"}</dd></div>
                <div><dt>Archive since</dt><dd>{rec.stats.first_ms ? fmtDate(rec.stats.first_ms, offset, { year: "numeric" }) : "none"}</dd></div>
              </dl>
              {rec.live && rec.live.warnings.length > 0 && <div className="notice warn small">{rec.live.warnings[0]}</div>}
              {!rec.running && <div className="notice warn small">The recorder is not running, so new activity is not being captured.</div>}
            </div>
          ) : <Loading />}
        </Card>
      </div>

      <Card title="Recent focus" sub="Most recent stays in a single window within the range" actions={<button className="link" onClick={() => go("timeline")}>Open timeline</button>}>
        {recent.data && recent.data.length ? (
          <table className="tbl compact">
            <tbody>
              {recent.data.map((r, i) => (
                <tr key={i} className="row-click" onClick={() => go("apps", r.app)}>
                  <td className="w-time">{fmtDate(r.start_ms, offset)} {fmtClock(r.start_ms, offset)}</td>
                  <td><span className="cell-app"><i className="swatch" style={{ background: appColor(r.app, dark) }} />{cleanAppName(r.app)}</span></td>
                  <td className="ellipsis wide">{cleanTitle(r.title) || <span className="muted">no title</span>}</td>
                  <td className="num">{fmtDur(r.ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : <p className="muted small">No recent activity.</p>}
      </Card>
    </div>
  );
}
