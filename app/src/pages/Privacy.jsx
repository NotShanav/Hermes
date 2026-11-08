import { useEffect, useState } from "react";
import { FolderOpen, Pause, Play, Plus, RotateCcw, Square, X } from "lucide-react";
import { api } from "../api";
import { useApp, useAsync } from "../store";
import { Button, Card, ErrorNote, KV, Loading, Notice, Pill, Toggle } from "../components/ui";
import { STATE_LABEL, STATE_TONE, useRecorder } from "../hooks";
import { ago, fmtDate, fmtNum } from "../util";

const BLURB = {
  recording: "Foreground application names and window titles are being recorded and encrypted.",
  paused: "Nothing is being read or recorded until you resume.",
  locked: "Not recording while the screen is locked. It continues when you unlock Windows.",
  asleep: "Not recording while the computer is asleep.",
  disconnected: "Not recording while this session is disconnected.",
  stopped: "The recorder is not running. Nothing is being captured.",
  unknown: "The recorder is running.",
};

export default function Privacy() {
  const { say, offset } = useApp();
  const { rec, refresh } = useRecorder(2500);
  const settings = useAsync(() => api.settingsGet(), []);
  const [s, setS] = useState(null);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [newEx, setNewEx] = useState("");
  const [needsRestart, setNeedsRestart] = useState(false);

  useEffect(() => { if (settings.data && !s) setS(settings.data.settings); }, [settings.data, s]);

  const act = async (a) => {
    setBusy(true);
    try { say(await api.collectorControl(a)); setNeedsRestart(false); } catch (e) { say(e.message, "bad"); }
    setBusy(false);
    setTimeout(refresh, 600);
  };

  const patch = async (p, restartNeeded) => {
    const next = { ...s, ...p };
    setS(next);
    try {
      await api.settingsSet(next);
      if (restartNeeded && rec && rec.running) setNeedsRestart(true);
    } catch (e) { say(e.message, "bad"); }
  };
  const addEx = () => {
    const v = newEx.trim().toLowerCase();
    if (!v) return;
    setNewEx("");
    if (!s.exclusions.includes(v)) patch({ exclusions: [...s.exclusions, v] }, true);
  };

  const toggleAuto = async (on) => {
    try { await api.autostart(on); await refresh(); say(on ? "Hermes will start with Windows" : "Start with Windows turned off"); } catch (e) { say(e.message, "bad"); }
  };

  if (!rec) return <Loading />;
  const st = rec.state;
  const run = rec.running;

  return (
    <div className="stack">
      <Card className={`recorder ${STATE_TONE[st]}`}>
        <div className="rec-head">
          <span className={`rec-big ${STATE_TONE[st]}`}><i className="rec-dot" /></span>
          <div className="grow">
            <h2 className="h-lg">{STATE_LABEL[st]}</h2>
            <p className="muted">{BLURB[st]}</p>
          </div>
          <div className="row gap">
            {!run && <Button variant="primary" icon={Play} disabled={busy || !rec.tray_exe_found} onClick={() => act("start")}>Start recording</Button>}
            {run && st === "paused" && <Button variant="primary" icon={Play} disabled={busy} onClick={() => act("resume")}>Resume</Button>}
            {run && st !== "paused" && <Button icon={Pause} disabled={busy} onClick={() => act("pause")}>Pause</Button>}
            {run && <Button icon={Square} disabled={busy} onClick={() => act("stop")}>Stop</Button>}
          </div>
        </div>
        {!rec.tray_exe_found && <Notice tone="warn">hermes-tray.exe was not found next to this app, so it cannot be started from here. Keep both files in the same folder.</Notice>}
        {rec.live && rec.live.warnings.map((w, i) => <Notice key={i} tone="warn">{w}</Notice>)}
        <dl className="kv-grid">
          <KV k="Events in archive" v={fmtNum(rec.stats.events)} />
          <KV k="Recorded this run" v={rec.live ? fmtNum(rec.live.events_recorded) : ""} />
          <KV k="Not stored this run" v={rec.live ? fmtNum(rec.live.events_dropped) : ""} />
          <KV k="Last event" v={rec.stats.last_ms ? ago(rec.stats.last_ms, Date.now()) : ""} />
        </dl>
        <p className="muted small">While recording, Hermes always shows its tray icon and an on-screen dot. Pausing from here or from the tray is immediate. To pause for a set time, use the tray menu.</p>
      </Card>

      {needsRestart && (
        <Notice tone="info" title="Restart the recorder to apply the new exclusions">
          <Button small icon={RotateCcw} disabled={busy} onClick={() => act("restart")}>Restart recorder</Button>
        </Notice>
      )}

      <div className="grid-2">
        <Card title="What Hermes records">
          <div className="two-col">
            <div>
              <h4 className="h-sm">Recorded</h4>
              <ul className="plain">
                <li>Name and path of the application in front</li>
                <li>Title of its window, as the app shows it</li>
                <li>When focus starts, ends and the title changes</li>
                <li>Lock, sleep, pause and logoff boundaries</li>
              </ul>
            </div>
            <div>
              <h4 className="h-sm">Never recorded</h4>
              <ul className="plain">
                <li>Keystrokes or typed text</li>
                <li>Clipboard contents</li>
                <li>Screen contents or screenshots</li>
                <li>Web addresses or page content</li>
                <li>Anything sent over a network</li>
              </ul>
            </div>
          </div>
        </Card>

        <Card title="Start-up">
          {!s ? <Loading /> : (
            <div className="stack tight">
              <Toggle checked={s.start_collecting_on_launch} onChange={(v) => patch({ start_collecting_on_launch: v })} label="Record as soon as the recorder starts" desc="Off means it starts paused until you resume." />
              <Toggle checked={s.show_indicator_dot} onChange={(v) => patch({ show_indicator_dot: v }, true)} label="Show the on-screen recording dot" desc="A small always-on-top dot in addition to the tray icon." />
              <Toggle checked={rec.autostart} onChange={toggleAuto} label="Start the recorder with Windows" desc="Adds a per-user startup entry that Windows lists in Settings." />
            </div>
          )}
        </Card>
      </div>

      <Card title="Applications Hermes never describes" sub="Time in these programs is recorded as a private block with no name or title.">
        {!s ? <Loading /> : (
          <div className="stack tight">
            <Toggle checked={s.default_exclusions} onChange={(v) => patch({ default_exclusions: v }, true)} label="Keep the built-in list" desc={`Password managers and the Windows sign-in screen (${settings.data.default_exclusions.slice(0, 6).join(", ")} and more).`} />
            <div className="chips">
              {s.exclusions.map((e) => (
                <span key={e} className="chip on static">{e}<button className="chip-x" onClick={() => patch({ exclusions: s.exclusions.filter((x) => x !== e) }, true)} aria-label={`Remove ${e}`}><X size={12} /></button></span>
              ))}
              {s.exclusions.length === 0 && <span className="muted small">No custom exclusions.</span>}
            </div>
            <div className="row gap">
              <input className="input grow" placeholder="Program name, e.g. mybank.exe" value={newEx} onChange={(e) => setNewEx(e.target.value)} onKeyDown={(e) => e.key === "Enter" && addEx()} />
              <Button icon={Plus} onClick={addEx} disabled={!newEx.trim()}>Exclude</Button>
            </div>
            {settings.data && settings.data.warning && <ErrorNote error={settings.data.warning} />}
          </div>
        )}
      </Card>

      <Card title="Your data" sub="Everything stays in one folder on this computer.">
        <dl className="kv-grid">
          <KV k="Data folder" v={rec.stats.data_dir} mono />
          <KV k="Archive started" v={rec.stats.first_ms ? fmtDate(rec.stats.first_ms, offset, { year: "numeric" }) : ""} />
        </dl>
        <Notice tone="info" title="Deleting and retention">
          Hermes keeps history permanently and signs it in sequence, so removing part of it would show up as tampering in the integrity check. There is no per-item delete or automatic expiry. To erase everything, stop the recorder, delete the data folder, and destroy the USB key file. You can export what you want to keep first from Archive & export.
        </Notice>
        <div className="row gap"><Button icon={FolderOpen} onClick={() => api.openFolder().catch((e) => say(e.message, "bad"))}>Open data folder</Button></div>
      </Card>
    </div>
  );
}
