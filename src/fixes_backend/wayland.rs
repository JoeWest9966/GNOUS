//! 手写的最小 Wayland 客户端（只够做剪贴板体检）。
//!
//! 为什么不用现成 crate：
//!   * 这台机器没有外网，cargo 只能离线，`wayland-client` 拉不到；
//!   * 系统里也没装 `wl-clipboard`（没有 wl-copy / wl-paste）。
//!
//! 支持的协议子集：
//!   wl_display / wl_registry / wl_callback / wl_seat
//!   wl_data_device_manager / wl_data_device / wl_data_offer / wl_data_source
//!
//! 用途：
//!   * `read_clipboard()` —— 以普通客户端身份读 CLIPBOARD（mime 列表 + 文本）
//!   * `hold_clipboard()` —— 以普通客户端身份占住 CLIPBOARD，被取数据时回吐内容，
//!                           并回报"有没有被 compositor 立刻 cancelled"

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::raw::{c_int, c_void};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

// ------------------------------------------------------------------ libc FFI

#[repr(C)]
struct Iovec {
    base: *mut c_void,
    len: usize,
}

#[repr(C)]
struct Msghdr {
    name: *mut c_void,
    namelen: u32,
    iov: *mut Iovec,
    iovlen: usize,
    control: *mut c_void,
    controllen: usize,
    flags: i32,
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn sendmsg(fd: c_int, msg: *const Msghdr, flags: c_int) -> isize;
    fn recvmsg(fd: c_int, msg: *mut Msghdr, flags: c_int) -> isize;
    fn poll(fds: *mut PollFd, nfds: u64, timeout: c_int) -> c_int;
}

const SOL_SOCKET: c_int = 1;
const SCM_RIGHTS: c_int = 1;
const MSG_NOSIGNAL: c_int = 0x4000;
const POLLIN: i16 = 1;

/// 把 (数据, 可选 fd) 用一个 sendmsg 发出去（Wayland 的 'h' 参数走 ancillary）
fn sendmsg_bytes(sock: c_int, bytes: &[u8], fd: Option<c_int>) -> bool {
    let mut iov = Iovec {
        base: bytes.as_ptr() as *mut c_void,
        len: bytes.len(),
    };
    let mut ctrl = [0u8; 32];
    let (ctrl_ptr, ctrl_len) = match fd {
        None => (std::ptr::null_mut(), 0usize),
        Some(f) => {
            // struct cmsghdr { size_t cmsg_len; int cmsg_level; int cmsg_type; } + int
            unsafe {
                let p = ctrl.as_mut_ptr();
                (p as *mut usize).write_unaligned(20); // 16 + 4
                (p.add(8) as *mut c_int).write_unaligned(SOL_SOCKET);
                (p.add(12) as *mut c_int).write_unaligned(SCM_RIGHTS);
                (p.add(16) as *mut c_int).write_unaligned(f);
            }
            (ctrl.as_mut_ptr() as *mut c_void, 24usize) // CMSG_SPACE(4)
        }
    };
    let msg = Msghdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: &mut iov,
        iovlen: 1,
        control: ctrl_ptr,
        controllen: ctrl_len,
        flags: 0,
    };
    let n = unsafe { sendmsg(sock, &msg, MSG_NOSIGNAL) };
    n == bytes.len() as isize
}

/// 收一段数据 + 顺带收 fd
fn recvmsg_bytes(sock: c_int, buf: &mut [u8], fdq: &mut Vec<c_int>, timeout_ms: c_int) -> isize {
    let mut pfd = PollFd {
        fd: sock,
        events: POLLIN,
        revents: 0,
    };
    let rc = unsafe { poll(&mut pfd, 1, timeout_ms) };
    if rc <= 0 {
        return 0;
    }
    let mut iov = Iovec {
        base: buf.as_mut_ptr() as *mut c_void,
        len: buf.len(),
    };
    let mut ctrl = [0u8; 128];
    let mut msg = Msghdr {
        name: std::ptr::null_mut(),
        namelen: 0,
        iov: &mut iov,
        iovlen: 1,
        control: ctrl.as_mut_ptr() as *mut c_void,
        controllen: 128,
        flags: 0,
    };
    let n = unsafe { recvmsg(sock, &mut msg, 0) };
    if n <= 0 {
        return n;
    }
    if msg.controllen >= 20 {
        let mut off = 0usize;
        while off + 16 <= msg.controllen {
            let cmsg_len = unsafe { (ctrl.as_ptr().add(off) as *const usize).read_unaligned() };
            if cmsg_len < 16 || off + cmsg_len > msg.controllen {
                break;
            }
            let level = unsafe { (ctrl.as_ptr().add(off + 8) as *const c_int).read_unaligned() };
            let ctype = unsafe { (ctrl.as_ptr().add(off + 12) as *const c_int).read_unaligned() };
            if level == SOL_SOCKET && ctype == SCM_RIGHTS {
                let nfd = (cmsg_len - 16) / 4;
                for i in 0..nfd {
                    let fd = unsafe {
                        (ctrl.as_ptr().add(off + 16 + i * 4) as *const c_int).read_unaligned()
                    };
                    fdq.push(fd);
                }
            }
            off += (cmsg_len + 7) & !7usize;
        }
    }
    n
}

