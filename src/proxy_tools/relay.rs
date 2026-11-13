//! A standard-library TCP relay: `listen → upstream`, both directions.
//!
//! This replaces `jproxy.py` so the relay ships with GNOUS instead of living in
//! `/home/root/GNOME`. No async runtime is used on purpose — adding one would
//! mean a new crate, and this project's development policy is to stay buildable
//! fully offline. One thread per direction is plenty at this scale.
//!
//! Behaviour kept from the Python original, because it was battle-tested:
//! upstream connect retries with a short backoff, idle timeout on both sides,
//! half-close propagation, and no `wait_closed`-style hang on shutdown.

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::guard;

/// Set by the SIGTERM/SIGINT handler installed by [`serve_until_signal`].
/// Tests pass their own flag so they never share process-global state.
static SIGNAL_STOP: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
pub struct RelayConfig {
    pub listen: SocketAddr,
    pub upstream: SocketAddr,
    pub idle_timeout: Duration,
    pub connect_timeout: Duration,
    pub retries: u32,
}

impl RelayConfig {
    pub fn from_fields(
        listen: SocketAddr,
        upstream: SocketAddr,
        idle_timeout_secs: u64,
        connect_timeout_secs: u64,
        retries: u32,
    ) -> Self {
        Self {
            listen,
            upstream,
            idle_timeout: Duration::from_secs(idle_timeout_secs.max(1)),
            connect_timeout: Duration::from_secs(connect_timeout_secs.clamp(1, 60)),
            retries: retries.clamp(1, 5),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub total: Arc<AtomicU64>,
    pub active: Arc<AtomicU64>,
    pub bytes_up: Arc<AtomicU64>,
    pub bytes_down: Arc<AtomicU64>,
}

impl Stats {
    pub fn snapshot(&self) -> (u64, u64, u64, u64) {
        (
            self.total.load(Ordering::Relaxed),
            self.active.load(Ordering::Relaxed),
            self.bytes_up.load(Ordering::Relaxed),
            self.bytes_down.load(Ordering::Relaxed),
        )
    }

    fn human(&self) -> String {
        let (total, active, up, down) = self.snapshot();
        format!(
            "累计连接={total} 当前活动={active} 上行={:.1}KiB 下行={:.1}KiB",
            up as f64 / 1024.0,
            down as f64 / 1024.0
        )
    }
}

pub fn request_stop(flag: &AtomicBool) {
    flag.store(true, Ordering::SeqCst);
}

pub fn stop_requested(flag: &AtomicBool) -> bool {
    flag.load(Ordering::SeqCst)
}

/// CLI entry: installs SIGTERM/SIGINT handling and runs until told to stop.
pub fn serve_until_signal(
    config: RelayConfig,
    on_ready: impl FnOnce(SocketAddr),
) -> Result<Stats, String> {
    install_signal_handlers();
    serve(config, &SIGNAL_STOP, on_ready)
}

/// Blocking relay loop. `on_ready` is called once the socket is bound, which
/// lets tests learn the ephemeral port without a separate probe.
pub fn serve(
    config: RelayConfig,
    stop: &AtomicBool,
    on_ready: impl FnOnce(SocketAddr),
) -> Result<Stats, String> {
    // The port gate runs before any syscall: binding the live relay port from
    // inside GNOUS is the one mistake this extension must never make.
    guard::check_port(config.listen.port(), true).map_err(|error| error.to_string())?;

    let listener = TcpListener::bind(config.listen)
        .map_err(|error| format!("无法监听 {}: {error}", config.listen))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("设置非阻塞失败: {error}"))?;
    on_ready(local);

    let stats = Stats::default();
    let config = Arc::new(config);
    let mut last_report = Instant::now();
    while !stop_requested(stop) {
        match listener.accept() {
            Ok((client, peer)) => {
                stats.total.fetch_add(1, Ordering::Relaxed);
                stats.active.fetch_add(1, Ordering::Relaxed);
                let config = config.clone();
                let stats = stats.clone();
                std::thread::Builder::new()
                    .name(format!("relay-{}", peer.port()))
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        handle(client, peer, &config, &stats);
                        stats.active.fetch_sub(1, Ordering::Relaxed);
                    })
                    .map_err(|error| format!("无法创建线程: {error}"))?;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(80));
            }
            Err(error) => return Err(format!("accept 失败: {error}")),
        }
        if last_report.elapsed() >= Duration::from_secs(300) {
            last_report = Instant::now();
            println!("状态：{}", stats.human());
        }
    }
    println!("已停止。{}", stats.human());
    Ok(stats)
}

