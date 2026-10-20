//! Classify Windows Update titles. Pure text helpers shared by collectors and rules.

/// Updates that arrive daily or are not Windows quality updates. They must not
/// count as "Windows was updated" (verified: Defender definitions appear daily).
pub fn is_definition_or_tool_update(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("kb2267602") // Defender security intelligence
        || t.contains("kb4052623") // Defender platform
        || t.contains("kb915597") // Defender antimalware platform
        || t.contains("kb890830") // Malicious Software Removal Tool
        || t.contains("security intelligence update")
        || t.contains("definition update")
}

/// A Windows update: has a KB number and is not a definition/tool update.
/// Microsoft Store app updates have no KB number (verified on machine A).
pub fn is_os_update(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("kb") && !is_definition_or_tool_update(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_title_classification() {
        assert!(is_os_update(
            "2026-10 Cumulative Update for Windows 11 Version 25H2 for x64-based Systems (KB5127690)"
        ));
        assert!(!is_os_update(
            "Security Intelligence Update for Microsoft Defender Antivirus - KB2267602 (Version 1.4)"
        ));
        assert!(!is_os_update("9NKSQGP7F2NH-5319275A.WhatsAppDesktop"));
        assert!(!is_os_update(
            "Windows Malicious Software Removal Tool x64 - v5.1 (KB890830)"
        ));
    }
}
