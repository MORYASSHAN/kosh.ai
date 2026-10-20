//! File helpers: Authenticode signature check, publisher (CompanyName) from
//! version info, shortcut (.lnk) targets, and executable path from a command line.
//! All read-only.

use regex::Regex;
use std::path::Path;
use windows::core::{Interface, GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Security::WinTrust::{
    WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
    WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE,
    WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};

/// Does the file carry a valid embedded Authenticode signature?
/// None when the file does not exist. Catalog-signed system files report false,
/// so callers only use this for files outside the Windows folder.
pub fn is_signed(path: &str) -> Option<bool> {
    if !Path::new(path).is_file() {
        return None;
    }
    let wide = HSTRING::from(path);
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: structures are fully initialised; state is closed below.
    let status = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut core::ffi::c_void,
        )
    };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: closes the state opened above.
    unsafe {
        let _ = WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut core::ffi::c_void,
        );
    }
    Some(status == 0)
}

/// CompanyName from the file's version resource.
pub fn company_name(path: &str) -> Option<String> {
    let wide = HSTRING::from(path);
    // SAFETY: two-call pattern with a correctly sized buffer.
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(
            PCWSTR(wide.as_ptr()),
            None,
            size,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
        )
        .ok()?;
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let tr = HSTRING::from(r"\VarFileInfo\Translation");
        let mut langs = Vec::new();
        if VerQueryValueW(
            buf.as_ptr() as *const core::ffi::c_void,
            PCWSTR(tr.as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            && len >= 4
        {
            let words = std::slice::from_raw_parts(ptr as *const u16, (len / 2) as usize);
            for pair in words.chunks(2) {
                if pair.len() == 2 {
                    langs.push(format!("{:04x}{:04x}", pair[0], pair[1]));
                }
            }
        }
        // Common fallbacks: US English Unicode, neutral Unicode.
        langs.extend([
            "040904b0".to_string(),
            "000004b0".to_string(),
            "040904e4".to_string(),
        ]);
        for l in langs {
            let key = HSTRING::from(format!(r"\StringFileInfo\{l}\CompanyName"));
            if VerQueryValueW(
                buf.as_ptr() as *const core::ffi::c_void,
                PCWSTR(key.as_ptr()),
                &mut ptr,
                &mut len,
            )
            .as_bool()
                && len > 1
            {
                let s = std::slice::from_raw_parts(ptr as *const u16, len as usize);
                let end = s.iter().position(|c| *c == 0).unwrap_or(s.len());
                let name = String::from_utf16_lossy(&s[..end]).trim().to_string();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
        None
    }
}

/// Target path and arguments of a .lnk shortcut.
pub fn resolve_lnk(path: &str) -> Option<(String, String)> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    // SAFETY: COM initialised on this thread; interfaces are released on drop.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(path), STGM_READ).ok()?;
        let mut target = [0u16; 1024];
        link.GetPath(&mut target, std::ptr::null_mut(), 0).ok()?;
        let mut args = [0u16; 1024];
        let _ = link.GetArguments(&mut args);
        let t = wide_to_string(&target);
        if t.is_empty() {
            return None;
        }
        Some((t, wide_to_string(&args)))
    }
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

fn env_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    #[allow(clippy::unwrap_used)]
    RE.get_or_init(|| Regex::new(r"%([A-Za-z0-9_()]+)%").unwrap())
}

/// Expand %VARS% using the current environment.
pub fn expand_env(s: &str) -> String {
    env_re()
        .replace_all(s, |c: &regex::Captures| {
            std::env::var(&c[1]).unwrap_or_else(|_| c[0].to_string())
        })
        .into_owned()
}

/// The executable path inside a command line, e.g.
/// `"C:\Program Files\App\app.exe" --min` -> `C:\Program Files\App\app.exe`.
pub fn image_from_command(cmd: &str) -> Option<String> {
    let c = expand_env(cmd.trim());
    if c.is_empty() {
        return None;
    }
    if let Some(rest) = c.strip_prefix('"') {
        return rest
            .split('"')
            .next()
            .map(str::to_string)
            .filter(|s| !s.is_empty());
    }
    let lower = c.to_ascii_lowercase();
    for ext in [".exe", ".com", ".bat", ".cmd", ".scr"] {
        if let Some(i) = lower.find(ext) {
            return Some(c[..i + ext.len()].to_string());
        }
    }
    c.split_whitespace().next().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_paths() {
        assert_eq!(
            image_from_command(r#""C:\Program Files\App\app.exe" --min"#).as_deref(),
            Some(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(
            image_from_command(r"C:\Program Files\App\app.exe /background").as_deref(),
            Some(r"C:\Program Files\App\app.exe")
        );
        assert_eq!(
            image_from_command(r"C:\Tools\run.cmd").as_deref(),
            Some(r"C:\Tools\run.cmd")
        );
        assert_eq!(image_from_command("  "), None);
    }

    #[test]
    fn notepad_is_not_embedded_signed_or_is() {
        // Smoke test: must not crash and must return Some for an existing file.
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let p = format!(r"{windir}\notepad.exe");
        if Path::new(&p).exists() {
            assert!(is_signed(&p).is_some());
        }
        assert_eq!(is_signed(r"C:\definitely\missing.exe"), None);
    }
}
