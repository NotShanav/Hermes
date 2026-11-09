import { Moon, Monitor, Sun } from "lucide-react";
import logo from "../assets/logo.png";
import { useApp } from "../store";
import { Card, KV, Notice, Segmented } from "../components/ui";
import { MIN } from "../util";

const ZONES = [-720, -660, -600, -540, -480, -420, -360, -300, -240, -210, -180, 0, 60, 120, 180, 210, 240, 270, 300, 330, 345, 360, 420, 480, 540, 570, 600, 660, 720].map((m) => ({
  m,
  label: `UTC${m < 0 ? "-" : "+"}${String(Math.floor(Math.abs(m) / 60)).padStart(2, "0")}:${String(Math.abs(m) % 60).padStart(2, "0")}`,
}));

export default function SettingsPage() {
  const { info, savePrefs, say } = useApp();
  const p = info.prefs;
  const save = (patch) => savePrefs(patch).catch((e) => say(e.message, "bad"));
  const tz = p.tz_override_minutes;
  return (
    <div className="stack narrow">
      <Card title="Appearance">
        <div className="setting">
          <div><b>Theme</b><p className="muted small">Dark is the default. System follows Windows.</p></div>
          <Segmented value={p.theme} onChange={(v) => save({ theme: v })} options={[
            { value: "dark", label: "Dark" }, { value: "light", label: "Light" }, { value: "system", label: "System" },
          ]} />
        </div>
      </Card>

      <Card title="Interface style">
        <div className="setting">
          <div>
            <b>Style</b>
            <p className="muted small">
              {p.ui_style === "fluidity"
                ? "Fluidity: frosted glass, slowly drifting colour blobs, liquid-glass highlights and fluid motion."
                : "Modern: clean solid panels. The original look."}
            </p>
          </div>
          <Segmented value={p.ui_style || "modern"} onChange={(v) => save({ ui_style: v })} options={[
            { value: "modern", label: "Modern" }, { value: "fluidity", label: "Fluidity" },
          ]} />
        </div>
        {p.ui_style === "fluidity" && (
          <div className="setting">
            <div>
              <b>Intensity</b>
              <p className="muted small">Full adds five blobs and liquid-glass refraction. Subtle keeps frosted glass and two quiet blobs, and is lighter on the graphics card.</p>
            </div>
            <Segmented value={p.effects || "full"} onChange={(v) => save({ effects: v })} options={[
              { value: "full", label: "Full" }, { value: "subtle", label: "Subtle" },
            ]} />
          </div>
        )}
        {p.ui_style === "fluidity" && (
          <p className="muted small fx-note">Animation pauses when the window is in the background or the archive is locked. Windows' reduced-motion and reduced-transparency settings are respected.</p>
        )}
      </Card>

      <Card title="Time">
        <div className="setting">
          <div><b>Time zone</b><p className="muted small">Days start and end at midnight in this zone. Currently UTC{info.offset_minutes >= 0 ? "+" : "-"}{Math.floor(Math.abs(info.offset_minutes) / 60)}:{String(Math.abs(info.offset_minutes) % 60).padStart(2, "0")}.</p></div>
          <select className="select" value={tz == null ? "system" : String(tz)} onChange={(e) => save({ tz_override_minutes: e.target.value === "system" ? null : Number(e.target.value) })} aria-label="Time zone">
            <option value="system">Windows time zone</option>
            {ZONES.map((z) => <option key={z.m} value={z.m}>{z.label}</option>)}
          </select>
        </div>
      </Card>

      <Card title="Security">
        <div className="setting">
          <div><b>Lock after inactivity</b><p className="muted small">Locking clears the decrypted history from memory. Recording is not affected.</p></div>
          <select className="select" value={p.auto_lock_minutes} onChange={(e) => save({ auto_lock_minutes: Number(e.target.value) })} aria-label="Auto-lock">
            <option value={0}>Never</option>
            {[2, 5, 10, 15, 30, 60].map((m) => <option key={m} value={m}>{m} minutes</option>)}
          </select>
        </div>
        <Notice tone="info">Closing this window also clears the history from memory. The recorder keeps running on its own.</Notice>
      </Card>

      <Card title="About">
        <div className="about">
          <img src={logo} width="64" height="64" alt="" />
          <dl className="kv-list grow">
            <KV k="Hermes app" v={`v${info.version}`} />
            <KV k="Author" v="NotShanav" />
            <KV k="Data folder" v={info.data_dir} mono />
            <KV k="Preferences file" v="hermes-app.json in the data folder (no archive content)" />
          </dl>
        </div>
      </Card>
    </div>
  );
}
