//! FinalShell 的字号：`~/.finalshell/config.json` 里的 `"font_size":N`，单位 **px**。
//!
//! 这个文件不能当普通 JSON 改：里面还有 `huw` 这类加密字段，重新序列化会把
//! 键序和转义弄乱。所以只做"定点文本替换"——找到 `"font_size":` 后面那段数字，
//! 原地换掉，其余字节一个不动。
//!
//! 另外 FinalShell 只在启动时读这个文件，运行中还会自己回写；所以写完要么重启它
//! （会断 SSH），要么明确告诉用户"下次启动才生效"。这里默认**不重启**。

use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const BIN: &str = "/usr/lib/finalshell/bin/FinalShell";

/// 可执行文件路径。允许用环境变量覆盖，只为能在测试里换成无害的程序
/// （真的去重启 FinalShell 会弹出一个 GUI 窗口，那是人的选择，不是测试的）。
pub fn bin() -> String {
    std::env::var("GNOUS_TERMFONT_FINALSHELL_BIN").unwrap_or_else(|_| BIN.to_string())
}
/// 定位用的 mark，注意冒号前面没有空格、`find` 命中后自己跳过空白。
const KEY: &str = "\"font_size\":";

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// 配置文件的原始文本，面板原样展示路径和大小。
    pub text: String,
    /// 现在写的字号，px。
    pub px: i64,
}

pub fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("GNOUS_TERMFONT_FINALSHELL_CONFIG") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/root"), PathBuf::from);
    home.join(".finalshell/config.json")
}

/// 找到 `"font_size":` 后面的数字，返回 `(值, 数字起点, 数字终点)`。
pub fn find_font_size(text: &str) -> Result<Option<(i64, usize, usize)>, String> {
    let Some(pos) = text.find(KEY) else {
        return Ok(None);
    };
    let bytes = text.as_bytes();
    let mut index = pos + KEY.len();
    while index < bytes.len() && (bytes[index] == b' ' || bytes[index] == b'\t') {
        index += 1;
    }
    let start = index;
    while index < bytes.len()
        && (bytes[index].is_ascii_digit() || bytes[index] == b'-' || bytes[index] == b'+')
    {
        index += 1;
    }
    if start == index {
        return Err("config.json 里 font_size 不是数字".to_string());
    }
    let value: i64 = text[start..index]
        .parse()
        .map_err(|_| format!("font_size 数值解析失败：{}", &text[start..index]))?;
    Ok(Some((value, start, index)))
}

/// 纯函数：把字号换掉，别的字节不动。
pub fn patch_font_size(text: &str, px: i64) -> Result<String, String> {
    let Some((_, start, end)) = find_font_size(text)? else {
        return Err("config.json 里没有 font_size 字段".to_string());
    };
    let mut out = String::with_capacity(text.len() + 4);
    out.push_str(&text[..start]);
    out.push_str(&px.to_string());
    out.push_str(&text[end..]);
    Ok(out)
}

pub fn read() -> Result<Config, String> {
    let path = config_path();
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("读不到 {}（{e}）", path.display()))?;
    let (px, _, _) = find_font_size(&text)?.ok_or_else(|| {
        format!(
            "{} 里没有 font_size 字段（FinalShell 装过并运行一次才有）",
            path.display()
        )
    })?;
    Ok(Config { text, px })
}

pub fn backup_path() -> PathBuf {
    config_path().with_extension("json.bak")
}

