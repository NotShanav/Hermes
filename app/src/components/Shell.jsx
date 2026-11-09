import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  AppWindow, BarChart3, ChevronDown, Clock, Database, Globe, LayoutDashboard, Lock, PanelLeftClose, PanelLeftOpen, Search, Settings as Cog, ShieldCheck, Tags, Calendar,
} from "lucide-react";
import logo from "../assets/logo.png";
import { api } from "../api";
import { useApp } from "../store";
import { RANGES, fmtDate, isoDay, rangeLabel, DAY } from "../util";
import { STATE_LABEL, STATE_TONE, useRecorder } from "../hooks";
import { LiquidLayer } from "./Fluid";

const NAV = [
  { id: "overview", label: "Overview", icon: LayoutDashboard },
  { id: "timeline", label: "Timeline", icon: Clock },
  { id: "apps", label: "Applications", icon: AppWindow },
  { id: "pages", label: "Pages & windows", icon: Globe },
  { id: "insights", label: "Insights", icon: BarChart3 },
  { id: "explore", label: "Search & explore", icon: Search },
];
const MANAGE = [
  { id: "categories", label: "Categories & rules", icon: Tags },
  { id: "privacy", label: "Privacy & recording", icon: ShieldCheck },
  { id: "archive", label: "Archive & export", icon: Database },
  { id: "settings", label: "Settings", icon: Cog },
];
export const PAGE_TITLES = Object.fromEntries([...NAV, ...MANAGE].map((n) => [n.id, n.label]));
const RANGE_PAGES = ["overview", "apps", "pages", "categories", "explore"];

