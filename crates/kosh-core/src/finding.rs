//! Findings: problems produced by deterministic rules.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Performance,
    Device,
    Stability,
    Security,
    Vulnerability,
    Hardware,
    Storage,
    Network,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Performance,
        Category::Device,
        Category::Stability,
        Category::Security,
        Category::Vulnerability,
        Category::Hardware,
        Category::Storage,
        Category::Network,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Category::Performance => "performance",
            Category::Device => "device",
            Category::Stability => "stability",
            Category::Security => "security",
            Category::Vulnerability => "vulnerability",
            Category::Hardware => "hardware",
            Category::Storage => "storage",
            Category::Network => "network",
        }
    }
}

/// Ordered from least to most serious, so `Severity::Critical > Severity::Low`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn key(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }

    /// "Fix now" in the report.
    pub fn is_urgent(self) -> bool {
        self >= Severity::High
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Severity::Info => "Info",
            Severity::Low => "Low",
            Severity::Medium => "Medium",
            Severity::High => "High",
            Severity::Critical => "Critical",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// Who can fix it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FixLevel {
    /// The user can fix it alone by following the steps.
    SelfFix,
    /// The user can fix it with Kosh's guidance (some steps are technical).
    GuidedAction,
    /// Needs a technician (hardware, BIOS, repeated failures).
    Technician,
}

impl FixLevel {
    pub fn key(self) -> &'static str {
        match self {
            FixLevel::SelfFix => "self",
            FixLevel::GuidedAction => "guided_action",
            FixLevel::Technician => "technician",
        }
    }
}

/// The ONLY things Kosh may do for the user. This list is closed on purpose:
/// none of these change anything on the computer. Adding a variant that
/// changes the system is forbidden in version 1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", content = "target")]
pub enum SafeAction {
    /// Open a Windows Settings page, e.g. `ms-settings:windowsupdate`.
    OpenSettings(String),
    /// Open a folder in Explorer / Finder.
    OpenFolder(String),
    /// Open a web page in the browser.
    OpenUrl(String),
    /// Put a command on the clipboard so the user can run it themselves.
    CopyCommand(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Evidence {
    /// e.g. "WMI Win32_PnPEntity", "EventLog System 1001"
    pub source: String,
    /// e.g. "ConfigManagerErrorCode"
    pub key: String,
    /// Raw value as collected.
    pub value: String,
    pub observed_at: Option<DateTime<Utc>>,
}

impl Evidence {
    pub fn new(source: impl Into<String>, key: impl Into<String>, value: impl ToString) -> Self {
        Evidence {
            source: source.into(),
            key: key.into(),
            value: value.to_string(),
            observed_at: None,
        }
    }

    pub fn at(mut self, t: Option<DateTime<Utc>>) -> Self {
        self.observed_at = t;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Finding {
    /// Stable rule id, e.g. "DEV-001".
    pub id: String,
    /// Which thing this finding is about when a rule can fire several times
    /// (a device name, an app name). None for single-instance rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub category: Category,
    pub severity: Severity,
    pub confidence: Confidence,
    pub title: String,
    pub summary: String,
    pub evidence: Vec<Evidence>,
    pub fix_level: FixLevel,
    pub fix_steps: Vec<String>,
    pub actions: Vec<SafeAction>,
    pub verify: String,
    pub references: Vec<String>,
}

impl Finding {
    /// A stable key for comparing findings across scans.
    pub fn key(&self) -> String {
        match &self.subject {
            Some(s) => format!("{}:{}", self.id, s),
            None => self.id.clone(),
        }
    }
}
