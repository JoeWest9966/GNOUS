//! 极小的 D-Bus 客户端：只做 GNOUS 需要的那几种消息。
//!
//! 为什么自己写：`display-mode` 用 PyGObject，装上 GNOUS 就要求机器有 GLib 的 Python 绑定；
//! 用 `busctl` / `gdbus` 则要解析别人的文本输出，还得猜 systemd 版本差异。这里是
//! 一个只依赖 `std` 的实现，签名解析、对齐、编解码都在本文件里，并且有单测。
//!
//! 支持范围（够用即止）：
//! - 消息类型：方法调用、方法返回、错误
//! - 参数类型：`y b u i x t d s o g v a… (…) {…}`
//! - 认证：`EXTERNAL`（unix socket 的 peer credential）
//!
//! 不支持：`UNIX_FD` 传递、信号接收、大端序总线。这些都是 mutter 的显示器配置用不到的。

use std::fmt;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

const METHOD_CALL: u8 = 1;
const METHOD_REPLY: u8 = 2;
const METHOD_ERROR: u8 = 3;

/// 消息头字段编号，见 D-Bus 规范。
const FIELD_PATH: u8 = 1;
const FIELD_INTERFACE: u8 = 2;
const FIELD_MEMBER: u8 = 3;
const FIELD_ERROR_NAME: u8 = 4;
const FIELD_REPLY_SERIAL: u8 = 5;
const FIELD_DESTINATION: u8 = 6;
const FIELD_SIGNATURE: u8 = 8;

#[derive(Debug)]
pub enum Error {
    NoBus(String),
    Io(std::io::Error),
    Auth(String),
    Protocol(String),
    /// 对端返回了 `org.freedesktop.DBus.Error.*`。
    Remote {
        name: String,
        message: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoBus(why) => write!(f, "找不到会话总线：{why}"),
            Error::Io(e) => write!(f, "总线读写失败：{e}"),
            Error::Auth(why) => write!(f, "总线认证失败：{why}"),
            Error::Protocol(why) => write!(f, "总线协议错误：{why}"),
            Error::Remote { name, message } => write!(f, "{name}：{message}"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// 一个 D-Bus 值。变体保留自己的签名，因为解码 `v` 必须靠它。
#[derive(Debug, Clone, PartialEq)]
pub enum V {
    Byte(u8),
    Bool(bool),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F64(f64),
    Str(String),
    Arr(Vec<V>),
    Struct(Vec<V>),
    Var(String, Box<V>),
}

impl V {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            V::Str(s) => Some(s),
            V::Var(_, v) => v.as_str(),
            _ => None,
        }
    }
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            V::U32(n) => Some(*n),
            V::Byte(n) => Some(*n as u32),
            V::Var(_, v) => v.as_u32(),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            V::Bool(b) => Some(*b),
            V::Var(_, v) => v.as_bool(),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            V::F64(n) => Some(*n),
            V::Var(_, v) => v.as_f64(),
            _ => None,
        }
    }
    pub fn items(&self) -> &[V] {
        match self {
            V::Arr(items) | V::Struct(items) => items,
            V::Var(_, v) => v.items(),
            _ => &[],
        }
    }
    /// `a{sv}` 查表：键是字符串。
    pub fn get(&self, key: &str) -> Option<&V> {
        self.items().iter().find_map(|entry| {
            let pair = entry.items();
            if pair.len() == 2 && pair[0].as_str() == Some(key) {
                Some(&pair[1])
            } else {
                None
            }
        })
    }
}

/// 签名解析结果。D-Bus 的签名是"后序遍历"的树，这里显式建树。
#[derive(Debug, Clone, PartialEq)]
pub enum Sig {
    Byte,
    Bool,
    U32,
    I32,
    U64,
    I64,
    F64,
    Str,
    /// 签名类型 `g`：1 字节长度、1 字节对齐（和 `s` 不一样！）
    SigStr,
    Var,
    Arr(Box<Sig>),
    Struct(Vec<Sig>),
}

