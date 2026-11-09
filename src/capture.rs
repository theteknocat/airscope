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
    /// None for a recording found already running (started by an earlier run of the app).
    child: Option<Child>,
    checked: Instant,
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
        Ok(Recording { child: Some(child), checked: Instant::now(), dir, file, cap, started: Instant::now() })
    }

    pub fn left(&self) -> Duration {
        self.cap.saturating_sub(self.started.elapsed())
    }

    /// A capture an earlier run of the app started that is still going: a btmon writing `capture.snoop` in a
    /// library session folder. The time left comes from the session's `meta.tsv`.
    pub fn adopt() -> Option<Recording> {
        let root = captures_dir()?;
        let (_, file) = btmon_files().into_iter().find(|(_, f)| session_dir_of(f).is_some() && f.starts_with(&root))?;
        let dir = file.parent()?.to_path_buf();
        let meta = std::fs::read_to_string(dir.join("meta.tsv")).ok()?;
        let get = |k: &str| meta.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('\t')?.trim().parse::<u64>().ok());
        let (start, cap) = (get("started")?, get("cap_secs")?);
        let elapsed = Duration::from_secs(now_unix().saturating_sub(start));
        Some(Recording { child: None, checked: Instant::now(), dir, file, cap: Duration::from_secs(cap), started: Instant::now().checked_sub(elapsed).unwrap_or_else(Instant::now) })
    }

    pub fn poll(&mut self) -> Outcome {
        let Some(child) = self.child.as_mut() else {
            // adopted: no child to wait for, so look for the btmon now and then (not every frame)
            if self.checked.elapsed() < Duration::from_secs(2) {
                return Outcome::Running;
            }
            self.checked = Instant::now();
            return if find_btmon(&self.file).is_some() { Outcome::Running } else { Outcome::Finished };
        };
        match child.try_wait() {
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
            None => match self.child.as_mut() {
                Some(c) => c.kill().map_err(|e| format!("Could not cancel: {e}")),
                None => Err("The capture has already ended.".into()),
            },
        }
    }
}

/// Every running `btmon -w <file>` as (pid, file), found by reading /proc (no external tool).
fn btmon_files() -> Vec<(u32, PathBuf)> {
    let mut out = vec![];
    for ent in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = ent.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let Ok(cmd) = std::fs::read(ent.path().join("cmdline")) else { continue };
        let args: Vec<&[u8]> = cmd.split(|b| *b == 0).collect();
        if !args.first().map_or(false, |a| a.ends_with(b"btmon")) {
            continue;
        }
        if let Some(w) = args.iter().position(|a| *a == b"-w") {
            if let Some(f) = args.get(w + 1).and_then(|f| std::str::from_utf8(f).ok()) {
                out.push((pid, PathBuf::from(f)));
            }
        }
    }
    out
}

/// Process id of the btmon writing to `file`.
fn find_btmon(file: &Path) -> Option<u32> {
    btmon_files().into_iter().find(|(_, f)| f == file).map(|(p, _)| p)
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

/// Deletes a saved session folder (only a direct child of the library that holds a capture file).
pub fn delete_session(dir: &Path) -> Result<(), String> {
    let root = captures_dir().ok_or("no library folder")?;
    if dir.parent() != Some(root.as_path()) || !dir.join(CAPTURE_FILE).is_file() {
        return Err("not a saved capture".into());
    }
    std::fs::remove_dir_all(dir).map_err(|e| format!("Could not delete: {e}"))
}

/// True when the saved summary is missing or older than the capture file (it grew after the last analysis).
pub fn needs_analysis(s: &Session) -> bool {
    let mt = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (mt(s.dir.join("summary.tsv")), mt(s.capture.clone())) {
        (Some(sum), Some(cap)) => sum < cap,
        _ => true,
    }
}

/// Reads a whole capture and saves its summary (run on a background thread for sessions nobody has opened).
pub fn analyse(dir: &Path, capture: &Path) {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(capture) else { return };
    let mut s = crate::snoop::Snoop::default();
    let mut buf = vec![0u8; 4 << 20];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if s.feed(&buf[..n]).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
    write_summary(dir, &Summary { records: s.records, drops: s.drops().count(), duration: s.duration() });
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