// --------------------------------------------------------------- 参数编解码

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

#[derive(Default)]
struct ArgsOut {
    buf: Vec<u8>,
}

impl ArgsOut {
    fn new() -> Self {
        Self::default()
    }
    fn uint(mut self, v: u32) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn string(mut self, s: &str) -> Self {
        // Wayland 的 string = u32 长度（含结尾 NUL）+ 内容 + NUL + 补齐到 4 字节
        let bytes = s.as_bytes();
        let len = bytes.len() as u32 + 1;
        self.buf.extend_from_slice(&len.to_le_bytes());
        self.buf.extend_from_slice(bytes);
        self.buf.push(0); // ← 结尾 NUL（少写这个会让后面所有参数错位）
        let pad = align4(len as usize) - len as usize;
        self.buf.extend(std::iter::repeat(0u8).take(pad));
        self
    }
    fn done(self) -> Vec<u8> {
        self.buf
    }
}

struct ArgsIn<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ArgsIn<'a> {
    fn new(d: &'a [u8]) -> Self {
        ArgsIn { data: d, pos: 0 }
    }
    fn uint(&mut self) -> Option<u32> {
        if self.pos + 4 > self.data.len() {
            return None;
        }
        let v = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().ok()?);
        self.pos += 4;
        Some(v)
    }
    fn string(&mut self) -> Option<String> {
        let len = self.uint()? as usize;
        if len == 0 {
            return Some(String::new());
        }
        if self.pos + len > self.data.len() {
            return None;
        }
        let raw = &self.data[self.pos..self.pos + len - 1];
        let s = String::from_utf8_lossy(raw).to_string();
        self.pos += align4(len);
        Some(s)
    }
}

struct Frame {
    obj: u32,
    opcode: u16,
    payload: Vec<u8>,
}

// ------------------------------------------------------------------- 连接

pub struct Conn {
    #[allow(dead_code)]
    sock: UnixStream,
    fd: c_int,
    next_id: u32,
    pending: Vec<u8>,
    fdq: Vec<c_int>,
    iface: HashMap<u32, String>,
    /// wl_display.error：只要出现就说明我们的请求不合协议（用来证明"探测本身没写错"）
    protocol_error: Option<String>,
}

fn user_manager_env(key: &str) -> Result<String, String> {
    let out = std::process::Command::new("systemctl")
        .args(["--user", "show-environment"])
        .env(
            "XDG_RUNTIME_DIR",
            std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/0".into()),
        )
        .output()
        .map_err(|e| format!("systemctl --user show-environment 失败: {e}"))?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| {
            line.split_once('=')
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        })
        .ok_or_else(|| format!("{key} 未设置"))
}

impl Conn {
    pub fn connect() -> Result<Conn, String> {
        let runtime =
            std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR 未设置".to_string())?;
        // GNOUS itself may run as an X11 client (GPUI), so the variable can be
        // absent from this process; the session compositor is still the one to
        // probe. Fall back to the systemd user manager environment.
        let name = std::env::var("WAYLAND_DISPLAY")
            .or_else(|_| user_manager_env("WAYLAND_DISPLAY"))
            .unwrap_or_else(|_| "wayland-0".into());
        let path = if name.starts_with('/') {
            name
        } else {
            format!("{}/{}", runtime, name)
        };
        let sock = UnixStream::connect(&path)
            .map_err(|e| format!("连不上 Wayland socket {}: {}", path, e))?;
        let fd = sock.as_raw_fd();
        let mut c = Conn {
            sock,
            fd,
            next_id: 2,
            pending: Vec::new(),
            fdq: Vec::new(),
            iface: HashMap::new(),
            protocol_error: None,
        };
        c.iface.insert(1, "wl_display".into());
        Ok(c)
    }

    fn alloc(&mut self, iface: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.iface.insert(id, iface.to_string());
        id
    }

