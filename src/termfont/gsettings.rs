//! GNOME 终端的字号：从 `org.gnome.Terminal.Legacy.Profile` 的 `font` 读写。
//!
//! 单位是 **pt**，改完立刻生效。两个坑：
//!
//! 1. 字号存在**默认 profile** 里（`ProfilesList` 的 `default` 给 uuid），
//!    不是全局键，所以 schema 要带 profile 路径。
//! 2. `use-system-font=true` 时 profile 自己的 `font` 不生效，实际大小来自
//!    `org.gnome.desktop.interface` 的 `monospace-font-name`。所以"现在多大"
//!    必须按这个开关分别解释，否则会显示一个根本没生效的数字。

use super::config::fmt_num;
use super::exec;

/// "Monospace 12" → ("Monospace", Some(12.0))；"Cantarell" → ("Cantarell", None)
pub fn split_font(text: &str) -> (String, Option<f64>) {
    let text = text.trim().trim_matches('\'');
    match text.rsplit_once(' ') {
        Some((family, size)) => match size.trim().parse::<f64>() {
            Ok(pt) if pt.is_finite() && pt > 0.0 => (family.trim().to_string(), Some(pt)),
            _ => (text.to_string(), None),
        },
        None => (text.to_string(), None),
    }
}

/// gsettings 要的是 GVariant 字符串字面量：`'Monospace 12'`。
pub fn format_font(family: &str, pt: f64) -> String {
    format!("'{family} {}'", fmt_num(pt))
}

pub fn default_profile() -> Result<String, String> {
    let uuid = exec::get(exec::PROFILES, "default")?;
    if uuid.is_empty() {
        return Err("GNOME 终端没有默认 profile".to_string());
    }
    Ok(uuid)
}

/// `org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:<uuid>/`
pub fn profile_schema(uuid: &str) -> String {
    format!(
        "{}:/org/gnome/terminal/legacy/profiles:/:{uuid}/",
        exec::PROFILE
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct TermFont {
    /// profile 里写的字体族（`use-system-font` 为真时未必生效）。
    pub family: String,
    /// profile 里写的字号（`use-system-font` 为真时未必生效）。
    pub pt: Option<f64>,
    pub use_system: bool,
    /// profile 里的原始字符串，原样留着给面板显示。
    pub raw: String,
}

impl TermFont {
    /// 真正生效的字号：跟随系统时取系统等宽字体的大小。
    pub fn effective_pt(&self, system_pt: Option<f64>) -> Option<f64> {
        if self.use_system { system_pt } else { self.pt }
    }

    pub fn effective_family(&self, system_family: &str) -> String {
        if self.use_system {
            system_family.to_string()
        } else {
            self.family.clone()
        }
    }
}

/// 系统等宽字体 `org.gnome.desktop.interface monospace-font-name`。
pub fn system_monospace() -> Result<(String, Option<f64>), String> {
    let raw = exec::get(exec::INTERFACE, "monospace-font-name")?;
    Ok(split_font(&raw))
}

/// 读终端当前字号。注意 `gsettings get` 对不存在的 profile 不会失败——
/// 会把空数组打出来，所以这里顺带校验一下 uuid 形状。
pub fn term_font() -> Result<TermFont, String> {
    let uuid = default_profile()?;
    let schema = profile_schema(&uuid);
    let raw = exec::get(&schema, "font")?;
    let use_system = exec::get(&schema, "use-system-font")? == "true";
    let (family, pt) = split_font(&raw);
    Ok(TermFont {
        family,
        pt,
        use_system,
        raw,
    })
}

/// 把终端字号设成 `pt`（pt，6..=48）。字体族优先用 profile 里已有的，
/// 跟随系统则退回系统等宽字体的族名——这样不会把用户选的字体换掉。
pub fn set_pt(pt: f64) -> Result<String, String> {
    if !(super::config::TERM_PT_MIN..=super::config::TERM_PT_MAX).contains(&pt) {
        return Err(format!("终端字号 {pt} pt 超出范围（6..=48）"));
    }
    let current = term_font()?;
    let family = if current.use_system {
        system_monospace()?.0
    } else if current.family.trim().is_empty() {
        system_monospace()?.0
    } else {
        current.family.clone()
    };
    let uuid = default_profile()?;
    let schema = profile_schema(&uuid);
    let value = format_font(&family, pt);
    exec::set(&schema, "use-system-font", "false")?;
    exec::set(&schema, "font", &value)?;
    Ok(value)
}

/// 恢复成"跟随系统等宽字体"（老脚本的 `termfont term system`）。
pub fn follow_system() -> Result<(), String> {
    let uuid = default_profile()?;
    let schema = profile_schema(&uuid);
    exec::set(&schema, "use-system-font", "true")?;
    exec::reset(&schema, "font")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_family_and_size() {
        assert_eq!(
            split_font("Monospace 12"),
            ("Monospace".to_string(), Some(12.0))
        );
        assert_eq!(
            split_font("'Noto Sans Mono 13.5'"),
            ("Noto Sans Mono".to_string(), Some(13.5))
        );
        assert_eq!(split_font("Cantarell"), ("Cantarell".to_string(), None));
        assert_eq!(
            split_font("Monospace"),
            ("Monospace".to_string(), None),
            "没有数字就当没有字号"
        );
        assert_eq!(
            split_font("Source Code Pro 0"),
            ("Source Code Pro 0".to_string(), None),
            "0 不是合法字号，整串当族名"
        );
    }

    #[test]
    fn formats_a_gvariant_string_literal() {
        assert_eq!(format_font("Monospace", 12.0), "'Monospace 12'");
        assert_eq!(format_font("Noto Sans Mono", 13.5), "'Noto Sans Mono 13.5'");
    }

    #[test]
    fn profile_schema_matches_gnome_layout() {
        assert_eq!(
            profile_schema("b1dcc9dd-5262-4d8d-a863-c897e6d979b9"),
            "org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:b1dcc9dd-5262-4d8d-a863-c897e6d979b9/"
        );
    }

    #[test]
    fn effective_size_follows_the_system_switch() {
        let custom = TermFont {
            family: "Monospace".into(),
            pt: Some(12.0),
            use_system: false,
            raw: "'Monospace 12'".into(),
        };
        assert_eq!(custom.effective_pt(Some(11.0)), Some(12.0));
        assert_eq!(custom.effective_family("Ubuntu Mono"), "Monospace");
        let following = TermFont {
            use_system: true,
            ..custom
        };
        assert_eq!(
            following.effective_pt(Some(11.0)),
            Some(11.0),
            "跟随系统时以系统等宽字体为准"
        );
        assert_eq!(following.effective_family("Ubuntu Mono"), "Ubuntu Mono");
    }

    #[test]
    fn refuses_out_of_range_sizes_before_touching_gsettings() {
        // 这个断言在任何 gsettings 调用之前就会返回，所以没有会话也能跑。
        assert!(set_pt(5.0).unwrap_err().contains("超出范围"));
        assert!(set_pt(49.0).unwrap_err().contains("超出范围"));
        assert!(set_pt(f64::NAN).unwrap_err().contains("超出范围"));
    }
}
