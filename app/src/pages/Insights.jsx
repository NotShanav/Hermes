import { useMemo, useState } from "react";
import { api } from "../api";
import { useApp, useAsync } from "../store";
import { Card, ErrorNote, Loading, Segmented, Stat } from "../components/ui";
import { BarList, CalendarHeat, LineCompare, WeekHeat } from "../components/charts";
import { DAY, MIN, appColor, cleanAppName, dayStart, delta, fmtDate, fmtDur, fmtNum, parts } from "../util";

function comparePresets(off) {
  const now = Date.now();
  const t = dayStart(now, off);
  const wd = parts(now, off).wd;
  const p = parts(now, off);
  const wk = t - wd * DAY;
  const m0 = Date.UTC(p.y, p.mo, 1) - off * MIN;
  const pm0 = Date.UTC(p.y, p.mo - 1, 1) - off * MIN;
  return {
    day: { label: "Today vs yesterday", a: { from: t, to: t + DAY }, b: { from: t - DAY, to: t }, an: "Today", bn: "Yesterday" },
    week: { label: "This week vs last week", a: { from: wk, to: t + DAY }, b: { from: wk - 7 * DAY, to: wk - 7 * DAY + (t + DAY - wk) }, an: "This week", bn: "Last week" },
    seven: { label: "Last 7 days vs the 7 before", a: { from: t - 6 * DAY, to: t + DAY }, b: { from: t - 13 * DAY, to: t - 6 * DAY }, an: "Last 7 days", bn: "Previous 7 days" },
    month: { label: "This month vs last month", a: { from: m0, to: t + DAY }, b: { from: pm0, to: pm0 + (t + DAY - m0) }, an: "This month", bn: "Last month" },
  };
}

function streaks(days) {
  const set = new Set(days.filter((d) => d.ms > 60000).map((d) => d.day_ms));
  const sorted = [...set].sort((a, b) => a - b);
  let best = 0, run = 0, prev = null;
  for (const d of sorted) {
    run = prev !== null && d - prev === DAY ? run + 1 : 1;
    best = Math.max(best, run);
    prev = d;
  }
  let cur = 0;
  const today = Math.floor((Date.now()) / DAY) * DAY; // approximate; only used to anchor the current streak
  let d = sorted.length ? sorted[sorted.length - 1] : null;
  if (d !== null) {
    while (set.has(d)) { cur++; d -= DAY; }
  }
  return { best, cur, last: sorted.length ? sorted[sorted.length - 1] : null, today };
}

