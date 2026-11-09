// Fluidity mode building blocks. Everything here is local and dependency-free: CSS blobs, an in-house SVG
// liquid-glass filter whose displacement map is a data: URL (allowed by the CSP), and small motion helpers.
// None of it renders in Modern mode.

import { useEffect, useRef, useState } from "react";
import { useApp } from "../store";

/** Fixed full-window field of slowly drifting colour blobs behind the whole app. Paused when the window is hidden,
 *  unfocused or the archive is locked, so an always-open app costs almost nothing while idle. */
export function Backdrop({ paused }) {
  useEffect(() => {
    const root = document.documentElement;
    const update = () => {
      const away = document.hidden || !document.hasFocus();
      root.dataset.paused = away || paused ? "1" : "0";
    };
    update();
    document.addEventListener("visibilitychange", update);
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    return () => {
      document.removeEventListener("visibilitychange", update);
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      delete root.dataset.paused;
    };
  }, [paused]);
  return (
    <div className="backdrop" aria-hidden="true">
      <i className="blob b1" />
      <i className="blob b2" />
      <i className="blob b3" />
      <i className="blob b4" />
      <i className="blob b5" />
      <i className="orb o1" />
      <i className="orb o2" />
      <i className="orb o3" />
      <i className="backdrop-grain" />
      <i className="backdrop-noise" />
    </div>
  );
}

// ---------------------------------------------------------------- liquid glass

const mapCache = new Map();

/** Displacement map for a rounded rectangle: neutral (128,128) in the middle, and along the rim a vector pointing
 *  inward that fades with distance from the edge (a convex bevel). Red = x, green = y, as feDisplacementMap expects. */
function makeMap(w, h, radius, bezel) {
  const key = `${w}x${h}r${radius}b${bezel}`;
  const hit = mapCache.get(key);
  if (hit) return hit;
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const g = c.getContext("2d");
  const img = g.createImageData(w, h);
  const hx = w / 2;
  const hy = h / 2;
  const r = Math.min(radius, hx, hy);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const px = x + 0.5 - hx;
      const py = y + 0.5 - hy;
      const qx = Math.abs(px) - (hx - r);
      const qy = Math.abs(py) - (hy - r);
      const ox = Math.max(qx, 0);
      const oy = Math.max(qy, 0);
      const len = Math.hypot(ox, oy);
      const sdf = len + Math.min(Math.max(qx, qy), 0) - r; // negative inside the shape
      const inside = -sdf;
      let dx = 0;
      let dy = 0;
      if (inside > 0 && inside < bezel) {
        const t = inside / bezel;
        const strength = (1 - t) * (1 - t); // strongest at the rim
        let nx;
        let ny; // outward normal
        if (len > 0) {
          nx = (px < 0 ? -1 : 1) * (ox / len);
          ny = (py < 0 ? -1 : 1) * (oy / len);
        } else if (qx > qy) {
          nx = px < 0 ? -1 : 1;
          ny = 0;
        } else {
          nx = 0;
          ny = py < 0 ? -1 : 1;
        }
        dx = -nx * strength; // inward
        dy = -ny * strength;
      }
      const i = (y * w + x) * 4;
      img.data[i] = Math.round(128 + 127 * dx);
      img.data[i + 1] = Math.round(128 + 127 * dy);
      img.data[i + 2] = 128;
      img.data[i + 3] = 255;
    }
  }
  g.putImageData(img, 0, 0);
  const url = c.toDataURL("image/png");
  if (mapCache.size > 40) mapCache.clear();
  mapCache.set(key, url);
  return url;
}

let filterSeq = 0;

/** A lens layer for a small, fixed-size glass element (give the parent `position: relative`). It refracts what is
 *  behind it through an SVG filter sized to the element. Renders nothing unless Fluidity is on at Full intensity. */
