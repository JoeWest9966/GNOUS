//! 执行外部命令的唯一入口：只允许 `gsettings`，且只碰三个 schema 里被点名的键。
//!
//! 这个扩展的活儿就是改字号，所以 `set` / `reset` 必须在允许清单里——但按键
//! 级别收窄：`org.gnome.desktop.interface` 只能读 `monospace-font-name`，
//! 终端 profile 只能改 `font` 和 `use-system-font`。除此之外的任何 schema、
//! 任何键、任何多余参数一律拒绝，这样"改字号"这个需求永远不可能变成"顺手改了
//! 别的设置"。

use std::time::Duration;

use crate::fixes_backend::process;

const TIMEOUT: Duration = Duration::from_secs(6);

pub const INTERFACE: &str = "org.gnome.desktop.interface";
pub const PROFILES: &str = "org.gnome.Terminal.ProfilesList";
pub const PROFILE: &str = "org.gnome.Terminal.Legacy.Profile";

/// 允许的 `(schema 前缀, 键, 允许的动词)`。
const ALLOWED: &[(&str, &str, &[&str])] = &[
    // 系统等宽字体：只读，用它当"跟随系统"时的参照。
    (INTERFACE, "monospace-font-name", &["get"]),
    // 终端默认 profile 的 uuid：只读。
    (PROFILES, "default", &["get"]),
    // 终端 profile 自己：这是唯一会被写的地方。
    (PROFILE, "font", &["get", "set", "reset"]),
    (PROFILE, "use-system-font", &["get", "set", "reset"]),
];

fn check_schema(schema: &str) -> Result<(), String> {
    if schema == INTERFACE || schema == PROFILES {
        return Ok(());
    }
    let Some(path) = schema.strip_prefix(&format!("{PROFILE}:")) else {
        return Err(format!("schema 不在允许清单里：{schema}"));
    };
    let path = path.trim_end_matches('/');
    let Some(uuid) = path.strip_prefix("/org/gnome/terminal/legacy/profiles:/:") else {
        return Err(format!("profile 路径不合法：{path}"));
    };
    let shaped = uuid.len() == 36
        && uuid.chars().enumerate().all(|(index, ch)| match index {
            8 | 13 | 18 | 23 => ch == '-',
            _ => ch.is_ascii_hexdigit(),
        });
    if !shaped {
        return Err(format!("profile uuid 不合法：{uuid}"));
    }
    Ok(())
}

fn check_value(key: &str, value: &str) -> Result<(), String> {
    match key {
        "use-system-font" => {
            if value == "true" || value == "false" {
                Ok(())
            } else {
                Err(format!("use-system-font 只接受 true/false，收到 {value}"))
            }
        }
        "font" => {
            // gsettings 的字体值是 GVariant 字符串字面量：`'Monospace 12'`。
            let inner = value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
                .ok_or_else(|| format!("字体值必须写成 '字体族 字号'，收到 {value}"))?;
            if inner.is_empty()
                || inner.len() > 120
                || inner.contains('\'')
                || inner.contains('\\')
                || inner.chars().any(|ch| ch.is_control())
            {
                return Err(format!("字体值里有不该出现的字符：{value}"));
            }
            Ok(())
        }
        other => Err(format!("键 {other} 不接受写入")),
    }
}

/// 允许清单检查。`argv` 不含程序名，形如 `["get", schema, key]`，
/// 写值的时候是 `["set", schema, key, value]`。
pub fn check(argv: &[&str]) -> Result<(), String> {
    let (verb, schema, key, value) = match argv {
        [verb, schema, key] => (*verb, *schema, *key, None),
        [verb, schema, key, value] => (*verb, *schema, *key, Some(*value)),
        other => return Err(format!("参数形状不对，拒绝执行：{other:?}")),
    };
    check_schema(schema)?;
    let Some((_, _, verbs)) = ALLOWED.iter().find(|(allowed_schema, allowed_key, _)| {
        (schema == *allowed_schema || schema.starts_with(&format!("{allowed_schema}:")))
            && key == *allowed_key
    }) else {
        return Err(format!("键不在允许清单里：{key}"));
    };
    if !verbs.contains(&verb) {
        return Err(format!("{verb} 不允许用在 {key} 上"));
    }
    match (verb, value) {
        ("set", Some(value)) => check_value(key, value),
        ("set", None) => Err(format!("set {key} 缺少取值")),
        (_, Some(_)) => Err(format!("{verb} {key} 不接受取值")),
        (_, None) => Ok(()),
    }
}

fn run(argv: &[&str]) -> Result<String, String> {
    check(argv)?;
    process::run_bounded("gsettings", argv, TIMEOUT)
}

pub fn get(schema: &str, key: &str) -> Result<String, String> {
    run(&["get", schema, key]).map(|out| unquote(out.trim()))
}

pub fn set(schema: &str, key: &str, value: &str) -> Result<(), String> {
    // `run` 里的 `check` 已经校验过 verb / schema / key / 取值，这里不再重复。
    run(&["set", schema, key, value]).map(|_| ())
}