/// 写字号。返回 `(旧值, 备份路径)`。先备份、再原子替换，**不重启**。
pub fn write_px(px: i64) -> Result<(i64, PathBuf), String> {
    if !(super::config::FS_PX_MIN..=super::config::FS_PX_MAX).contains(&px) {
        return Err(format!(
            "FinalShell 字号 {px} px 超出范围（{}..={}）",
            super::config::FS_PX_MIN,
            super::config::FS_PX_MAX
        ));
    }
    let path = config_path();
    let config = read()?;
    let patched = patch_font_size(&config.text, px)?;
    if patched == config.text {
        // 已经是目标值：不写、不备份，免得把 .bak 覆盖成一个没用的副本。
        return Ok((config.px, backup_path()));
    }
    let backup = backup_path();
    std::fs::write(&backup, &config.text)
        .map_err(|e| format!("备份到 {} 失败：{e}", backup.display()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 {} 失败：{e}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut file =
            std::fs::File::create(&tmp).map_err(|e| format!("写 {} 失败：{e}", tmp.display()))?;
        file.write_all(patched.as_bytes())
            .map_err(|e| format!("写 {} 失败：{e}", tmp.display()))?;
        file.sync_all().ok();
    }
    // 保留原文件权限（FinalShell 自己建的是 0644，别因为重建变成别的）。
    if let Ok(meta) = std::fs::metadata(&path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 {} 失败：{e}", path.display()))?;
    Ok((config.px, backup))
}

/// 进程的 argv[0] 才算"是不是它"。
///
/// 用整条命令行做子串匹配会误杀：编辑器的命令行里也可能写着这个路径，某个
/// shell 脚本里更是随便就能出现。这里只认 argv[0]。
pub fn argv0_matches(argv0: &str, target: &str) -> bool {
    if argv0.is_empty() {
        return false;
    }
    if argv0 == target {
        return true;
    }
    // 有些启动方式只给文件名（`FinalShell`），此时按 basename 比。
    !argv0.contains('/')
        && target
            .rsplit('/')
            .next()
            .is_some_and(|name| !name.is_empty() && argv0 == name)
}

fn argv0(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).unwrap_or("")
}

/// 扫 `/proc` 找正在跑的 FinalShell，不依赖 `pgrep`。
pub fn running_pids() -> Vec<u32> {
    let me = std::process::id();
    let target = bin();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut pids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if argv0_matches(argv0(&raw), &target) {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    pids
}

pub fn is_running() -> bool {
    !running_pids().is_empty()
}

/// 重启 FinalShell。**会断开所有 SSH 会话**，只允许人手显式触发。
///
/// 顺序和老脚本一致：SIGTERM → 最多等 4 秒 → SIGKILL 剩下的 → 重新拉起。
pub fn restart() -> Result<u32, String> {
    let pids = running_pids();
    for pid in &pids {
        unsafe { libc::kill(*pid as i32, libc::SIGTERM) };
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline && is_running() {
        std::thread::sleep(Duration::from_millis(100));
    }
    for pid in running_pids() {
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    }
    if !pids.is_empty() {
        std::thread::sleep(Duration::from_millis(300));
    }
    let mut command = std::process::Command::new(bin());
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // 脱离当前进程组，这样关掉 GNOUS 不会把 FinalShell 一起带走。
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command
        .spawn()
        .map_err(|e| format!("重新拉起 {} 失败：{e}", bin()))?;
    std::thread::sleep(Duration::from_millis(1200));
    Ok(pids.len() as u32)
}

use std::os::unix::process::CommandExt as _;

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const SAMPLE: &str = r#"{"font_size":16,"huw":"abc","theme":"dark","font_name":"Monospace"}"#;

    #[test]
    fn finds_the_size_in_place() {
        let (px, start, end) = find_font_size(SAMPLE).unwrap().unwrap();
        assert_eq!(px, 16);
        assert_eq!(&SAMPLE[start..end], "16");
    }

    #[test]
    fn tolerates_spaces_after_the_colon() {
        let text = r#"{"font_size":  18 ,"x":1}"#;
        assert_eq!(find_font_size(text).unwrap().unwrap().0, 18);
        assert_eq!(
            patch_font_size(text, 20).unwrap(),
            r#"{"font_size":  20 ,"x":1}"#
        );
    }

    #[test]
    fn patches_only_the_number() {
        let patched = patch_font_size(SAMPLE, 17).unwrap();
        assert_eq!(
            patched,
            r#"{"font_size":17,"huw":"abc","theme":"dark","font_name":"Monospace"}"#
        );
        // 键序、加密字段、其它字节全部原样。
        assert!(patched.contains(r#""huw":"abc""#));
        assert_eq!(patched.len(), SAMPLE.len(), "16 → 17 位数不变");

        // 变位数时才改长度，其它字节仍然不动。
        let widened = patch_font_size(SAMPLE, 9).unwrap();
        assert_eq!(widened.len(), SAMPLE.len() - 1);
        assert!(widened.contains(r#""font_size":9"#));
    }

    #[test]
    fn patching_twice_is_stable() {
        let once = patch_font_size(SAMPLE, 13).unwrap();
        let twice = patch_font_size(&once, 13).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn missing_or_broken_field_is_an_error() {
        assert!(find_font_size(r#"{"a":1}"#).unwrap().is_none());
        assert!(patch_font_size(r#"{"a":1}"#, 12).is_err());
        assert!(find_font_size(r#"{"font_size":"16"}"#).is_err());
        assert!(find_font_size(r#"{"font_size":}"#).is_err());
    }

    #[test]
    fn refuses_absurd_sizes_before_writing() {
        assert!(write_px(5).unwrap_err().contains("超出范围"));
        assert!(write_px(201).unwrap_err().contains("超出范围"));
    }

    /// 真机集成测试（用无害程序）: 能认出以某个路径跑起来的进程，并在重启时
    /// 先 SIGTERM 再拉起。这里把 BIN 指到 `/usr/bin/sleep`，不碰 FinalShell。
    #[test]
    fn detects_and_restarts_a_process_by_its_path() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if !std::path::Path::new("/usr/bin/sleep").exists() {
            return;
        }
        unsafe { std::env::set_var("GNOUS_TERMFONT_FINALSHELL_BIN", "/usr/bin/sleep") };
        let mut victim = std::process::Command::new("/usr/bin/sleep")
            .arg("120")
            .spawn()
            .expect("能起一个 sleep");
        std::thread::sleep(Duration::from_millis(200));
        let pids = running_pids();
        assert!(
            pids.contains(&victim.id()),
            "应该认出 {victim:?}，实际 {pids:?}"
        );
        let killed = restart().expect("重启路径要能跑通");
        assert!(killed >= 1, "至少应该给一个进程发信号");
        // 原来的 sleep 必须已经不在了（被 SIGTERM 收掉）。
        let status = victim.wait().expect("能 wait");
        assert!(!status.success(), "sleep 应该是被信号杀掉的");
        // 重启会拉起一个新的 `sleep`（无参数，立刻退出），别留进程。
        for pid in running_pids() {
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
        unsafe { std::env::remove_var("GNOUS_TERMFONT_FINALSHELL_BIN") };
    }

    /// 回归：曾经用"整条命令行包含这个路径"来认进程，结果把一条只是**提到**
    /// 该路径的 bash 命令当成 FinalShell，restart 直接把它 SIGTERM 了。
    #[test]
    fn only_argv0_counts_when_looking_for_the_process() {
        assert!(argv0_matches("/usr/lib/finalshell/bin/FinalShell", BIN));
        assert!(argv0_matches("FinalShell", BIN), "只给文件名也认");
        assert!(!argv0_matches("/bin/bash", BIN), "命令行里提到路径不算");
        assert!(!argv0_matches("/usr/bin/vim", BIN));
        assert!(!argv0_matches("", BIN));
        assert!(argv0_matches("/usr/bin/sleep", "/usr/bin/sleep"));
        assert!(!argv0_matches("/usr/bin/sleep", "/usr/bin/sleepy"));
        assert!(argv0_matches("sleep", "/usr/bin/sleep"));
    }

    #[test]
    fn backup_name_matches_the_old_script() {
        // ~/.finalshell/config.json → ~/.finalshell/config.json.bak
        assert!(backup_path().to_string_lossy().ends_with("config.json.bak"));
    }

    #[test]
    fn runs_the_whole_write_path_on_a_temp_file() {
        let dir = std::env::temp_dir().join(format!("gnous-termfont-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, SAMPLE).unwrap();
        unsafe { std::env::set_var("GNOUS_TERMFONT_FINALSHELL_CONFIG", &path) };
        let (old, backup) = write_px(19).expect("应该能写");
        assert_eq!(old, 16);
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains(r#""font_size":19"#));
        assert!(after.contains(r#""huw":"abc""#), "加密字段必须原样保留");
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            SAMPLE,
            "备份是原文"
        );
        // 幂等：第二次不覆盖备份。
        std::fs::write(&backup, "sentinel").unwrap();
        let (old, _) = write_px(19).unwrap();
        assert_eq!(old, 19);
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "sentinel");
        unsafe { std::env::remove_var("GNOUS_TERMFONT_FINALSHELL_CONFIG") };
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
