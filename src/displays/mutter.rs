//! mutter 的 `org.gnome.Mutter.DisplayConfig` 类型化封装。
//!
//! 只包两件事：读当前状态（`GetCurrentState`）和申请新布局（`ApplyMonitorsConfig`）。
//! 显示的真相在 mutter 里，`~/.config/monitors.xml` 只是它的持久化影子，所以这里不读 xml。

use super::dbus::{Conn, Result, V};

pub const IFACE: &str = "org.gnome.Mutter.DisplayConfig";
pub const OBJ: &str = "/org/gnome/Mutter/DisplayConfig";

/// `ApplyMonitorsConfig` 的 method 参数。
pub const METHOD_VERIFY: u32 = 1;
pub const METHOD_TEMPORARY: u32 = 2;
pub const METHOD_PERSISTENT: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct Mode {
    pub id: String,
    pub width: i32,
    pub height: i32,
    pub rate: f64,
    pub preferred: bool,
    pub current: bool,
}

impl Mode {
    pub fn area(&self) -> i64 {
        self.width as i64 * self.height as i64
    }

    /// 界面上那一行：`2560x1440@60.000`。
    pub fn label(&self) -> String {
        format!("{}x{}@{:.3}", self.width, self.height, self.rate)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    pub connector: String,
    pub vendor: String,
    pub product: String,
    pub serial: String,
    pub modes: Vec<Mode>,
}

impl Monitor {
    /// `preferred` 优先，其次当前模式，最后分辨率最高的那个。
    pub fn best_mode(&self) -> Option<&Mode> {
        self.modes
            .iter()
            .find(|m| m.preferred)
            .or_else(|| self.modes.iter().find(|m| m.current))
            .or_else(|| {
                self.modes
                    .iter()
                    .max_by_key(|m| (m.area(), (m.rate * 1000.0) as i64))
            })
    }

    pub fn label(&self) -> String {
        format!("{} {} {}", self.connector, self.vendor, self.product)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Logical {
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub primary: bool,
    pub connectors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub serial: u32,
    pub monitors: Vec<Monitor>,
    pub logical: Vec<Logical>,
    pub layout_mode: Option<u32>,
}

impl State {
    pub fn monitor(&self, connector: &str) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.connector == connector)
    }

    /// 当前真正点亮的接口（按布局里的顺序）。
    pub fn active(&self) -> Vec<String> {
        let mut out = Vec::new();
        for logical in &self.logical {
            for c in &logical.connectors {
                if !out.contains(c) {
                    out.push(c.clone());
                }
            }
        }
        out
    }

    pub fn active_sorted(&self) -> Vec<String> {
        let mut out = self.active();
        out.sort();
        out
    }

