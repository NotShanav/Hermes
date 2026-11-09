import { useCallback, useEffect, useState } from "react";
import { useEffect as useEffectR, useRef, useState as useStateR } from "react";
import { FolderOpen, KeyRound, LockKeyhole, Pause, Play, Square, Usb } from "lucide-react";
import { open as pickPath } from "@tauri-apps/plugin-dialog";
import logo from "../assets/logo.png";
import { api } from "../api";
import { useApp } from "../store";
import { Button, ErrorNote, Pill } from "./ui";
import ResetWizard from "./ResetWizard";
import { Elastic, GooHalo, LiquidLayer } from "./Fluid";
import { STATE_LABEL, STATE_TONE, useRecorder } from "../hooks";
import { ago, fmtNum } from "../util";

const SLIDE_MAX = 108;

/**
 * The logo, with a hidden feature: drag it to the right and a keyhole is revealed behind it. Pressing the
 * keyhole opens the clean-slate reset. There is deliberately no visible hint. Documented in app/README.md.
 */
function SecretLogo({ onOpen }) {
  const [x, setX] = useStateR(0);
  const [drag, setDrag] = useStateR(false);
  const start = useRef(null);

  const down = (e) => {
    start.current = { px: e.clientX, x0: x };
    setDrag(true);
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const move = (e) => {
    if (!start.current) return;
    setX(Math.max(0, Math.min(SLIDE_MAX, start.current.x0 + e.clientX - start.current.px)));
  };
  const up = () => {
    if (!start.current) return;
    start.current = null;
    setDrag(false);
    setX((v) => (v > SLIDE_MAX * 0.5 ? SLIDE_MAX : 0));
  };
  // Slide back by itself if left open.
  useEffectR(() => {
    if (x < SLIDE_MAX || drag) return;
    const t = setTimeout(() => setX(0), 7000);
    return () => clearTimeout(t);
  }, [x, drag]);

  const reveal = Math.min(1, x / (SLIDE_MAX * 0.6));
  const ready = x >= SLIDE_MAX * 0.9;
  return (
    <div className="logo-slide">
      <button className="keyhole" style={{ opacity: reveal, pointerEvents: ready ? "auto" : "none", transform: `scale(${0.8 + 0.2 * reveal})` }} onClick={onOpen} tabIndex={-1} aria-hidden={!ready}>
        <LockKeyhole size={34} strokeWidth={1.6} />
      </button>
      <img className="logo-img" src={logo} alt="Hermes" width="96" height="96" draggable={false}
        style={{ transform: `translateX(${x}px)`, transition: drag ? "none" : "transform 0.28s cubic-bezier(0.2, 0.9, 0.3, 1)" }}
        onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up} />
    </div>
  );
}

function RecorderStrip() {
  const { say } = useApp();
  const { rec, refresh } = useRecorder(3000);
  const [busy, setBusy] = useState(false);
  if (!rec) return null;
  const act = async (a) => {
    setBusy(true);
    try {
      say(await api.collectorControl(a));
    } catch (e) {
      say(e.message, "bad");
    }
    setBusy(false);
    refresh();
  };
  const st = rec.state;
  return (
    <div className="lock-rec">
      <span className={`rec-pill static ${STATE_TONE[st] || "off"}`}>
        <i className="rec-dot" />
        {STATE_LABEL[st]}
      </span>
      <span className="muted small">
        {rec.stats.last_ms ? `Last event ${ago(rec.stats.last_ms, Date.now())} · ${fmtNum(rec.stats.events)} events` : "Nothing recorded yet"}
      </span>
      <span className="grow" />
      {!rec.running && <Button small icon={Play} disabled={busy || !rec.stats.exists} onClick={() => act("start")}>Start recording</Button>}
      {rec.running && st === "paused" && <Button small icon={Play} disabled={busy} onClick={() => act("resume")}>Resume</Button>}
      {rec.running && st !== "paused" && <Button small icon={Pause} disabled={busy} onClick={() => act("pause")}>Pause</Button>}
      {rec.running && <Button small icon={Square} disabled={busy} onClick={() => act("stop")}>Stop</Button>}
    </div>
  );
}

