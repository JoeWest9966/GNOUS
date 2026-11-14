//! 终端字体扩展的目标值：`$XDG_CONFIG_HOME/gnous/termfont.json`。
//!
//! 这里只存"想调成多大"，不存"现在多大"——现在多大永远去 gsettings 和
//! FinalShell 的配置文件里读，面板不缓存也不猜。
//!
//! 缺省值刻意等于本机此刻的真实状态（12 pt / 16 px），装上 GNOUS 不会悄悄
//! 改变任何东西；要改字号必须显式点一次 TERM / FS / ALL。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// GNOME 终端字号的合法范围，单位 pt。老脚本用的是 6..=48。
pub const TERM_PT_MIN: f64 = 6.0;
pub const TERM_PT_MAX: f64 = 48.0;
/// FinalShell 字号的合法范围，单位 px。
pub const FS_PX_MIN: i64 = 6;
pub const FS_PX_MAX: i64 = 200;
/// 两者单位不同：1 pt = 4/3 px。
pub const PX_PER_PT: f64 = 4.0 / 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Targets {
    /// GNOME 终端想调成的字号，pt。
    pub term_pt: f64,
    /// FinalShell 想调成的字号，px。
    pub fs_px: i64,
    /// 为真时 `ALL` 用 `term_pt` 换算出的 px，而不是 `fs_px`。
    pub auto_convert: bool,
}

impl Default for Targets {
    fn default() -> Self {
        Targets {
            term_pt: 12.0,
            fs_px: 16,
            auto_convert: true,
        }
    }
}

impl Targets {
    /// `ALL` 实际会写进 FinalShell 的 px。
    pub fn convert(&self) -> i64 {
        if self.auto_convert {
            pt2px(self.term_pt)
        } else {
            self.fs_px
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.term_pt.is_finite() || !(TERM_PT_MIN..=TERM_PT_MAX).contains(&self.term_pt) {
            return Err(format!(
                "终端字号 {} pt 超出范围（{TERM_PT_MIN}..={TERM_PT_MAX}）",
                self.term_pt
            ));
        }
        if !(FS_PX_MIN..=FS_PX_MAX).contains(&self.fs_px) {
            return Err(format!(
                "FinalShell 字号 {} px 超出范围（{FS_PX_MIN}..={FS_PX_MAX}）",
                self.fs_px
            ));
        }
        Ok(())
    }

    /// 步进：`delta` 为 +1/-1，终端按 0.5 pt 走，FinalShell 按 1 px 走。
    pub fn stepped(&self, term_steps: i32, fs_steps: i32) -> Targets {
        let mut next = *self;
        if term_steps != 0 {
            let pt = (self.term_pt + f64::from(term_steps) * 0.5).clamp(TERM_PT_MIN, TERM_PT_MAX);
            next.term_pt = (pt * 2.0).round() / 2.0;
            if next.auto_convert {
                next.fs_px = pt2px(next.term_pt);
            }
        }
        if fs_steps != 0 {
            next.fs_px = (self.fs_px + i64::from(fs_steps)).clamp(FS_PX_MIN, FS_PX_MAX);
            // 手动拧 FinalShell 时就不该再被换算覆盖回去。
            if fs_steps != 0 && next.auto_convert && term_steps == 0 {
                next.auto_convert = false;
            }
        }
        next
    }

    pub fn summary(&self) -> String {
        format!(
            "{} pt / {} px · 换算 {}",
            fmt_num(self.term_pt),
            self.fs_px,
            if self.auto_convert { "开" } else { "关" }
        )
    }
}

pub fn pt2px(pt: f64) -> i64 {
    (pt * PX_PER_PT).round() as i64
}

pub fn px2pt(px: i64) -> f64 {
    px as f64 / PX_PER_PT
}

/// `12` / `12.5`，去掉多余的小数位。
pub fn fmt_num(n: f64) -> String {
    if (n - n.round()).abs() < 0.01 {
        format!("{}", n.round() as i64)
    } else {
        format!("{n:.1}")
    }
}

pub fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("GNOUS_TERMFONT_CONFIG") {
        return PathBuf::from(path);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("gnous/termfont.json")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/root"), PathBuf::from)
}

/// 读目标值。文件不存在是正常情况（返回默认值），格式坏了才报。
pub fn load() -> (Targets, Option<String>) {
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (Targets::default(), None);
    };
    match serde_json::from_str::<Targets>(&text) {
        Ok(targets) => match targets.validate() {
            Ok(()) => (targets, None),
            Err(why) => (
                Targets::default(),
                Some(format!("{} 不合法：{why}（按默认值显示）", path.display())),
            ),
        },
        Err(error) => (
            Targets::default(),
            Some(format!(
                "{} 解析失败：{error}（按默认值显示）",
                path.display()
            )),
        ),
    }
}

