//! Bluetooth problems from the system journal: what BlueZ and the kernel logged, for any time range, without a
//! recording. Less detail than a capture (no reason codes, no packet timing) but it reaches back in time.
//! Read-only: runs `journalctl` as the current user.

use std::process::Command;

/// How bad a log line is. `Problem` = a link was lost or the adapter complained; `Symptom` = something stopped
/// answering or a connection attempt failed; `Info` = normal-looking, but a run of them is a drop and reconnect.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sev {
    Problem,
    Symptom,
    Info,
}

/// Which device a line is about, as far as the line says.
#[derive(Clone, PartialEq, Debug)]
pub enum Who {
    Mac(String),
    Name(String),
    None,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub unix: f64,
    pub sev: Sev,
    pub who: Who,
    /// Plain-language description.
    pub text: String,
    /// Who logged it ("bluetoothd" or "kernel"), where in BlueZ ("profiles/input/device.c, control_connect_cb", empty
    /// for the kernel) and the message as logged: shown in the hover popup.
    pub source: String,
    pub origin: String,
    pub detail: String,
}

/// First 17-character `XX:XX:XX:XX:XX:XX` address in the text, upper-cased.
fn find_mac(s: &str) -> Option<String> {
    let b = s.as_bytes();
    (0..b.len().saturating_sub(16)).find_map(|i| {
        let w = &b[i..i + 17];
        let ok = w.iter().enumerate().all(|(k, c)| if k % 3 == 2 { *c == b':' } else { c.is_ascii_hexdigit() });
        ok.then(|| s[i..i + 17].to_ascii_uppercase())
    })
}

/// Sorts one journal message (without time and host) into a known kind of line, or `None` for the many lines that
/// aren't about a problem. `kernel` is true for kernel messages.
pub fn classify(msg: &str, kernel: bool) -> Option<(Sev, Who, String)> {
    let mac = find_mac(msg).map_or(Who::None, Who::Mac);
    if kernel {
        if msg.contains("BLUETOOTH HID") && msg.contains("input,hidraw") {
            // "... BLUETOOTH HID v0.01 Keyboard [K380 Keyboard] on aa:bb:..": the adapter's address follows, not the device's
            let name = msg.split_once('[').and_then(|(_, r)| r.split_once(']')).map(|(n, _)| n.to_string());
            return Some((Sev::Info, name.clone().map_or(Who::None, Who::Name), format!("{} registered as a new input device (it reconnected)", name.as_deref().unwrap_or("a Bluetooth input device"))));
        }
        let m = msg.to_ascii_lowercase();
        if m.contains("hci_conn_timeout") {
            return Some((Sev::Problem, Who::Name("Bluetooth adapter".into()), "kernel warning in hci_conn_timeout (a connection timed out)".into()));
        }
        if (m.contains("bluetooth") || m.contains("hci0")) && (m.contains("error") || m.contains("failed") || m.contains("timeout") || m.contains("timed out")) {
            return Some((Sev::Problem, Who::Name("Bluetooth adapter".into()), msg.trim_start_matches("Bluetooth: ").to_string()));
        }
        return None;
    }
    if msg.contains("HIDP GET_REPORT request timed out") {
        return Some((Sev::Symptom, mac, "the device stopped answering (HID report timed out)".into()));
    }
    if msg.contains("Host is down") {
        return Some((Sev::Problem, mac, "could not reconnect: host is down".into()));
    }
    if msg.contains("No reply to Suspend request") {
        return Some((Sev::Symptom, mac, "audio device did not answer a suspend request".into()));
    }
    if msg.contains("profile connect failed for") {
        let why = msg.rsplit_once(": ").map_or("", |(_, w)| w.trim());
        return Some((Sev::Symptom, mac, format!("audio profile connect failed ({})", why.to_lowercase())));
    }
    if msg.contains("failed connect to") && msg.contains("Connection refused") {
        return Some((Sev::Symptom, mac, "a profile refused the connection".into()));
    }
    None
}

/// One `journalctl -o short-unix` line: "1794190377.123456 host ident[pid]: message".
fn parse_line(line: &str, kernel: bool) -> Option<Entry> {
    let (ts, rest) = line.split_once(' ')?;
    let unix: f64 = ts.parse().ok()?;
    let (ident, msg) = rest.split_once(": ")?;
    let (sev, who, text) = classify(msg, kernel)?;
    let source = ident.rsplit(' ').next().unwrap_or(ident).split('[').next().unwrap_or("").to_string();
    // BlueZ prefixes messages with "path/file.c:function() "
    let (origin, detail) = match msg.split_once(' ') {
        Some((f, d)) if f.contains(".c:") && f.ends_with("()") => (f.trim_end_matches("()").replacen(".c:", ".c, ", 1), d.to_string()),
        _ => (String::new(), msg.to_string()),
    };
    Some(Entry { unix, sev, who, text, source, origin, detail })
}

