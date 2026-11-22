//! File chooser through the desktop portal (`org.freedesktop.portal.FileChooser`), using the zbus the app already has:
//! opens the desktop's own file dialog (KDE's on Plasma) and also works from an AppImage or a Flatpak. No extra
//! dependency. The dialog runs on a thread so the window keeps drawing; `poll` it every frame.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use zbus::zvariant::{OwnedValue, Value};

/// The dialog's D-Bus connection and request path, filled in by its thread, so `cancel` can close it.
type Req = std::sync::Arc<std::sync::Mutex<Option<(zbus::blocking::Connection, String)>>>;

pub struct Pick {
    rx: Receiver<Result<Option<PathBuf>, String>>,
    tx: Sender<Result<Option<PathBuf>, String>>, // lets `cancel` answer for a portal that sends no reply after Close
    req: Req,
}

impl Pick {
    /// The desktop's "open a file" dialog.
    pub fn start(title: &str) -> Pick {
        Self::spawn("OpenFile", title, None)
    }

    /// The desktop's "save as" dialog, with `name` suggested as the file name; the result is where to write.
    pub fn save(title: &str, name: &str) -> Pick {
        Self::spawn("SaveFile", title, Some(name.to_string()))
    }

    fn spawn(method: &'static str, title: &str, name: Option<String>) -> Pick {
        let (tx, rx) = channel();
        let title = title.to_string();
        let req = Req::default();
        let theirs = req.clone();
        let (tx2, tx_thread) = (tx.clone(), tx);
        std::thread::spawn(move || {
            let _ = tx_thread.send(run(method, &title, name.as_deref(), &theirs));
        });
        Pick { rx, tx: tx2, req }
    }

    /// Closes the dialog and reports it as cancelled. False if the portal could not be reached.
    /// shortcut: the portal sends no reply after Close, so the dialog's thread stays blocked on its socket for the
    /// rest of the run (one idle thread per cancelled dialog); upgrade if dialogs ever get cancelled in bulk.
    pub fn cancel(&self) -> bool {
        let Some((conn, path)) = self.req.lock().ok().and_then(|g| g.clone()) else { return false };
        let ok = conn.call_method(Some("org.freedesktop.portal.Desktop"), path.as_str(), Some("org.freedesktop.portal.Request"), "Close", &()).is_ok();
        if ok {
            let _ = self.tx.send(Ok(None));
        }
        ok
    }

    /// None while the dialog is still open; Some(Ok(None)) when it was cancelled.
    pub fn poll(&self) -> Option<Result<Option<PathBuf>, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("The file chooser closed unexpectedly.".into())),
        }
    }
}

fn s<E: std::fmt::Display>(e: E) -> String {
    format!("No file chooser available ({e}). Drag the file onto the window instead.")
}

fn run(method: &str, title: &str, name: Option<&str>, req: &Req) -> Result<Option<PathBuf>, String> {
    let conn = zbus::blocking::Connection::session().map_err(s)?;
    let me = conn.unique_name().map(|n| n.as_str().trim_start_matches(':').replace('.', "_")).ok_or("no D-Bus name")?;
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0); // one request name per dialog
    let token = format!("airscope{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let path = format!("/org/freedesktop/portal/desktop/request/{me}/{token}");
    if let Ok(mut g) = req.lock() {
        *g = Some((conn.clone(), path.clone()));
    }
    // listen for the answer before asking, so it cannot be missed
    let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface("org.freedesktop.portal.Request").map_err(s)?.member("Response").map_err(s)?.path(path.as_str()).map_err(s)?.build();
    let mut replies = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(4)).map_err(s)?;
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("handle_token", Value::from(token.as_str()));
    if let Some(n) = name {
        opts.insert("current_name", Value::from(n));
    }
    conn.call_method(Some("org.freedesktop.portal.Desktop"), "/org/freedesktop/portal/desktop", Some("org.freedesktop.portal.FileChooser"), method, &("", title, opts)).map_err(s)?;
    let msg = replies.next().ok_or("The file chooser closed unexpectedly.")?.map_err(s)?;
    let (code, res): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize().map_err(s)?;
    if code != 0 {
        return Ok(None); // cancelled
    }
    let uris: Vec<String> = res.get("uris").and_then(|v| v.try_clone().ok()).and_then(|v| v.try_into().ok()).unwrap_or_default();
    Ok(uris.first().map(|u| PathBuf::from(decode(u.strip_prefix("file://").unwrap_or(u)))))
}

/// Undo the %20-style escapes of a file URI.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_uri_escapes() {
        assert_eq!(super::decode("/home/peter/My%20Files/a%2Bb.snoop"), "/home/peter/My Files/a+b.snoop");
        assert_eq!(super::decode("/plain/100%"), "/plain/100%");
    }
}