    pub fn describe(&self) -> String {
        let active = self.active();
        let mut lines = Vec::new();
        for m in &self.monitors {
            let on = active.contains(&m.connector);
            let current = m
                .modes
                .iter()
                .find(|mode| mode.current)
                .map(Mode::label)
                .unwrap_or_else(|| "-".into());
            lines.push(format!(
                "  [{}] {:<8} {:<5} {:<14} 当前={current}",
                if on { "ON " } else { "OFF" },
                m.connector,
                m.vendor,
                m.product
            ));
        }
        let layout = self
            .logical
            .iter()
            .map(|l| {
                format!(
                    "{}@({},{}) 缩放 {} · {}",
                    l.connectors.join("+"),
                    l.x,
                    l.y,
                    l.scale,
                    if l.primary { "主屏" } else { "扩展" }
                )
            })
            .collect::<Vec<_>>()
            .join("  ");
        format!(
            "显示器:\n{}\n布局: {}",
            lines.join("\n"),
            if layout.is_empty() { "(无)" } else { &layout }
        )
    }
}

fn s(v: &V) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn i32_of(v: &V) -> i32 {
    match v {
        V::I32(n) => *n,
        V::U32(n) => *n as i32,
        _ => 0,
    }
}

fn f64_of(v: &V) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

pub fn parse_state(body: &V) -> Result<State> {
    let parts = body.items();
    if parts.len() < 3 {
        return Err(super::dbus::Error::Protocol(format!(
            "GetCurrentState 应答字段不足：{}",
            parts.len()
        )));
    }
    let serial = parts[0].as_u32().unwrap_or(0);
    let mut monitors = Vec::new();
    for entry in parts[1].items() {
        let fields = entry.items();
        if fields.len() < 2 {
            continue;
        }
        let spec = fields[0].items();
        let mut modes = Vec::new();
        for m in fields[1].items() {
            let mf = m.items();
            if mf.len() < 4 {
                continue;
            }
            let props = mf.get(6);
            modes.push(Mode {
                id: s(&mf[0]),
                width: i32_of(&mf[1]),
                height: i32_of(&mf[2]),
                rate: f64_of(&mf[3]),
                preferred: props
                    .and_then(|p| p.get("is-preferred"))
                    .and_then(V::as_bool)
                    .unwrap_or(false),
                current: props
                    .and_then(|p| p.get("is-current"))
                    .and_then(V::as_bool)
                    .unwrap_or(false),
            });
        }
        monitors.push(Monitor {
            connector: spec.first().map(s).unwrap_or_default(),
            vendor: spec.get(1).map(s).unwrap_or_default(),
            product: spec.get(2).map(s).unwrap_or_default(),
            serial: spec.get(3).map(s).unwrap_or_default(),
            modes,
        });
    }

    let mut logical = Vec::new();
    for entry in parts[2].items() {
        let f = entry.items();
        if f.len() < 6 {
            continue;
        }
        logical.push(Logical {
            x: i32_of(&f[0]),
            y: i32_of(&f[1]),
            scale: f64_of(&f[2]),
            primary: f[3].as_bool().unwrap_or(false),
            connectors: f[5].items().iter().map(|m| s(&m.items()[0])).collect(),
        });
    }

    Ok(State {
        serial,
        monitors,
        logical,
        layout_mode: parts
            .get(3)
            .and_then(|p| p.get("layout-mode"))
            .and_then(V::as_u32),
    })
}

pub fn get_state(conn: &mut Conn) -> Result<State> {
    let body = conn.call(
        "org.gnome.Mutter.DisplayConfig",
        OBJ,
        IFACE,
        "GetCurrentState",
        "",
        &V::Struct(Vec::new()),
    )?;
    parse_state(&body)
}

/// 申请目标接口组成的布局：第一个为主屏，其余按顺序横排。
/// 返回 `(logical_monitors 值, 目标接口列表)`。
pub fn plan(state: &State, targets: &[&str]) -> std::result::Result<(V, Vec<String>), String> {
    plan_with(state, targets, |monitor| monitor.best_mode().cloned())
}

/// 同上，但由调用方决定每个接口用哪个模式（多屏幕页面按配置的
/// `preferred` / `max` / `current` 策略挑）。
pub fn plan_with<F>(
    state: &State,
    targets: &[&str],
    choose: F,
) -> std::result::Result<(V, Vec<String>), String>
where
    F: Fn(&Monitor) -> Option<Mode>,
{
    // 每块屏各占一个 logical monitor，横向依次排开：
    // 把两块不同分辨率的屏塞进同一个 logical monitor 是镜像语义，mut­ter 会
    // 直接以 "Monitors modes in logical monitor not equal" 拒绝（真机验证过）。
    let mut logical = Vec::new();
    let mut chosen = Vec::new();
    let mut x = 0i32;
    for conn in targets {
        let Some(monitor) = state.monitor(conn) else {
            return Err(format!("mutter 不认识接口 {conn}"));
        };
        let Some(mode) = choose(monitor) else {
            return Err(format!("{conn} 没有可用模式"));
        };
        let width = (mode.width as f64 / 1.0).round() as i32;
        logical.push(V::Struct(vec![
            V::I32(x),
            V::I32(0),
            V::F64(1.0),
            V::U32(0),
            V::Bool(chosen.is_empty()),
            V::Arr(vec![V::Struct(vec![
                V::Str(monitor.connector.clone()),
                V::Str(mode.id.clone()),
                V::Arr(Vec::new()),
            ])]),
        ]));
        chosen.push(monitor.connector.clone());
        x += width;
    }
    if logical.is_empty() {
        return Err("没有可点亮的接口".to_string());
    }
    Ok((V::Arr(logical), chosen))
}

/// 申请布局。`method` 用 `METHOD_VERIFY` 时只校验不生效。
pub fn apply(conn: &mut Conn, state: &State, logical: V, method: u32) -> Result<()> {
    conn.call(
        "org.gnome.Mutter.DisplayConfig",
        OBJ,
        IFACE,
        "ApplyMonitorsConfig",
        "uua(iiduba(ssa{sv}))a{sv}",
        &V::Struct(vec![
            V::U32(state.serial),
            V::U32(method),
            logical,
            V::Arr(vec![V::Struct(vec![
                V::Str("layout-mode".into()),
                V::Var("u".into(), Box::new(V::U32(1))),
            ])]),
        ]),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> V {
        let mode = |id: &str, w: i32, h: i32, rate: f64, props: Vec<(&str, bool)>| {
            V::Struct(vec![
                V::Str(id.into()),
                V::I32(w),
                V::I32(h),
                V::F64(rate),
                V::F64(1.0),
                V::Arr(vec![]),
                V::Arr(
                    props
                        .into_iter()
                        .map(|(k, v)| {
                            V::Struct(vec![
                                V::Str(k.into()),
                                V::Var("b".into(), Box::new(V::Bool(v))),
                            ])
                        })
                        .collect(),
                ),
            ])
        };
        let monitor = |conn: &str, modes: Vec<V>| {
            V::Struct(vec![
                V::Struct(vec![
                    V::Str(conn.into()),
                    V::Str("VND".into()),
                    V::Str("PROD".into()),
                    V::Str("0".into()),
                ]),
                V::Arr(modes),
                V::Arr(vec![]),
            ])
        };
        let logical = |conn: &str| {
            V::Struct(vec![
                V::I32(0),
                V::I32(0),
                V::F64(1.0),
                V::U32(0),
                V::Bool(true),
                V::Arr(vec![V::Struct(vec![
                    V::Str(conn.into()),
                    V::Str("VND".into()),
                    V::Str("PROD".into()),
                    V::Str("0".into()),
                ])]),
                V::Arr(vec![]),
            ])
        };
        V::Struct(vec![
            V::U32(15),
            V::Arr(vec![
                monitor(
                    "HDMI-1",
                    vec![
                        mode(
                            "2560x1440@60.000",
                            2560,
                            1440,
                            60.0,
                            vec![("is-preferred", true)],
                        ),
                        mode(
                            "2048x1152@59.909",
                            2048,
                            1152,
                            59.909,
                            vec![("is-current", true)],
                        ),
                    ],
                ),
                monitor(
                    "eDP-1",
                    vec![mode(
                        "1920x1080@60.000",
                        1920,
                        1080,
                        60.0,
                        vec![("is-preferred", true)],
                    )],
                ),
            ]),
            V::Arr(vec![logical("HDMI-1")]),
            V::Arr(vec![V::Struct(vec![
                V::Str("layout-mode".into()),
                V::Var("u".into(), Box::new(V::U32(1))),
            ])]),
        ])
    }

    #[test]
    fn parses_get_current_state() {
        let state = parse_state(&sample_state()).expect("应该能解析");
        assert_eq!(state.serial, 15);
        assert_eq!(state.monitors.len(), 2);
        assert_eq!(state.active(), vec!["HDMI-1"]);
        assert_eq!(state.layout_mode, Some(1));
        let hdmi = state.monitor("HDMI-1").expect("HDMI-1");
        assert_eq!(
            hdmi.best_mode().map(|m| m.id.clone()),
            Some("2560x1440@60.000".into())
        );
        let edp = state.monitor("eDP-1").expect("eDP-1");
        assert_eq!(edp.modes.len(), 1);
        assert!(state.describe().contains("[ON ] HDMI-1"));
    }

    #[test]
    fn plans_the_requested_connectors() {
        let state = parse_state(&sample_state()).unwrap();
        let (logical, chosen) = plan(&state, &["eDP-1", "HDMI-1"]).expect("两屏都该能规划");
        assert_eq!(chosen, vec!["eDP-1".to_string(), "HDMI-1".to_string()]);
        // 每块屏一个 logical monitor，横向排开，只有第一个是主屏。
        let logicals = logical.items();
        assert_eq!(logicals.len(), 2);
        assert_eq!(logicals[0].items()[0].as_u32(), None);
        assert_eq!(logicals[0].items()[0].items().len(), 0);
        let first = logicals[0].items();
        assert_eq!(first[5].items()[0].items()[0].as_str(), Some("eDP-1"));
        assert_eq!(first[4].as_bool(), Some(true));
        let second = logicals[1].items();
        assert_eq!(second[5].items()[0].items()[0].as_str(), Some("HDMI-1"));
        assert_eq!(second[4].as_bool(), Some(false));
        // 1920x1080 在左，所以第二块屏的 x 是 1920。
        assert!(
            matches!(second[0], V::I32(1920)),
            "第二块屏应在 x=1920，实际 {:?}",
            second[0]
        );
    }

    #[test]
    fn refuses_unknown_connectors() {
        let state = parse_state(&sample_state()).unwrap();
        let err = plan(&state, &["DP-9"]).expect_err("不存在的接口必须报错");
        assert!(err.contains("DP-9"), "实际: {err}");
    }

    /// 真机集成测试：把当前布局原样交给 mutter 校验一遍（METHOD_VERIFY，不生效）。
    /// 这条断言能证明 ApplyMonitorsConfig 的编码是对的——包括嵌套数组与变体。
    #[test]
    fn verify_accepts_the_current_layout_when_a_session_bus_exists() {
        let Ok(mut conn) = Conn::session() else {
            return;
        };
        let state = get_state(&mut conn).expect("有会话总线时 GetCurrentState 必须成功");
        let active = state.active();
        if active.is_empty() {
            return; // 没有点亮的屏就没法校验
        }
        let refs: Vec<&str> = active.iter().map(String::as_str).collect();
        let (logical, chosen) = plan(&state, &refs).expect("当前布局必须能重新规划");
        assert_eq!(chosen, active, "重新规划出来的接口顺序应与现状一致");
        apply(&mut conn, &state, logical, METHOD_VERIFY)
            .expect("mutter 应该接受它自己当前布局的 verify 请求");
    }

    /// 真机集成测试：两块屏都在时，"扩展"布局也必须能通过 verify。
    /// 这条断言守着一个真机踩过的坑：把不同模式的两块屏塞进同一个
    /// logical monitor 会被 mutter 以 InvalidArgs 拒绝。
    #[test]
    fn verify_accepts_an_extended_layout_when_two_monitors_are_connected() {
        let Ok(mut conn) = Conn::session() else {
            return;
        };
        let state = get_state(&mut conn).expect("有会话总线时 GetCurrentState 必须成功");
        let usable: Vec<String> = state
            .monitors
            .iter()
            .filter(|m| m.best_mode().is_some())
            .map(|m| m.connector.clone())
            .collect();
        if usable.len() < 2 {
            return; // 只有一块屏时没有"扩展"可测
        }
        let refs: Vec<&str> = usable.iter().map(String::as_str).collect();
        let (logical, _) = plan(&state, &refs).expect("两块屏应该能规划扩展布局");
        apply(&mut conn, &state, logical, METHOD_VERIFY).expect("扩展布局的 verify 必须通过");
    }

    /// 真机集成测试：有会话总线就比对一下 mutter 的真实应答。
    #[test]
    fn reads_live_state_when_a_session_bus_exists() {
        let Ok(mut conn) = Conn::session() else {
            return; // 会话里没有总线：跳过
        };
        // 有总线就必须真的读通，读不通是 bug，不能静默跳过。
        let state = get_state(&mut conn).expect("有会话总线时 GetCurrentState 必须成功");
        assert!(!state.monitors.is_empty(), "真机上至少该看到一个接口");
        for logical in &state.logical {
            for connector in &logical.connectors {
                assert!(
                    state.monitor(connector).is_some(),
                    "布局里的 {connector} 应该在 monitors 里出现"
                );
            }
        }
        for monitor in &state.monitors {
            assert!(!monitor.connector.is_empty());
        }
    }
}
