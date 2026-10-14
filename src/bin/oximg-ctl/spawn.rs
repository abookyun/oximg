//! The child `oximg` server: which binary, which env, readiness, and
//! teardown. Everything a proof spawns goes through `spawn_server`.

use crate::args::Opts;
use crate::error::CtlError;
use crate::http::Http;
use serde_json::json;
use std::io::{BufRead, BufReader};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Unix: forward SIGINT/SIGTERM to the spawned child. `signal(2)`
/// replaces the default disposition, so the wrapper stays alive to
/// `wait()` while the server drains. Forced kill is Drop / timeout only.
#[cfg(unix)]
mod unix_child {
    use std::sync::Once;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    static CHILD: AtomicI32 = AtomicI32::new(0);
    static SIGNALED: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }

    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;

    extern "C" fn forward(sig: i32) {
        let pid = CHILD.load(Ordering::SeqCst);
        if pid > 0 {
            SIGNALED.store(true, Ordering::SeqCst);
            unsafe {
                kill(pid, sig);
            }
        }
    }

    pub fn install() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| unsafe {
            signal(SIGINT, forward);
            signal(SIGTERM, forward);
        });
    }

    pub fn set_pid(pid: u32) {
        SIGNALED.store(false, Ordering::SeqCst);
        CHILD.store(pid as i32, Ordering::SeqCst);
    }

    pub fn clear() {
        CHILD.store(0, Ordering::SeqCst);
        SIGNALED.store(false, Ordering::SeqCst);
    }

    pub fn signaled() -> bool {
        SIGNALED.load(Ordering::SeqCst)
    }
}

const SPAWN_TIMEOUT: Duration = Duration::from_secs(15);

fn default_images_dir() -> PathBuf {
    let baked = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    if baked.is_dir() {
        baked
    } else {
        PathBuf::from("tests/fixtures")
    }
}

pub fn resolve_bin(explicit: Option<&Path>) -> Result<PathBuf, CtlError> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    if let Ok(p) = std::env::var("OXIMG_BIN")
        && !p.is_empty()
    {
        return Ok(PathBuf::from(p));
    }
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let sib = dir.join(if cfg!(windows) { "oximg.exe" } else { "oximg" });
        if sib.is_file() {
            return Ok(sib);
        }
    }
    Ok(PathBuf::from("oximg"))
}

pub fn images_dir(opts: &Opts) -> PathBuf {
    opts.images_dir.clone().unwrap_or_else(default_images_dir)
}

fn env_named(opts: &Opts, name: &str) -> bool {
    opts.env.iter().any(|(k, _)| k == name)
}

fn env_value<'a>(opts: &'a Opts, name: &str) -> Option<&'a str> {
    // Last --env wins, matching Command::env overwrite order.
    opts.env
        .iter()
        .rev()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

pub fn child_auto_rotate(opts: &Opts) -> bool {
    if let Some(v) = env_value(opts, "OXIMG_AUTO_ROTATE") {
        return v != "0";
    }
    std::env::var("OXIMG_AUTO_ROTATE").as_deref() != Ok("0")
}

