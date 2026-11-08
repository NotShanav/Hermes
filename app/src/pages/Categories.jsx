import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { api } from "../api";
import { useApp, useAsync, useCategoryMap } from "../store";
import { Button, Card, ErrorNote, Loading, Notice, Pill } from "../components/ui";
import { appColor, cleanAppName, fmtDur } from "../util";

export default function Categories() {
  const { info, range, rev, savePrefs, say, dark } = useApp();
  const cats = useCategoryMap();
  const sum = useAsync(() => api.summary(range.from, range.to), [range.from, range.to, rev]);
  const [pattern, setPattern] = useState("");
  const [target, setTarget] = useState("development");
  const [name, setName] = useState("");
  const [color, setColor] = useState("#9B7BFF");
  const prefs = info.prefs;

  const run = async (fn, ok) => {
    try {
      await fn();
      if (ok) say(ok);
    } catch (e) {
      say(e.message, "bad");
    }
  };
  const addRule = (p, c) => run(() => savePrefs({ rules: [...prefs.rules.filter((r) => r.pattern.toLowerCase() !== p.toLowerCase()), { pattern: p, category: c }] }), "Rule saved");
  const delRule = (i) => run(() => savePrefs({ rules: prefs.rules.filter((_, k) => k !== i) }));
  const addCat = () => {
    const id = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    if (!id || cats[id]) return say("Choose a name that is not already used.", "bad");
    run(() => savePrefs({ custom_categories: [...prefs.custom_categories, { id, name: name.trim(), color }] }), "Category added").then(() => setName(""));
  };
  const delCat = (id) =>
    run(() => savePrefs({ custom_categories: prefs.custom_categories.filter((c) => c.id !== id), rules: prefs.rules.filter((r) => r.category !== id) }), "Category removed");

  const allCats = info.categories;
  const unassigned = sum.data ? sum.data.apps.filter((a) => a.category === "other").slice(0, 12) : [];
  const customIds = new Set(prefs.custom_categories.map((c) => c.id));

  return (
    <div className="stack">
      <Notice tone="info" title="Categories only change how data is shown">
        Hermes stores application names, not categories. Rules are applied when charts are drawn, so you can change them at any time and the whole history updates. Nothing in the encrypted archive is modified.
      </Notice>
      {sum.error && <ErrorNote error={sum.error} />}
      <div className="grid-2">
        <Card title="Categories" sub="Time in the selected range">
          {!sum.data ? <Loading /> : (
            <ul className="cat-list">
              {allCats.map((c) => {
                const row = sum.data.categories.find((x) => x.category === c.id);
                return (
                  <li key={c.id}>
                    <i className="swatch" style={{ background: c.color }} />
                    <span className="grow">{c.name}</span>
                    <span className="muted small">{row ? fmtDur(row.ms) : "none"}</span>
                    {customIds.has(c.id) && <button className="icon-btn" onClick={() => delCat(c.id)} aria-label={`Remove ${c.name}`}><Trash2 size={15} /></button>}
                  </li>
                );
              })}
            </ul>
          )}
          <div className="row gap add-row">
            <input className="input grow" placeholder="New category name" value={name} onChange={(e) => setName(e.target.value)} />
            <input type="color" className="color-in" value={color} onChange={(e) => setColor(e.target.value)} aria-label="Category colour" />
            <Button icon={Plus} onClick={addCat} disabled={!name.trim()}>Add</Button>
          </div>
        </Card>

        <Card title="Rules" sub="First match wins. Your rules come before the built-in lists.">
          {prefs.rules.length === 0 ? <p className="muted small">No rules yet. Built-in lists already cover common browsers, editors, chat apps and media players.</p> : (
            <ul className="cat-list">
              {prefs.rules.map((r, i) => (
                <li key={i}>
                  <code>{r.pattern}</code>
                  <span className="muted small">goes to</span>
                  <Pill>{cats[r.category] ? cats[r.category].name : r.category}</Pill>
                  <span className="grow" />
                  <button className="icon-btn" onClick={() => delRule(i)} aria-label={`Remove rule ${r.pattern}`}><Trash2 size={15} /></button>
                </li>
              ))}
            </ul>
          )}
          <div className="row gap add-row">
            <input className="input grow" placeholder="Part of the program name, e.g. blender" value={pattern} onChange={(e) => setPattern(e.target.value)} />
            <select className="select" value={target} onChange={(e) => setTarget(e.target.value)} aria-label="Category">
              {allCats.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
            </select>
            <Button icon={Plus} onClick={() => { addRule(pattern.trim(), target); setPattern(""); }} disabled={!pattern.trim()}>Add rule</Button>
          </div>
        </Card>
      </div>

      <Card title="Not yet sorted" sub="Applications that currently land in Other, most time first">
        {!sum.data ? <Loading /> : unassigned.length === 0 ? <p className="muted small">Everything in this range has a category.</p> : (
          <table className="tbl compact">
            <tbody>
              {unassigned.map((a) => (
                <tr key={a.app}>
                  <td><span className="cell-app"><i className="swatch" style={{ background: appColor(a.app, dark) }} />{cleanAppName(a.app)}</span></td>
                  <td className="num muted">{fmtDur(a.ms)}</td>
                  <td className="num">
                    <select className="select small" defaultValue="" onChange={(e) => e.target.value && addRule(a.app, e.target.value)} aria-label={`Category for ${a.app}`}>
                      <option value="">Move to</option>
                      {allCats.filter((c) => c.id !== "other").map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                    </select>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Card>
    </div>
  );
}