/// 原子写：临时文件 + rename，避免半个 JSON。
pub fn save(targets: &Targets) -> Result<PathBuf, String> {
    targets.validate()?;
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 {} 失败：{e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(targets).map_err(|e| format!("序列化失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, format!("{text}\n"))
        .map_err(|e| format!("写 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 {} 失败：{e}", path.display()))?;
    Ok(path)
}

pub fn dry_run() -> bool {
    matches!(
        std::env::var("GNOUS_TERMFONT_DRY_RUN").as_deref(),
        Ok("1") | Ok("true")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn defaults_match_the_machine_right_now() {
        let targets = Targets::default();
        assert_eq!(targets.term_pt, 12.0);
        assert_eq!(targets.fs_px, 16);
        assert!(targets.validate().is_ok());
        assert_eq!(targets.convert(), 16, "12 pt 正好换算成 16 px");
    }

    #[test]
    fn conversion_is_four_thirds() {
        assert_eq!(pt2px(12.0), 16);
        assert_eq!(pt2px(13.0), 17, "17.33 四舍五入到 17");
        assert_eq!(pt2px(11.0), 15, "14.67 四舍五入到 15");
        assert!((px2pt(16) - 12.0).abs() < 1e-9);
        assert_eq!(fmt_num(12.0), "12");
        assert_eq!(fmt_num(12.5), "12.5");
        assert_eq!(fmt_num(11.25), "11.2");
    }

    #[test]
    fn rejects_out_of_range() {
        let mut targets = Targets::default();
        targets.term_pt = 5.5;
        assert!(targets.validate().is_err());
        targets.term_pt = 48.5;
        assert!(targets.validate().is_err());
        targets.term_pt = f64::NAN;
        assert!(targets.validate().is_err());
        targets.term_pt = 12.0;
        targets.fs_px = 5;
        assert!(targets.validate().is_err());
        targets.fs_px = 201;
        assert!(targets.validate().is_err());
    }

    #[test]
    fn stepping_term_keeps_the_conversion_in_sync() {
        let targets = Targets::default();
        let up = targets.stepped(1, 0);
        assert_eq!(up.term_pt, 12.5);
        assert_eq!(up.fs_px, 17, "12.5 pt ≈ 16.67 px → 17");
        assert!(up.auto_convert, "换算开关不该被 pt 步进改掉");
        assert_eq!(up.stepped(1, 0).term_pt, 13.0);
        let down = targets.stepped(-1, 0);
        assert_eq!(down.term_pt, 11.5);
        assert_eq!(down.fs_px, 15, "11.5 pt ≈ 15.33 px → 15");
    }

    #[test]
    fn stepping_finalshell_turns_the_conversion_off() {
        let targets = Targets::default();
        let next = targets.stepped(0, 1);
        assert_eq!(next.fs_px, 17);
        assert!(!next.auto_convert, "手动调 px 之后不该再被换算覆盖");
    }

    #[test]
    fn stepping_clamps_at_the_edges() {
        let low = Targets {
            term_pt: TERM_PT_MIN,
            fs_px: FS_PX_MIN,
            auto_convert: false,
        };
        assert_eq!(low.stepped(-1, 0).term_pt, TERM_PT_MIN);
        assert_eq!(low.stepped(0, -1).fs_px, FS_PX_MIN);
        let high = Targets {
            term_pt: TERM_PT_MAX,
            fs_px: FS_PX_MAX,
            auto_convert: false,
        };
        assert_eq!(high.stepped(1, 0).term_pt, TERM_PT_MAX);
        assert_eq!(high.stepped(0, 1).fs_px, FS_PX_MAX);
    }

    #[test]
    fn roundtrips_through_disk() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir =
            std::env::temp_dir().join(format!("gnous-termfont-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("termfont.json");
        unsafe { std::env::set_var("GNOUS_TERMFONT_CONFIG", &path) };
        let targets = Targets {
            term_pt: 13.5,
            fs_px: 18,
            auto_convert: false,
        };
        let written = save(&targets).expect("应该能写");
        assert_eq!(written, path);
        let (loaded, warning) = load();
        assert!(warning.is_none(), "{warning:?}");
        assert_eq!(loaded, targets);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("autoConvert"),
            "字段名是 camelCase"
        );
        unsafe { std::env::remove_var("GNOUS_TERMFONT_CONFIG") };
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn broken_json_falls_back_with_a_reason() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("gnous-termfont-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("termfont.json");
        std::fs::write(&path, "{ not json").unwrap();
        unsafe { std::env::set_var("GNOUS_TERMFONT_CONFIG", &path) };
        let (targets, warning) = load();
        assert_eq!(targets, Targets::default());
        assert!(warning.unwrap_or_default().contains("解析失败"));
        unsafe { std::env::remove_var("GNOUS_TERMFONT_CONFIG") };
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