export function LiquidLayer({ radius = 12, bezel = 12, depth = 7, chroma = 0.1 }) {
  const { liquid } = useApp();
  const ref = useRef(null);
  const id = useRef(`hm-lens-${++filterSeq}`);
  const [box, setBox] = useState(null);

  useEffect(() => {
    if (!liquid) return undefined;
    const el = ref.current;
    if (!el) return undefined;
    const measure = () => setBox({ w: Math.round(el.offsetWidth), h: Math.round(el.offsetHeight) });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    // A highlight that follows the cursor across the glass.
    const host = el.parentElement;
    const move = (e) => {
      const r = host.getBoundingClientRect();
      el.style.setProperty("--sx", `${((e.clientX - r.left) / Math.max(1, r.width)) * 100}%`);
      el.style.setProperty("--sy", `${((e.clientY - r.top) / Math.max(1, r.height)) * 100}%`);
      el.style.setProperty("--sa", "1");
    };
    const leave = () => el.style.setProperty("--sa", "0");
    if (host) {
      host.addEventListener("pointermove", move);
      host.addEventListener("pointerleave", leave);
    }
    return () => {
      ro.disconnect();
      if (host) {
        host.removeEventListener("pointermove", move);
        host.removeEventListener("pointerleave", leave);
      }
    };
  }, [liquid]);

  if (!liquid) return null;
  const ready = box && box.w > 8 && box.h > 8;
  const map = ready ? makeMap(box.w, box.h, radius, bezel) : null;
  const S = depth * 1.2; // a few pixels of bend at the rim: refraction, not smearing
  const bf = ready ? `url(#${id.current}) blur(0.4px) saturate(1.5) brightness(1.06)` : undefined;
  return (
    <>
      {ready && (
        <svg className="liquid-defs" width="0" height="0" aria-hidden="true" focusable="false">
          <defs>
            <filter id={id.current} x="0" y="0" width={box.w} height={box.h} filterUnits="userSpaceOnUse" colorInterpolationFilters="sRGB">
              <feImage href={map} x="0" y="0" width={box.w} height={box.h} preserveAspectRatio="none" result="map" />
              {/* Red, green and blue are refracted by slightly different amounts, then recombined: colour fringing at the rim. */}
              <feDisplacementMap in="SourceGraphic" in2="map" scale={S * (1 - chroma)} xChannelSelector="R" yChannelSelector="G" result="dr" />
              <feColorMatrix in="dr" type="matrix" values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0" result="r" />
              <feDisplacementMap in="SourceGraphic" in2="map" scale={S} xChannelSelector="R" yChannelSelector="G" result="dg" />
              <feColorMatrix in="dg" type="matrix" values="0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0" result="g" />
              <feDisplacementMap in="SourceGraphic" in2="map" scale={S * (1 + chroma)} xChannelSelector="R" yChannelSelector="G" result="db" />
              <feColorMatrix in="db" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0" result="b" />
              <feBlend in="r" in2="g" mode="screen" result="rg" />
              <feBlend in="rg" in2="b" mode="screen" />
            </filter>
          </defs>
        </svg>
      )}
      <span ref={ref} className="liquid-layer" style={ready ? { backdropFilter: bf, WebkitBackdropFilter: bf, borderRadius: radius } : { borderRadius: radius }} />
    </>
  );
}

// ---------------------------------------------------------------- lock-screen delight

/** Three soft shapes that melt into each other behind the logo (gooey metaball filter, small area only). */
export function GooHalo() {
  const { liquid } = useApp();
  if (!liquid) return null;
  return (
    <>
      <svg className="liquid-defs" width="0" height="0" aria-hidden="true" focusable="false">
        <defs>
          <filter id="hm-goo" x="-20%" y="-20%" width="140%" height="140%">
            <feGaussianBlur in="SourceGraphic" stdDeviation="9" result="b" />
            <feColorMatrix in="b" mode="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 20 -9" />
          </filter>
        </defs>
      </svg>
      <div className="goo-halo" aria-hidden="true">
        <i className="g1" />
        <i className="g2" />
        <i className="g3" />
      </div>
    </>
  );
}

/** Lets the button lean a few pixels toward the cursor. The OUTER element never moves: it owns the pointer events and
 *  the hit area, and the position is always measured from it. Only the INNER element is transformed, so the pointer can
 *  never "escape" the thing that is measuring it (that feedback loop made the first version slide away from the cursor). */
export function Elastic({ children }) {
  const { liquid } = useApp();
  const inner = useRef(null);
  if (!liquid) return children;
  const clamp = (v) => Math.max(-1, Math.min(1, v));
  const move = (e) => {
    const box = e.currentTarget.getBoundingClientRect(); // outer: never transformed
    const el = inner.current;
    if (!el) return;
    const dx = clamp((e.clientX - (box.left + box.width / 2)) / (box.width / 2));
    const dy = clamp((e.clientY - (box.top + box.height / 2)) / (box.height / 2));
    el.style.transition = "transform 0.14s ease-out"; // follow smoothly, no overshoot while the pointer is moving
    el.style.transform = `translate(${(dx * 3).toFixed(2)}px, ${(dy * 2.5).toFixed(2)}px) scale(1.025)`;
  };
  const leave = () => {
    const el = inner.current;
    if (!el) return;
    el.style.transition = ""; // the springy release comes from the stylesheet
    el.style.transform = "";
  };
  return (
    <span className="elastic" onPointerMove={move} onPointerLeave={leave}>
      <span ref={inner} className="elastic-in">
        {children}
      </span>
    </span>
  );
}
