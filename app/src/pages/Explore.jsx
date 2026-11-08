import { useEffect, useState } from "react";
import { Download, Search } from "lucide-react";
import { api } from "../api";
import { useApp, useAsync } from "../store";
import { Button, Card, Drawer, Empty, ErrorNote, KV, Loading, Modal, Notice, Pill, Toggle } from "../components/ui";
import { KINDS, exportFlow } from "../exporter";
import { cleanAppName, fmtClockSec, fmtDate, fmtDateTime, fmtDur, fmtNum, kindTone } from "../util";

const PAGE = 100;

export default function Explore() {
  const { range, offset, rev, exploreSeed, setExploreSeed, say } = useApp();
  const [text, setText] = useState("");
  const [app, setApp] = useState("");
  const [kinds, setKinds] = useState([]);
  const [useRange, setUseRange] = useState(true);
  const [newest, setNewest] = useState(true);
  const [page, setPage] = useState(0);
  const [open, setOpen] = useState(null);
  const [confirm, setConfirm] = useState(null);

  useEffect(() => {
    if (exploreSeed) {
      setText(exploreSeed.text || "");
      setApp(exploreSeed.app || "");
      setKinds(exploreSeed.kinds || []);
      setUseRange(exploreSeed.allTime ? false : true);
      setPage(0);
      setExploreSeed(null);
    }
  }, [exploreSeed, setExploreSeed]);

  const filter = { from_ms: useRange ? range.from : null, to_ms: useRange ? range.to : null, app: app || null, text: text || null, kinds };
  const key = JSON.stringify(filter);
  const res = useAsync(() => api.explore(filter, page * PAGE, PAGE, newest), [key, page, newest, rev]);
  useEffect(() => setPage(0), [key]);

  const toggleKind = (v) => setKinds((k) => (k.includes(v) ? k.filter((x) => x !== v) : [...k, v]));
  const total = res.data ? res.data.total : 0;
  const pages = Math.max(1, Math.ceil(total / PAGE));

  const doExport = async (fmt) => {
    setConfirm(null);
    try {
      const r = await exportFlow(filter, fmt);
      if (r) say(`Exported ${fmtNum(r.n)} events to ${r.path}`);
    } catch (e) {
      say(e.message, "bad");
    }
  };

  return (
    <div className="stack">
      <Card pad>
        <div className="filters">
          <label className="search-in wide"><Search size={15} /><input value={text} onChange={(e) => setText(e.target.value)} placeholder="Search application, path, window class or title" aria-label="Search text" /></label>
          <label className="search-in"><input value={app} onChange={(e) => setApp(e.target.value)} placeholder="Application contains" aria-label="Application filter" /></label>
          <Button icon={Download} onClick={() => setConfirm(true)} disabled={!total}>Export results</Button>
        </div>
        <div className="row between center wrap filters-2">
          <div className="chips">
            {KINDS.map((k) => (
              <button key={k.v} className={`chip ${kinds.includes(k.v) ? "on" : ""}`} onClick={() => toggleKind(k.v)} aria-pressed={kinds.includes(k.v)}>{k.l}</button>
            ))}
            {kinds.length > 0 && <button className="link" onClick={() => setKinds([])}>Clear</button>}
          </div>
          <div className="row gap center">
            <Toggle checked={useRange} onChange={setUseRange} label="Limit to the top-bar date range" />
            <Toggle checked={newest} onChange={setNewest} label="Newest first" />
          </div>
        </div>
      </Card>

      {res.error && <ErrorNote error={res.error} />}
      <Card title={res.data ? `${fmtNum(total)} ${total === 1 ? "event" : "events"}` : "Events"} pad={false} actions={res.loading && <span className="spin small" />}>
        {!res.data ? <div className="card-b"><Loading /></div> : res.data.rows.length === 0 ? (
          <div className="card-b"><Empty title="No events match" body="Loosen the filters or widen the date range." /></div>
        ) : (
          <>
            <div className="table-scroll">
              <table className="tbl compact">
                <thead><tr><th>#</th><th>Time</th><th>Event</th><th>Application</th><th>Title or detail</th></tr></thead>
                <tbody>
                  {res.data.rows.map((r) => (
                    <tr key={r.seq} className="row-click" onClick={() => setOpen(r)}>
                      <td className="muted num">{r.seq}</td>
                      <td className="w-time">{fmtDate(r.ts_ms, offset)} {fmtClockSec(r.ts_ms, offset)}</td>
                      <td><Pill tone={kindTone(r.kind)}>{r.kind.toLowerCase()}</Pill></td>
                      <td>{r.app ? cleanAppName(r.app) : <span className="muted">none</span>}</td>
                      <td className="ellipsis wide">{r.title || r.detail || <span className="muted">none</span>}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="pager">
              <span className="muted small">Page {page + 1} of {fmtNum(pages)}</span>
              <div className="row gap">
                <Button small disabled={page === 0} onClick={() => setPage(page - 1)}>Previous</Button>
                <Button small disabled={page + 1 >= pages} onClick={() => setPage(page + 1)}>Next</Button>
              </div>
            </div>
          </>
        )}
      </Card>

      <Drawer open={!!open} onClose={() => setOpen(null)} title="Event details">
        {open && (
          <dl className="kv-list">
            <KV k="Sequence" v={`#${open.seq}`} />
            <KV k="Time" v={fmtDateTime(open.ts_ms, offset)} />
            <KV k="Event" v={open.kind.toLowerCase()} />
            <KV k="Application" v={open.app} />
            <KV k="Executable path" v={open.exe_path} mono />
            <KV k="Window title" v={open.title} />
            <KV k="Window class" v={open.window_class} mono />
            <KV k="Detail" v={open.detail} />
            <KV k="Process id" v={open.pid ? String(open.pid) : ""} />
            <KV k="Recording session" v={open.session} mono />
            {open.focus_ms > 0 && <KV k="In focus for" v={fmtDur(open.focus_ms)} />}
          </dl>
        )}
      </Drawer>

      <Modal open={!!confirm} onClose={() => setConfirm(null)} title="Export as plain text"
        footer={<><Button onClick={() => setConfirm(null)}>Cancel</Button><Button onClick={() => doExport("json")}>Save as JSON</Button><Button variant="primary" onClick={() => doExport("csv")}>Save as CSV</Button></>}>
        <Notice tone="warn" title="The file is not encrypted">
          An export contains application names and window titles for {fmtNum(total)} events. Store it somewhere safe and delete it when you are done.
        </Notice>
      </Modal>
    </div>
  );
}
