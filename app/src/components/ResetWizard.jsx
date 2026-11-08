import { useState } from "react";
import { AlertTriangle, FolderOpen, KeyRound, Usb } from "lucide-react";
import { open as pickPath } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { Button, ErrorNote, Modal, Notice } from "./ui";

/**
 * Clean slate. Reached only through the hidden keyhole on the lock screen.
 * Steps: 0 warning and confirmation, 1 choose the USB drive, 2 working, 3 done.
 */
export default function ResetWizard({ open, onClose, onFinished }) {
  const [step, setStep] = useState(0);
  const [understood, setUnderstood] = useState(false);
  const [word, setWord] = useState("");
  const [deleteOld, setDeleteOld] = useState(false);
  const [usb, setUsb] = useState("");
  const [replaceKey, setReplaceKey] = useState(false);
  const [needReplace, setNeedReplace] = useState(false);
  const [err, setErr] = useState(null);
  const [out, setOut] = useState(null);
  const [busy, setBusy] = useState(false);

  const close = () => {
    if (busy) return;
    setStep(0); setUnderstood(false); setWord(""); setDeleteOld(false); setUsb(""); setReplaceKey(false); setNeedReplace(false); setErr(null); setOut(null);
    onClose();
  };

  const browse = async () => {
    const f = await pickPath({ directory: true, multiple: false });
    if (f) setUsb(f);
  };

  const run = async () => {
    setBusy(true);
    setErr(null);
    setStep(2);
    try {
      const r = await api.resetArchive(usb, deleteOld, replaceKey, word);
      setOut(r);
      setStep(3);
    } catch (e) {
      setErr(e.message);
      if (/already holds a Hermes key file/i.test(e.message)) setNeedReplace(true);
      setStep(1);
    }
    setBusy(false);
  };

  const finish = async () => {
    setBusy(true);
    setErr(null);
    try {
      await api.collectorControl("start").catch(() => {}); // best effort: the new recorder picks up the new key
      await api.unlock([out.share_b]);
      await onFinished();
    } catch (e) {
      setErr(e.message);
    }
    setBusy(false);
  };

  const canStart = understood && word.trim() === "RESET";

  let footer = null;
  if (step === 0) footer = (<><Button onClick={close}>Cancel</Button><Button variant="primary" disabled={!canStart} onClick={() => setStep(1)}>Continue</Button></>);
  if (step === 1) footer = (<><Button onClick={() => setStep(0)}>Back</Button><Button variant="primary" icon={KeyRound} disabled={!usb.trim() || busy} onClick={run}>Create new keys</Button></>);
  if (step === 3) footer = (<Button variant="primary" icon={KeyRound} disabled={busy} onClick={finish}>{busy ? "Starting" : "Unlock and start recording"}</Button>);

  return (
    <Modal open={open} onClose={close} title={step === 3 ? "New keys created" : "Start with a clean slate"} footer={footer}>
      {step === 0 && (
        <div className="stack tight">
          <Notice tone="bad" title="This cannot be undone">
            <div>
              Hermes will stop recording, retire the current archive and its keys, and create a brand-new archive with a new key set.
              Everything recorded so far becomes <b>unreadable in Hermes forever</b>, even for you. Use this only if a key is lost for good.
            </div>
          </Notice>
          <label className="check-row"><input type="checkbox" checked={understood} onChange={(e) => setUnderstood(e.target.checked)} /><span>I understand my existing history will be lost.</span></label>
          <label className="check-row"><input type="checkbox" checked={deleteOld} onChange={(e) => setDeleteOld(e.target.checked)} /><span>Also delete the old encrypted files from this computer. Leave this off to keep them, locked, in a folder named reset-... inside the Hermes data folder.</span></label>
          <label className="field"><span>Type <b>RESET</b> to continue</span><input className="input" value={word} onChange={(e) => setWord(e.target.value)} spellCheck={false} autoComplete="off" /></label>
        </div>
      )}

      {step === 1 && (
        <div className="stack tight">
          <ol className="guide">
            <li><b>Plug in the USB drive</b> that will hold the new key. Use a drive you will keep away from this computer.</li>
            <li><b>Choose it below.</b> Hermes writes one small key file, <code>hermes-share-b.key</code>, to the drive.</li>
            <li><b>Back that file up</b> to a second USB drive straight afterwards. If both copies are lost the new history is lost too.</li>
          </ol>
          <label className="field">
            <span>USB drive or folder</span>
            <div className="row gap">
              <input className="input grow" placeholder="E:\" value={usb} onChange={(e) => setUsb(e.target.value)} spellCheck={false} />
              <Button icon={FolderOpen} onClick={browse}>Browse</Button>
            </div>
          </label>
          {needReplace && (
            <label className="check-row">
              <input type="checkbox" checked={replaceKey} onChange={(e) => setReplaceKey(e.target.checked)} />
              <span>This drive already has an old Hermes key file. Replace it. (It is kept under a new name unless you chose to delete the old data.)</span>
            </label>
          )}
          <ErrorNote error={err} />
        </div>
      )}

      {step === 2 && (
        <div className="loading"><span className="spin" /><span className="muted">Stopping the recorder and creating new keys</span></div>
      )}

      {step === 3 && out && (
        <div className="stack tight">
          <Notice tone="warn" title="Back up the new key file now">
            <div className="pre">{out.share_b}</div>
            <div>Copy it to a second USB drive and keep both away from this computer. Without it the new history can never be read.</div>
          </Notice>
          {out.kept_at && <p className="muted small">The retired archive was kept, locked, at {out.kept_at}. It can only be read with your old keys. You can delete that folder at any time.</p>}
          {out.old_key_copy && <p className="muted small">The old key file on the drive was renamed to {out.old_key_copy}.</p>}
          {!out.kept_at && <p className="muted small">The old archive was deleted.</p>}
          <ErrorNote error={err} />
        </div>
      )}
    </Modal>
  );
}

export const ResetIcon = AlertTriangle;