pub fn remote_source_base(opts: &Opts) -> bool {
    env_value(opts, "OXIMG_SOURCE_BASE_URL")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn inherited_unset(name: &str) -> bool {
    match std::env::var(name) {
        Err(_) => true,
        Ok(v) => v.trim().is_empty(),
    }
}

fn parse_bind(v: &str) -> Option<IpAddr> {
    v.trim().parse().ok()
}

fn effective_bind(opts: &Opts, loopback: bool) -> IpAddr {
    // An explicit --env OXIMG_BIND= (even empty) is the caller's value.
    // Empty matches the server: unset/blank → 0.0.0.0. Do not fall
    // through to a parent OXIMG_BIND that the child will not use.
    if env_named(opts, "OXIMG_BIND") {
        return env_value(opts, "OXIMG_BIND")
            .and_then(parse_bind)
            .unwrap_or(IpAddr::from([0, 0, 0, 0]));
    }
    if loopback {
        return IpAddr::from([127, 0, 0, 1]);
    }
    if let Some(ip) = std::env::var("OXIMG_BIND")
        .ok()
        .as_deref()
        .and_then(parse_bind)
    {
        return ip;
    }
    IpAddr::from([0, 0, 0, 0])
}

fn http_host(bind: IpAddr) -> String {
    match bind {
        IpAddr::V4(v) if v.is_unspecified() => "127.0.0.1".into(),
        IpAddr::V6(v) if v.is_unspecified() => "[::1]".into(),
        IpAddr::V6(v) => format!("[{v}]"),
        IpAddr::V4(v) => v.to_string(),
    }
}

pub struct Spawned {
    pub child: Child,
    pub port: u16,
    pub host: String,
    pub bin: PathBuf,
    pub images_dir: PathBuf,
    pub kill_on_drop: bool,
}

impl Drop for Spawned {
    fn drop(&mut self) {
        #[cfg(unix)]
        unix_child::clear();
        if self.kill_on_drop {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl Spawned {
    pub fn base(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// The base URL to drive: `--base` as given (a foreign tree), or a
/// loopback server spawned for this command. Keep the returned guard
/// bound until the last request; dropping it reaps the server.
pub fn server_base(
    opts: &Opts,
    base: Option<&str>,
    http: &Http,
) -> Result<(Option<Spawned>, String), CtlError> {
    match base {
        Some(b) => Ok((None, b.trim_end_matches('/').to_string())),
        None => {
            let spawned = spawn_server(opts, None, true, http)?;
            let base = spawned.base();
            Ok((Some(spawned), base))
        }
    }
}

pub fn spawn_server(
    opts: &Opts,
    port: Option<u16>,
    loopback: bool,
    http: &Http,
) -> Result<Spawned, CtlError> {
    let bin = resolve_bin(opts.bin.as_deref())?;
    let images_dir = images_dir(opts);
    if !images_dir.is_dir() {
        return Err(CtlError::fail(
            format!("images dir {} does not exist", images_dir.display()),
            Some("pass --images-dir or run from the oximg checkout"),
        ));
    }
    let mut cmd = Command::new(&bin);
    cmd.env(
        "PORT",
        port.map(|p| p.to_string()).unwrap_or_else(|| "0".into()),
    )
    .env("IMAGES_DIR", &images_dir)
    .stdout(Stdio::null())
    .stderr(Stdio::piped());
    let bind = effective_bind(opts, loopback);
    // A local proof must not publish IMAGES_DIR on every interface.
    if loopback && !env_named(opts, "OXIMG_BIND") {
        cmd.env("OXIMG_BIND", "127.0.0.1");
    }
    // A control-plane loop should not size itself to the host; leave
    // the operator in charge if they already set a count. Empty inherited
    // values are unset — the server treats them that way too.
    if inherited_unset("OXIMG_WORKERS") && !env_named(opts, "OXIMG_WORKERS") {
        cmd.env("OXIMG_WORKERS", "1");
    }
    // Fixture get/matrix send unsigned paths. A shell with signing
    // enabled would otherwise 403 every cell. serve still inherits.
    if loopback {
        if !env_named(opts, "OXIMG_KEY") {
            cmd.env_remove("OXIMG_KEY");
        }
        if !env_named(opts, "OXIMG_SALT") {
            cmd.env_remove("OXIMG_SALT");
        }
    }
    // IMAGES_DIR is ignored when a source URL is set. Fixture get/matrix
    // must stay on the committed tree unless the caller opts in.
    if loopback && !env_named(opts, "OXIMG_SOURCE_BASE_URL") {
        cmd.env_remove("OXIMG_SOURCE_BASE_URL");
    }
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }
    let host = http_host(bind);
    #[cfg(unix)]
    unix_child::install();
    let deadline = Instant::now() + SPAWN_TIMEOUT;
    let mut child = cmd.spawn().map_err(|e| {
        CtlError::fail(
            format!("spawn {}: {e}", bin.display()),
            Some("build the binary first: cargo build --release"),
        )
    })?;
    #[cfg(unix)]
    unix_child::set_pid(child.id());
    let stderr = child.stderr.take().expect("stderr piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        let mut boot = String::new();
        let mut port = None;
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    boot.push_str(&line);
                    if let Some(rest) = line.strip_prefix("oximg listening on :") {
                        port = rest.split_whitespace().next().and_then(|p| p.parse().ok());
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        std::thread::spawn(move || {
            let mut sink = std::io::stderr();
            let _ = std::io::copy(&mut reader, &mut sink);
        });
        let _ = tx.send((port, boot));
    });
    let remaining = deadline.saturating_duration_since(Instant::now());
    let (port_found, boot) = match rx.recv_timeout(remaining) {
        Ok(v) => v,
        Err(_) => {
            #[cfg(unix)]
            unix_child::clear();
            let _ = child.kill();
            let _ = child.wait();
            return Err(CtlError::fail(
                "server did not print a listening line within 15s",
                Some("check --bin points at oximg, and that cmake/nasm are installed"),
            ));
        }
    };
    let Some(port) = port_found else {
        #[cfg(unix)]
        unix_child::clear();
        // stderr closed (or the reader errored) without a listening
        // line. The child may still be alive — a wrong --bin that
        // inherits and idles. Do not unbounded-wait past SPAWN_TIMEOUT.
        let leftover = deadline.saturating_duration_since(Instant::now());
        let status = wait_child_until(&mut child, leftover);
        let hint = if boot.is_empty() {
            "check --bin points at oximg, and that cmake/nasm are installed"
        } else {
            "a set-but-invalid OXIMG_* is the usual cause"
        };
        let mut err = CtlError::fail(
            format!("server exited before becoming healthy: {status:?}"),
            Some(hint),
        );
        if !boot.is_empty() {
            err.json["stderr"] = json!(boot.trim());
        }
        return Err(err);
    };
    // The copy thread only sees lines after the listening line; keep
    // the promised "stderr is forwarded" contract for boot warnings.
    eprint!("{boot}");
    let mut spawned = Spawned {
        child,
        port,
        host: host.clone(),
        bin,
        images_dir,
        kill_on_drop: true,
    };
    loop {
        if let Ok(res) = http.get(
            &format!("http://{host}:{port}/health"),
            None,
            Duration::from_secs(2),
        ) && res.status == 200
        {
            return Ok(spawned);
        }
        if let Ok(Some(status)) = spawned.child.try_wait() {
            return Err(CtlError::fail(
                format!("server exited before /health: {status}"),
                None,
            ));
        }
        if Instant::now() > deadline {
            return Err(CtlError::fail(
                "server did not become healthy within 15s",
                Some("the listening line was printed but /health did not answer 200"),
            ));
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn wait_child_until(child: &mut Child, budget: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Some(st),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                return child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                return child.wait().ok();
            }
        }
    }
}

pub fn wait_served_child(spawned: &mut Spawned) -> std::io::Result<ExitStatus> {
    #[cfg(unix)]
    {
        // Drain after SIGINT/SIGTERM, then SIGKILL so a stuck request
        // cannot hang the wrapper (and SIGKILL of the wrapper cannot
        // leave the child orphaned because we reaped it first).
        const GRACE: Duration = Duration::from_secs(10);
        let mut grace_from = None;
        loop {
            if let Some(st) = spawned.child.try_wait()? {
                return Ok(st);
            }
            if unix_child::signaled() {
                let start = grace_from.get_or_insert_with(Instant::now);
                if start.elapsed() >= GRACE {
                    let _ = spawned.child.kill();
                    return spawned.child.wait();
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    #[cfg(not(unix))]
    {
        spawned.child.wait()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opts with the given --env pairs. Tests only use cases an explicit
    /// --env decides, so the parent shell's env cannot leak in.
    fn opts(env: &[(&str, &str)]) -> Opts {
        Opts {
            bin: None,
            images_dir: None,
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            pretty: false,
            dry_run: false,
        }
    }

    #[test]
    fn loopback_proofs_bind_localhost_unless_told_otherwise() {
        assert_eq!(
            effective_bind(&opts(&[]), true),
            IpAddr::from([127, 0, 0, 1])
        );
        let v6 = opts(&[("OXIMG_BIND", "::1")]);
        assert_eq!(effective_bind(&v6, true), "::1".parse::<IpAddr>().unwrap());
        assert_eq!(effective_bind(&v6, false), "::1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn an_explicit_empty_bind_is_the_servers_default_not_the_parents() {
        let empty = opts(&[("OXIMG_BIND", "")]);
        assert_eq!(effective_bind(&empty, true), IpAddr::from([0, 0, 0, 0]));
        let junk = opts(&[("OXIMG_BIND", "not-an-ip")]);
        assert_eq!(effective_bind(&junk, false), IpAddr::from([0, 0, 0, 0]));
    }

    #[test]
    fn the_last_env_flag_wins() {
        let o = opts(&[("OXIMG_BIND", "10.0.0.1"), ("OXIMG_BIND", "10.0.0.2")]);
        assert_eq!(env_value(&o, "OXIMG_BIND"), Some("10.0.0.2"));
        assert!(env_named(&o, "OXIMG_BIND"));
        assert!(!env_named(&o, "OXIMG_KEY"));
    }

    #[test]
    fn unspecified_binds_are_dialed_on_loopback() {
        assert_eq!(http_host(IpAddr::from([0, 0, 0, 0])), "127.0.0.1");
        assert_eq!(http_host("::".parse().unwrap()), "[::1]");
        assert_eq!(http_host("::1".parse().unwrap()), "[::1]");
        assert_eq!(http_host(IpAddr::from([10, 1, 2, 3])), "10.1.2.3");
    }

    #[test]
    fn source_base_and_auto_rotate_read_the_env_flags() {
        assert!(!remote_source_base(&opts(&[(
            "OXIMG_SOURCE_BASE_URL",
            "  "
        )])));
        assert!(remote_source_base(&opts(&[(
            "OXIMG_SOURCE_BASE_URL",
            "http://x"
        )])));
        assert!(!child_auto_rotate(&opts(&[("OXIMG_AUTO_ROTATE", "0")])));
        assert!(child_auto_rotate(&opts(&[("OXIMG_AUTO_ROTATE", "1")])));
    }

    #[test]
    fn an_explicit_bin_is_used_as_given() {
        let p = Path::new("/opt/oximg/bin/oximg");
        assert_eq!(resolve_bin(Some(p)).unwrap(), p);
    }

    #[test]
    fn a_given_base_spawns_nothing_and_loses_its_trailing_slash() {
        let http = Http::new().unwrap();
        let (guard, base) = server_base(&opts(&[]), Some("http://h:1//"), &http).unwrap();
        assert!(guard.is_none());
        assert_eq!(base, "http://h:1");
    }
}