fn journal(args: &[&str], kernel: bool) -> Result<Vec<Entry>, String> {
    let out = Command::new("/usr/bin/journalctl").args(args).args(["-o", "short-unix", "--no-pager", "-q"]).output().map_err(|e| format!("Cannot run journalctl: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("The system log could not be read ({}). Your user may need to be in the systemd-journal group.", err.trim().lines().next().unwrap_or("journalctl failed")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().take(400_000).filter_map(|l| parse_line(l, kernel)).collect())
}

/// When the journal's oldest Bluetooth entry was logged (unix seconds): reads just the first line and stops.
pub fn oldest_entry() -> Option<f64> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let mut child = Command::new("/usr/bin/journalctl").args(["-u", "bluetooth.service", "--since", "@0", "-o", "short-unix", "--no-pager", "-q"]).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut line = String::new();
    BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
    let _ = child.kill();
    let _ = child.wait();
    line.split_once(' ')?.0.parse().ok()
}

/// Bluetooth problems between two moments (unix seconds), oldest first.
pub fn read(since: i64, until: i64) -> Result<Vec<Entry>, String> {
    let (s, u) = (format!("@{since}"), format!("@{until}"));
    let mut all = journal(&["-u", "bluetooth.service", "--since", &s, "--until", &u], false)?;
    all.extend(journal(&["-k", "--since", &s, "--until", &u], true)?);
    all.sort_by(|a, b| a.unix.partial_cmp(&b.unix).unwrap_or(std::cmp::Ordering::Equal));
    Ok(all)
}

/// When this boot began (unix seconds), from /proc/stat.
pub fn boot_time() -> Option<i64> {
    std::fs::read_to_string("/proc/stat").ok()?.lines().find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())
}

/// Groups of lines no more than `gap` seconds apart that hold at least two lines (a lone reconnect isn't a burst):
/// (first moment, last moment, lines), unix seconds.
pub fn burst_spans(entries: &[Entry], gap: f64) -> Vec<(f64, f64, usize)> {
    let mut out: Vec<(f64, f64, usize)> = vec![];
    for e in entries {
        match out.last_mut() {
            Some(b) if e.unix - b.1 <= gap => {
                b.1 = e.unix;
                b.2 += 1;
            }
            _ => out.push((e.unix, e.unix, 1)),
        }
    }
    out.retain(|b| b.2 >= 2);
    out
}

pub fn bursts(entries: &[Entry], gap: f64) -> usize {
    burst_spans(entries, gap).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_lines_are_recognised_and_noise_is_dropped() {
        let hid = "profiles/input/device.c:hidp_get_report_cb() Device F4:73:35:AA:00:2F HIDP GET_REPORT request timed out";
        let (sev, who, _) = classify(hid, false).unwrap();
        assert_eq!((sev, who), (Sev::Symptom, Who::Mac("F4:73:35:AA:00:2F".into())));
        assert_eq!(classify("profiles/input/device.c:control_connect_cb() connect to F4:73:35:AA:00:2F: Host is down (112)", false).unwrap().0, Sev::Problem);
        assert_eq!(classify("plugins/policy.c:policy_grace_timeout() Fallback profile connection failed: No such file or directory (2)", false), None);
        assert_eq!(classify("Endpoint registered: sender=:1.3 path=/MediaEndpoint/A2DPSource/sbc", false), None);
    }

    #[test]
    fn kernel_keyboard_registration_names_the_keyboard() {
        let l = "hid-generic 0005:05AC:0220.0007: input,hidraw3: BLUETOOTH HID v0.01 Keyboard [K380 Keyboard] on d0:ab:d5:11:22:33";
        let (sev, who, text) = classify(l, true).unwrap();
        assert_eq!((sev, who), (Sev::Info, Who::Name("K380 Keyboard".into())));
        assert!(text.contains("K380 Keyboard"));
        assert_eq!(classify("NET: Registered PF_BLUETOOTH protocol family", true), None);
    }

    #[test]
    fn a_journal_line_is_split_into_time_and_message() {
        let e = parse_line("1794190377.123456 peter-pc bluetoothd[2148]: profiles/input/device.c:control_connect_cb() connect to F4:73:35:AA:00:2F: Host is down (112)", false).unwrap();
        assert!((e.unix - 1794190377.123456).abs() < 1e-6);
        assert_eq!(e.who, Who::Mac("F4:73:35:AA:00:2F".into()));
        assert_eq!((e.source.as_str(), e.origin.as_str()), ("bluetoothd", "profiles/input/device.c, control_connect_cb"));
        assert_eq!(e.detail, "connect to F4:73:35:AA:00:2F: Host is down (112)");
    }

    #[test]
    fn bursts_need_two_lines_close_together() {
        let e = |t: f64| Entry { unix: t, sev: Sev::Symptom, who: Who::None, text: String::new(), source: String::new(), origin: String::new(), detail: String::new() };
        assert_eq!(bursts(&[e(0.0), e(30.0), e(1000.0), e(5000.0), e(5050.0), e(5100.0)], 120.0), 2);
        assert_eq!(bursts(&[e(0.0), e(1000.0)], 120.0), 0);
    }
}