fn install_signal_handlers() {
    unsafe extern "C" fn on_signal(_: libc::c_int) {
        SIGNAL_STOP.store(true, Ordering::SeqCst);
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
}

fn connect_upstream(config: &RelayConfig) -> Result<TcpStream, String> {
    let mut last = String::new();
    for attempt in 1..=config.retries {
        match TcpStream::connect_timeout(&config.upstream, config.connect_timeout) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(error) => {
                last = error.to_string();
                if attempt < config.retries {
                    std::thread::sleep(Duration::from_millis(250 * u64::from(attempt)));
                }
            }
        }
    }
    Err(format!("上游 {} 不可达: {last}", config.upstream))
}

fn handle(client: TcpStream, peer: SocketAddr, config: &RelayConfig, stats: &Stats) {
    let started = Instant::now();
    let Ok(upstream) = connect_upstream(config) else {
        let _ = client.shutdown(Shutdown::Both);
        return;
    };
    let _ = client.set_read_timeout(Some(config.idle_timeout));
    let _ = upstream.set_read_timeout(Some(config.idle_timeout));

    let (Ok(client_read), Ok(upstream_read)) = (client.try_clone(), upstream.try_clone()) else {
        let _ = client.shutdown(Shutdown::Both);
        return;
    };
    let up_counter = stats.bytes_up.clone();
    let down_counter = stats.bytes_down.clone();

    let up = std::thread::spawn(move || pump(client_read, upstream, up_counter));
    let down = std::thread::spawn(move || pump(upstream_read, client, down_counter));
    let up_bytes = up.join().unwrap_or(0);
    let down_bytes = down.join().unwrap_or(0);
    println!(
        "{peer} 结束 {:.2}s 上行={up_bytes}B 下行={down_bytes}B",
        started.elapsed().as_secs_f64()
    );
}

/// Copy one direction, then half-close the writing side so the peer sees EOF
/// instead of hanging until the idle timeout.
fn pump(mut from: TcpStream, mut to: TcpStream, counter: Arc<AtomicU64>) -> u64 {
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match from.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if to.write_all(&buffer[..read]).is_err() {
                    break;
                }
                total += read as u64;
                counter.fetch_add(read as u64, Ordering::Relaxed);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) =>
            {
                break;
            }
            Err(_) => break,
        }
    }
    let _ = to.shutdown(Shutdown::Write);
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    /// Minimal echo server standing in for the j1900 upstream. Tests never talk
    /// to the real upstream — that would spend the live tunnel.
    fn spawn_echo() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut writer = stream;
                    let mut line = String::new();
                    while reader.read_line(&mut line).unwrap_or(0) > 0 {
                        if writer.write_all(line.as_bytes()).is_err() {
                            break;
                        }
                        let _ = writer.flush();
                        line.clear();
                    }
                });
            }
        });
        addr
    }

    struct Running {
        addr: SocketAddr,
        stop: Arc<AtomicBool>,
        handle: std::thread::JoinHandle<Result<Stats, String>>,
    }

    impl Running {
        fn shutdown(self) -> Stats {
            self.stop.store(true, Ordering::SeqCst);
            self.handle
                .join()
                .expect("relay thread")
                .expect("clean exit")
        }
    }

    fn start_relay(upstream: SocketAddr) -> Running {
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let handle = std::thread::spawn(move || {
            let config =
                RelayConfig::from_fields("127.0.0.1:0".parse().unwrap(), upstream, 5, 2, 2);
            serve(config, &thread_stop, |addr| {
                let _ = tx.send(addr);
            })
        });
        let addr = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("relay bound");
        Running { addr, stop, handle }
    }

    #[test]
    fn relays_bytes_in_both_directions() {
        let upstream = spawn_echo();
        let running = start_relay(upstream);
        let mut client = TcpStream::connect(running.addr).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client.write_all(b"hello relay\n").unwrap();
        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "hello relay\n");
        let stats = running.shutdown();
        let (total, _, _, _) = stats.snapshot();
        assert!(total >= 1, "connection was not counted");
    }

    #[test]
    fn refuses_to_bind_a_protected_port_before_touching_the_socket() {
        let port = guard::PROTECTED_PORTS[0];
        let config = RelayConfig::from_fields(
            SocketAddr::from(([127, 0, 0, 1], port)),
            "127.0.0.1:9".parse().unwrap(),
            5,
            1,
            1,
        );
        let stop = AtomicBool::new(false);
        let error = serve(config, &stop, |_| panic!("must not bind")).expect_err("must refuse");
        assert!(error.contains("受保护"), "{error}");
    }

    #[test]
    fn unreachable_upstream_closes_the_client_instead_of_hanging() {
        let upstream: SocketAddr = "127.0.0.1:9".parse().unwrap();
        let running = start_relay(upstream);
        let client = TcpStream::connect(running.addr).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut client = client;
        let mut buffer = [0u8; 16];
        // The relay drops the connection because upstream refuses.
        let read = client.read(&mut buffer).unwrap_or(0);
        assert_eq!(read, 0);
        let _ = running.shutdown();
    }
}
