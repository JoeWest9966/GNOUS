use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt as _;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(8);

pub fn run(program: &str, args: &[&str]) -> Result<String, String> {
    run_with_env(program, args, &[], DEFAULT_TIMEOUT)
}

/// Run a command against the current user's systemd manager, deriving the
/// session bus/runtime dir from this process instead of hard-coding root.
pub fn run_user(program: &str, args: &[&str]) -> Result<String, String> {
    user_command(program, args, DEFAULT_TIMEOUT)
}

pub fn run_user_timeout(program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    user_command(program, args, timeout)
}

fn user_command(program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let env = user_manager_env();
    let refs: Vec<(&str, &str)> = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    run_with_env(program, args, &refs, timeout)
}

fn user_manager_env() -> Vec<(String, String)> {
    let uid = unsafe { libc::getuid() };
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{uid}"));
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .unwrap_or_else(|_| format!("unix:path={runtime}/bus"));
    vec![
        ("XDG_RUNTIME_DIR".into(), runtime),
        ("DBUS_SESSION_BUS_ADDRESS".into(), bus),
    ]
}

/// All backend commands share one bounded-process adapter. It captures stdout
/// and stderr, kills the process group on timeout, and never inherits an
/// interactive stdin.
pub fn run_with_env(
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: Duration,
) -> Result<String, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    for (key, value) in env {
        command.env(key, value);
    }
    let child = command
        .spawn()
        .map_err(|e| format!("无法执行 {program}: {e}"))?;
    let pid = child.id() as i32;
    let deadline = Instant::now() + timeout;
    let done = Arc::new(AtomicBool::new(false));
    let watch_done = done.clone();
    let watchdog = std::thread::spawn(move || {
        while Instant::now() < deadline {
            if watch_done.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        if !watch_done.load(Ordering::Relaxed) {
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
    });
    let output = child
        .wait_with_output()
        .map_err(|e| format!("等待 {program} 失败: {e}"));
    // A timeout kills the whole process group; success cancels the watchdog
    // before the PGID can be reused.
    done.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    let output = output?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr);
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&err);
    }
    if output.status.success() {
        Ok(text.trim().to_string())
    } else {
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".into());
        Err(format!("{program} 退出码 {code}：{}", text.trim()))
    }
}

/// `systemd-run --user --pipe --wait` can leave descendants holding the pipe;
/// this variant reads concurrently and always enforces the deadline.
pub fn run_bounded(program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let env = user_manager_env();
    let refs: Vec<(&str, &str)> = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    run_bounded_with_env(program, args, &refs, timeout)
}

fn run_bounded_with_env(
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: Duration,
) -> Result<String, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法执行 {program}: {e}"))?;
    let pid = child.id() as i32;
    let mut stdout = child.stdout.take().unwrap();
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::new();
    let mut timed_out = false;
    let mut buf = [0u8; 8192];
    loop {
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(e) => return Err(format!("等待 {program} 失败: {e}")),
        }
        // Poll the pipe: readable → drain what is available; not readable
        // within 20ms → loop back and re-check the child.
        let mut fds = [libc::pollfd {
            fd: stdout.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 1, 20) };
        if ready > 0 && fds[0].revents & (libc::POLLIN | libc::POLLHUP) != 0 {
            match stdout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => bytes.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
    }
    if timed_out {
        unsafe { libc::kill(-pid, libc::SIGKILL) };
        let _ = child.wait();
        return Err(format!("{program} 超时（{}ms）", timeout.as_millis()));
    }
    // The child exited; a grandchild may still hold the write end forever,
    // so drain only what is already readable and give up quickly.
    let fd = stdout.as_raw_fd();
    loop {
        let mut fds = [libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        }];
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 1, 50) };
        if ready <= 0 || fds[0].revents & (libc::POLLIN | libc::POLLHUP) == 0 {
            break;
        }
        match stdout.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
        if bytes.len() > (1 << 20) {
            break;
        }
    }
    let _ = child.wait();
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

pub fn read_proc_environ(pid: u32) -> Vec<(String, String)> {
    let Ok(bytes) = std::fs::read(format!("/proc/{pid}/environ")) else {
        return Vec::new();
    };
    bytes
        .split(|b| *b == 0)
        .filter(|item| !item.is_empty())
        .filter_map(|item| {
            String::from_utf8_lossy(item)
                .split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

pub fn proc_env(pid: u32, key: &str) -> Option<String> {
    read_proc_environ(pid)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}

pub fn find_pids_by_comm(name: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let prefix: String = name.chars().take(15).collect();
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm")) {
            if comm.trim().starts_with(&prefix) {
                pids.push(pid);
            }
        }
    }
    pids.sort_unstable();
    pids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_output_includes_stdout() {
        let out = run("/bin/sh", &["-c", "printf hello"]).unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    fn failed_command_is_visible() {
        let err = run("/bin/sh", &["-c", "printf nope >&2; exit 3"]).unwrap_err();
        assert!(err.contains("退出码 3"));
        assert!(err.contains("nope"));
    }
}
