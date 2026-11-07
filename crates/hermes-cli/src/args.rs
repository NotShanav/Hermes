use std::collections::{HashMap, HashSet};

/// Minimal argument parser: `command [positionals] [--flag value]... [--switch]...`
#[derive(Debug, Default, Clone)]
pub struct Args {
    pub command: String,
    pub positionals: Vec<String>,
    flags: HashMap<String, Vec<String>>,
    switches: HashSet<String>,
}

const SWITCHES: &[&str] = &["--quiet", "--no-default-exclusions", "--paused", "--force", "--allow-same-drive", "--utc", "--help"];

impl Args {
    pub fn parse<I: IntoIterator<Item = String>>(it: I) -> Result<Args, String> {
        let mut it = it.into_iter();
        let mut a = Args { command: it.next().unwrap_or_else(|| "help".into()), ..Default::default() };
        while let Some(x) = it.next() {
            if SWITCHES.contains(&x.as_str()) {
                a.switches.insert(x);
            } else if x.starts_with("--") {
                let v = it.next().ok_or_else(|| format!("{x} needs a value"))?;
                a.flags.entry(x).or_default().push(v);
            } else {
                a.positionals.push(x);
            }
        }
        Ok(a)
    }
    pub fn value(&self, f: &str) -> Option<&str> {
        self.flags.get(f).and_then(|v| v.last()).map(|s| s.as_str())
    }
    pub fn values(&self, f: &str) -> Vec<String> {
        self.flags.get(f).cloned().unwrap_or_default()
    }
    pub fn has(&self, s: &str) -> bool {
        self.switches.contains(s)
    }
    /// Rejects flags the command does not know (typos must not be silently ignored).
    pub fn check_known(&self, flags: &[&str], switches: &[&str]) -> Result<(), String> {
        for f in self.flags.keys() {
            if !flags.contains(&f.as_str()) {
                return Err(format!("unknown option {f} for `hermes {}`", self.command));
            }
        }
        for s in &self.switches {
            if !switches.contains(&s.as_str()) && s != "--help" {
                return Err(format!("unknown option {s} for `hermes {}`", self.command));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(s: &str) -> Args {
        Args::parse(s.split_whitespace().map(String::from)).unwrap()
    }

    #[test]
    fn parses_commands_flags_switches_positionals_and_repeats() {
        let a = p("export --out a.csv --share-b E:\\k1 --share-b F:\\k2 --force text");
        assert_eq!(a.command, "export");
        assert_eq!(a.value("--out"), Some("a.csv"));
        assert_eq!(a.values("--share-b").len(), 2);
        assert!(a.has("--force") && !a.has("--quiet"));
        assert_eq!(a.positionals, vec!["text".to_string()]);
        assert_eq!(Args::parse(Vec::<String>::new()).unwrap().command, "help");
        assert!(Args::parse(["x".to_string(), "--out".to_string()]).is_err());
    }

    #[test]
    fn unknown_options_are_rejected() {
        let a = p("status --bogus 1");
        assert!(a.check_known(&["--data-dir"], &[]).unwrap_err().contains("--bogus"));
        let a = p("status --force");
        assert!(a.check_known(&["--data-dir"], &[]).is_err());
        assert!(p("status --data-dir x").check_known(&["--data-dir"], &[]).is_ok());
    }
}
