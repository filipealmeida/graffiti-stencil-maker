//! `stencil-studio`: double-click launcher. Starts `stencil studio` from the same folder on a free port, opens the
//! browser once the server answers, and stops the server when this window closes or the launcher is interrupted.

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{exit, Command};
use std::time::{Duration, Instant};

fn fail(msg: &str) -> ! {
    eprintln!("stencil-studio: {msg}");
    // on a double-click the window would vanish before the message can be read
    eprintln!("Press Enter to close.");
    let _ = std::io::stdin().read_line(&mut String::new());
    exit(1)
}

fn stencil_path() -> PathBuf {
    let name = if cfg!(windows) {
        "stencil.exe"
    } else {
        "stencil"
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .filter(|p| p.exists());
    beside.unwrap_or_else(|| PathBuf::from(name))
}

/// The preferred port if it is free, otherwise any free one.
fn free_port(preferred: u16) -> u16 {
    [preferred, 0]
        .iter()
        .find_map(|&p| TcpListener::bind(("127.0.0.1", p)).ok())
        .and_then(|l| l.local_addr().ok())
        .map_or(preferred, |a| a.port())
}

fn main() {
    // an optional port: `stencil-studio 8080`
    let preferred = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(5173);
    let port = free_port(preferred);
    let exe = stencil_path();
    let mut child = Command::new(&exe)
        .args(["studio", "--no-open", "--port", &port.to_string()])
        .spawn()
        .unwrap_or_else(|e| fail(&format!("cannot start {}: {e}", exe.display())));
    let start = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            fail(&format!(
                "the studio stopped at once ({status}); this build of stencil may lack the studio, see INSTALL.md"
            ));
        }
        if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200))
            .is_ok()
        {
            break;
        }
        if start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            fail("the studio did not start within 20 seconds");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let url = format!("http://127.0.0.1:{port}/");
    println!(
        "Stencil studio {} is running at {url}\nClose this window (or press Ctrl+C) to stop it.",
        env!("CARGO_PKG_VERSION")
    );
    if let Err(e) = open::that(&url) {
        println!("Could not open the browser ({e}): open {url} yourself.");
    }
    // Ctrl+C reaches the child too, since both share the console, so waiting is enough
    let _ = child.wait();
}
