//! Tray icon artwork, generated in code (no image files to ship or tamper with).
//! The four looks must be unmistakable at 16x16: red = recording, grey bars = paused,
//! hollow ring = not recording because the system is locked/asleep, amber = recording with a problem.

use hermes_tracker::CollectionState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconKind {
    Recording,
    Paused,
    Inactive,
    Warning,
}

pub fn kind_for(state: CollectionState, has_warnings: bool) -> IconKind {
    match state {
        CollectionState::Recording if has_warnings => IconKind::Warning,
        CollectionState::Recording => IconKind::Recording,
        CollectionState::Paused => IconKind::Paused,
        _ => IconKind::Inactive,
    }
}

pub const RED: (u8, u8, u8) = (0xD8, 0x2B, 0x2B);
pub const AMBER: (u8, u8, u8) = (0xF2, 0xA9, 0x00);
const GREY: (u8, u8, u8) = (0x6E, 0x6E, 0x6E);
const WHITE: (u8, u8, u8) = (0xFF, 0xFF, 0xFF);
const DARK: (u8, u8, u8) = (0x20, 0x20, 0x20);

fn cov(d: f32, edge: f32) -> f32 {
    (edge + 0.5 - d).clamp(0.0, 1.0)
}

/// Premultiplied BGRA, top-down, `size*size*4` bytes.
pub fn pixels(kind: IconKind, size: u32) -> Vec<u8> {
    let s = size as f32;
    let (cx, cy, r) = (s / 2.0 - 0.5, s / 2.0 - 0.5, s * 0.46);
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32, y as f32);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let (mut color, mut a) = match kind {
                IconKind::Recording => (RED, cov(d, r)),
                IconKind::Warning => (AMBER, cov(d, r)),
                IconKind::Paused => (GREY, cov(d, r)),
                IconKind::Inactive => (GREY, (cov(d, r) - cov(d, r - s * 0.12)).clamp(0.0, 1.0)),
            };
            match kind {
                IconKind::Paused => {
                    // two white vertical bars
                    let bar = |x0: f32| (fx - x0).abs() <= s * 0.07 && (fy - cy).abs() <= s * 0.22;
                    if bar(cx - s * 0.14) || bar(cx + s * 0.14) {
                        color = WHITE;
                        a = a.max(if d <= r { 1.0 } else { 0.0 });
                    }
                }
                IconKind::Warning => {
                    // dark exclamation mark
                    let stem = (fx - cx).abs() <= s * 0.06 && fy >= cy - s * 0.27 && fy <= cy + s * 0.10;
                    let dot = (fx - cx).abs() <= s * 0.07 && (fy - (cy + s * 0.25)).abs() <= s * 0.07;
                    if stem || dot {
                        color = DARK;
                    }
                }
                IconKind::Recording => {
                    // thin white ring inside the red disc: reads as a "record" button even in greyscale
                    let ring = (d - r * 0.62).abs() <= (s * 0.04).max(0.5);
                    if ring && d < r {
                        color = WHITE;
                    }
                }
                IconKind::Inactive => {}
            }
            let i = ((y * size + x) * 4) as usize;
            let alpha = (a * 255.0).round() as u32;
            out[i] = ((color.2 as u32 * alpha) / 255) as u8;
            out[i + 1] = ((color.1 as u32 * alpha) / 255) as u8;
            out[i + 2] = ((color.0 as u32 * alpha) / 255) as u8;
            out[i + 3] = alpha as u8;
        }
    }
    out
}