    fn send(&mut self, obj: u32, opcode: u16, args: &[u8]) -> Result<(), String> {
        self.send_with_fd(obj, opcode, args, None)
    }

    fn send_with_fd(
        &mut self,
        obj: u32,
        opcode: u16,
        args: &[u8],
        fd: Option<c_int>,
    ) -> Result<(), String> {
        let size = 8 + args.len();
        let mut msg = Vec::with_capacity(size);
        msg.extend_from_slice(&obj.to_le_bytes());
        let so = ((size as u32) << 16) | opcode as u32;
        msg.extend_from_slice(&so.to_le_bytes());
        msg.extend_from_slice(args);
        if std::env::var("CLIPFIX_DEBUG").is_ok() {
            eprintln!(">> obj={} opcode={} args={:?}", obj, opcode, args);
        }
        if sendmsg_bytes(self.fd, &msg, fd) {
            Ok(())
        } else {
            Err("写 Wayland socket 失败".into())
        }
    }

    fn take_frame(&mut self) -> Option<Frame> {
        if self.pending.len() < 8 {
            return None;
        }
        let obj = u32::from_le_bytes(self.pending[0..4].try_into().ok()?);
        let so = u32::from_le_bytes(self.pending[4..8].try_into().ok()?);
        let size = (so >> 16) as usize;
        let opcode = (so & 0xffff) as u16;
        if size < 8 || self.pending.len() < size {
            return None;
        }
        let payload = self.pending[8..size].to_vec();
        self.pending.drain(..size);
        if obj == 1 && opcode == 0 {
            let mut r = ArgsIn::new(&payload);
            let _obj = r.uint();
            let code = r.uint().unwrap_or(0);
            let msg = r.string().unwrap_or_default();
            if self.protocol_error.is_none() {
                self.protocol_error = Some(format!("code={} {}", code, msg));
            }
        }
        if std::env::var("CLIPFIX_DEBUG").is_ok() {
            eprintln!("<< obj={} opcode={} payload={:?}", obj, opcode, payload);
        }
        Some(Frame {
            obj,
            opcode,
            payload,
        })
    }

    fn frame(&mut self, timeout: Duration) -> Result<Option<Frame>, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(f) = self.take_frame() {
                return Ok(Some(f));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let mut buf = [0u8; 8192];
            let wait = deadline
                .saturating_duration_since(now)
                .as_millis()
                .min(1000) as c_int;
            let n = recvmsg_bytes(self.fd, &mut buf, &mut self.fdq, wait.max(1));
            if n > 0 {
                self.pending.extend_from_slice(&buf[..n as usize]);
            } else {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    fn next_fd(&mut self) -> Option<c_int> {
        if self.fdq.is_empty() {
            None
        } else {
            Some(self.fdq.remove(0))
        }
    }

    #[allow(dead_code)]
    fn sync(&mut self, timeout: Duration) -> Result<(), String> {
        let cb = self.alloc("wl_callback");
        self.send(1, 0, &ArgsOut::new().uint(cb).done())?;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.frame(deadline.saturating_duration_since(Instant::now()))? {
                Some(f) if f.obj == cb && f.opcode == 0 => return Ok(()),
                Some(_) => continue,
                None => break,
            }
        }
        Err("wl_display.sync 超时".into())
    }

    /// 返回 (registry_id, globals)
    fn globals(&mut self) -> Result<(u32, Vec<(u32, String, u32)>), String> {
        let reg = self.alloc("wl_registry");
        self.send(1, 1, &ArgsOut::new().uint(reg).done())?;
        let cb = self.alloc("wl_callback");
        self.send(1, 0, &ArgsOut::new().uint(cb).done())?;
        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            match self.frame(deadline.saturating_duration_since(Instant::now()))? {
                Some(f) if f.obj == reg && f.opcode == 0 => {
                    let mut r = ArgsIn::new(&f.payload);
                    let name = r.uint().unwrap_or(0);
                    let iface = r.string().unwrap_or_default();
                    let ver = r.uint().unwrap_or(1);
                    out.push((name, iface, ver));
                }
                Some(f) if f.obj == cb && f.opcode == 0 => return Ok((reg, out)),
                Some(_) => continue,
                None => break,
            }
        }
        Err("枚举 Wayland 全局对象超时".into())
    }

    fn bind(&mut self, reg: u32, name: u32, iface: &str, ver: u32, max_ver: u32) -> u32 {
        let id = self.alloc(iface);
        let v = ver.min(max_ver);
        let args = ArgsOut::new().uint(name).string(iface).uint(v).uint(id);
        let _ = self.send(reg, 0, &args.done());
        id
    }
}