export function Sidebar() {
  const { route, go, info, lockNow, fluid } = useApp();
  const [collapsed, setCollapsed] = useState(!!info.prefs.sidebar_collapsed);
  const { rec } = useRecorder(5000);
  const navRef = useRef(null);
  const [pill, setPill] = useState(null);

  // Fluidity: one glass pill that glides between the navigation items.
  useLayoutEffect(() => {
    if (!fluid) return undefined;
    const nav = navRef.current;
    if (!nav) return undefined;
    const measure = () => {
      const on = nav.querySelector(".nav-item.on");
      setPill(on ? { y: on.offsetTop, h: on.offsetHeight } : null);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(nav);
    return () => ro.disconnect();
  }, [fluid, route.page, collapsed]);

  const toggle = () => {
    const next = !collapsed;
    setCollapsed(next);
    // Persist quietly: no refetch of data is needed for a layout preference.
    api.savePrefs({ ...info.prefs, sidebar_collapsed: next }).catch(() => {});
  };

  const Item = ({ n }) => {
    const Icon = n.icon;
    const on = route.page === n.id;
    return (
      <button className={`nav-item ${on ? "on" : ""}`} onClick={() => go(n.id)} title={collapsed ? n.label : undefined} aria-current={on ? "page" : undefined}>
        <Icon size={18} strokeWidth={1.9} />
        <span className="nav-label">{n.label}</span>
      </button>
    );
  };

  const st = rec ? rec.state : "unknown";
  return (
    <aside className={`side ${collapsed ? "collapsed" : ""}`}>
      <div className="brand">
        <img src={logo} alt="" width="34" height="34" />
        <span className="brand-name">Hermes</span>
      </div>
      <nav className="nav" ref={navRef}>
        {fluid && pill && (
          <span className="nav-pill" style={{ transform: `translateY(${pill.y}px)`, height: pill.h }}>
            <LiquidLayer radius={12} bezel={11} depth={6} />
          </span>
        )}
        {NAV.map((n) => <Item key={n.id} n={n} />)}
        <div className="nav-sep" />
        {MANAGE.map((n) => <Item key={n.id} n={n} />)}
      </nav>
      <div className="side-foot">
        <button className={`rec-pill ${STATE_TONE[st] || "off"}`} onClick={() => go("privacy")} title="Open recording controls">
          <i className="rec-dot" />
          <span className="nav-label">{rec ? STATE_LABEL[st] : "Checking recorder"}</span>
          <LiquidLayer radius={10} bezel={9} depth={5} />
        </button>
        <button className="nav-item" onClick={lockNow} title="Lock the archive and clear it from memory">
          <Lock size={18} strokeWidth={1.9} />
          <span className="nav-label">Lock</span>
        </button>
        <button className="nav-item" onClick={toggle} title={collapsed ? "Expand sidebar" : "Collapse sidebar"}>
          {collapsed ? <PanelLeftOpen size={18} strokeWidth={1.9} /> : <PanelLeftClose size={18} strokeWidth={1.9} />}
          <span className="nav-label">Collapse</span>
        </button>
      </div>
    </aside>
  );
}

function RangePicker() {
  const { rangeKey, setRangeKey, custom, setCustom, range, offset } = useApp();
  const [open, setOpen] = useState(false);
  const ref = useRef(null);
  useEffect(() => {
    if (!open) return;
    const h = (e) => ref.current && !ref.current.contains(e.target) && setOpen(false);
    window.addEventListener("mousedown", h);
    return () => window.removeEventListener("mousedown", h);
  }, [open]);
  const today = isoDay(Date.now(), offset);
  const [a, setA] = useState(custom.from || isoDay(range.from, offset));
  const [b, setB] = useState(custom.to || isoDay(Math.min(range.to - DAY, Date.now()), offset));
  return (
    <div className="range" ref={ref}>
      <button className="btn" onClick={() => setOpen(!open)} aria-haspopup="listbox" aria-expanded={open}>
        <Calendar size={15} />
        <span>{rangeLabel(rangeKey, range, offset, custom)}</span>
        <ChevronDown size={14} />
        <LiquidLayer radius={10} bezel={9} depth={5} />
      </button>
      {open && (
        <div className="popover">
          <LiquidLayer radius={12} bezel={12} depth={6} soft />
          {RANGES.map((r) => (
            <button key={r.key} className={`pop-item ${rangeKey === r.key ? "on" : ""}`} onClick={() => { setRangeKey(r.key); setOpen(false); }}>
              {r.label}
            </button>
          ))}
          <div className="pop-sep" />
          <div className="pop-custom">
            <label>From<input type="date" value={a} max={today} onChange={(e) => setA(e.target.value)} /></label>
            <label>To<input type="date" value={b} max={today} onChange={(e) => setB(e.target.value)} /></label>
            <button className="btn primary small" disabled={!a || !b || a > b} onClick={() => { setCustom({ from: a, to: b }); setRangeKey("custom"); setOpen(false); }}>
              Apply dates
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

export function Topbar() {
  const { route, go, setExploreSeed } = useApp();
  const [q, setQ] = useState("");
  const isDetail = route.page === "apps" && route.param;
  const submit = (e) => {
    e.preventDefault();
    if (!q.trim()) return;
    setExploreSeed({ text: q.trim() });
    go("explore");
    setQ("");
  };
  return (
    <header className="top">
      <div className="crumbs">
        {isDetail ? (
          <>
            <button className="crumb-link" onClick={() => go("apps")}>Applications</button>
            <span className="muted">/</span>
            <h1>{route.param.replace(/\.exe$/i, "")}</h1>
          </>
        ) : (
          <h1>{PAGE_TITLES[route.page]}</h1>
        )}
      </div>
      <form className="gsearch" onSubmit={submit} role="search">
        <Search size={15} />
        <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search apps and window titles" aria-label="Search recorded activity" />
      </form>
      {RANGE_PAGES.includes(route.page) && <RangePicker />}
      <LiquidLayer radius={18} bezel={18} depth={8} soft />
    </header>
  );
}

export function Toast() {
  const { toast } = useApp();
  if (!toast) return null;
  return (
    <div className={`toast ${toast.tone}`} role="status" key={toast.id}>
      {toast.text}
      <LiquidLayer radius={12} bezel={12} depth={6} soft />
    </div>
  );
}