impl Sig {
    pub fn parse(text: &str) -> Result<Sig> {
        let mut chars = text.chars().peekable();
        let sig = parse_one(&mut chars)?;
        if chars.next().is_some() {
            return Err(Error::Protocol(format!("签名有多余内容：{text}")));
        }
        Ok(sig)
    }

    pub fn parse_list(text: &str) -> Result<Vec<Sig>> {
        let mut chars = text.chars().peekable();
        let mut out = Vec::new();
        while chars.peek().is_some() {
            out.push(parse_one(&mut chars)?);
        }
        Ok(out)
    }

    /// 对齐要求：8 字节类型必须落在 8 的倍数上。
    pub fn alignment(&self) -> usize {
        match self {
            Sig::Byte => 1,
            Sig::Bool | Sig::U32 | Sig::I32 | Sig::Str | Sig::Var | Sig::Arr(_) => 4,
            Sig::SigStr => 1,
            Sig::U64 | Sig::I64 | Sig::F64 | Sig::Struct(_) => 8,
        }
    }
}

fn parse_one(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Sig> {
    let c = chars
        .next()
        .ok_or_else(|| Error::Protocol("签名意外结束".into()))?;
    Ok(match c {
        'y' => Sig::Byte,
        'b' => Sig::Bool,
        'u' => Sig::U32,
        'i' => Sig::I32,
        'x' => Sig::I64,
        't' => Sig::U64,
        'd' => Sig::F64,
        's' | 'o' => Sig::Str,
        'g' => Sig::SigStr,
        'v' => Sig::Var,
        'a' => Sig::Arr(Box::new(parse_one(chars)?)),
        '(' => {
            let mut fields = Vec::new();
            loop {
                match chars.peek() {
                    Some(')') => {
                        chars.next();
                        break;
                    }
                    Some(_) => fields.push(parse_one(chars)?),
                    None => return Err(Error::Protocol("结构体签名没有闭合".into())),
                }
            }
            Sig::Struct(fields)
        }
        // `{sv}` 只在数组里出现，解码时按"两元素结构体"处理。
        '{' => {
            let mut fields = Vec::new();
            loop {
                match chars.peek() {
                    Some('}') => {
                        chars.next();
                        break;
                    }
                    Some(_) => fields.push(parse_one(chars)?),
                    None => return Err(Error::Protocol("字典签名没有闭合".into())),
                }
            }
            if fields.len() != 2 {
                return Err(Error::Protocol("字典条目必须是两个字段".into()));
            }
            Sig::Struct(fields)
        }
        other => return Err(Error::Protocol(format!("不支持的签名类型 {other}"))),
    })
}

fn pad(buf: &mut Vec<u8>, align: usize) {
    while buf.len() % align != 0 {
        buf.push(0);
    }
}

fn encode_into(buf: &mut Vec<u8>, sig: &Sig, value: &V) -> Result<()> {
    match (sig, value) {
        (Sig::Byte, V::Byte(n)) => buf.push(*n),
        (Sig::Bool, V::Bool(b)) => {
            pad(buf, 4);
            buf.extend_from_slice(&(*b as u32).to_le_bytes());
        }
        (Sig::U32, V::U32(n)) => {
            pad(buf, 4);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        (Sig::I32, V::I32(n)) => {
            pad(buf, 4);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        (Sig::U64, V::U64(n)) => {
            pad(buf, 8);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        (Sig::I64, V::I64(n)) => {
            pad(buf, 8);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        (Sig::F64, V::F64(n)) => {
            pad(buf, 8);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        (Sig::Str, V::Str(s)) => encode_str(buf, s),
        (Sig::SigStr, V::Str(s)) => encode_sig_str(buf, s),
        (Sig::Var, V::Var(inner_sig, inner)) => {
            encode_sig_str(buf, inner_sig);
            let parsed = Sig::parse(inner_sig)?;
            encode_into(buf, &parsed, inner)?;
        }
        (Sig::Arr(elem), V::Arr(items)) => {
            pad(buf, 4);
            let len_at = buf.len();
            buf.extend_from_slice(&0u32.to_le_bytes());
            pad(buf, elem.alignment());
            let start = buf.len();
            for item in items {
                encode_into(buf, elem, item)?;
            }
            // 数组长度**不含**最后一个元素的尾部填充；填充留给后面统一补。
            let len = (buf.len() - start) as u32;
            buf[len_at..len_at + 4].copy_from_slice(&len.to_le_bytes());
            pad(buf, elem.alignment());
        }
        (Sig::Struct(fields), V::Struct(values)) => {
            if fields.len() != values.len() {
                return Err(Error::Protocol(format!(
                    "结构体字段数不匹配：签名 {} 个，值 {} 个",
                    fields.len(),
                    values.len()
                )));
            }
            pad(buf, 8);
            for (f, v) in fields.iter().zip(values) {
                encode_into(buf, f, v)?;
            }
        }
        _ => return Err(Error::Protocol(format!("值 {value:?} 与签名 {sig:?} 不符"))),
    }
    Ok(())
}

/// SIGNATURE：1 字节长度，无对齐填充。
fn encode_sig_str(buf: &mut Vec<u8>, s: &str) {
    buf.push(s.len() as u8);
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
}

fn encode_str(buf: &mut Vec<u8>, s: &str) {
    pad(buf, 4);
    buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    /// 对齐到 `align` 的倍数。数组/结构体末尾的填充可能超出数据长度
    /// （D-Bus 的数组长度不含最后元素的尾部填充），这里按"被截断的对齐"处理：
    /// 越界就停在末尾，真正的短缺留给 `take` 报错。
    fn align(&mut self, align: usize) -> Result<()> {
        while self.pos % align != 0 && self.pos < self.data.len() {
            self.pos += 1;
        }
        if self.pos > self.data.len() {
            self.pos = self.data.len();
        }
        Ok(())
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos + n;
        if end > self.data.len() {
            return Err(Error::Protocol(format!(
                "数据提前结束：需要 {n} 字节，剩余 {}",
                self.data.len() - self.pos
            )));
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32> {
        self.align(4)?;
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn str(&mut self) -> Result<String> {
        let len = self.u32()? as usize;
        let bytes = self.take(len + 1)?;
        Ok(String::from_utf8_lossy(&bytes[..len]).into_owned())
    }
}

/// 解码"数组数据"（不含前面的长度字段）。最后元素可能没有尾部填充。
fn decode_array_data(elem: &Sig, data: &[u8]) -> Result<Vec<V>> {
    let mut cur = Cursor::new(data);
    cur.align(elem.alignment())?;
    let end = data.len();
    let mut items = Vec::new();
    while cur.pos < end {
        items.push(decode_one(elem, &mut cur)?);
    }
    Ok(items)
}

fn decode_sig(text: &str, cur: &mut Cursor<'_>) -> Result<V> {
    let sig = Sig::parse(text)?;
    decode_one(&sig, cur)
}

fn decode_ctx(text: &str, cur: &mut Cursor<'_>) -> Result<V> {
    let sig = match Sig::parse(text) {
        Ok(sig) => sig,
        Err(e) => return Err(e),
    };
    let start = cur.pos;
    decode_one(&sig, cur).map_err(|e| {
        Error::Protocol(format!(
            "{e}（签名 {text}，起点 {start}，终点 {}，数据长 {}）",
            cur.pos,
            cur.data.len()
        ))
    })
}

fn decode_one(sig: &Sig, cur: &mut Cursor<'_>) -> Result<V> {
    Ok(match sig {
        Sig::Byte => V::Byte(cur.take(1)?[0]),
        Sig::Bool => V::Bool(cur.u32()? != 0),
        Sig::U32 => V::U32(cur.u32()?),
        Sig::I32 => {
            cur.align(4)?;
            V::I32(i32::from_le_bytes(cur.take(4)?.try_into().unwrap()))
        }
        Sig::U64 => {
            cur.align(8)?;
            V::U64(u64::from_le_bytes(cur.take(8)?.try_into().unwrap()))
        }
        Sig::I64 => {
            cur.align(8)?;
            V::I64(i64::from_le_bytes(cur.take(8)?.try_into().unwrap()))
        }
        Sig::F64 => {
            cur.align(8)?;
            V::F64(f64::from_le_bytes(cur.take(8)?.try_into().unwrap()))
        }
        Sig::Str => V::Str(cur.str()?),
        Sig::SigStr => {
            let len = cur.take(1)?[0] as usize;
            let bytes = cur.take(len + 1)?;
            V::Str(String::from_utf8_lossy(&bytes[..len]).into_owned())
        }
        Sig::Var => {
            let len = cur.take(1)?[0] as usize;
            let bytes = cur.take(len + 1)?;
            let inner = String::from_utf8_lossy(&bytes[..len]).into_owned();
            let value = decode_sig(&inner, cur)?;
            V::Var(inner, Box::new(value))
        }
        Sig::Arr(elem) => {
            let len = cur.u32()? as usize;
            cur.align(elem.alignment())?;
            let end = cur.pos + len;
            if end > cur.data.len() {
                return Err(Error::Protocol("数组越界".into()));
            }
            let mut items = Vec::new();
            while cur.pos < end {
                items.push(decode_one(elem, cur)?);
            }
            V::Arr(items)
        }
        Sig::Struct(fields) => {
            cur.align(8)?;
            let mut values = Vec::with_capacity(fields.len());
            for f in fields {
                values.push(decode_one(f, cur)?);
            }
            cur.align(8)?;
            V::Struct(values)
        }
    })
}

/// 会话总线连接。串行号自增，一次调用一次往返。
pub struct Conn {
    stream: UnixStream,
    serial: u32,
    /// 上一次回复里对端的唯一名，只用于诊断信息。
    pub unique_name: String,
}

fn bus_address() -> Result<PathBuf> {
    if let Ok(addr) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        for part in addr.split(';') {
            if let Some(path) = part.strip_prefix("unix:path=") {
                return Ok(PathBuf::from(path));
            }
            if let Some(name) = part.strip_prefix("unix:abstract=") {
                // abstract socket 在 Linux 上用 `\0` 前缀表示。
                return Err(Error::NoBus(format!(
                    "只支持 unix:path=，不支持 abstract={name}"
                )));
            }
        }
        return Err(Error::NoBus(format!("无法解析地址 {addr}")));
    }
    let uid = unsafe { libc::getuid() };
    let guess = PathBuf::from(format!("/run/user/{uid}/bus"));
    if guess.exists() {
        return Ok(guess);
    }
    Err(Error::NoBus(format!("{} 不存在", guess.display())))
}

impl Conn {
    pub fn session() -> Result<Conn> {
        let path = bus_address()?;
        let stream = UnixStream::connect(&path).map_err(|e| {
            Error::NoBus(format!(
                "连不上 {}：{e}（图形会话没起或不在会话里）",
                path.display()
            ))
        })?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(6)))?;
        stream.set_write_timeout(Some(std::time::Duration::from_secs(6)))?;
        let mut conn = Conn {
            stream,
            serial: 0,
            unique_name: String::new(),
        };
        conn.authenticate()?;
        // 总线规定：每条新连接的第一条消息必须是 Hello，否则会被直接断开。
        let name = conn.call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "Hello",
            "",
            &V::Struct(Vec::new()),
        )?;
        conn.unique_name = name.as_str().unwrap_or_default().to_string();
        Ok(conn)
    }

    fn authenticate(&mut self) -> Result<()> {
        self.stream.write_all(&[0])?;
        let uid = unsafe { libc::getuid() };
        let hex: String = format!("{uid}")
            .as_bytes()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect();
        self.stream
            .write_all(format!("AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let line = self.read_line()?;
        if !line.starts_with("OK") {
            return Err(Error::Auth(line));
        }
        self.stream.write_all(b"BEGIN\r\n")?;
        self.stream.flush()?;
        Ok(())
    }

    fn read_line(&mut self) -> Result<String> {
        let mut out = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let n = self.stream.read(&mut byte)?;
            if n == 0 {
                return Err(Error::Auth("总线在认证阶段断开".into()));
            }
            if byte[0] == b'\n' {
                break;
            }
            if byte[0] != b'\r' {
                out.push(byte[0]);
            }
            if out.len() > 4096 {
                return Err(Error::Auth("认证应答过长".into()));
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    fn next_serial(&mut self) -> u32 {
        self.serial += 1;
        if self.serial == 0 {
            self.serial = 1;
        }
        self.serial
    }

    /// 发一次方法调用并读回它的回复。返回的是**去掉外层结构后的**第一个返回值；
    /// 多返回值时用 `call_multi`。
    pub fn call(
        &mut self,
        dest: &str,
        path: &str,
        iface: &str,
        member: &str,
        sig: &str,
        body: &V,
    ) -> Result<V> {
        let mut reply = self.call_multi(dest, path, iface, member, sig, body)?;
        match reply.len() {
            0 => Ok(V::Struct(Vec::new())),
            1 => Ok(reply.remove(0)),
            _ => Ok(V::Struct(reply)),
        }
    }

    pub fn call_multi(
        &mut self,
        dest: &str,
        path: &str,
        iface: &str,
        member: &str,
        sig: &str,
        body: &V,
    ) -> Result<Vec<V>> {
        let serial = self.next_serial();
        let mut payload = Vec::new();
        if !sig.is_empty() {
            let signatures = Sig::parse_list(sig)?;
            let values = body.items();
            if signatures.len() != values.len() {
                return Err(Error::Protocol(format!(
                    "参数个数不匹配：签名 {sig} 需要 {} 个，给了 {}",
                    signatures.len(),
                    values.len()
                )));
            }
            for (s, v) in signatures.iter().zip(values) {
                pad(&mut payload, s.alignment());
                encode_into(&mut payload, s, v)?;
            }
        }

        let mut fields = Vec::new();
        encode_field(&mut fields, FIELD_PATH, "o", &V::Str(path.into()))?;
        encode_field(&mut fields, FIELD_INTERFACE, "s", &V::Str(iface.into()))?;
        encode_field(&mut fields, FIELD_MEMBER, "s", &V::Str(member.into()))?;
        encode_field(&mut fields, FIELD_DESTINATION, "s", &V::Str(dest.into()))?;
        if !sig.is_empty() {
            encode_field(&mut fields, FIELD_SIGNATURE, "g", &V::Str(sig.into()))?;
        }

        let mut msg = Vec::new();
        msg.push(b'l');
        msg.push(METHOD_CALL);
        msg.push(0);
        msg.push(1);
        msg.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        msg.extend_from_slice(&serial.to_le_bytes());
        // 头字段是 a(yv)：先长度，再 8 字节对齐的条目。
        msg.extend_from_slice(&(fields.len() as u32).to_le_bytes());
        pad(&mut msg, 8);
        msg.extend_from_slice(&fields);
        pad(&mut msg, 8);
        msg.extend_from_slice(&payload);
        if std::env::var_os("GNOUS_DBUS_DEBUG").is_some() {
            let hex: String = msg.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("SEND {hex}");
        }
        self.stream.write_all(&msg)?;
        self.stream.flush()?;

        loop {
            let (kind, _flags, reply_serial, error_name, sig_text, body_bytes) =
                self.read_message()?;
            if reply_serial != Some(serial) {
                // 不是我们这条的回复（例如广播信号），跳过。
                continue;
            }
            if std::env::var_os("GNOUS_DBUS_DEBUG").is_some() {
                eprintln!(
                    "REPLY kind={kind} sig={sig_text:?} body_len={} first={:02x?}",
                    body_bytes.len(),
                    &body_bytes[..body_bytes.len().min(24)]
                );
            }
            let signatures = if sig_text.is_empty() {
                Vec::new()
            } else {
                Sig::parse_list(&sig_text)?
            };
            let mut cur = Cursor::new(&body_bytes);
            let mut values = Vec::with_capacity(signatures.len());
            for s in &signatures {
                let text = signature_text(s);
                values.push(decode_ctx(&text, &mut cur)?);
            }
            match kind {
                METHOD_REPLY => return Ok(values),
                METHOD_ERROR => {
                    let name = error_name.unwrap_or_else(|| "org.freedesktop.DBus.Error".into());
                    let message = values
                        .first()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default();
                    return Err(Error::Remote { name, message });
                }
                other => {
                    return Err(Error::Protocol(format!("意外消息类型 {other}")));
                }
            }
        }
    }

    fn read_message(&mut self) -> Result<(u8, u8, Option<u32>, Option<String>, String, Vec<u8>)> {
        let mut head = [0u8; 16];
        self.read_exact(&mut head)?;
        if head[0] != b'l' && head[0] != b'B' {
            return Err(Error::Protocol("头字节不是字节序标记".into()));
        }
        if head[0] == b'B' {
            return Err(Error::Protocol("不支持大端序总线".into()));
        }
        let kind = head[1];
        let flags = head[2];
        let body_len = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize;
        let fields_len = u32::from_le_bytes(head[12..16].try_into().unwrap()) as usize;
        let padded_fields = (fields_len + 7) & !7;
        let mut fields_bytes = vec![0u8; padded_fields];
        self.read_exact(&mut fields_bytes)?;
        fields_bytes.truncate(fields_len);
        let mut body = vec![0u8; body_len];
        self.read_exact(&mut body)?;

        // head 里已经读过数组长度，剩下的就是数组数据本身，不能再当 `a(yv)` 解。
        let entry_sig = Sig::Struct(vec![Sig::Byte, Sig::Var]);
        let entries = V::Arr(decode_array_data(&entry_sig, &fields_bytes)?);
        let mut reply_serial = None;
        let mut error_name = None;
        let mut signature = String::new();
        for entry in entries.items() {
            let pair = entry.items();
            let Some(code) = pair.first().map(|v| match v {
                V::Byte(b) => *b,
                _ => 0,
            }) else {
                continue;
            };
            let Some(value) = pair.get(1) else { continue };
            match code {
                FIELD_REPLY_SERIAL => reply_serial = value.as_u32(),
                FIELD_ERROR_NAME => error_name = value.as_str().map(str::to_owned),
                FIELD_SIGNATURE => signature = value.as_str().unwrap_or_default().to_owned(),
                _ => {}
            }
        }
        Ok((kind, flags, reply_serial, error_name, signature, body))
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.stream.read_exact(buf)?;
        Ok(())
    }
}

/// 头字段是 `(y v)`：变体里的类型签名必须**逐字**符合规范
/// （PATH 是 `o`、SIGNATURE 是 `g`，写成 `s` 会被严格的 daemon 直接拒绝）。
fn encode_field(buf: &mut Vec<u8>, code: u8, type_text: &str, value: &V) -> Result<()> {
    pad(buf, 8);
    buf.push(code);
    encode_sig_str(buf, type_text);
    match (type_text, value) {
        ("o" | "s", V::Str(text)) => {
            pad(buf, 4);
            buf.extend_from_slice(&(text.len() as u32).to_le_bytes());
            buf.extend_from_slice(text.as_bytes());
            buf.push(0);
        }
        ("g", V::Str(text)) => encode_sig_str(buf, text),
        ("u", V::U32(n)) => {
            pad(buf, 4);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        _ => {
            return Err(Error::Protocol(format!(
                "头字段 {type_text} 不支持 {value:?}"
            )));
        }
    }
    Ok(())
}

fn signature_text(sig: &Sig) -> String {
    match sig {
        Sig::Byte => "y".into(),
        Sig::Bool => "b".into(),
        Sig::U32 => "u".into(),
        Sig::I32 => "i".into(),
        Sig::U64 => "t".into(),
        Sig::I64 => "x".into(),
        Sig::F64 => "d".into(),
        Sig::Str => "s".into(),
        Sig::SigStr => "g".into(),
        Sig::Var => "v".into(),
        Sig::Arr(inner) => format!("a{}", signature_text(inner)),
        Sig::Struct(fields) => format!(
            "({})",
            fields
                .iter()
                .map(signature_text)
                .collect::<Vec<_>>()
                .join("")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_type_has_a_one_byte_length() {
        let mut buf = Vec::new();
        encode_sig_str(&mut buf, "a{sv}");
        assert_eq!(buf, vec![5, b'a', b'{', b's', b'v', b'}', 0]);
        let mut cur = Cursor::new(&buf);
        assert_eq!(
            decode_one(&Sig::SigStr, &mut cur).unwrap(),
            V::Str("a{sv}".into())
        );
        assert_eq!(cur.pos, buf.len());
    }

    #[test]
    fn parses_nested_signatures() {
        let sig = Sig::parse_list("a(iiduba(ssa{sv}))a{sv}")
            .expect("mutter 的 ApplyMonitorsConfig 签名应该能解析");
        assert_eq!(sig.len(), 2, "两个参数");
        assert!(matches!(sig[0], Sig::Arr(_)));
        assert!(matches!(sig[1], Sig::Arr(_)));
        let Sig::Arr(inner) = &sig[0] else {
            unreachable!()
        };
        let Sig::Struct(fields) = &**inner else {
            panic!("a(…) 的元素应该是结构体");
        };
        assert_eq!(fields.len(), 6, "x y scale transform primary monitors");
    }

    #[test]
    fn rejects_garbage_signatures() {
        assert!(Sig::parse("a(").is_err());
        assert!(Sig::parse("q").is_err());
        assert!(Sig::parse("uu").is_err());
    }

    #[test]
    fn roundtrips_every_supported_type() {
        let cases: Vec<(&str, V)> = vec![
            ("y", V::Byte(7)),
            ("b", V::Bool(true)),
            ("u", V::U32(4294967295)),
            ("i", V::I32(-42)),
            ("t", V::U64(u64::MAX)),
            ("x", V::I64(i64::MIN)),
            ("d", V::F64(59.909)),
            ("s", V::Str("HDMI-1".into())),
            ("v", V::Var("u".into(), Box::new(V::U32(1)))),
            (
                "a(ss)",
                V::Arr(vec![
                    V::Struct(vec![V::Str("HDMI-1".into()), V::Str("XMI".into())]),
                    V::Struct(vec![V::Str("eDP-1".into()), V::Str("CMN".into())]),
                ]),
            ),
            (
                "a{sv}",
                V::Arr(vec![
                    V::Struct(vec![
                        V::Str("is-preferred".into()),
                        V::Var("b".into(), Box::new(V::Bool(true))),
                    ]),
                    V::Struct(vec![
                        V::Str("layout-mode".into()),
                        V::Var("u".into(), Box::new(V::U32(1))),
                    ]),
                ]),
            ),
            (
                "a(iiduba(ssa{sv}))",
                V::Arr(vec![V::Struct(vec![
                    V::I32(0),
                    V::I32(0),
                    V::F64(1.0),
                    V::U32(0),
                    V::Bool(true),
                    V::Arr(vec![V::Struct(vec![
                        V::Str("HDMI-1".into()),
                        V::Str("2048x1152@59.909".into()),
                        V::Arr(vec![]),
                    ])]),
                ])]),
            ),
        ];
        for (sig, value) in cases {
            let parsed = Sig::parse(sig).expect(sig);
            let mut buf = Vec::new();
            encode_into(&mut buf, &parsed, &value).expect(sig);
            let mut cur = Cursor::new(&buf);
            let decoded = decode_one(&parsed, &mut cur).expect(sig);
            assert_eq!(decoded, value, "签名 {sig} 往返不一致");
            assert_eq!(cur.pos, buf.len(), "签名 {sig} 没有恰好读完");
        }
    }

    #[test]
    fn alignment_follows_the_spec() {
        let sig = Sig::parse("(yut)").unwrap();
        let mut buf = Vec::new();
        let value = V::Struct(vec![V::Byte(1), V::U32(2), V::U64(3)]);
        encode_into(&mut buf, &sig, &value).unwrap();
        // 1 byte + 3 pad + 4 bytes + 8 bytes = 16
        assert_eq!(buf.len(), 16);
    }

    #[test]
    fn bus_errors_are_reported_not_swallowed() {
        // 只要没有会话总线，就必须是明确的 NoBus，而不是 panic。
        let saved = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok();
        unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus") };
        let err = match Conn::session() {
            Ok(_) => panic!("不该连上不存在的总线"),
            Err(e) => e,
        };
        match saved {
            Some(v) => unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", v) },
            None => unsafe { std::env::remove_var("DBUS_SESSION_BUS_ADDRESS") },
        }
        assert!(matches!(err, Error::NoBus(_)), "实际是 {err}");
    }
}
