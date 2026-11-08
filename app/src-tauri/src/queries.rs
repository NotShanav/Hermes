//! Aggregations over focus segments. Pure functions: no I/O, no Tauri types, unit-tested.

use crate::prefs::{categorize, Rule};
use crate::segments::{slices, Seg, US_DAY, US_HOUR};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Serialize, Default, Clone, Debug)]
pub struct Totals {
    pub active_ms: i64,
    pub apps: u64,
    pub focus_periods: u64,
    pub longest_ms: i64,
    pub avg_focus_ms: i64,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
    pub days_active: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct AppRow {
    pub app: String,
    pub category: String,
    pub ms: i64,
    pub share: f64,
    pub focus_periods: u64,
    pub avg_ms: i64,
    pub longest_ms: i64,
    pub days: u64,
    pub first_ms: i64,
    pub last_ms: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct CatRow {
    pub category: String,
    pub ms: i64,
    pub share: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct TitleRow {
    pub title: String,
    pub app: String,
    pub category: String,
    pub ms: i64,
    pub visits: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct DayRow {
    /// Midnight of the local day, expressed as if local time were UTC (format it with getUTC* in the UI).
    pub day_ms: i64,
    pub ms: i64,
    pub apps: u64,
    pub focus_periods: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Summary {
    pub totals: Totals,
    /// Active milliseconds per local hour of day (24 entries) summed over the range.
    pub hourly: Vec<i64>,
    pub apps: Vec<AppRow>,
    pub categories: Vec<CatRow>,
    pub days: Vec<DayRow>,
    pub titles: Vec<TitleRow>,
}

#[derive(Default)]
struct DayAcc {
    us: i64,
    apps: HashSet<String>,
    focus: HashSet<u64>,
}

pub fn summarize<'a, I: Iterator<Item = &'a Seg>>(it: I, rules: &[Rule], off: i32, from_us: i64, to_us: i64) -> Summary {
    let mut app_us: HashMap<String, i64> = HashMap::new();
    let mut app_first: HashMap<String, i64> = HashMap::new();
    let mut app_last: HashMap<String, i64> = HashMap::new();
    let mut app_days: HashMap<String, HashSet<i64>> = HashMap::new();
    let mut focus: HashMap<u64, (String, i64)> = HashMap::new();
    let mut titles: HashMap<(String, String), (i64, u64)> = HashMap::new();
    let mut hourly = vec![0i64; 24];
    let mut days: BTreeMap<i64, DayAcc> = BTreeMap::new();
    let mut first: Option<i64> = None;
    let mut last: Option<i64> = None;
    let mut total_us = 0i64;

    for sg in it {
        let a = sg.start_us.max(from_us);
        let b = sg.end_us.min(to_us);
        if b <= a {
            continue;
        }
        let dur = b - a;
        total_us += dur;
        *app_us.entry(sg.app.clone()).or_insert(0) += dur;
        let f = app_first.entry(sg.app.clone()).or_insert(a);
        if a < *f {
            *f = a;
        }
        let l = app_last.entry(sg.app.clone()).or_insert(b);
        if b > *l {
            *l = b;
        }
        focus.entry(sg.focus).or_insert_with(|| (sg.app.clone(), 0)).1 += dur;
        if !sg.title.trim().is_empty() {
            let t = titles.entry((sg.app.clone(), sg.title.clone())).or_insert((0, 0));
            t.0 += dur;
            t.1 += 1;
        }
        first = Some(first.map_or(a, |x| x.min(a)));
        last = Some(last.map_or(b, |x| x.max(b)));
        slices(a, b, off, US_HOUR, |h, s0, e0| {
            hourly[h.rem_euclid(24) as usize] += e0 - s0;
        });
        slices(a, b, off, US_DAY, |d, s0, e0| {
            let acc = days.entry(d).or_default();
            acc.us += e0 - s0;
            acc.apps.insert(sg.app.clone());
            acc.focus.insert(sg.focus);
            app_days.entry(sg.app.clone()).or_default().insert(d);
        });
    }

    // Focus periods per application.
    let mut per_app: HashMap<String, (u64, i64)> = HashMap::new(); // (periods, longest us)
    let mut longest = 0i64;
    for (app, us) in focus.values() {
        let e = per_app.entry(app.clone()).or_insert((0, 0));
        e.0 += 1;
        if *us > e.1 {
            e.1 = *us;
        }
        if *us > longest {
            longest = *us;
        }
    }

    let total_f = total_us.max(1) as f64;
    let mut apps: Vec<AppRow> = app_us
        .iter()
        .map(|(app, us)| {
            let (periods, long_us) = per_app.get(app).copied().unwrap_or((0, 0));
            AppRow {
                app: app.clone(),
                category: categorize(app, rules),
                ms: us / 1000,
                share: *us as f64 / total_f,
                focus_periods: periods,
                avg_ms: if periods > 0 { us / periods as i64 / 1000 } else { 0 },
                longest_ms: long_us / 1000,
                days: app_days.get(app).map_or(0, |d| d.len() as u64),
                first_ms: app_first.get(app).copied().unwrap_or(0) / 1000,
                last_ms: app_last.get(app).copied().unwrap_or(0) / 1000,
            }
        })
        .collect();
    apps.sort_by(|x, y| y.ms.cmp(&x.ms).then(x.app.cmp(&y.app)));

    let mut cat_us: HashMap<String, i64> = HashMap::new();
    for (app, us) in &app_us {
        *cat_us.entry(categorize(app, rules)).or_insert(0) += *us;
    }
    let mut categories: Vec<CatRow> = cat_us.into_iter().map(|(category, us)| CatRow { category, ms: us / 1000, share: us as f64 / total_f }).collect();
    categories.sort_by(|x, y| y.ms.cmp(&x.ms).then(x.category.cmp(&y.category)));

    let mut title_rows: Vec<TitleRow> = titles
        .into_iter()
        .map(|((app, title), (us, visits))| TitleRow { category: categorize(&app, rules), title, app, ms: us / 1000, visits })
        .collect();
    title_rows.sort_by(|x, y| y.ms.cmp(&x.ms).then(x.title.cmp(&y.title)));
    title_rows.truncate(300);

    let day_rows: Vec<DayRow> = days
        .iter()
        .map(|(d, acc)| DayRow { day_ms: d * 86_400_000, ms: acc.us / 1000, apps: acc.apps.len() as u64, focus_periods: acc.focus.len() as u64 })
        .collect();

    let periods = focus.len() as u64;
    let totals = Totals {
        active_ms: total_us / 1000,
        apps: apps.len() as u64,
        focus_periods: periods,
        longest_ms: longest / 1000,
        avg_focus_ms: if periods > 0 { total_us / periods as i64 / 1000 } else { 0 },
        first_ms: first.map(|x| x / 1000),
        last_ms: last.map(|x| x / 1000),
        days_active: day_rows.len() as u64,
    };

    Summary { totals, hourly: hourly.into_iter().map(|u| u / 1000).collect(), apps, categories, days: day_rows, titles: title_rows }
}

#[derive(Serialize, Clone, Debug)]
pub struct TlSeg {
    pub s: i64,
    pub e: i64,
    pub app: String,
    pub category: String,
    pub title: String,
    pub focus: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Timeline {
    pub segments: Vec<TlSeg>,
    pub truncated: bool,
}

pub fn timeline(segs: &[Seg], rules: &[Rule], from_us: i64, to_us: i64, limit: usize) -> Timeline {
    let mut out = Vec::new();
    let mut truncated = false;
    for sg in segs {
        let a = sg.start_us.max(from_us);
        let b = sg.end_us.min(to_us);
        if b <= a {
            continue;
        }
        if out.len() >= limit {
            truncated = true;
            break;
        }
        out.push(TlSeg { s: a / 1000, e: b / 1000, app: sg.app.clone(), category: categorize(&sg.app, rules), title: sg.title.clone(), focus: sg.focus });
    }
    Timeline { segments: out, truncated }
}

#[derive(Serialize, Clone, Debug)]
pub struct FocusRow {
    pub start_ms: i64,
    pub end_ms: i64,
    pub ms: i64,
    pub app: String,
    pub category: String,
    pub title: String,
    pub titles: u64,
}

struct FocusAcc {
    start: i64,
    end: i64,
    us: i64,
    app: String,
    title: String,
    titles: u64,
}

/// One row per uninterrupted stay in a window ("session explorer"), newest first.
pub fn focus_rows<'a, I: Iterator<Item = &'a Seg>>(it: I, rules: &[Rule], from_us: i64, to_us: i64, limit: usize) -> Vec<FocusRow> {
    let mut m: HashMap<u64, FocusAcc> = HashMap::new();
    for sg in it {
        let a = sg.start_us.max(from_us);
        let b = sg.end_us.min(to_us);
        if b <= a {
            continue;
        }
        let acc = m.entry(sg.focus).or_insert_with(|| FocusAcc { start: a, end: b, us: 0, app: sg.app.clone(), title: sg.title.clone(), titles: 0 });
        if a < acc.start {
            acc.start = a;
            acc.title = sg.title.clone();
        }
        if b > acc.end {
            acc.end = b;
        }
        acc.us += b - a;
        acc.titles += 1;
    }
    let mut rows: Vec<FocusRow> = m
        .into_values()
        .map(|x| FocusRow { start_ms: x.start / 1000, end_ms: x.end / 1000, ms: x.us / 1000, category: categorize(&x.app, rules), app: x.app, title: x.title, titles: x.titles })
        .collect();
    rows.sort_by(|a, b| b.start_ms.cmp(&a.start_ms));
    rows.truncate(limit);
    rows
}

/// Active milliseconds per weekday (Monday = 0) and local hour: a 7 x 24 grid.
pub fn heat<'a, I: Iterator<Item = &'a Seg>>(it: I, off: i32, from_us: i64, to_us: i64) -> Vec<Vec<i64>> {
    let mut g = vec![vec![0i64; 24]; 7];
    for sg in it {
        let a = sg.start_us.max(from_us);
        let b = sg.end_us.min(to_us);
        if b <= a {
            continue;
        }
        slices(a, b, off, US_HOUR, |h, s0, e0| {
            let day = h.div_euclid(24);
            let wd = (day + 3).rem_euclid(7) as usize; // 1970-01-01 was a Thursday
            g[wd][h.rem_euclid(24) as usize] += (e0 - s0) / 1000;
        });
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_s: i64, end_s: i64, app: &str, title: &str, focus: u64) -> Seg {
        Seg { start_us: start_s * 1_000_000, end_us: end_s * 1_000_000, app: app.into(), exe: String::new(), title: title.into(), focus }
    }

    #[test]
    fn summary_totals_apps_titles_and_clipping() {
        let segs = vec![seg(0, 60, "a.exe", "x", 1), seg(60, 90, "a.exe", "y", 1), seg(90, 150, "chrome.exe", "page", 2)];
        let s = summarize(segs.iter(), &[], 0, 0, i64::MAX / 4);
        assert_eq!(s.totals.active_ms, 150_000);
        assert_eq!(s.totals.focus_periods, 2);
        assert_eq!(s.totals.longest_ms, 90_000);
        assert_eq!(s.apps[0].app, "a.exe");
        assert_eq!(s.apps[0].focus_periods, 1, "a title change is not a new focus period");
        assert_eq!(s.apps[1].category, "browsing");
        assert_eq!(s.titles.len(), 3);
        assert_eq!(s.hourly[0], 150_000);
        // Clip to the last minute only.
        let c = summarize(segs.iter(), &[], 0, 90 * 1_000_000, i64::MAX / 4);
        assert_eq!(c.totals.active_ms, 60_000);
        assert_eq!(c.apps.len(), 1);
    }

    #[test]
    fn heat_puts_time_on_the_right_weekday() {
        // 1970-01-01 (Thursday) 10:00 UTC for one hour.
        let segs = vec![seg(10 * 3600, 11 * 3600, "a.exe", "", 1)];
        let g = heat(segs.iter(), 0, 0, i64::MAX / 4);
        assert_eq!(g[3][10], 3_600_000);
        assert_eq!(g.iter().flatten().sum::<i64>(), 3_600_000);
    }

    #[test]
    fn focus_rows_merge_title_segments() {
        let segs = vec![seg(0, 10, "a.exe", "x", 1), seg(10, 30, "a.exe", "y", 1), seg(30, 40, "b.exe", "z", 2)];
        let r = focus_rows(segs.iter(), &[], 0, i64::MAX / 4, 10);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].app.as_str(), r[0].ms), ("b.exe", 10_000));
        assert_eq!((r[1].ms, r[1].titles), (30_000, 2));
    }
}