fn pick_text_mime(mimes: &[String]) -> Option<String> {
    for want in [
        "text/plain;charset=utf-8",
        "text/plain;charset=UTF-8",
        "UTF8_STRING",
        "text/plain",
    ] {
        if let Some(m) = mimes.iter().find(|m| m.eq_ignore_ascii_case(want)) {
            return Some(m.clone());
        }
    }
    None
}

pub struct ClipRead {
    pub got_offer: bool,
    pub mimes: Vec<String>,
    pub text: Option<String>,
    pub error: Option<String>,
    /// 协议层自证：这些行能证明"探测本身没写错"
    pub sanity: Vec<String>,
}

/// 以普通客户端身份读 CLIPBOARD
pub fn read_clipboard(timeout: Duration) -> ClipRead {
    let mut out = ClipRead {
        got_offer: false,
        mimes: Vec::new(),
        text: None,
        error: None,
        sanity: Vec::new(),
    };
    let mut c = match Conn::connect() {
        Ok(c) => c,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };
    let (reg, globals) = match c.globals() {
        Ok(g) => g,
        Err(e) => {
            out.error = Some(e);
            return out;
        }
    };
    let seat_g = globals.iter().find(|(_, i, _)| i == "wl_seat").cloned();
    let ddm_g = globals
        .iter()
        .find(|(_, i, _)| i == "wl_data_device_manager")
        .cloned();
    let (Some(seat_g), Some(ddm_g)) = (seat_g, ddm_g) else {
        out.error = Some("compositor 未提供 wl_seat / wl_data_device_manager".into());
        return out;
    };
    let seat = c.bind(reg, seat_g.0, "wl_seat", seat_g.2, 4);
    let ddm = c.bind(reg, ddm_g.0, "wl_data_device_manager", ddm_g.2, 3);

    // 顺手把 wl_seat 的 name/capabilities 收下来，作为"握手成功"的证据
    let mut seat_name: Option<String> = None;
    let mut seat_caps: Option<u32> = None;
    let seat_deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < seat_deadline {
        match c.frame(seat_deadline.saturating_duration_since(Instant::now())) {
            Ok(Some(f)) if f.obj == seat => {
                let mut r = ArgsIn::new(&f.payload);
                match f.opcode {
                    0 => seat_caps = r.uint(),
                    1 => seat_name = r.string(),
                    _ => {}
                }
            }
            Ok(Some(_)) => continue,
            _ => break,
        }
    }

    let dd = c.alloc("wl_data_device");
    let _ = c.send(ddm, 1, &ArgsOut::new().uint(dd).uint(seat).done());

    let mut offers: HashMap<u32, Vec<String>> = HashMap::new();
    let mut current: Option<u32> = None;
    let mut saw_selection_event = false;
    let deadline = Instant::now() + timeout;

    while Instant::now() < deadline {
        let f = match c.frame(deadline.saturating_duration_since(Instant::now())) {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                out.error = Some(e);
                break;
            }
        };
        if f.obj == dd {
            match f.opcode {
                0 => {
                    let mut r = ArgsIn::new(&f.payload);
                    if let Some(id) = r.uint() {
                        c.iface.insert(id, "wl_data_offer".into());
                        offers.entry(id).or_default();
                    }
                }
                5 => {
                    saw_selection_event = true;
                    let mut r = ArgsIn::new(&f.payload);
                    let id = r.uint().unwrap_or(0);
                    current = if id == 0 { None } else { Some(id) };
                    if id != 0 {
                        // 收一下 wl_data_offer.offer(mime)
                        let sub = Instant::now() + Duration::from_millis(400);
                        while Instant::now() < sub {
                            match c.frame(sub.saturating_duration_since(Instant::now())) {
                                Ok(Some(f2)) => {
                                    if f2.obj == id && f2.opcode == 0 {
                                        let mut r2 = ArgsIn::new(&f2.payload);
                                        if let Some(m) = r2.string() {
                                            offers.entry(id).or_default().push(m);
                                        }
                                    }
                                }
                                _ => break,
                            }
                        }
                    }
                    break;
                }
                _ => {}
            }
        } else if f.opcode == 0 && c.iface.get(&f.obj).map(|s| s.as_str()) == Some("wl_data_offer")
        {
            let mut r = ArgsIn::new(&f.payload);
            if let Some(m) = r.string() {
                offers.entry(f.obj).or_default().push(m);
            }
        }
    }

    if let Some(id) = current {
        out.got_offer = true;
        out.mimes = offers.get(&id).cloned().unwrap_or_default();
        if let Some(mime) = pick_text_mime(&out.mimes) {
            match UnixStream::pair() {
                Ok((mine, theirs)) => {
                    let args = ArgsOut::new().string(&mime).done();
                    let ok = c.send_with_fd(id, 1, &args, Some(mine.as_raw_fd())).is_ok();
                    drop(mine);
                    if ok {
                        let _ = theirs.set_read_timeout(Some(Duration::from_millis(900)));
                        let mut data = Vec::new();
                        let mut buf = [0u8; 4096];
                        let mut rd = theirs;
                        loop {
                            match rd.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => {
                                    data.extend_from_slice(&buf[..n]);
                                    if data.len() > (1 << 20) {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                        out.text = Some(String::from_utf8_lossy(&data).to_string());
                    } else {
                        out.error = Some("把 fd 传给 compositor 失败".into());
                    }
                }
                Err(e) => out.error = Some(format!("socketpair 失败: {}", e)),
            }
        }
    } else if !saw_selection_event {
        out.error = Some("compositor 从未发送 wl_data_device.selection 事件".into());
    }

    out.sanity.push(format!(
        "握手: globals={} seat={} caps={}",
        globals.len(),
        seat_name.unwrap_or_else(|| "?".into()),
        seat_caps
            .map(|c| format!(
                "{}{}",
                if c & 1 != 0 { "pointer " } else { "" },
                if c & 2 != 0 { "keyboard" } else { "" }
            ))
            .unwrap_or_else(|| "?".into())
    ));
    out.sanity.push(match &c.protocol_error {
        Some(e) => format!("协议错误: {} ← 探测可能有问题，请把这段发我", e),
        None => "协议错误: 无（说明我们的请求都被 compositor 正常接受）".into(),
    });
    out
}

pub struct HoldResult {
    pub cancelled: bool,
    pub requests: u32,
    pub error: Option<String>,
    pub sanity: Vec<String>,
}

/// 占住 CLIPBOARD 并回吐 token，期间持续响应 compositor 取数据
pub fn hold_clipboard(token: &str, hold: Duration) -> HoldResult {
    let mut res = HoldResult {
        cancelled: false,
        requests: 0,
        error: None,
        sanity: Vec::new(),
    };
    let mut c = match Conn::connect() {
        Ok(c) => c,
        Err(e) => {
            res.error = Some(e);
            return res;
        }
    };
    let (reg, globals) = match c.globals() {
        Ok(g) => g,
        Err(e) => {
            res.error = Some(e);
            return res;
        }
    };
    let seat_g = globals.iter().find(|(_, i, _)| i == "wl_seat").cloned();
    let ddm_g = globals
        .iter()
        .find(|(_, i, _)| i == "wl_data_device_manager")
        .cloned();
    let (Some(seat_g), Some(ddm_g)) = (seat_g, ddm_g) else {
        res.error = Some("compositor 未提供 wl_seat / wl_data_device_manager".into());
        return res;
    };
    let seat = c.bind(reg, seat_g.0, "wl_seat", seat_g.2, 4);
    let ddm = c.bind(reg, ddm_g.0, "wl_data_device_manager", ddm_g.2, 3);

    let src = c.alloc("wl_data_source");
    if let Err(e) = c.send(ddm, 0, &ArgsOut::new().uint(src).done()) {
        res.error = Some(e);
        return res;
    }
    let _ = c.send(
        src,
        0,
        &ArgsOut::new().string("text/plain;charset=utf-8").done(),
    );
    let dd = c.alloc("wl_data_device");
    let _ = c.send(ddm, 1, &ArgsOut::new().uint(dd).uint(seat).done());
    if let Err(e) = c.send(dd, 1, &ArgsOut::new().uint(src).uint(0).done()) {
        res.error = Some(e);
        return res;
    }

    let deadline = Instant::now() + hold;
    while Instant::now() < deadline {
        match c.frame(deadline.saturating_duration_since(Instant::now())) {
            Ok(Some(f)) => {
                if f.obj == src {
                    match f.opcode {
                        1 => {
                            // send(mime, fd)：compositor 来取数据
                            res.requests += 1;
                            if let Some(fd) = c.next_fd() {
                                let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
                                let _ = file.write_all(token.as_bytes());
                                let _ = file.flush();
                                // file drop 即 close，compositor 收到 EOF
                            }
                        }
                        2 => res.cancelled = true,
                        _ => {}
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                res.error = Some(e);
                break;
            }
        }
    }
    res.sanity.push(match &c.protocol_error {
        Some(e) => format!("协议错误: {}", e),
        None => "协议错误: 无".into(),
    });
    res
}
