//! 极简 Xlib 访问层。
//!
//! 关键决定：**不用 -lX11 链接**，改成运行时 `dlopen("libX11.so.6")` + `dlsym`。
//! 原因：这台机器只有 libX11 运行时（libX11.so.6），没装 libx11-dev（缺 libX11.so
//! 开发符号），链接期直接 `cannot find -lX11`；装 dev 包需要外网，而且会改系统。
//! dlopen 版本对系统零侵入，编译也不依赖任何 -dev 包。

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};
use std::sync::OnceLock;

type XOpenDisplayFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type XCloseDisplayFn = unsafe extern "C" fn(*mut c_void) -> c_int;
type XDefaultRootWindowFn = unsafe extern "C" fn(*mut c_void) -> c_ulong;
type XInternAtomFn = unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_ulong;
type XGetSelectionOwnerFn = unsafe extern "C" fn(*mut c_void, c_ulong) -> c_ulong;
type XGetWindowPropertyFn = unsafe extern "C" fn(
    *mut c_void,
    c_ulong,
    c_ulong,
    c_long,
    c_long,
    c_int,
    c_ulong,
    *mut c_ulong,
    *mut c_int,
    *mut c_ulong,
    *mut c_ulong,
    *mut *mut c_uchar,
) -> c_int;
type XQueryTreeFn = unsafe extern "C" fn(
    *mut c_void,
    c_ulong,
    *mut c_ulong,
    *mut c_ulong,
    *mut *mut c_ulong,
    *mut c_uint,
) -> c_int;
type XFreeFn = unsafe extern "C" fn(*mut c_void) -> c_int;
type XSetErrorHandlerFn =
    unsafe extern "C" fn(extern "C" fn(*mut c_void, *mut c_void) -> c_int) -> usize;
type XSyncFn = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_NOW: c_int = 2;

struct Xlib {
    x_open_display: XOpenDisplayFn,
    x_close_display: XCloseDisplayFn,
    x_default_root_window: XDefaultRootWindowFn,
    x_intern_atom: XInternAtomFn,
    x_get_selection_owner: XGetSelectionOwnerFn,
    x_get_window_property: XGetWindowPropertyFn,
    x_query_tree: XQueryTreeFn,
    x_free: XFreeFn,
    x_set_error_handler: XSetErrorHandlerFn,
    x_sync: XSyncFn,
}

static XLIB: OnceLock<Option<Xlib>> = OnceLock::new();

fn load() -> Option<&'static Xlib> {
    XLIB.get_or_init(|| unsafe {
        let mut handle = std::ptr::null_mut();
        for name in ["libX11.so.6", "libX11.so"] {
            let c = CString::new(name).ok()?;
            handle = dlopen(c.as_ptr(), RTLD_NOW);
            if !handle.is_null() {
                break;
            }
        }
        if handle.is_null() {
            return None;
        }
        macro_rules! sym {
            ($n:expr) => {{
                let c = CString::new($n).ok()?;
                let p = dlsym(handle, c.as_ptr());
                if p.is_null() {
                    return None;
                }
                std::mem::transmute(p)
            }};
        }
        Some(Xlib {
            x_open_display: sym!("XOpenDisplay"),
            x_close_display: sym!("XCloseDisplay"),
            x_default_root_window: sym!("XDefaultRootWindow"),
            x_intern_atom: sym!("XInternAtom"),
            x_get_selection_owner: sym!("XGetSelectionOwner"),
            x_get_window_property: sym!("XGetWindowProperty"),
            x_query_tree: sym!("XQueryTree"),
            x_free: sym!("XFree"),
            x_set_error_handler: sym!("XSetErrorHandler"),
            x_sync: sym!("XSync"),
        })
    })
    .as_ref()
}

/// 吞掉 X11 错误：窗口树随时可能变化，默认 handler 会直接 abort 整个进程
extern "C" fn ignore_x_error(_dpy: *mut c_void, _ev: *mut c_void) -> c_int {
    0
}

pub struct X11 {
    lib: &'static Xlib,
    dpy: *mut c_void,
}

unsafe impl Send for X11 {}

impl X11 {
    pub fn open() -> Option<X11> {
        let lib = load()?;
        unsafe {
            (lib.x_set_error_handler)(ignore_x_error);
        }
        let name = std::env::var("DISPLAY").ok();
        let cname = name.as_ref().and_then(|n| CString::new(n.as_str()).ok());
        let dpy = unsafe {
            match &cname {
                Some(c) => (lib.x_open_display)(c.as_ptr()),
                None => (lib.x_open_display)(std::ptr::null()),
            }
        };
        if dpy.is_null() {
            None
        } else {
            Some(X11 { lib, dpy })
        }
    }

    pub fn atom(&self, name: &str) -> u64 {
        let c = CString::new(name).unwrap();
        unsafe { (self.lib.x_intern_atom)(self.dpy, c.as_ptr(), 0) }
    }

    pub fn root(&self) -> u64 {
        unsafe { (self.lib.x_default_root_window)(self.dpy) }
    }

    pub fn selection_owner(&self, selection: &str) -> u64 {
        let a = self.atom(selection);
        unsafe { (self.lib.x_get_selection_owner)(self.dpy, a) }
    }

