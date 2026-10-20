//! Remove personal data before anything leaves the device.
//!
//! Replaces: the user name, the computer name, personal folder names
//! (`C:\Users\<name>`, `/home/<name>`, `/Users/<name>`), IPv4 addresses and MAC addresses.

use regex::Regex;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Debug, Clone, Default)]
pub struct Anonymizer {
    /// Words to replace with `<user>` (user name, full name...).
    pub user_names: Vec<String>,
    /// Words to replace with `<computer>`.
    pub computer_names: Vec<String>,
}

fn profile_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // C:\Users\name, C:/Users/name, /Users/name, /home/name
        #[allow(clippy::unwrap_used)]
        Regex::new(r#"(?i)((?:[a-z]:)?[\\/](?:users|home)[\\/]+)([^\\/"<>|:*?\r\n]+)"#).unwrap()
    })
}

fn ipv4_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    #[allow(clippy::unwrap_used)]
    RE.get_or_init(|| Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap())
}

fn mac_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    #[allow(clippy::unwrap_used)]
    RE.get_or_init(|| Regex::new(r"\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b").unwrap())
}

/// Folder names under C:\Users that are not personal.
const SHARED_PROFILES: [&str; 4] = ["public", "default", "default user", "all users"];

impl Anonymizer {
    /// Build an anonymizer for the current user and computer.
    pub fn for_current_user() -> Self {
        let mut a = Anonymizer::default();
        for var in ["USERNAME", "USER", "LOGNAME"] {
            if let Ok(v) = std::env::var(var) {
                a.user_names.push(v);
            }
        }
        for var in ["COMPUTERNAME", "HOSTNAME"] {
            if let Ok(v) = std::env::var(var) {
                a.computer_names.push(v);
            }
        }
        a
    }

    pub fn text(&self, input: &str) -> String {
        let s = self.names_only(input);
        let s = ipv4_re().replace_all(&s, "<ip>").into_owned();
        mac_re().replace_all(&s, "<mac>").into_owned()
    }

    /// Replace names and profile folders but keep dotted numbers (used for version fields,
    /// where "1.2.3.4" is a version, not an IP address).
    pub fn names_only(&self, input: &str) -> String {
        let mut s = profile_re()
            .replace_all(input, |c: &regex::Captures| {
                let name = &c[2];
                if SHARED_PROFILES.contains(&name.to_lowercase().as_str()) {
                    c[0].to_string()
                } else {
                    format!("{}<user>", &c[1])
                }
            })
            .into_owned();
        s = replace_words(&s, &self.user_names, "<user>");
        s = replace_words(&s, &self.computer_names, "<computer>");
        s
    }

    pub fn value(&self, v: &mut Value) {
        self.value_in(v, false);
    }

    fn value_in(&self, v: &mut Value, version_field: bool) {
        match v {
            Value::String(s) => {
                *s = if version_field {
                    self.names_only(s)
                } else {
                    self.text(s)
                }
            }
            Value::Array(items) => items
                .iter_mut()
                .for_each(|i| self.value_in(i, version_field)),
            Value::Object(map) => map.iter_mut().for_each(|(k, i)| {
                let is_version = k.contains("version") || k == "matched_range";
                self.value_in(i, is_version)
            }),
            _ => {}
        }
    }

    /// Anonymize any serializable value (Snapshot, findings...).
    pub fn apply<T: Serialize + DeserializeOwned>(
        &self,
        input: &T,
    ) -> Result<T, serde_json::Error> {
        let mut v = serde_json::to_value(input)?;
        self.value(&mut v);
        serde_json::from_value(v)
    }
}

/// Case-insensitive whole-word replacement. Words shorter than 3 characters are
/// ignored so we never mangle ordinary text.
fn replace_words(input: &str, words: &[String], with: &str) -> String {
    let mut out = input.to_string();
    for w in words {
        if w.chars().count() < 3 {
            continue;
        }
        let pattern = format!(r"(?i)\b{}\b", regex::escape(w));
        if let Ok(re) = Regex::new(&pattern) {
            out = re.replace_all(&out, with).into_owned();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anon() -> Anonymizer {
        Anonymizer {
            user_names: vec!["Shaan".into()],
            computer_names: vec!["LAPTOP-AB12CD".into()],
        }
    }

    #[test]
    fn replaces_profile_paths() {
        let a = anon();
        assert_eq!(
            a.text(r"C:\Users\Someone\Downloads\x.exe"),
            r"C:\Users\<user>\Downloads\x.exe"
        );
        assert_eq!(a.text("/home/bob/.env"), "/home/<user>/.env");
        assert_eq!(a.text("/Users/alice/Desktop"), "/Users/<user>/Desktop");
    }

    #[test]
    fn keeps_shared_profiles() {
        let a = anon();
        assert_eq!(
            a.text(r"C:\Users\Public\Desktop"),
            r"C:\Users\Public\Desktop"
        );
    }

    #[test]
    fn replaces_names_ip_and_mac() {
        let a = anon();
        let out = a.text("shaan on laptop-ab12cd at 192.168.1.20 mac 00:1A:2B:3C:4D:5E");
        assert_eq!(out, "<user> on <computer> at <ip> mac <mac>");
    }

    #[test]
    fn does_not_replace_inside_words() {
        let a = anon();
        assert_eq!(a.text("Shaanxi province"), "Shaanxi province");
    }

    #[test]
    fn version_fields_keep_dotted_numbers() {
        let a = anon();
        let mut v = serde_json::json!({
            "version": "1.2.3.4",
            "note": "server 10.1.2.3",
            "name": "Shaan's app"
        });
        a.value(&mut v);
        assert_eq!(v["version"], "1.2.3.4");
        assert_eq!(v["note"], "server <ip>");
        assert_eq!(v["name"], "<user>'s app");
        assert_eq!(a.text("10.0.26200"), "10.0.26200");
    }
}