pub fn reset(schema: &str, key: &str) -> Result<(), String> {
    run(&["reset", schema, key]).map(|_| ())
}

/// gsettings 打印的字符串值带单引号，去掉它们再交给上层。
pub fn unquote(text: &str) -> String {
    text.trim().trim_matches('\'').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE_URI: &str = "org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:b1dcc9dd-5262-4d8d-a863-c897e6d979b9/";

    #[test]
    fn allows_the_four_keys_we_actually_need() {
        assert!(check(&["get", INTERFACE, "monospace-font-name"]).is_ok());
        assert!(check(&["get", PROFILES, "default"]).is_ok());
        assert!(check(&["get", PROFILE_URI, "font"]).is_ok());
        assert!(check(&["set", PROFILE_URI, "font", "'Monospace 12'"]).is_ok());
        assert!(check(&["reset", PROFILE_URI, "font"]).is_ok());
        assert!(check(&["set", PROFILE_URI, "use-system-font", "true"]).is_ok());
        assert!(
            check(&["set", PROFILE_URI, "font"]).is_err(),
            "set 不给值等于半条命令，必须拒绝"
        );
    }

    #[test]
    fn refuses_writes_to_the_read_only_keys() {
        // 系统等宽字体是"参照物"，绝不能被这个扩展改掉。
        assert!(check(&["set", INTERFACE, "monospace-font-name"]).is_err());
        assert!(check(&["reset", INTERFACE, "monospace-font-name"]).is_err());
        assert!(check(&["set", PROFILES, "default"]).is_err());
    }

    #[test]
    fn refuses_unrelated_schemas_and_keys() {
        for argv in [
            ["set", "org.gnome.desktop.background", "picture-uri"],
            ["set", "org.gnome.desktop.interface", "font-name"],
            ["set", "org.gnome.desktop.interface", "clock-format"],
            ["set", "org.gnome.Terminal.Legacy.Settings", "theme-variant"],
            ["get", PROFILE_URI, "palette"],
            ["set", PROFILE_URI, "background-color"],
        ] {
            assert!(check(&argv).is_err(), "{argv:?} 必须被拒绝");
        }
    }

    #[test]
    fn refuses_a_profile_path_that_is_not_a_uuid() {
        let bad =
            "org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:../../etc/";
        assert!(check(&["set", bad, "font"]).is_err());
        let bad = "org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:short/";
        assert!(check(&["set", bad, "font"]).is_err());
        // 另一个 schema 冒用 profile 前缀也不行。
        let bad =
            "org.gnome.Terminal.Legacy.ProfileX/schemas/org.gnome.Terminal.Legacy.Profile:/a/";
        assert!(check(&["set", bad, "font"]).is_err());
    }

    /// 回归：`set` 带取值是四个参数，曾经被"参数形状不对"一刀切拒掉，
    /// 结果是 UI 上点了 TERM 只回一句拒绝、字号没变。
    #[test]
    fn accepts_a_four_argument_set_with_a_valid_value() {
        assert!(check(&["set", PROFILE_URI, "use-system-font", "false"]).is_ok());
        assert!(check(&["set", PROFILE_URI, "font", "'Monospace 12.5'"]).is_ok());
        // 取值仍然要过校验。
        assert!(check(&["set", PROFILE_URI, "use-system-font", "yes"]).is_err());
        assert!(check(&["set", PROFILE_URI, "font", "Monospace 12"]).is_err());
    }

    #[test]
    fn refuses_extra_arguments() {
        assert!(check(&["set", PROFILE_URI, "font", "--verbose"]).is_err());
        assert!(check(&["get", PROFILE_URI, "font", "extra"]).is_err());
        assert!(check(&["get", INTERFACE, "monospace-font-name", "'x'"]).is_err());
        assert!(check(&["reset", PROFILE_URI, "font", "x"]).is_err());
        assert!(check(&[]).is_err());
    }

    #[test]
    fn validates_the_values_it_writes() {
        assert!(check(&["set", PROFILE_URI, "font", "'Monospace 12'"]).is_ok());
        assert!(check(&["set", PROFILE_URI, "font", "'Noto Sans Mono 13.5'"]).is_ok());
        assert!(check(&["set", PROFILE_URI, "font", "Monospace 12"]).is_err());
        assert!(check(&["set", PROFILE_URI, "font", "''"]).is_err());
        assert!(check(&["set", PROFILE_URI, "font", "'a\\'b'"]).is_err());
        assert!(check(&["set", PROFILE_URI, "use-system-font", "true"]).is_ok());
        assert!(check(&["set", PROFILE_URI, "use-system-font"],).is_err());
        assert!(check(&["set", PROFILE_URI, "use-system-font", "TRUE"]).is_err());
        assert!(check(&["get", PROFILE_URI, "font", "x"]).is_err());
    }

    #[test]
    fn unquotes_gsettings_output() {
        assert_eq!(unquote("'Monospace 12'\n"), "Monospace 12");
        assert_eq!(unquote("true\n"), "true");
        assert_eq!(unquote("''"), "");
    }
}
