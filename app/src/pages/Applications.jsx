import { useMemo, useState } from "react";
import { ArrowDown, ArrowUp, Search } from "lucide-react";
import { api } from "../api";
import { useApp, useAsync, useCategoryMap } from "../store";
import { Card, Empty, ErrorNote, Loading, Pill, Stat } from "../components/ui";
import { BarList, DayBars, HourBars, WeekHeat } from "../components/charts";
import { ago, appColor, cleanAppName, cleanTitle, fmtClock, fmtDate, fmtDur, fmtNum } from "../util";

function List() {
  const { range, rev, go, dark } = useApp();
  const cats = useCategoryMap();
  const [q, setQ] = useState("");
  const [cat, setCat] = useState("all");
  const [sort, setSort] = useState({ key: "ms", dir: -1 });
  const sum = useAsync(() => api.summary(range.from, range.to), [range.from, range.to, rev]);

  const rows = useMemo(() => {
    if (!sum.data) return [];
    let r = sum.data.apps.filter((a) => (cat === "all" || a.category === cat) && cleanAppName(a.app).toLowerCase().includes(q.trim().toLowerCase()));
    const k = sort.key;
    r = [...r].sort((a, b) => (k === "app" ? a.app.localeCompare(b.app) * sort.dir : ((a[k] || 0) - (b[k] || 0)) * sort.dir));
    return r;
  }, [sum.data, q, cat, sort]);

  if (sum.error) return <ErrorNote error={sum.error} />;
  if (!sum.data) return <Loading />;
  const used = sum.data.categories.map((c) => c.category);
  const max = Math.max(1, ...sum.data.apps.map((a) => a.ms));
  const Th = ({ k, children, num }) => (
    <th className={`sortable ${num ? "num" : ""}`} onClick={() => setSort((s) => ({ key: k, dir: s.key === k ? -s.dir : -1 }))} aria-sort={sort.key === k ? (sort.dir < 0 ? "descending" : "ascending") : "none"}>
      {children}
      {sort.key === k && (sort.dir < 0 ? <ArrowDown size={12} /> : <ArrowUp size={12} />)}
    </th>
  );

  return (
    <div className="stack">
      <div className="row between center wrap">
        <div className="chips">
          <button className={`chip ${cat === "all" ? "on" : ""}`} onClick={() => setCat("all")}>All</button>
          {used.map((c) => (
            <button key={c} className={`chip ${cat === c ? "on" : ""}`} onClick={() => setCat(c)}>
              <i className="swatch" style={{ background: cats[c] ? cats[c].color : "#C4B5FD" }} />
              {cats[c] ? cats[c].name : c}
            </button>
          ))}
        </div>
        <label className="search-in"><Search size={15} /><input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter applications" aria-label="Filter applications" /></label>
      </div>
      <Card pad={false}>
        {rows.length === 0 ? <div className="card-b"><Empty title="No applications match" body="Try another category or a wider date range." /></div> : (
          <div className="table-scroll">
            <table className="tbl">
              <thead>
                <tr>
                  <Th k="app">Application</Th><th>Category</th><Th k="ms" num>Time</Th><th className="share-h">Share</th>
                  <Th k="focus_periods" num>Focus periods</Th><Th k="avg_ms" num>Average</Th><Th k="longest_ms" num>Longest</Th><Th k="days" num>Days</Th><Th k="last_ms" num>Last used</Th>
                </tr>
              </thead>
              <tbody>
                {rows.map((a) => (
                  <tr key={a.app} className="row-click" onClick={() => go("apps", a.app)}>
                    <td><span className="cell-app"><i className="swatch" style={{ background: appColor(a.app, dark) }} />{cleanAppName(a.app)}</span></td>
                    <td><Pill>{cats[a.category] ? cats[a.category].name : a.category}</Pill></td>
                    <td className="num strong">{fmtDur(a.ms)}</td>
                    <td className="share"><span className="bl-bar thin"><i style={{ width: `${(a.ms / max) * 100}%`, background: appColor(a.app, dark) }} /></span><span className="muted small">{(a.share * 100).toFixed(1)}%</span></td>
                    <td className="num">{fmtNum(a.focus_periods)}</td>
                    <td className="num">{fmtDur(a.avg_ms)}</td>
                    <td className="num">{fmtDur(a.longest_ms)}</td>
                    <td className="num">{a.days}</td>
                    <td className="num muted">{ago(a.last_ms, Date.now())}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}

function Detail({ app }) {
  const { range, rev, offset, dark, go } = useApp();
  const cats = useCategoryMap();
  const d = useAsync(() => api.appDetail(app, range.from, range.to), [app, range.from, range.to, rev]);
  if (d.error) return <ErrorNote error={d.error} />;
  if (!d.data) return <Loading />;
  const s = d.data.summary;
  const row = s.apps[0];
  if (!row) return <Empty title={`${cleanAppName(app)} has no activity in this range`} body="Pick a wider range from the top bar." action={<button className="btn" onClick={() => go("apps")}>Back to applications</button>} />;
  const titles = s.titles.slice(0, 8).map((t, i) => ({ key: i, label: cleanTitle(t.title) || t.title, value: t.ms, valueLabel: fmtDur(t.ms), color: appColor(app, dark), sub: `${fmtNum(t.visits)} ${t.visits === 1 ? "visit" : "visits"}` }));
  return (
    <div className="stack">
      <div className="row gap center">
        <i className="swatch big" style={{ background: appColor(app, dark) }} />
        <div>
          <h2 className="h-lg">{cleanAppName(app)}</h2>
          <span className="muted small">{app} · <Pill>{cats[row.category] ? cats[row.category].name : row.category}</Pill> · first seen {fmtDate(row.first_ms, offset, { year: "numeric" })}</span>
        </div>
      </div>
      <div className="grid-4">
        <Stat label="Total time" value={fmtDur(row.ms)} sub={`${(row.share * 100).toFixed(1)}% of range, ${row.days} days`} />
        <Stat label="Focus periods" value={fmtNum(row.focus_periods)} sub={`${fmtDur(row.avg_ms)} average`} />
        <Stat label="Longest focus" value={fmtDur(row.longest_ms)} />
        <Stat label="Last used" value={ago(row.last_ms, Date.now())} sub={`${fmtDate(row.last_ms, offset)} ${fmtClock(row.last_ms, offset)}`} />
      </div>
      <div className="grid-3-2">
        <Card title="Per day"><DayBars days={s.days} from={range.from} to={range.to} offset={offset} onPick={() => {}} /></Card>
        <Card title="Hour of day" sub="Across the range"><HourBars values={s.hourly} color={appColor(app, dark)} /></Card>
      </div>
      <div className="grid-2">
        <Card title="Weekly rhythm"><WeekHeat grid={d.data.heat} /></Card>
        <Card title="Window titles" sub="Where the time went inside this application"><BarList items={titles} empty="No window titles were recorded." /></Card>
      </div>
      <Card title="Recent focus periods" pad={false}>
        <div className="table-scroll">
          <table className="tbl compact">
            <thead><tr><th>Start</th><th>Window</th><th className="num">Duration</th></tr></thead>
            <tbody>
              {d.data.focus.slice(0, 60).map((r, i) => (
                <tr key={i}>
                  <td className="w-time">{fmtDate(r.start_ms, offset)} {fmtClock(r.start_ms, offset)}</td>
                  <td className="ellipsis wide">{r.title || <span className="muted">no title</span>}</td>
                  <td className="num">{fmtDur(r.ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}

export default function Applications() {
  const { route } = useApp();
  return route.param ? <Detail app={route.param} /> : <List />;
}