export default function LockScreen() {
  const { info, refreshInfo, say, lockReason, setLockReason } = useApp();
  const [stats, setStats] = useState(null);
  const [found, setFound] = useState([]);
  const [picked, setPicked] = useState([]);
  const [usb, setUsb] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState(null);
  const [created, setCreated] = useState(null);
  const [showReset, setShowReset] = useState(false);

  const poll = useCallback(async () => {
    try {
      const [s, f] = await Promise.all([api.lockInfo(), api.detectKeys()]);
      setStats(s);
      setFound(f);
    } catch (e) {
      setErr(e.message);
    }
  }, []);
  useEffect(() => {
    poll();
    const t = setInterval(poll, 2500);
    return () => clearInterval(t);
  }, [poll]);

  const files = Array.from(new Set([...picked, ...found]));

  const unlock = async (list) => {
    setBusy(true);
    setErr(null);
    try {
      await api.unlock(list);
      setLockReason(null);
      await refreshInfo();
    } catch (e) {
      setErr(e.message);
    }
    setBusy(false);
  };

  const browseKey = async () => {
    const f = await pickPath({ multiple: false, filters: [{ name: "Hermes key file", extensions: ["key"] }, { name: "All files", extensions: ["*"] }] });
    if (f) setPicked((p) => Array.from(new Set([...p, f])));
  };
  const browseUsb = async () => {
    const f = await pickPath({ directory: true, multiple: false });
    if (f) setUsb(f);
  };
  const create = async () => {
    setBusy(true);
    setErr(null);
    try {
      const r = await api.initArchive(usb);
      setCreated(r);
      say("Encrypted archive created");
      await poll();
    } catch (e) {
      setErr(e.message);
    }
    setBusy(false);
  };

  const needsSetup = stats && !stats.exists;

  return (
    <div className="lock">
      <div className="lock-art">
        <div className="logo-stage"><GooHalo /><SecretLogo onOpen={() => setShowReset(true)} /></div>
        <h1>Your activity.<br />Two keys to read it.</h1>
        <ul className="lock-facts">
          <li><b>Encrypted before it is saved.</b> The recorder holds only a public key and cannot read what it wrote.</li>
          <li><b>Readable only with both keys.</b> One is tied to this Windows account, the other lives on your USB drive.</li>
          <li><b>Nothing leaves this computer.</b> No network, no cloud, no keystrokes.</li>
        </ul>
        <span className="muted small lock-path">{info.data_dir}</span>
      </div>

      <div className="lock-panel">
        {!stats && <p className="muted">Looking for your archive</p>}

        {needsSetup && !created && (
          <>
            <h2>Set up Hermes</h2>
            <p className="muted">
              Plug in the USB drive that will hold the second key. Hermes creates the encrypted archive on this computer and writes one key file to the drive.
              The key file must stay off this computer's own drive.
            </p>
            <label className="field">
              <span>USB drive or folder</span>
              <div className="row gap">
                <input className="input grow" placeholder="E:\" value={usb} onChange={(e) => setUsb(e.target.value)} spellCheck={false} />
                <Button icon={FolderOpen} onClick={browseUsb}>Browse</Button>
              </div>
            </label>
            <ErrorNote error={err} />
            <Button variant="primary" icon={Usb} disabled={busy || !usb.trim()} onClick={create}>{busy ? "Creating" : "Create archive and key file"}</Button>
          </>
        )}

        {needsSetup && created && (
          <>
            <h2>Archive created</h2>
            <div className="notice warn">
              <strong>Back up the key file now.</strong>
              <div className="pre">{created.share_b}</div>
              <div>If this file is lost, or Windows is reinstalled, the history can never be read again. Copy it to a second USB drive and keep both away from this computer.</div>
            </div>
            <ErrorNote error={err} />
            <Button variant="primary" icon={KeyRound} disabled={busy} onClick={() => unlock([created.share_b])}>{busy ? "Unlocking" : "Unlock now"}</Button>
          </>
        )}

        {stats && stats.exists && (
          <>
            <h2>Unlock your history</h2>
            {lockReason && <div className="notice warn">{lockReason}</div>}
            <p className="muted">
              Recording keeps running while this screen is shown, and does not need the USB drive. Reading the history needs the key file from your USB drive and this Windows account.
            </p>
            <div className="keybox">
              <div className="keybox-h">
                <span>Key file</span>
                <Button small icon={FolderOpen} onClick={browseKey}>Choose file</Button>
              </div>
              {files.length === 0 ? (
                <p className="muted small keybox-empty"><Usb size={16} /> Plug in the USB drive. Hermes looks for hermes-share-b.key on every drive.</p>
              ) : (
                files.map((f) => (
                  <div className="key-row" key={f}>
                    <KeyRound size={15} />
                    <span className="ellipsis">{f}</span>
                    <Pill tone="ok" dot>{found.includes(f) ? "Found" : "Chosen"}</Pill>
                  </div>
                ))
              )}
            </div>
            <div className="keysets small muted">
              {stats.keys.map((k) => (
                <span key={k.id}>
                  Key set {k.id.slice(0, 8)} · {k.active ? "active" : "retired"} · {k.share_a_present ? "this PC's key present" : "this PC's key MISSING"}
                </span>
              ))}
            </div>
            <ErrorNote error={err} />
            <Elastic>
              <Button variant="primary" className="glass-btn" icon={KeyRound} disabled={busy || files.length === 0} onClick={() => unlock(files)}>
                {busy ? "Unlocking and verifying" : "Unlock"}
                <LiquidLayer radius={10} bezel={10} depth={6} />
              </Button>
            </Elastic>
            {busy && <p className="muted small">Decrypting and verifying signatures. Large archives take a moment.</p>}
          </>
        )}

        <div className="grow" />
        <RecorderStrip />
        <ResetWizard open={showReset && !!(stats && stats.exists)} onClose={() => setShowReset(false)} onFinished={async () => { setShowReset(false); setLockReason(null); await refreshInfo(); }} />
      </div>
    </div>
  );
}
