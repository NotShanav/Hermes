import { useState } from "react";
import { Download, RefreshCw } from "lucide-react";
import { api } from "../api";
import { useApp, useAsync } from "../store";
import { Button, Card, ErrorNote, KV, Loading, Modal, Notice, Pill, Segmented, Stat } from "../components/ui";
import { exportFlow } from "../exporter";
import { fmtBytes, fmtDate, fmtDateTime, fmtNum } from "../util";

export default function Archive() {
  const { offset, say, range, rangeKey, bump } = useApp();
  const integ = useAsync(() => api.integrity(), []);
  const stats = useAsync(() => api.lockInfo(), []);
  const [again, setAgain] = useState(null);
  const [busy, setBusy] = useState(false);
  const [fmt, setFmt] = useState("csv");
  const [scope, setScope] = useState("range");
  const [confirm, setConfirm] = useState(false);
  const [counted, setCounted] = useState(null);

  const ig = again || integ.data;
  const st = stats.data;

  const recheck = async () => {
    setBusy(true);
    try { setAgain(await api.recheck()); bump(); say("Archive re-read and verified"); } catch (e) { say(e.message, "bad"); }
    setBusy(false);
  };

  const filter = scope === "all" ? {} : { from_ms: range.from, to_ms: range.to };
  const openExport = async () => {
    try {
      const r = await api.explore(filter, 0, 1, true);
      setCounted(r.total);
      setConfirm(true);
    } catch (e) { say(e.message, "bad"); }
  };
  const doExport = async () => {
    setConfirm(false);
    try {
      const r = await exportFlow(filter, fmt);
      if (r) say(`Exported ${fmtNum(r.n)} events to ${r.path}`);
    } catch (e) { say(e.message, "bad"); }
  };

  return (
    <div className="stack">
      {st && (
        <div className="grid-4">
          <Stat label="Events" value={fmtNum(st.events)} sub={`schema v${st.schema}`} />
          <Stat label="Size on disk" value={fmtBytes(st.size_bytes)} sub="database and journal" />
          <Stat label="First event" value={st.first_ms ? fmtDate(st.first_ms, offset, { year: "numeric" }) : "none"} />
          <Stat label="Last shutdown" value={st.clean_shutdown == null ? "unknown" : st.clean_shutdown ? "Clean" : "Not clean"} sub={st.clean_shutdown ? "" : "still running, or interrupted"} />
        </div>
      )}

      <Card title="Integrity" sub="Checks that no record was changed, removed or injected." actions={<Button small icon={RefreshCw} disabled={busy} onClick={recheck}>{busy ? "Verifying" : "Re-run check"}</Button>}>
        {integ.error && <ErrorNote error={integ.error} />}
        {!ig ? <Loading /> : (
          <div className="stack">
            <Notice tone={ig.clean ? "ok" : "bad"} title={ig.clean ? "No problems found" : "Problems found"}>
              <div className="pre">{ig.summary}</div>
            </Notice>
            <dl className="kv-grid">
              <KV k="Signed checkpoints verified" v={`${fmtNum(ig.checkpoints_verified)} of ${fmtNum(ig.checkpoints_total)}`} />
              <KV k="Records that failed authentication" v={fmtNum(ig.tampered)} />
              <KV k="Records from locked key sets" v={fmtNum(ig.locked)} />
              <KV k="Older-format records" v={fmtNum(ig.legacy_rows)} />
              <KV k="Missing sequence numbers" v={ig.seq_gaps.length ? JSON.stringify(ig.seq_gaps) : "none"} />
              <KV k="Checked" v={fmtDateTime(ig.opened_ms, offset)} />
            </dl>
            {ig.notes.map((n, i) => <p key={i} className="muted small">{n}</p>)}
            <p className="muted small">Not detectable by this check: removal of the newest records, or restoring an older copy of the whole archive.</p>
          </div>
        )}
      </Card>

      <Card title="Key sets" sub="Reading needs this computer's key and the USB key file together.">
        {!st ? <Loading /> : (
          <table className="tbl compact">
            <thead><tr><th>Key set</th><th>Status</th><th>Signs checkpoints</th><th>This computer's key</th><th>Created</th></tr></thead>
            <tbody>
              {st.keys.map((k) => (
                <tr key={k.id}>
                  <td><code>{k.id.slice(0, 16)}</code></td>
                  <td><Pill tone={k.active ? "ok" : "muted"}>{k.active ? "active" : "retired"}</Pill></td>
                  <td>{k.signing ? "yes" : <Pill tone="warn">no, older set</Pill>}</td>
                  <td>{k.share_a_present ? "present" : <Pill tone="bad">missing</Pill>}</td>
                  <td>{fmtDate(k.created_ms, offset, { year: "numeric" })}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>

      <Card title="Export centre" sub="Write history to a file you can open in a spreadsheet or another tool.">
        <div className="stack">
          <div className="row gap center wrap">
            <span className="muted small">Format</span>
            <Segmented small value={fmt} onChange={setFmt} options={[{ value: "csv", label: "CSV" }, { value: "json", label: "JSON" }]} />
            <span className="muted small">Scope</span>
            <Segmented small value={scope} onChange={setScope} options={[{ value: "range", label: rangeKey === "all" ? "All time" : "Top-bar date range" }, { value: "all", label: "Everything" }]} />
            <span className="grow" />
            <Button variant="primary" icon={Download} onClick={openExport}>Export</Button>
          </div>
          <p className="muted small">Exports are plain text and contain application names and window titles. For finer filters by text, application or event type, use Search & explore.</p>
        </div>
      </Card>

      <Modal open={confirm} onClose={() => setConfirm(false)} title="Export as plain text"
        footer={<><Button onClick={() => setConfirm(false)}>Cancel</Button><Button variant="primary" onClick={doExport}>Choose file and save</Button></>}>
        <Notice tone="warn" title="The file is not encrypted">
          {fmtNum(counted)} events will be written as {fmt.toUpperCase()}. Store the file somewhere safe and delete it when you are done.
        </Notice>
      </Modal>
    </div>
  );
}
