//! Recording Bluetooth captures with the system's `btmon`, started through `pkexec` so the desktop shows its own
//! password prompt (no terminal, and no helper of our own running as root), plus the library of saved sessions.
//!
//! A session is a folder `<data dir>/airscope/captures/<YYYY-MM-DD_HH-MM-SS>/` holding `capture.snoop`, `meta.tsv`,
//! `summary.tsv` (written once the capture has been analysed) and `report.md` (once a report has been saved).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const CAPTURE_FILE: &str = "capture.snoop";
pub const REPORT_FILE: &str = "report.md";
const BTMON: &str = "/usr/bin/btmon";
const TIMEOUT: &str = "/usr/bin/timeout";
const KILL: &str = "/usr/bin/kill";
const PKEXEC: &str = "/usr/bin/pkexec";

pub fn captures_dir() -> Option<PathBuf> {
    let base = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("airscope/captures"))
}

/// The session folder a capture file belongs to, if it is in the library.
pub fn session_dir_of(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    (dir.parent()? == captures_dir()? && file.file_name()? == CAPTURE_FILE).then(|| dir.to_path_buf())
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// "YYYY-MM-DD_HH-MM-SS" in local time.
fn stamp() -> String {
    let secs = now_unix() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

// ---------- recording ----------

pub struct Recording {
    child: Child,
    pub dir: PathBuf,
    pub file: PathBuf,
    pub cap: Duration,
    started: Instant,
}

pub enum Outcome {
    Running,
    Finished,
    Failed(String),
}

impl Recording {
    /// Starts `btmon -w` as root for at most `cap`, after the desktop's password prompt. The capture file is created
    /// here first so it stays owned by the user (btmon, running as root, then only writes into it).
    pub fn start(cap: Duration) -> Result<Recording, String> {
        for bin in [PKEXEC, TIMEOUT, BTMON] {
            if !Path::new(bin).exists() {
                return Err(format!("{bin} was not found on this system."));
            }
        }
        let dir = captures_dir().ok_or("No home folder to save captures in.")?.join(stamp());
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
        let file = dir.join(CAPTURE_FILE);
        std::fs::File::create(&file).map_err(|e| format!("Cannot create {}: {e}", file.display()))?;
        let _ = std::fs::write(dir.join("meta.tsv"), format!("started\t{}\ncap_secs\t{}\n", now_unix(), cap.as_secs()));
        let child = Command::new(PKEXEC)
            .arg(TIMEOUT)
            .arg(cap.as_secs().to_string())
            .arg(BTMON)
            .arg("-w")
            .arg(&file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start the password prompt: {e}"))?;
        Ok(Recording { child, dir, file, cap, started: Instant::now() })
    }

    pub fn left(&self) -> Duration {
        self.cap.saturating_sub(self.started.elapsed())
    }

    pub fn poll(&mut self) -> Outcome {
        match self.child.try_wait() {
            Ok(None) => Outcome::Running,
            Ok(Some(st)) => {
                if std::fs::metadata(&self.file).map_or(false, |m| m.len() > 16) || st.success() {
                    return Outcome::Finished;
                }
                let _ = std::fs::remove_dir_all(&self.dir); // nothing was recorded: do not leave an empty session
                Outcome::Failed(match st.code() {
                    Some(126) => "The password prompt was dismissed or the password was wrong.".into(),
                    Some(127) => "No password prompt could be shown (is a polkit agent running?).".into(),
                    c => format!("The capture could not be started (exit code {c:?})."),
                })
            }
            Err(e) => Outcome::Failed(e.to_string()),
        }
    }

    /// Ends the capture early. Once btmon is running as root only root can stop it, so this asks for the password
    /// again; before the first prompt has been answered the waiting pkexec is simply cancelled.
    pub fn stop(&mut self) -> Result<(), String> {
        match find_btmon(&self.file) {
            Some(pid) => Command::new(PKEXEC).arg(KILL).arg("-TERM").arg(pid.to_string()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ()).map_err(|e| format!("Could not start the password prompt: {e}")),
            None => self.child.kill().map_err(|e| format!("Could not cancel: {e}")),
        }
    }
}

/// Process id of the btmon writing to `file`, found by reading /proc (no external tool).
fn find_btmon(file: &Path) -> Option<u32> {
    let want = file.to_str()?.as_bytes();
    for ent in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = ent.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let Ok(cmd) = std::fs::read(ent.path().join("cmdline")) else { continue };
        let args: Vec<&[u8]> = cmd.split(|b| *b == 0).collect();
        if args.first().map_or(false, |a| a.ends_with(b"btmon")) && args.iter().any(|a| *a == want) {
            return Some(pid);
        }
    }
    None
}

// ---------- library ----------

pub struct Summary {
    pub records: u64,
    pub drops: usize,
    pub duration: f64,
}

pub struct Session {
    pub dir: PathBuf,
    /// Folder name, "YYYY-MM-DD_HH-MM-SS" (the time the capture was started).
    pub name: String,
    pub capture: PathBuf,
    pub size: u64,
    pub report: bool,
    pub summary: Option<Summary>,
}

pub fn read_summary(dir: &Path) -> Option<Summary> {
    let t = std::fs::read_to_string(dir.join("summary.tsv")).ok()?;
    let f: Vec<&str> = t.trim().split('\t').collect();
    Some(Summary { records: f.first()?.parse().ok()?, drops: f.get(1)?.parse().ok()?, duration: f.get(2)?.parse().ok()? })
}

pub fn write_summary(dir: &Path, s: &Summary) {
    let _ = std::fs::write(dir.join("summary.tsv"), format!("{}\t{}\t{}\n", s.records, s.drops, s.duration));
}

/// Saved sessions, newest first.
pub fn list() -> Vec<Session> {
    captures_dir().map(|d| list_in(&d)).unwrap_or_default()
}

pub fn list_in(root: &Path) -> Vec<Session> {
    let mut out: Vec<Session> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            let capture = dir.join(CAPTURE_FILE);
            let size = std::fs::metadata(&capture).ok()?.len();
            Some(Session { name: e.file_name().to_string_lossy().into_owned(), report: dir.join(REPORT_FILE).exists(), summary: read_summary(&dir), dir, capture, size })
        })
        .collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_lists_newest_first_with_summaries() {
        let root = std::env::temp_dir().join(format!("airscope-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for n in ["2026-11-08_10-00-00", "2026-11-08_18-30-00"] {
            std::fs::create_dir_all(root.join(n)).unwrap();
            std::fs::write(root.join(n).join(CAPTURE_FILE), b"btsnoop\0").unwrap();
        }
        std::fs::create_dir_all(root.join("not-a-session")).unwrap();
        write_summary(&root.join("2026-11-08_18-30-00"), &Summary { records: 100, drops: 3, duration: 61.5 });
        let l = list_in(&root);
        assert_eq!(l.len(), 2, "folders without a capture are ignored");
        assert_eq!(l[0].name, "2026-11-08_18-30-00");
        let s = l[0].summary.as_ref().unwrap();
        assert_eq!((s.records, s.drops), (100, 3));
        assert!(l[1].summary.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn finds_nothing_for_an_unused_file() {
        assert!(find_btmon(Path::new("/nonexistent/capture.snoop")).is_none());
    }
}
