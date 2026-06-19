//! Diagnostic: run `claude /login` in a PTY, auto-dismiss prompts (CR for
//! Enter — raw-mode TUIs expect \r), dump every URL + full output.
//! Run: cargo run --example probe_cli_login

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

fn all_urls(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(idx) = rest.find("http") {
        let end = idx + rest[idx..].find(char::is_whitespace).unwrap_or(rest[idx..].len());
        let url = rest[idx..end].trim_end_matches(|c: char| ".,;:)]}>\"'".contains(c));
        out.push(url.to_string());
        rest = &rest[end..];
    }
    out
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut bytes = s.chars().peekable();
    while let Some(c) = bytes.next() {
        if c == '\u{1b}' {
            if bytes.peek() == Some(&'[') {
                bytes.next();
                for cc in bytes.by_ref() {
                    if cc.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pair = native_pty_system().openpty(PtySize {
        rows: 50,
        cols: 200,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new("claude");
    cmd.arg("/login");
    cmd.cwd("/tmp");
    let child = pair.slave.spawn_command(cmd)?;
    drop(child);
    drop(pair.slave);

    let mut writer = pair.master.take_writer()?;
    let mut reader = pair.master.try_clone_reader()?;
    let acc: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let acc2 = acc.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut s) = acc2.lock() {
                        s.push_str(&String::from_utf8_lossy(&buf[..n]));
                    }
                }
            }
        }
    });

    let start = Instant::now();
    let mut last_send = Instant::now();
    let mut toggle = false;
    while start.elapsed() < Duration::from_secs(25) {
        std::thread::sleep(Duration::from_millis(200));
        if last_send.elapsed() >= Duration::from_millis(700) {
            let _ = if toggle {
                writer.write_all(b"y\r")
            } else {
                writer.write_all(b"\r")
            };
            let _ = writer.flush();
            toggle = !toggle;
            last_send = Instant::now();
        }
    }

    let final_out = acc.lock().map(|s| s.clone()).unwrap_or_default();
    let urls = all_urls(&final_out);
    println!("=== URLS SEEN ({}) ===", urls.len());
    for u in &urls {
        println!("  {u}");
    }
    let clean = strip_ansi(&final_out);
    println!("=== FULL OUTPUT (last 2500) ===");
    let start_print = clean.len().saturating_sub(2500);
    println!("{}", &clean[start_print..]);
    Ok(())
}