    fn get_prop(&self, win: u64, prop: &str) -> Option<(i32, Vec<u8>)> {
        let a = self.atom(prop);
        let mut actual_type: c_ulong = 0;
        let mut actual_format: c_int = 0;
        let mut nitems: c_ulong = 0;
        let mut bytes_after: c_ulong = 0;
        let mut data: *mut c_uchar = std::ptr::null_mut();
        let rc = unsafe {
            (self.lib.x_get_window_property)(
                self.dpy,
                win,
                a,
                0,
                4096,
                0,
                0,
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut data,
            )
        };
        if rc != 0 || data.is_null() || nitems == 0 {
            if !data.is_null() {
                unsafe { (self.lib.x_free)(data as *mut c_void) };
            }
            return None;
        }
        let nbytes = match actual_format {
            8 => nitems as usize,
            32 => (nitems as usize) * std::mem::size_of::<c_long>(),
            _ => {
                unsafe { (self.lib.x_free)(data as *mut c_void) };
                return None;
            }
        };
        let bytes = unsafe { std::slice::from_raw_parts(data as *const u8, nbytes).to_vec() };
        unsafe { (self.lib.x_free)(data as *mut c_void) };
        Some((actual_format as i32, bytes))
    }

    pub fn prop_string(&self, win: u64, prop: &str) -> Option<String> {
        let (fmt, bytes) = self.get_prop(win, prop)?;
        if fmt != 8 {
            return None;
        }
        let s = bytes
            .split(|b| *b == 0)
            .next()
            .map(|b| String::from_utf8_lossy(b).to_string())
            .unwrap_or_default();
        if s.is_empty() { None } else { Some(s) }
    }

    pub fn prop_cardinal32(&self, win: u64, prop: &str) -> Option<u32> {
        let (fmt, bytes) = self.get_prop(win, prop)?;
        if fmt != 32 || bytes.len() < std::mem::size_of::<c_long>() {
            return None;
        }
        let mut v: c_long = 0;
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                &mut v as *mut c_long as *mut u8,
                std::mem::size_of::<c_long>(),
            );
        }
        Some(v as u32)
    }

    pub fn wm_class(&self, win: u64) -> Option<String> {
        let (fmt, bytes) = self.get_prop(win, "WM_CLASS")?;
        if fmt != 8 {
            return None;
        }
        let parts: Vec<String> = bytes
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| String::from_utf8_lossy(b).to_string())
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" / "))
        }
    }

    fn children(&self, win: u64) -> Vec<u64> {
        let mut root: c_ulong = 0;
        let mut parent: c_ulong = 0;
        let mut kids: *mut c_ulong = std::ptr::null_mut();
        let mut n: c_uint = 0;
        let ok = unsafe {
            (self.lib.x_query_tree)(self.dpy, win, &mut root, &mut parent, &mut kids, &mut n)
        };
        if ok == 0 || kids.is_null() || n == 0 {
            return Vec::new();
        }
        let v = unsafe { std::slice::from_raw_parts(kids, n as usize).to_vec() };
        unsafe { (self.lib.x_free)(kids as *mut c_void) };
        v
    }

    pub fn find_windows_by_class(&self, needle: &str, max_depth: u32) -> Vec<(u64, String)> {
        let needle = needle.to_lowercase();
        let mut out = Vec::new();
        let mut stack = vec![(self.root(), 0u32)];
        let mut guard = 0;
        while let Some((w, depth)) = stack.pop() {
            guard += 1;
            if guard > 20000 {
                break;
            }
            if let Some(class) = self.wm_class(w) {
                if class.to_lowercase().contains(&needle) {
                    out.push((w, class));
                }
            }
            if depth < max_depth {
                for k in self.children(w) {
                    stack.push((k, depth + 1));
                }
            }
        }
        unsafe { (self.lib.x_sync)(self.dpy, 0) };
        out
    }

    pub fn describe_window(&self, win: u64) -> WindowInfo {
        WindowInfo {
            id: win,
            wm_class: self.wm_class(win),
            wm_name: self
                .prop_string(win, "WM_NAME")
                .or_else(|| self.prop_string(win, "_NET_WM_NAME")),
            pid: self.prop_cardinal32(win, "_NET_WM_PID"),
        }
    }
}

impl Drop for X11 {
    fn drop(&mut self) {
        unsafe { (self.lib.x_close_display)(self.dpy) };
    }
}

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: u64,
    pub wm_class: Option<String>,
    pub wm_name: Option<String>,
    pub pid: Option<u32>,
}

impl WindowInfo {
    pub fn is_anonymous(&self) -> bool {
        self.wm_class.is_none() && self.wm_name.is_none() && self.pid.is_none()
    }

    pub fn summary(&self) -> String {
        if self.is_anonymous() {
            return format!(
                "0x{:x}  ← 隐形窗口：无 WM_CLASS / WM_NAME / _NET_WM_PID（这是 compositor 自己占坑的窗口）",
                self.id
            );
        }
        let mut parts = vec![format!("0x{:x}", self.id)];
        if let Some(c) = &self.wm_class {
            parts.push(format!("class={}", c));
        }
        if let Some(n) = &self.wm_name {
            parts.push(format!("name={}", n));
        }
        if let Some(p) = self.pid {
            parts.push(format!("pid={}", p));
        }
        parts.join("  ")
    }
}
