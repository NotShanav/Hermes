import { useMemo, useState } from "react";
import { Search } from "lucide-react";
import { api } from "../api";
import { useApp, useAsync } from "../store";
import { Card, Empty, ErrorNote, Loading, Notice, Segmented } from "../components/ui";
import { appColor, cleanAppName, cleanTitle, fmtDur, fmtNum, isBrowser } from "../util";

export default function PagesWindows() {
  const { range, rev, dark, go, setExploreSeed } = useApp();
  const [mode, setMode] = useState("browser");
  const [q, setQ] = useState("");
  const sum = useAsync(() => api.summary(range.from, range.to), [range.from, range.to, rev]);

  const rows = useMemo(() => {
    if (!sum.data) return [];
    const m = new Map();
    for (const t of sum.data.titles) {
      const br = isBrowser(t.app);
      if (mode === "browser" && !br) continue;
      if (mode === "other" && br) continue;
      const title = br ? cleanTitle(t.title) : t.title;
      if (!title) continue;
      const key = `${t.app}\u0000${title}`;
      const cur = m.get(key) || { title, app: t.app, ms: 0, visits: 0 };
      cur.ms += t.ms;
      cur.visits += t.visits;
      m.set(key, cur);
    }
    const needle = q.trim().toLowerCase();
    return [...m.values()].filter((r) => !needle || r.title.toLowerCase().includes(needle)).sort((a, b) => b.ms - a.ms).slice(0, 200);
  }, [sum.data, mode, q]);

  if (sum.error) return <ErrorNote error={sum.error} />;
  if (!sum.data) return <Loading />;
  const max = Math.max(1, ...rows.map((r) => r.ms));
  const open = (r) => { setExploreSeed({ text: r.title, app: r.app }); go("explore"); };

  return (
    <div className="stack">
      <Notice tone="info" title="Titles, not addresses">
        Hermes records the title of the window in front, never the web address or page content. Browser tabs therefore appear by page title, and the same site can show up under many titles.
      </Notice>
      <div className="row between center wrap">
        <Segmented value={mode} onChange={setMode} options={[{ value: "browser", label: "Browser pages" }, { value: "other", label: "Other windows" }, { value: "all", label: "Everything" }]} />
        <label className="search-in"><Search size={15} /><input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter titles" aria-label="Filter titles" /></label>
      </div>
      <Card pad={false}>
        {rows.length === 0 ? <div className="card-b"><Empty title="No titles to show" body="Nothing matches this filter in the selected range." /></div> : (
          <div className="table-scroll">
            <table className="tbl">
              <thead><tr><th>Title</th><th>Application</th><th className="num">Time</th><th className="share-h">Relative</th><th className="num">Visits</th></tr></thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={i} className="row-click" onClick={() => open(r)} title="Find these events in Search & explore">
                    <td className="ellipsis wide">{r.title}</td>
                    <td><span className="cell-app"><i className="swatch" style={{ background: appColor(r.app, dark) }} />{cleanAppName(r.app)}</span></td>
                    <td className="num strong">{fmtDur(r.ms)}</td>
                    <td className="share"><span className="bl-bar thin"><i style={{ width: `${(r.ms / max) * 100}%`, background: appColor(r.app, dark) }} /></span></td>
                    <td className="num">{fmtNum(r.visits)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      <p className="muted small">Shows the 300 titles with the most time in the range; the list above is trimmed to 200.</p>
    </div>
  );
}
