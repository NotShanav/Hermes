import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { useApp } from "../store";
import { LiquidLayer } from "./Fluid";

export function Card({ title, sub, actions, children, className = "", pad = true, onClick }) {
  return (
    <section className={`card ${onClick ? "clickable" : ""} ${className}`} onClick={onClick}>
      {(title || actions) && (
        <header className="card-h">
          <div>
            {title && <h3>{title}</h3>}
            {sub && <p className="muted small">{sub}</p>}
          </div>
          {actions && <div className="card-actions">{actions}</div>}
        </header>
      )}
      <div className={pad ? "card-b" : ""}>{children}</div>
    </section>
  );
}

export function Stat({ label, value, sub, delta, onClick, hint }) {
  const up = delta != null && delta > 0.005;
  const down = delta != null && delta < -0.005;
  return (
    <button className={`stat ${onClick ? "clickable" : ""}`} onClick={onClick} disabled={!onClick} title={hint}>
      <span className="stat-label">{label}</span>
      <span className="stat-value"><span className="roll" key={String(value)}>{value}</span></span>
      <span className="stat-sub">
        {delta != null && (
          <span className={`delta ${up ? "up" : down ? "down" : ""}`}>
            {up ? "+" : ""}
            {Math.round(delta * 100)}%
          </span>
        )}
        {sub && <span className="muted">{sub}</span>}
      </span>
    </button>
  );
}

export function Empty({ title, body, action }) {
  return (
    <div className="empty">
      <div className="empty-mark" aria-hidden="true" />
      <h4>{title}</h4>
      {body && <p className="muted">{body}</p>}
      {action}
    </div>
  );
}

export function Loading({ label = "Loading" }) {
  return (
    <div className="loading" role="status">
      <span className="spin" />
      <span className="muted small">{label}</span>
    </div>
  );
}

export function ErrorNote({ error, retry }) {
  if (!error) return null;
  return (
    <div className="notice bad" role="alert">
      <div className="pre">{error}</div>
      {retry && (
        <button className="btn small" onClick={retry}>
          Try again
        </button>
      )}
    </div>
  );
}

export function Notice({ tone = "info", title, children }) {
  return (
    <div className={`notice ${tone}`}>
      {title && <strong>{title}</strong>}
      <div>{children}</div>
    </div>
  );
}

export function Button({ children, variant = "default", small, icon: Icon, className = "", ...rest }) {
  return (
    <button {...rest} className={`btn ${variant} ${small ? "small" : ""} ${className}`}>
      {Icon && <Icon size={small ? 14 : 16} strokeWidth={2} />}
      {children}
      {variant === "primary" && <LiquidLayer radius={small ? 8 : 10} bezel={small ? 8 : 10} depth={small ? 5 : 6} />}
    </button>
  );
}

export function Toggle({ checked, onChange, label, desc, disabled }) {
  return (
    <label className={`toggle-row ${disabled ? "disabled" : ""}`}>
      <span className="toggle-text">
        <span>{label}</span>
        {desc && <span className="muted small">{desc}</span>}
      </span>
      <span className="switch">
        <input type="checkbox" checked={!!checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
        <span className="track" />
      </span>
    </label>
  );
}

export function Segmented({ value, onChange, options, small }) {
  const { fluid } = useApp();
  const ref = useRef(null);
  const [thumb, setThumb] = useState(null);
  // Fluidity: one glass thumb that glides to the selected option.
  useLayoutEffect(() => {
    if (!fluid) return undefined;
    const el = ref.current;
    if (!el) return undefined;
    const measure = () => {
      const on = el.querySelector("button.on");
      setThumb(on ? { x: on.offsetLeft, w: on.offsetWidth } : null);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(measure);
    return () => ro.disconnect();
  }, [fluid, value, options.length]);
  return (
    <div className={`seg ${small ? "small" : ""}`} role="tablist" ref={ref}>
      {fluid && thumb && (
        <span className="seg-thumb" style={{ transform: `translateX(${thumb.x}px)`, width: thumb.w }}>
          <LiquidLayer radius={8} bezel={8} depth={5} />
        </span>
      )}
      {options.map((o) => (
        <button key={o.value} role="tab" aria-selected={value === o.value} className={value === o.value ? "on" : ""} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Pill({ tone = "muted", children, dot }) {
  return (
    <span className={`pill ${tone}`}>
      {dot && <i className="dot" />}
      {children}
    </span>
  );
}

export function Swatch({ color, size = 10 }) {
  return <i className="swatch" style={{ background: color, width: size, height: size }} />;
}

export function Drawer({ open, onClose, title, children }) {
  useEffect(() => {
    if (!open) return;
    const k = (e) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="drawer-wrap" onMouseDown={onClose}>
      <aside className="drawer" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label={title}>
        <header>
          <h3>{title}</h3>
          <button className="icon-btn" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </header>
        <div className="drawer-b">{children}</div>
      </aside>
    </div>
  );
}

export function Modal({ open, onClose, title, children, footer }) {
  useEffect(() => {
    if (!open) return;
    const k = (e) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="modal-wrap" onMouseDown={onClose}>
      <div className="modal" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label={title}>
        <h3>{title}</h3>
        <div className="modal-b">{children}</div>
        {footer && <div className="modal-f">{footer}</div>}
      </div>
    </div>
  );
}

export function KV({ k, v, mono }) {
  return (
    <div className="kv">
      <dt>{k}</dt>
      <dd className={mono ? "mono" : ""}>{v || <span className="muted">none</span>}</dd>
    </div>
  );
}
