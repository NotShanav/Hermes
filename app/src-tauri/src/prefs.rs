//! App-only preferences (`<data-dir>/hermes-app.json`): theme, time zone, auto-lock and the category rules.
//! Categories are a VIEW over the archive: they are applied when data is displayed and never written into it.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Rule {
    /// Matched case-insensitively against the executable name (`chrome.exe`, or just `chrome`).
    pub pattern: String,
    pub category: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CategoryDef {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Prefs {
    /// dark | light | system
    pub theme: String,
    /// None = use the Windows time zone.
    pub tz_override_minutes: Option<i32>,
    /// Lock the archive after this many idle minutes in the app (0 = never).
    pub auto_lock_minutes: u32,
    pub rules: Vec<Rule>,
    pub custom_categories: Vec<CategoryDef>,
    pub sidebar_collapsed: bool,
    /// modern (the original solid look) | fluidity (glass, blobs, liquid motion)
    pub ui_style: String,
    /// Fluidity intensity: full | subtle
    pub effects: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { theme: "dark".into(), tz_override_minutes: None, auto_lock_minutes: 10, rules: vec![], custom_categories: vec![], sidebar_collapsed: false, ui_style: "modern".into(), effects: "full".into() }
    }
}

impl Prefs {
    pub fn load(path: &Path) -> Prefs {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(serde_json::to_string_pretty(self).expect("prefs serialise").as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)
    }
}

/// (id, name, colour)
pub const BUILTIN: &[(&str, &str, &str)] = &[
    ("browsing", "Browsing", "#6EA8FE"),
    ("development", "Development", "#9B7BFF"),
    ("communication", "Communication", "#F472B6"),
    ("media", "Media", "#F5B04C"),
    ("creative", "Creative", "#2DD4BF"),
    ("office", "Office & notes", "#86D26B"),
    ("games", "Games", "#FB923C"),
    ("system", "System", "#8E8AA8"),
    ("private", "Private", "#5A5670"),
    ("other", "Other", "#C4B5FD"),
];

const LISTS: &[(&str, &[&str])] = &[
    ("browsing", &["chrome.exe", "msedge.exe", "firefox.exe", "brave.exe", "opera.exe", "vivaldi.exe", "arc.exe", "iexplore.exe", "zen.exe"]),
    (
        "development",
        &[
            "code.exe", "cursor.exe", "devenv.exe", "idea64.exe", "pycharm64.exe", "rustrover64.exe", "webstorm64.exe", "clion64.exe", "studio64.exe", "windowsterminal.exe", "wt.exe",
            "powershell.exe", "pwsh.exe", "cmd.exe", "conhost.exe", "notepad++.exe", "sublime_text.exe", "githubdesktop.exe", "postman.exe", "docker desktop.exe", "windhawk.exe",
        ],
    ),
    ("communication", &["discord.exe", "slack.exe", "teams.exe", "ms-teams.exe", "telegram.exe", "whatsapp.exe", "zoom.exe", "signal.exe", "outlook.exe", "olk.exe", "thunderbird.exe", "skype.exe"]),
    ("media", &["spotify.exe", "vlc.exe", "mpv.exe", "potplayermini64.exe", "wmplayer.exe", "itunes.exe", "musicbee.exe", "media.player.exe", "mpc-hc64.exe"]),
    ("creative", &["photoshop.exe", "illustrator.exe", "figma.exe", "blender.exe", "premiere pro.exe", "afterfx.exe", "resolve.exe", "obs64.exe", "gimp-2.10.exe", "krita.exe", "inkscape.exe"]),
    ("office", &["winword.exe", "excel.exe", "powerpnt.exe", "onenote.exe", "acrord32.exe", "acrobat.exe", "notion.exe", "obsidian.exe", "notepad.exe", "sumatrapdf.exe", "calibre.exe"]),
    ("games", &["steam.exe", "steamwebhelper.exe", "epicgameslauncher.exe", "battle.net.exe", "riotclientservices.exe", "valorant-win64-shipping.exe", "gog galaxy.exe"]),
    (
        "system",
        &[
            "explorer.exe", "systemsettings.exe", "taskmgr.exe", "applicationframehost.exe", "searchhost.exe", "searchapp.exe", "startmenuexperiencehost.exe", "shellexperiencehost.exe",
            "lockapp.exe", "regedit.exe", "mmc.exe", "control.exe", "snippingtool.exe", "screenclippinghost.exe", "widgets.exe",
        ],
    ),
];

/// Category id for an application name: user rules first, then the built-in lists, else `other`.
pub fn categorize(app: &str, rules: &[Rule]) -> String {
    if app.starts_with('<') {
        return "private".into();
    }
    let lower = app.to_lowercase();
    for r in rules {
        let p = r.pattern.trim().to_lowercase();
        if !p.is_empty() && (lower == p || lower.contains(&p)) {
            return r.category.clone();
        }
    }
    for (id, names) in LISTS {
        if names.contains(&lower.as_str()) {
            return (*id).to_string();
        }
    }
    "other".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_user_rules_and_private_markers() {
        assert_eq!(categorize("Chrome.exe", &[]), "browsing");
        assert_eq!(categorize("<excluded application>", &[]), "private");
        assert_eq!(categorize("mygame.exe", &[]), "other");
        let rules = vec![Rule { pattern: "mygame".into(), category: "games".into() }, Rule { pattern: "chrome".into(), category: "office".into() }];
        assert_eq!(categorize("mygame.exe", &rules), "games");
        assert_eq!(categorize("chrome.exe", &rules), "office", "user rules win over built-in lists");
    }

    #[test]
    fn old_prefs_files_without_the_style_fields_still_load() {
        let d = std::env::temp_dir().join(format!("hermes-prefs-old-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("hermes-app.json");
        std::fs::write(&p, r#"{"theme":"light","auto_lock_minutes":5}"#).unwrap();
        let x = Prefs::load(&p);
        assert_eq!((x.theme.as_str(), x.auto_lock_minutes), ("light", 5));
        assert_eq!((x.ui_style.as_str(), x.effects.as_str()), ("modern", "full"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn prefs_roundtrip_and_defaults() {
        let d = std::env::temp_dir().join(format!("hermes-prefs-test-{}", std::process::id()));
        let p = d.join("hermes-app.json");
        assert_eq!(Prefs::load(&p).theme, "dark");
        let mut x = Prefs::default();
        x.auto_lock_minutes = 3;
        x.save(&p).unwrap();
        assert_eq!(Prefs::load(&p).auto_lock_minutes, 3);
        let _ = std::fs::remove_dir_all(d);
    }
}