export default function Insights() {
  const { offset, rev, dark, go, setPendingDay } = useApp();
  const [preset, setPreset] = useState("seven");
  const year = useAsync(() => api.summary(Date.now() - 400 * DAY, Date.now() + DAY), [rev]);
  const P = useMemo(() => comparePresets(offset), [offset]);
  const cmp = P[preset];
  const A = useAsync(() => api.summary(cmp.a.from, cmp.a.to), [preset, rev, offset]);
  const B = useAsync(() => api.summary(cmp.b.from, cmp.b.to), [preset, rev, offset]);
  const heat = useAsync(() => api.heatmap(Date.now() - 90 * DAY, Date.now() + DAY), [rev]);

  const hl = useMemo(() => {
    if (!year.data) return null;
    const days = year.data.days;
    const active = days.filter((d) => d.ms > 0);
    const busiest = active.reduce((m, d) => (d.ms > (m ? m.ms : 0) ? d : m), null);
    const total = active.reduce((a, d) => a + d.ms, 0);
    return { busiest, avg: active.length ? total / active.length : 0, activeDays: active.length, ...streaks(days) };
  }, [year.data]);

  const movers = useMemo(() => {
    if (!A.data || !B.data) return { up: [], down: [] };
    const m = new Map();
    for (const a of A.data.apps) m.set(a.app, { app: a.app, a: a.ms, b: 0 });
    for (const b of B.data.apps) m.set(b.app, { ...(m.get(b.app) || { app: b.app, a: 0 }), b: b.ms });
    const all = [...m.values()].map((x) => ({ ...x, d: x.a - (x.b || 0) }));
    return {
      up: all.filter((x) => x.d > 0).sort((x, y) => y.d - x.d).slice(0, 5),
      down: all.filter((x) => x.d < 0).sort((x, y) => x.d - y.d).slice(0, 5),
    };
  }, [A.data, B.data]);

  const openDay = (dayLocalMs) => {
    // The calendar reports local midnight expressed as UTC; the timeline wants the real UTC instant.
    setPendingDay(dayLocalMs - offset * MIN);
    go("timeline");
  };

  const mk = (items, sign) => items.map((x) => ({ key: x.app, label: cleanAppName(x.app), value: Math.abs(x.d), valueLabel: `${sign}${fmtDur(Math.abs(x.d))}`, color: appColor(x.app, dark), onClick: () => go("apps", x.app) }));

  return (
    <div className="stack">
      <Card title="Year at a glance" sub="Active time per day over the last 53 weeks. Click a day to open it in the timeline.">
        {year.error ? <ErrorNote error={year.error} /> : !year.data ? <Loading /> : <CalendarHeat days={year.data.days} offset={offset} onPick={openDay} />}
      </Card>

      {hl && (
        <div className="grid-4">
          <Stat label="Busiest day" value={hl.busiest ? fmtDur(hl.busiest.ms) : "none"} sub={hl.busiest ? new Date(hl.busiest.day_ms).toLocaleDateString(undefined, { timeZone: "UTC", month: "short", day: "numeric", year: "numeric" }) : ""} />
          <Stat label="Average active day" value={fmtDur(hl.avg)} sub={`${fmtNum(hl.activeDays)} active days in 13 months`} />
          <Stat label="Longest streak" value={`${hl.best} ${hl.best === 1 ? "day" : "days"}`} sub="in a row with activity" />
          <Stat label="Current streak" value={`${hl.cur} ${hl.cur === 1 ? "day" : "days"}`} sub="ending at the latest active day" />
        </div>
      )}

      <Card title="Compare periods" actions={<Segmented small value={preset} onChange={setPreset} options={Object.entries(P).map(([k, v]) => ({ value: k, label: v.label.split(" vs ")[0] }))} />} sub={cmp.label}>
        {!A.data || !B.data ? <Loading /> : (
          <div className="stack">
            <div className="grid-4">
              <Stat label="Active time" value={fmtDur(A.data.totals.active_ms)} delta={delta(A.data.totals.active_ms, B.data.totals.active_ms)} sub={`${cmp.bn}: ${fmtDur(B.data.totals.active_ms)}`} />
              <Stat label="Focus periods" value={fmtNum(A.data.totals.focus_periods)} delta={delta(A.data.totals.focus_periods, B.data.totals.focus_periods)} sub={`${cmp.bn}: ${fmtNum(B.data.totals.focus_periods)}`} />
              <Stat label="Average focus" value={fmtDur(A.data.totals.avg_focus_ms)} delta={delta(A.data.totals.avg_focus_ms, B.data.totals.avg_focus_ms)} sub={`${cmp.bn}: ${fmtDur(B.data.totals.avg_focus_ms)}`} />
              <Stat label="Applications" value={fmtNum(A.data.totals.apps)} delta={delta(A.data.totals.apps, B.data.totals.apps)} sub={`${cmp.bn}: ${fmtNum(B.data.totals.apps)}`} />
            </div>
            <div className="grid-2">
              <div><h4 className="h-sm">Hour-of-day profile</h4><LineCompare a={A.data.hourly} b={B.data.hourly} labelA={cmp.an} labelB={cmp.bn} /></div>
              <div className="grid-2 tight">
                <div><h4 className="h-sm">Used more</h4><BarList items={mk(movers.up, "+")} empty="No increases." /></div>
                <div><h4 className="h-sm">Used less</h4><BarList items={mk(movers.down, "-")} empty="No decreases." /></div>
              </div>
            </div>
          </div>
        )}
      </Card>

      <Card title="Weekly rhythm" sub="Last 90 days, active time by weekday and hour">
        {heat.error ? <ErrorNote error={heat.error} /> : !heat.data ? <Loading /> : <WeekHeat grid={heat.data} />}
      </Card>
    </div>
  );
}
