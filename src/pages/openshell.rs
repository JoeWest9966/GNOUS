//! Renderer for the `openshell` page-level extension.
//!
//! Eight views over 45 blocks. Every number, name and line of output comes
//! from [`crate::openshell`], which shells out to the real `openshell` CLI on a
//! background thread. When the gateway has nothing to report the block says so;
//! it never fills the gap with sample data.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Div, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled, WeakEntity, div, px,
};

use crate::openshell::{self, Action, State};
use crate::theme::{
    accent, accent_hover, alpha, black, caps, dim, h, line, line_faint, line_strong, muted, ok,
    text, txt, v, warn,
};
use crate::{App, Overview};

const MONO: &str = "Noto Sans Mono CJK SC";
/// Height of a verbatim output well inside a block.
const WELL: f32 = 150.;
/// Read-only probes offered by the exec console. Nothing here mutates the
/// sandbox.
const EXEC_PRESETS: &[&str] = &["uname -a", "id", "ls -la /", "df -h", "env | sort"];
/// Local ports offered by the forward block.
const FORWARD_PORTS: &[u16] = &[3000, 8080, 8000, 8888];
/// Endpoints offered by the policy dry-run block: `(endpoint, binary)`.
const POLICY_ENDPOINTS: &[(&str, &str)] = &[
    ("api.github.com:443:read-only:rest:enforce", "/usr/bin/gh"),
    ("pypi.org:443:read-only:rest:enforce", "/usr/local/bin/pip"),
    (
        "registry.npmjs.org:443:read-only:rest:enforce",
        "/usr/bin/node",
    ),
];
/// CLI aliases, from `openshell --help`.
const ALIASES: &[&str] = &["sb", "svc", "fwd", "pol", "lg", "gw", "dr"];

// ============================================================================
// Entry point
// ============================================================================

pub fn body(view: &str, overview: &Overview, weak: &WeakEntity<App>) -> Div {
    let state = overview.openshell;
    let (title, kicker, blocks) = match view {
        "sandboxes" => ("沙箱舰队", "SANDBOXES", 7),
        "inside" => ("沙箱内部", "INSIDE", 5),
        "network" => ("网络暴露", "NETWORK", 4),
        "policy" => ("策略与出站控制", "POLICY", 5),
        "credentials" => ("凭据与提供方", "CREDENTIALS", 7),
        "observe" => ("日志与设置", "OBSERVE", 5),
        "tooling" => ("工具与批量", "TOOLING", 6),
        _ => ("网关与身份", "GATEWAY", 6),
    };
    let grid = match view {
        "sandboxes" => sandboxes_view(state, weak),
        "inside" => inside_view(state, weak),
        "network" => network_view(state, weak),
        "policy" => policy_view(state, weak),
        "credentials" => credentials_view(state, weak),
        "observe" => observe_view(state, weak),
        "tooling" => tooling_view(state, weak),
        _ => gateway_view(state, weak),
    };
    v().w_full()
        .child(hero(title, kicker, blocks, state, weak))
        .child(if matches!(view, "gateway") {
            metrics(state)
        } else {
            strip(state)
        })
        .child(grid)
        .child(footer(state))
}

// ============================================================================
// Chrome
// ============================================================================

fn hero(title: &str, kicker: &str, blocks: usize, state: &State, weak: &WeakEntity<App>) -> Div {
    let target = state.active_sandbox();
    h().w_full()
        .items_end()
        .justify_between()
        .gap(px(24.))
        .mb(px(28.))
        .child(
            v().flex_1()
                .min_w_0()
                .child(caps(
                    &format!("自研扩展 · openshell · {kicker} · {blocks} 区块"),
                    11.,
                    FontWeight::NORMAL,
                    dim(),
                    0.26,
                ))
                .child(txt(title, 42., FontWeight::EXTRA_LIGHT, text()).line_height(px(46.)).mt(px(14.)))
                .child(
                    txt(
                        "全部区块直接读取 openshell CLI 的 stdout：网关、沙箱、策略、凭据、日志。失败原样显示，不伪造绿灯。",
                        13.,
                        FontWeight::NORMAL,
                        muted(),
                    )
                    .mt(px(12.)),
                ),
        )
        .child(
            h().items_center()
                .gap(px(18.))
                .child(meta(
                    "TARGET",
                    &target.clone().unwrap_or_else(|| "—".into()),
                    if target.is_some() { text() } else { dim() },
                ))
                .child(meta(
                    "PROBED",
                    if state.checked {
                        &state.snap.probed_at
                    } else {
                        "未读取"
                    },
                    if state.checked { muted() } else { dim() },
                ))
                .child(button(
                    Action::Refresh,
                    if state.busy { "读取中…" } else { "REFRESH" }.into(),
                    Btn::Solid,
                    state.busy,
                    false,
                    weak,
                )),
        )
}

fn meta(label: &str, value: &str, color: Hsla) -> Div {
    v().items_end()
        .gap(px(4.))
        .child(caps(label, 9., FontWeight::NORMAL, dim(), 0.22))
        .child(mono(value.to_string(), 11.5, color))
}

fn metrics(state: &State) -> Div {
    let health = state.snap.health();
    let items = [
        (
            "网关",
            if health.connected {
                "CONNECTED".to_string()
            } else {
                "UNREACHABLE".to_string()
            },
            String::new(),
            if health.connected { ok() } else { accent() },
        ),
        ("沙箱", health.sandboxes.to_string(), "个".into(), text()),
        (
            "运行中",
            health.running.to_string(),
            format!("/ {}", health.sandboxes),
            ok(),
        ),
        (
            "需关注",
            health.attention.to_string(),
            "个".into(),
            if health.attention > 0 {
                warn()
            } else {
                muted()
            },
        ),
        (
            "命令失败",
            if state.checked {
                health.failures.to_string()
            } else {
                "—".to_string()
            },
            if state.checked {
                "条".into()
            } else {
                String::new()
            },
            if state.checked && health.failures > 0 {
                accent()
            } else {
                muted()
            },
        ),
    ];
    let mut row = h()
        .w_full()
        .items_stretch()
        .border_t(px(1.))
        .border_b(px(1.))
        .border_color(line())
        .mb(px(28.));
    for (index, (label, value, unit, color)) in items.into_iter().enumerate() {
        row = row.child(
            v().flex_1()
                .min_w_0()
                .py(px(20.))
                .when(index > 0, |this| {
                    this.pl(px(24.)).border_l(px(1.)).border_color(line())
                })
                .child(caps(label, 10., FontWeight::NORMAL, dim(), 0.22))
                .child(
                    h().items_end()
                        .gap(px(7.))
                        .mt(px(12.))
                        .child(
                            txt(
                                value,
                                if index == 0 { 24. } else { 34. },
                                FontWeight::EXTRA_LIGHT,
                                color,
                            )
                            .line_height(px(38.)),
                        )
                        .when(!unit.is_empty(), |this| {
                            this.child(txt(unit, 12., FontWeight::LIGHT, muted()))
                        }),
                ),
        );
    }
    row
}

/// One-line state strip under the hero for the non-Gateway views.
fn strip(state: &State) -> Div {
    let health = state.snap.health();
    let parts: [(String, Hsla); 6] = [
        (
            if health.connected {
                "gateway connected".to_string()
            } else {
                "gateway unreachable".to_string()
            },
            if health.connected { ok() } else { accent() },
        ),
        (
            format!("sandboxes {}", health.sandboxes),
            if health.sandboxes > 0 { text() } else { dim() },
        ),
        (
            format!("running {}", health.running),
            if health.running > 0 { ok() } else { dim() },
        ),
        (
            format!("providers {}", health.providers),
            if health.providers > 0 { text() } else { dim() },
        ),
        (
            format!("forwards {}", health.forwards),
            if health.forwards > 0 { text() } else { dim() },
        ),
        (
            format!("exposed {}", health.services),
            if health.services > 0 { text() } else { dim() },
        ),
    ];
    let mut row = h()
        .w_full()
        .items_center()
        .gap(px(22.))
        .pb(px(14.))
        .mb(px(18.))
        .border_b(px(1.))
        .border_color(line());
    for (label, color) in parts {
        row = row.child(
            h().items_center()
                .gap(px(6.))
                .child(div().size(px(4.)).rounded_full().bg(color))
                .child(mono(label, 10.5, color)),
        );
    }
    row
}

fn footer(state: &State) -> Div {
    let health = state.snap.health();
    h().w_full()
        .items_center()
        .justify_between()
        .gap(px(24.))
        .mt(px(28.))
        .pt(px(16.))
        .border_t(px(1.))
        .border_color(line())
        .child(txt(
            if state.checked {
                format!(
                    "刷新于 {} · 失败 {} 条 · 动作记录 {} 条",
                    state.snap.probed_at,
                    health.failures,
                    state.log.len()
                )
            } else {
                "尚未探测：进入分区或点 REFRESH 才会读网关".to_string()
            },
            11.,
            FontWeight::NORMAL,
            dim(),
        ))
        .child(txt(
            if state.busy {
                "执行中：等待 CLI 返回"
            } else {
                "危险动作二次确认 · 交互式命令请在终端执行"
            },
            11.,
            FontWeight::NORMAL,
            if state.busy { warn() } else { dim() },
        ))
}

// ============================================================================
// Blocks
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Risk {
    Read,
    Write,
    Danger,
}

impl Risk {
    fn label(self) -> &'static str {
        match self {
            Risk::Read => "READ",
            Risk::Write => "WRITE",
            Risk::Danger => "DANGER",
        }
    }
    fn color(self) -> Hsla {
        match self {
            Risk::Read => alpha(0x37d67ac0),
            Risk::Write => alpha(0xf5a623cc),
            Risk::Danger => alpha(0xe31937d0),
        }
    }
}

fn block(span: f32, title: &str, en: &str, cmd: &str, risk: Risk, body: impl IntoElement) -> Div {
    v().flex_grow(span)
        .flex_basis(px(0.))
        .min_w_0()
        .border(px(1.))
        .border_color(line())
        .bg(black())
        .child(
            h().items_start()
                .justify_between()
                .gap(px(10.))
                .p(px(12.))
                .border_b(px(1.))
                .border_color(line())
                .child(
                    v().min_w_0()
                        .child(txt(title, 12., FontWeight::NORMAL, text()))
                        .child(caps(en, 8., FontWeight::NORMAL, dim(), 0.18).mt(px(3.))),
                )
                .child(pill(risk.label(), risk.color())),
        )
        .child(
            div()
                .px(px(12.))
                .py(px(6.))
                .border_b(px(1.))
                .border_color(line_faint())
                .child(mono(format!("› {cmd}"), 9.5, dim())),
        )
        .child(v().p(px(12.)).gap(px(10.)).child(body))
}

fn rows(cards: Vec<Div>) -> Div {
    h().w_full().items_start().gap(px(10.)).children(cards)
}

fn pill(label: &str, color: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .border(px(1.))
        .border_color(line())
        .px(px(7.))
        .py(px(2.))
        .child(caps(label, 8.5, FontWeight::NORMAL, color, 0.12))
}

fn mono(raw: impl Into<String>, size: f32, color: Hsla) -> Div {
    div()
        .font_family(MONO)
        .text_size(px(size))
        .line_height(px(size * 1.72))
        .text_color(color)
        .child(raw.into())
}

/// `label   value` rows, the panel's densest layout.
fn kv(items: Vec<(&str, String, Hsla)>) -> Div {
    let mut list = v().gap(px(5.));
    for (label, value, color) in items {
        list = list.child(
            h().items_start()
                .gap(px(10.))
                .child(
                    div()
                        .w(px(96.))
                        .flex_shrink_0()
                        .overflow_hidden()
                        .child(caps(label, 9., FontWeight::NORMAL, dim(), 0.08)),
                )
                .child(div().flex_1().min_w_0().child(mono(value, 10.5, color))),
        );
    }
    list
}

/// Label above value, for keys too long for [`kv`]'s fixed label column.
fn kv_stack(items: Vec<(&str, String, Hsla)>) -> Div {
    let mut list = v().gap(px(9.));
    for (label, value, color) in items {
        list = list.child(
            v().gap(px(2.))
                .child(caps(label, 9., FontWeight::NORMAL, dim(), 0.08))
                .child(mono(value, 10.5, color)),
        );
    }
    list
}

/// Verbatim stdout, one child per line, clipped to `limit` lines.
fn verbatim(id: &'static str, raw: &str, limit: usize, fallback: &str) -> Stateful<Div> {
    let trimmed = raw.trim_end();
    if trimmed.is_empty() {
        return v()
            .id(format!("os-well-{id}"))
            .p(px(9.))
            .border(px(1.))
            .border_color(line_faint())
            .child(note(fallback));
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    let mut well = v().gap(px(0.));
    for line in lines.iter().take(limit) {
        well = well.child(mono((*line).to_string(), 10., line_color(line)));
    }
    if lines.len() > limit {
        well = well.child(note(&format!(
            "… 另有 {} 行，终端执行可看全",
            lines.len() - limit
        )));
    }
    v().id(format!("os-well-{id}"))
        .max_h(px(WELL))
        .overflow_hidden()
        .overflow_scroll()
        .bg(alpha(0x050506ff))
        .border(px(1.))
        .border_color(line_faint())
        .p(px(9.))
        .child(well)
}

fn line_color(line: &str) -> Hsla {
    let lower = line.to_lowercase();
    if lower.contains("error") || lower.contains("deny") || lower.contains("fail") {
        accent()
    } else if lower.contains("warn") || lower.contains("pending") {
        warn()
    } else if lower.contains("[info") || lower.contains("info]") || lower.contains("ok") {
        dim()
    } else {
        muted()
    }
}

fn note(raw: &str) -> Div {
    div()
        .text_size(px(10.5))
        .line_height(px(18.))
        .text_color(dim())
        .child(raw.to_string())
}

fn warn_note(raw: &str) -> Div {
    div()
        .text_size(px(10.5))
        .line_height(px(18.))
        .text_color(warn())
        .child(raw.to_string())
}

/// A tiny table: first column strong, the rest muted.
fn table(headers: &[&str], rows: Vec<Vec<(String, Hsla)>>) -> Div {
    let mut head = h()
        .gap(px(10.))
        .pb(px(5.))
        .border_b(px(1.))
        .border_color(line());
    for header in headers {
        head = head.child(div().flex_1().min_w_0().child(caps(
            header,
            8.,
            FontWeight::NORMAL,
            dim(),
            0.16,
        )));
    }
    let mut body = v();
    if rows.is_empty() {
        body = body.child(note("网关未返回条目"));
    }
    let total = rows.len();
    for (index, cells) in rows.into_iter().enumerate() {
        let mut row = h().gap(px(10.)).py(px(5.));
        if index + 1 < total {
            row = row.border_b(px(1.)).border_color(alpha(0xffffff06));
        }
        for (cell_index, (value, color)) in cells.into_iter().enumerate() {
            row = row.child(div().flex_1().min_w_0().child(mono(
                value,
                10.,
                if cell_index == 0 { color } else { muted() },
            )));
        }
        body = body.child(row);
    }
    v().child(head).child(body)
}

fn chips(labels: &[&str]) -> Div {
    let mut row = h().gap(px(5.)).overflow_hidden();
    for label in labels {
        row = row.child(
            div()
                .border(px(1.))
                .border_color(line())
                .px(px(7.))
                .py(px(3.))
                .child(mono((*label).to_string(), 9.5, muted())),
        );
    }
    row
}

fn big(value: &str, unit: &str, color: Hsla) -> Div {
    h().items_end()
        .gap(px(8.))
        .child(txt(value, 28., FontWeight::EXTRA_LIGHT, color).line_height(px(32.)))
        .when(!unit.is_empty(), |this| {
            this.child(txt(unit, 10., FontWeight::NORMAL, dim()))
        })
}

fn command_box(command: &str) -> Div {
    let mut body = v();
    for line in command.lines() {
        body = body.child(mono(line.to_string(), 9.5, line_strong()));
    }
    v().overflow_hidden()
        .bg(alpha(0x050506ff))
        .border(px(1.))
        .border_color(line_faint())
        .p(px(9.))
        .child(body)
}

// ============================================================================
// Buttons
// ============================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Btn {
    Solid,
    Ghost,
    Danger,
}

fn button(
    action: Action,
    label: String,
    kind: Btn,
    disabled: bool,
    armed: bool,
    weak: &WeakEntity<App>,
) -> Stateful<Div> {
    let (border, bg, fg) = match (kind, armed) {
        (Btn::Solid, _) => (accent(), accent(), text()),
        (Btn::Danger, _) => (accent(), black(), accent()),
        (Btn::Ghost, true) => (accent(), accent(), text()),
        (Btn::Ghost, false) => (line_strong(), black(), muted()),
    };
    let id = format!("openshell-{}", action.command());
    let weak = weak.clone();
    div()
        .id(id)
        .cursor_pointer()
        .flex_shrink_0()
        .px(px(13.))
        .py(px(8.))
        .border(px(1.))
        .border_color(border)
        .bg(bg)
        .when(disabled, |this| this.opacity(0.4))
        .hover(|this| match kind {
            Btn::Solid => this.bg(accent_hover()),
            _ => this.bg(alpha(0xffffff0a)),
        })
        .child(caps(&label, 9., FontWeight::MEDIUM, fg, 0.12))
        .on_click(move |_, _, cx| {
            if disabled {
                return;
            }
            let action = action.clone();
            let _ = weak.update(cx, |app, cx| app.run_openshell(action, cx));
        })
}

fn confirm(state: &State, action: &Action) -> (bool, String) {
    let armed = state.armed.as_ref() == Some(action);
    let label = if armed {
        format!("确认 {}", action.label())
    } else {
        action.label().to_string()
    };
    (armed, label)
}

// ============================================================================
// Gateway
// ============================================================================

fn gateway_view(state: &State, _weak: &WeakEntity<App>) -> Div {
    let snap = &state.snap;
    let summary = openshell::status_summary(&snap.status);
    let connected = summary
        .iter()
        .find(|(key, _)| key == "status")
        .is_some_and(|(_, value)| value == "connected");

    let status = block(
        4.,
        "网关状态卡",
        "Gateway Status",
        "openshell status -o json",
        Risk::Read,
        v().child(big(
            if connected {
                "CONNECTED"
            } else {
                "UNREACHABLE"
            },
            "",
            if connected { ok() } else { accent() },
        ))
        .child(kv(summary
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone(), muted()))
            .collect()))
        .when(!snap.status.ok, |this| {
            this.child(warn_note(&snap.status.text))
        }),
    );

    let gateways = snap.gateways();
    let switch_rows = gateways
        .iter()
        .map(|gateway| {
            vec![
                (
                    format!(
                        "{}{}",
                        gateway.name,
                        if gateway.active { " · active" } else { "" }
                    ),
                    if gateway.active { text() } else { muted() },
                ),
                (gateway.endpoint.clone(), muted()),
                (
                    format!(
                        "{} / {} / {}{}",
                        gateway.kind,
                        gateway.auth,
                        gateway.source,
                        if gateway.is_remote { " / remote" } else { "" }
                    ),
                    dim(),
                ),
            ]
        })
        .collect();
    let switch = block(
        4.,
        "网关切换器",
        "Gateway Switcher",
        "openshell gateway list",
        Risk::Read,
        v().child(table(&["NAME", "ENDPOINT", "TYPE / AUTH"], switch_rows))
            .child(note(
                "切换 active 网关会改变之后所有命令的目标，面板不提供一键切换；在终端执行 `openshell gateway select <name>`。",
            )),
    );

    let auth_rows = openshell::whoami_summary(&snap.whoami);
    let auth = block(
        4.,
        "认证与身份",
        "Auth / Whoami",
        "openshell whoami -o json",
        Risk::Read,
        v().child(kv(auth_rows
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone(), muted()))
            .collect()))
            .child(note(
                "login / logout 是交互式流程（OIDC 设备码），面板只读展示当前身份。",
            )),
    );

    let runtime = block(
        5.,
        "网关运行时",
        "Runtime Info",
        "openshell gateway info",
        Risk::Read,
        verbatim("runtime", &snap.gateway_info.text, 22, "（无运行时信息）"),
    );

    let doctor_ok = snap.doctor.ok;
    let doctor = block(
        4.,
        "环境体检",
        "Doctor Check",
        "openshell doctor check",
        Risk::Read,
        v().child(big(
            if doctor_ok { "OK" } else { "BLOCKED" },
            "",
            if doctor_ok { ok() } else { accent() },
        ))
        .child(verbatim("doctor", &snap.doctor.text, 14, "（无输出）"))
        .child(note(
            "doctor 检查本地网关前置条件（Docker / 驱动）；失败不代表网关本身不可用。",
        )),
    );

    v().w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(rows(vec![status, switch, auth]))
        .child(rows(vec![runtime, doctor]))
}

// ============================================================================
// Sandboxes
// ============================================================================

fn sandboxes_view(state: &State, weak: &WeakEntity<App>) -> Div {
    let snap = &state.snap;
    let sandboxes = snap.sandboxes();

    let mut table_rows = Vec::new();
    for sbx in &sandboxes {
        table_rows.push(vec![
            (
                sbx.name.clone(),
                if state.target.as_deref() == Some(sbx.name.as_str()) {
                    text()
                } else {
                    muted()
                },
            ),
            (
                sbx.phase.clone(),
                if matches!(sbx.phase.as_str(), "Ready" | "Running") {
                    ok()
                } else {
                    warn()
                },
            ),
            (sbx.workspace.clone(), muted()),
            (sbx.created_at.clone(), dim()),
            (format!("v{}", sbx.policy_version), dim()),
        ]);
    }
    let list = block(
        8.,
        "沙箱总览",
        "Sandbox Table",
        "openshell sandbox list -o json",
        Risk::Read,
        v().child(table(
            &["NAME", "PHASE", "WORKSPACE", "CREATED", "POLICY"],
            table_rows,
        ))
        .child(if sandboxes.len() > 1 {
            select_row(&sandboxes, weak)
        } else {
            v()
        })
        .child(if sandboxes.is_empty() {
            note("网关还没有沙箱。用右侧「创建向导」建第一个；面板不会伪造沙箱来填表。")
        } else {
            note("点击名称切换 TARGET：右侧详情、policy、logs、ssh 都跟着换。")
        }),
    );

    let create_name = format!("os-{}", snap.probed_at.replace(':', ""));
    let create_action = Action::Create {
        name: create_name.clone(),
    };
    let (create_armed, create_label) = confirm(state, &create_action);
    let create = block(
        4.,
        "创建向导",
        "Create Sandbox",
        "openshell sandbox create --detach",
        Risk::Write,
        v().child(kv(vec![
            ("NAME", create_name, muted()),
            (
                "IMAGE",
                "网关默认（vm driver · ubuntu 24.04）".into(),
                muted(),
            ),
            ("CMD", "sleep infinity".into(), muted()),
            ("CPU / MEM", "未指定，走网关默认".into(), dim()),
        ]))
        .child(note(
            "表单化的 CPU / 内存 / 模板 / provider 需要网关侧模板源；首版只提交最小参数，命令原样可见。",
        ))
        .child(button(
            create_action,
            create_label,
            Btn::Ghost,
            state.busy,
            create_armed,
            weak,
        )),
    );

    let target = state.active_sandbox();
    let mut power_actions: Vec<Stateful<Div>> = Vec::new();
    if let Some(name) = target.clone() {
        for action in [
            Action::Start(name.clone()),
            Action::Stop(name.clone()),
            Action::Suspend(name.clone()),
            Action::Resume(name.clone()),
            Action::Delete(name.clone()),
        ] {
            let kind = if action.is_destructive() {
                Btn::Danger
            } else if matches!(action, Action::Resume(_)) {
                Btn::Solid
            } else {
                Btn::Ghost
            };
            let (armed, label) = confirm(state, &action);
            power_actions.push(button(action, label, kind, state.busy, armed, weak));
        }
    }
    let power = block(
        4.,
        "电源控制",
        "Power Controls",
        "sandbox start / stop / suspend / resume",
        Risk::Write,
        v().child(big(
            &target.clone().unwrap_or_else(|| "无沙箱".into()),
            "",
            if target.is_some() { text() } else { dim() },
        ))
        .child(kv(vec![
            (
                "phase",
                target
                    .as_deref()
                    .and_then(|name| state.sandbox(name))
                    .map_or_else(|| "—".into(), |sbx| sbx.phase),
                muted(),
            ),
            ("stop", "保留工作区".into(), dim()),
            ("suspend", "保留内存态".into(), dim()),
        ]))
        .child(h().gap(px(6.)).children(power_actions)),
    );

    let detail = target
        .as_deref()
        .and_then(|_| openshell::sandbox_one(&snap.sandbox_get));
    let meters = block(
        4.,
        "单沙箱详情",
        "Sandbox Detail",
        "openshell sandbox get -o json",
        Risk::Read,
        v().child(kv(vec![
            (
                "WORKSPACE",
                detail
                    .as_ref()
                    .map_or_else(|| "—".into(), |sbx| sbx.workspace.clone()),
                muted(),
            ),
            (
                "POLICY",
                detail
                    .as_ref()
                    .map_or_else(|| "—".into(), |sbx| format!("v{}", sbx.policy_version)),
                muted(),
            ),
            (
                "ID",
                detail
                    .as_ref()
                    .map_or_else(|| "—".into(), |sbx| sbx.id.chars().take(8).collect()),
                dim(),
            ),
            ("ISOLATION", isolation(&detail), dim()),
            ("CONDITIONS", conditions(&detail), dim()),
        ]))
        .child(note(
            "网关不上报 CPU / 内存占用：这里只展示沙箱上报的配置与条件，不画资源仪表。",
        )),
    );

    let templates = snap.templates();
    let template_rows = templates
        .iter()
        .map(|item| {
            vec![
                (item.get("name").to_string(), text()),
                (item.get("image").to_string(), muted()),
                (item.get("cpu").to_string(), dim()),
            ]
        })
        .collect();
    let template = block(
        6.,
        "模板库",
        "Template Library",
        "openshell sandbox template list -o json",
        Risk::Read,
        v().child(table(&["TEMPLATE", "IMAGE", "CPU"], template_rows))
            .child(note(
                "模板由网关侧创建（`sandbox template create`），面板只读列出。",
            )),
    );

    let workspaces: Vec<String> = sandboxes
        .iter()
        .map(|sbx| sbx.workspace.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let workspace = block(
        3.,
        "工作区范围",
        "Workspace Scope",
        "--workspace / --all-workspaces",
        Risk::Read,
        v().child(if workspaces.is_empty() {
            note("尚无沙箱，工作区列表来自 sandbox list。")
        } else {
            kv(workspaces
                .iter()
                .map(|name| ("workspace", name.clone(), muted()))
                .collect())
        })
        .child(note(
            "面板不传 -g / --workspace，跟随 CLI 默认作用域；跨工作区统计需在终端加 --all-workspaces。",
        )),
    );

    let stopped = sandboxes
        .iter()
        .filter(|sbx| !matches!(sbx.phase.as_str(), "Ready" | "Running"))
        .count();
    let reclaim = block(
        3.,
        "回收站",
        "Reclaim",
        "openshell sandbox delete <name>",
        Risk::Danger,
        v().child(big(
            &stopped.to_string(),
            "可回收",
            if stopped > 0 { warn() } else { dim() },
        ))
        .child(kv(vec![
            (
                "target",
                target.clone().unwrap_or_else(|| "—".into()),
                muted(),
            ),
            ("effect", "删除不可逆".into(), accent()),
        ]))
        .child(note("DELETE 需要二次确认；批量删除不进面板。")),
    );

    v().w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(rows(vec![list, create]))
        .child(rows(vec![power, meters]))
        .child(rows(vec![template, workspace, reclaim]))
}

fn select_row(sandboxes: &[openshell::Sandbox], weak: &WeakEntity<App>) -> Div {
    let mut row = h().gap(px(6.));
    for sbx in sandboxes.iter().take(8) {
        let name = sbx.name.clone();
        let weak = weak.clone();
        row = row.child(
            div()
                .id(format!("os-select-{}", sbx.name))
                .cursor_pointer()
                .border(px(1.))
                .border_color(line())
                .px(px(7.))
                .py(px(3.))
                .hover(|this| this.bg(alpha(0xffffff0a)))
                .child(mono(sbx.name.clone(), 9.5, muted()))
                .on_click(move |_, _, cx| {
                    let name = name.clone();
                    let _ = weak.update(cx, |app, cx| app.select_openshell_target(name, cx));
                }),
        );
    }
    row
}

fn isolation(detail: &Option<openshell::Sandbox>) -> String {
    detail
        .as_ref()
        .and_then(|sbx| {
            sbx.annotations
                .iter()
                .find(|(key, _)| key.ends_with("isolation-backend"))
                .map(|(_, value)| value.clone())
        })
        .unwrap_or_else(|| "—".into())
}

fn conditions(detail: &Option<openshell::Sandbox>) -> String {
    detail.as_ref().map_or_else(
        || "—".into(),
        |sbx| {
            if sbx.conditions.is_empty() {
                "全部 Ready".into()
            } else {
                sbx.conditions
                    .iter()
                    .map(|(key, value)| format!("{key}: {value}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            }
        },
    )
}

// ============================================================================
// Inside
// ============================================================================

fn inside_view(state: &State, weak: &WeakEntity<App>) -> Div {
    let target = state.active_sandbox();
    let mut presets: Vec<Stateful<Div>> = Vec::new();
    if let Some(sandbox) = target.clone() {
        for preset in EXEC_PRESETS {
            let action = Action::Exec {
                sandbox: sandbox.clone(),
                command: (*preset).to_string(),
            };
            let (armed, _) = confirm(state, &action);
            presets.push(button(
                action,
                if armed {
                    "确认 RUN".to_string()
                } else {
                    (*preset).to_string()
                },
                Btn::Ghost,
                state.busy,
                armed,
                weak,
            ));
        }
    }
    let console = block(
        6.,
        "命令执行台",
        "Exec Console",
        "openshell sandbox exec <name> -- sh -c",
        Risk::Write,
        v().child(h().gap(px(6.)).children(presets))
            .child(if target.is_some() {
                note("每条预设都是一次真实 exec，输出原样回传；都是只读探测。")
            } else {
                warn_note("没有沙箱：先在 Sandboxes 视图创建或选中一个。")
            })
            .child(match &state.exec_output {
                Some(outcome) => verbatim(
                    "exec",
                    &format!("$ {}\n{}", outcome.command, outcome.output),
                    26,
                    "（无输出）",
                ),
                None => verbatim("exec-empty", "", 0, "尚未执行命令"),
            }),
    );

    let transfer = block(
        4.,
        "文件推拉",
        "Transfer",
        "sandbox upload / download",
        Risk::Write,
        v().child(command_box(
            "openshell sandbox upload <name> ./src /workspace/src",
        ))
        .child(command_box(
            "openshell sandbox download <name> /workspace/out.csv ./",
        ))
        .child(note(
            "git 工作树内默认按 .gitignore 过滤；面板暂不选本地路径，避免误传大目录。",
        )),
    );

    let ssh = block(
        4.,
        "SSH 接入片段",
        "SSH Config",
        "openshell sandbox ssh-config",
        Risk::Read,
        verbatim(
            "ssh",
            &state.snap.ssh_config.text,
            16,
            "（无 TARGET 或网关未返回）",
        ),
    );

    let editor = block(
        3.,
        "编辑器直通",
        "Editor Bridge",
        "sandbox connect --editor vscode",
        Risk::Write,
        v().child(chips(&["vscode", "cursor"]))
            .child(command_box(
                "openshell sandbox connect <name> --editor vscode",
            ))
            .child(note("会安装 OpenShell 托管的 SSH 配置并拉起远端编辑器。")),
    );

    let connect = block(
        5.,
        "终端接入",
        "Connect",
        "openshell sandbox connect <name>",
        Risk::Write,
        v().child(command_box("openshell sandbox connect <name>"))
            .child(kv(vec![
                (
                    "target",
                    target.clone().unwrap_or_else(|| "—".into()),
                    muted(),
                ),
                ("detach", "Ctrl-D 断开，进程保留".into(), dim()),
            ]))
            .child(note(
                "交互式 shell 需要 PTY，面板内不代跑；这里给出可直接粘贴的命令。",
            )),
    );

    v().w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(rows(vec![console, transfer]))
        .child(rows(vec![ssh, editor, connect]))
}

// ============================================================================
// Network
// ============================================================================

fn network_view(state: &State, weak: &WeakEntity<App>) -> Div {
    let snap = &state.snap;
    let target = state.active_sandbox();
    let forwards = snap.forwards();
    let forward_rows = forwards
        .iter()
        .map(|item| {
            vec![
                (item.get("local").to_string(), text()),
                (item.get("sandbox").to_string(), muted()),
                (item.get("state").to_string(), dim()),
            ]
        })
        .collect();

    let mut forward_actions: Vec<Stateful<Div>> = Vec::new();
    if let Some(sandbox) = target.clone() {
        for port in FORWARD_PORTS.iter().take(2) {
            let action = Action::ForwardStart {
                port: *port,
                sandbox: sandbox.clone(),
            };
            let (armed, label) = confirm(state, &action);
            forward_actions.push(button(
                action,
                format!(":{} {label}", port),
                Btn::Ghost,
                state.busy,
                armed,
                weak,
            ));
        }
    }
    for port in FORWARD_PORTS.iter().take(2) {
        let action = Action::ForwardStop { port: *port };
        let (armed, label) = confirm(state, &action);
        forward_actions.push(button(
            action,
            format!(":{} {label}", port),
            Btn::Danger,
            state.busy,
            armed,
            weak,
        ));
    }
    let forward = block(
        4.,
        "端口转发",
        "Forwards",
        "openshell forward start / stop / list",
        Risk::Write,
        v().child(table(&["LOCAL", "SANDBOX", "STATE"], forward_rows))
            .child(h().gap(px(6.)).children(forward_actions))
            .child(if target.is_none() {
                warn_note("没有沙箱：start 需要目标沙箱。")
            } else {
                note("start 会常驻后台；stop 按本地端口停。")
            }),
    );

    let tunnel = block(
        4.,
        "服务隧道",
        "Service Tunnel",
        "openshell forward service",
        Risk::Write,
        v().child(command_box(
            "openshell forward service <name> --target-port 8000 --local 8000",
        ))
        .child(kv(vec![
            ("target host", "127.0.0.1（仅 loopback）".into(), muted()),
            ("local", "省略则与目标端口相同".into(), dim()),
        ]))
        .child(note(
            "经 gRPC 打洞到沙箱内 loopback 服务，适合长期挂着的开发服务。",
        )),
    );

    let services = snap.services();
    let service_rows = services
        .iter()
        .map(|item| {
            vec![
                (item.get("name").to_string(), text()),
                (item.get("sandbox").to_string(), muted()),
                (item.get("target_port").to_string(), dim()),
            ]
        })
        .collect();
    let expose_action = target.clone().map(|sandbox| Action::Expose {
        sandbox,
        port: 3000,
        service: "web".into(),
    });
    let expose = block(
        6.,
        "服务暴露",
        "Expose Service",
        "openshell service expose <name> 3000 web",
        Risk::Write,
        v().child(table(&["SERVICE", "SANDBOX", "PORT"], service_rows))
            .child(h().gap(px(6.)).children(expose_action.map(|action| {
                let (armed, label) = confirm(state, &action);
                button(action, label, Btn::Ghost, state.busy, armed, weak)
            })))
            .child(note(
                "把沙箱内 HTTP 服务挂成网关端点；默认 strip Authorization 头。",
            )),
    );

    let endpoints = block(
        4.,
        "端点清单",
        "Endpoint Inventory",
        "openshell service list / get",
        Risk::Read,
        verbatim(
            "endpoints",
            &snap.service_list.text,
            14,
            "（网关未返回已暴露服务）",
        ),
    );

    v().w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(rows(vec![forward, tunnel]))
        .child(rows(vec![expose, endpoints]))
}

// ============================================================================
// Policy
// ============================================================================

fn policy_view(state: &State, weak: &WeakEntity<App>) -> Div {
    let snap = &state.snap;
    let summary = openshell::status_summary(&snap.policy);
    let policy_rows: Vec<(String, String)> =
        serde_json::from_str::<serde_json::Value>(snap.policy.text.trim())
            .map(|value| {
                value
                    .as_object()
                    .map(|map| {
                        map.iter()
                            .filter(|(key, _)| *key != "sandbox")
                            .map(|(key, item)| (key.clone(), openshell::json_scalar(item)))
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_else(|_| summary.clone());

    let viewer = block(
        6.,
        "策略总览",
        "Policy Viewer",
        "openshell policy get <name> -o json",
        Risk::Read,
        v().child(kv(policy_rows
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone(), muted()))
            .collect()))
        .child(note(
            "CLI 只输出策略摘要（版本 / 哈希 / 生效状态）；端点与 L7 规则的逐条明细不在 `-o json` 里，面板不臆造。",
        )),
    );

    let mut dry_run_buttons: Vec<Stateful<Div>> = Vec::new();
    if let Some(sandbox) = state.active_sandbox() {
        for (endpoint, binary) in POLICY_ENDPOINTS {
            let action = Action::PolicyDryRun {
                sandbox: sandbox.clone(),
                endpoint: (*endpoint).to_string(),
                binary: (*binary).to_string(),
            };
            let (armed, label) = confirm(state, &action);
            dry_run_buttons.push(button(
                action,
                format!("{label} · {}", endpoint.split(':').next().unwrap_or("")),
                Btn::Ghost,
                state.busy,
                armed,
                weak,
            ));
        }
    }
    let edit = block(
        6.,
        "增量编辑",
        "Policy Update",
        "openshell policy update --add-endpoint --dry-run",
        Risk::Write,
        v().child(chips(
            &POLICY_ENDPOINTS
                .iter()
                .map(|(endpoint, _)| endpoint.split(':').next().unwrap_or(endpoint))
                .collect::<Vec<_>>(),
        ))
        .child(h().items_start().gap(px(6.)).children(dry_run_buttons))
        .child(note(
            "只做 --dry-run：合并结果先算出来看，不发网关。真正 apply 需要在终端确认。",
        )),
    );

    let last_dry_run = state
        .log
        .iter()
        .rev()
        .find(|outcome| outcome.command.contains("--dry-run"));
    let dry_run = block(
        6.,
        "试算差异",
        "Dry Run Diff",
        "openshell policy update --dry-run",
        Risk::Read,
        match last_dry_run {
            Some(outcome) => verbatim("dry-run", &outcome.output, 18, "（无输出）"),
            None => verbatim(
                "dry-run-empty",
                "",
                0,
                "尚未试算：左侧任一端点按钮会执行一次真实的 --dry-run",
            ),
        },
    );

    let revisions = snap.policy_revisions();
    let history_rows = revisions
        .iter()
        .map(|item| {
            vec![
                (item.get("version").to_string(), text()),
                (item.get("status").to_string(), muted()),
                (item.get("created_at_ms").to_string(), dim()),
            ]
        })
        .collect();
    let history = block(
        4.,
        "策略历史",
        "Policy History",
        "openshell policy list <name>",
        Risk::Read,
        table(&["REV", "STATUS", "CREATED MS"], history_rows),
    );

    let global_rows: Vec<(String, String)> = snap
        .settings_rows()
        .into_iter()
        .filter(|(key, _)| key.contains("policy") || key.contains("proposal"))
        .collect();
    let global = block(
        6.,
        "全局策略旋钮",
        "Global Knobs",
        "openshell settings get --global",
        Risk::Write,
        v().child(kv_stack(global_rows
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone(), muted()))
            .collect()))
            .child(note(
                "全局锁与提案模式通过 `policy set --global` / `settings set` 修改；面板只读，避免一键改掉所有沙箱的出站策略。",
            )),
    );

    v().w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(rows(vec![viewer, edit]))
        .child(rows(vec![dry_run, history]))
        .child(rows(vec![global]))
}

// ============================================================================
// Credentials
// ============================================================================

fn credentials_view(state: &State, weak: &WeakEntity<App>) -> Div {
    let snap = &state.snap;
    let providers = snap.providers();
    let provider_rows = providers
        .iter()
        .map(|item| {
            vec![
                (item.get("name").to_string(), text()),
                (item.get("type").to_string(), muted()),
                (item.get("credential_source").to_string(), dim()),
            ]
        })
        .collect();
    let list = block(
        4.,
        "Provider 列表",
        "Providers",
        "openshell provider list -o json",
        Risk::Read,
        v().child(table(&["NAME", "TYPE", "CREDENTIAL"], provider_rows))
            .child(note("密钥值永远不经过面板；这里只列键名与来源。")),
    );

    let create = block(
        5.,
        "创建 Provider",
        "Create Provider",
        "openshell provider create",
        Risk::Write,
        v().child(command_box(
            "openshell provider create --name <name> --type openai --from-existing",
        ))
        .child(note(
            "凭据读取是交互式的（security / 环境变量），面板不代持密钥，也不把密钥写进命令行。",
        )),
    );

    let attached = snap.attached_providers();
    let attached_rows = attached
        .iter()
        .map(|item| {
            vec![
                (item.get("name").to_string(), text()),
                (item.get("type").to_string(), muted()),
                (item.get("state").to_string(), dim()),
            ]
        })
        .collect();
    let mut attach_buttons: Vec<Stateful<Div>> = Vec::new();
    if let (Some(sandbox), Some(provider)) = (
        state.active_sandbox(),
        providers.first().and_then(|item| {
            let name = item.get("name");
            if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            }
        }),
    ) {
        let attach = Action::Attach {
            sandbox: sandbox.clone(),
            provider: provider.clone(),
        };
        let (armed, label) = confirm(state, &attach);
        attach_buttons.push(button(
            attach,
            format!("{label} {provider}"),
            Btn::Ghost,
            state.busy,
            armed,
            weak,
        ));
        let detach = Action::Detach { sandbox, provider };
        let (armed, label) = confirm(state, &detach);
        attach_buttons.push(button(detach, label, Btn::Danger, state.busy, armed, weak));
    }
    let matrix = block(
        6.,
        "附加矩阵",
        "Attach Matrix",
        "sandbox provider list / attach / detach",
        Risk::Write,
        v().child(table(&["PROVIDER", "TYPE", "STATE"], attached_rows))
            .child(h().gap(px(6.)).children(attach_buttons))
            .child(if providers.is_empty() {
                note("网关还没有 provider：先 CREATE，再 attach 到沙箱。")
            } else {
                note("attach 后新进程才会拿到凭据、策略与环境变量。")
            }),
    );

    let profile_rows = snap
        .profiles()
        .iter()
        .map(|item| {
            vec![
                (item.get("name").to_string(), text()),
                (item.get("type").to_string(), muted()),
                (item.get("endpoints").to_string(), dim()),
            ]
        })
        .collect();
    let profiles = block(
        5.,
        "Profile 浏览器",
        "Profiles",
        "openshell profile list -o json",
        Risk::Read,
        v().child(table(&["PROFILE", "TYPE", "ENDPOINTS"], profile_rows))
            .child(note(
                "空列表时 CLI 直接返回 `[]`：面板照实显示，不套示例 profile。",
            )),
    );

    let import = block(
        4.,
        "导入与校验",
        "Import & Lint",
        "openshell profile lint / import",
        Risk::Write,
        v().child(command_box("openshell profile lint --file <profile.yaml>"))
            .child(command_box(
                "openshell profile import --file <profile.yaml>",
            ))
            .child(note(
                "导入前先 lint；错误会定位到字段，面板不代替网关做校验。",
            )),
    );

    let refresh_action = Action::ProviderRefreshStatus;
    let (refresh_armed, refresh_label) = confirm(state, &refresh_action);
    let refresh = block(
        6.,
        "凭据轮换",
        "Credential Refresh",
        "openshell provider refresh status",
        Risk::Write,
        v().child(button(
            refresh_action,
            refresh_label,
            Btn::Ghost,
            state.busy,
            refresh_armed,
            weak,
        ))
        .child(
            match state
                .log
                .iter()
                .rev()
                .find(|outcome| outcome.command.contains("provider refresh"))
            {
                Some(outcome) => verbatim("refresh", &outcome.output, 14, "（无输出）"),
                None => verbatim("refresh-empty", "", 0, "尚未查询轮换状态"),
            },
        )
        .child(note(
            "轮换只记录网关侧请求；真正的密钥更新仍在 KEY 的托管处完成。",
        )),
    );

    let health = snap.health();
    let size = block(
        3.,
        "凭据健康",
        "Secret Health",
        "（聚合 provider / attach 状态）",
        Risk::Read,
        v().child(big(
            &health.providers.to_string(),
            "providers",
            if health.providers > 0 { ok() } else { dim() },
        ))
        .child(kv(vec![
            (
                "attached",
                snap.attached_providers().len().to_string(),
                muted(),
            ),
            ("rotation", "CLI 不提供到期日期".into(), dim()),
        ]))
        .child(note(
            "没有到期字段就不画倒计时：宁可少一块，也不编一个数字。",
        )),
    );

    v().w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(rows(vec![list, create, size]))
        .child(rows(vec![matrix]))
        .child(rows(vec![profiles, import, refresh]))
}

// ============================================================================
// Observe
// ============================================================================

fn observe_view(state: &State, _weak: &WeakEntity<App>) -> Div {
    let snap = &state.snap;
    let logs = block(
        6.,
        "日志流",
        "Log Stream",
        "openshell logs <name> -n 60",
        Risk::Read,
        verbatim("logs", &snap.logs.text, 24, "（无 TARGET 或没有日志）"),
    );

    let mut warn_lines = 0usize;
    let mut error_lines = 0usize;
    let mut excerpt = v().gap(px(0.));
    for line in snap.logs.text.lines() {
        let lower = line.to_lowercase();
        let level = if lower.contains("error") {
            error_lines += 1;
            Some(accent())
        } else if lower.contains("warn") || lower.contains("deny") {
            warn_lines += 1;
            Some(warn())
        } else {
            None
        };
        if let Some(color) = level
            && warn_lines + error_lines <= 12
        {
            excerpt = excerpt.child(mono(line.to_string(), 9.5, color));
        }
    }
    let search = block(
        6.,
        "日志检索",
        "Log Search",
        "logs --level warn --source sandbox",
        Risk::Read,
        v().child(kv(vec![
            ("ERROR", error_lines.to_string(), accent()),
            ("WARN / DENY", warn_lines.to_string(), warn()),
            (
                "TOTAL LINES",
                snap.logs.text.lines().count().to_string(),
                muted(),
            ),
        ]))
        .child(v().id("os-search").max_h(px(WELL)).overflow_scroll().child(
            if warn_lines + error_lines == 0 {
                note("当前日志里没有 ERROR / WARN / DENY 行。")
            } else {
                excerpt
            },
        ))
        .child(note(
            "在已取回的 60 行内本地过滤；时间窗与服务端过滤仍需 `--since` / `--source` 参数。",
        )),
    );

    let settings_rows = snap.settings_rows();
    let settings = block(
        5.,
        "设置编辑器",
        "Settings",
        "openshell settings get --global",
        Risk::Write,
        v().child(table(
            &["KEY", "VALUE", "SCOPE"],
            settings_rows
                .iter()
                .map(|(key, value)| {
                    vec![
                        (key.clone(), text()),
                        (value.clone(), muted()),
                        ("global".into(), dim()),
                    ]
                })
                .collect(),
        ))
        .child(note(
            "写入走 `settings set --global|--sandbox`；面板只读，避免误开 OCSF 等全局开关。",
        )),
    );

    let timeline = block(
        5.,
        "动作时间线",
        "Activity Timeline",
        "（面板侧记录）",
        Risk::Read,
        timeline_view(state, 8),
    );

    let probe = block(
        4.,
        "原始输出探针",
        "Raw Probe",
        "openshell ... -o json",
        Risk::Read,
        match state.log.last() {
            Some(outcome) => verbatim("dry-run", &outcome.output, 18, "（无输出）"),
            None => verbatim(
                "probe-empty",
                "",
                0,
                "执行任一动作后，这里保留它的原始 stdout",
            ),
        },
    );

    v().w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(rows(vec![logs, search]))
        .child(rows(vec![settings, timeline]))
        .child(rows(vec![probe]))
        .children(Vec::<Div>::new())
        .child(mono_strip(state))
}

/// Small helper kept separate so `observe_view` stays readable.
fn mono_strip(state: &State) -> Div {
    let last = state.log.last();
    h().w_full()
        .items_center()
        .justify_between()
        .gap(px(20.))
        .pt(px(8.))
        .child(mono(
            match last {
                Some(outcome) => format!("$ {}", outcome.command),
                None => "$ （等待第一个动作）".into(),
            },
            9.5,
            dim(),
        ))
        .child(mono(
            format!(
                "{} · {}",
                if state.busy { "busy" } else { "idle" },
                if last.is_some_and(|outcome| !outcome.ok) {
                    "last exit != 0"
                } else {
                    "last exit 0 / 未执行"
                }
            ),
            9.5,
            dim(),
        ))
}

fn timeline_view(state: &State, limit: usize) -> Div {
    if state.log.is_empty() {
        return note("还没有动作。面板里的每次 START / EXEC / EXPOSE 都会留在这里。");
    }
    let mut list = v().gap(px(6.));
    for outcome in state.log.iter().rev().take(limit) {
        list = list.child(
            h().items_start()
                .gap(px(8.))
                .child(
                    div()
                        .size(px(5.))
                        .mt(px(5.))
                        .rounded_full()
                        .bg(if outcome.ok { ok() } else { accent() }),
                )
                .child(
                    v().min_w_0()
                        .child(mono(outcome.command.clone(), 9.5, muted()))
                        .child(mono(
                            outcome.output.lines().next().unwrap_or("").to_string(),
                            9.,
                            if outcome.ok { dim() } else { accent() },
                        )),
                ),
        );
    }
    list
}

// ============================================================================
// Tooling
// ============================================================================

fn tooling_view(state: &State, _weak: &WeakEntity<App>) -> Div {
    let snap = &state.snap;
    let compose = block(
        6.,
        "命令生成器",
        "Command Composer",
        "任意子命令拼装",
        Risk::Read,
        v().child(chips(&[
            "sandbox exec",
            "--workdir",
            "--env",
            "--timeout",
            "--tty",
            "--no-login-shell",
        ]))
        .child(command_box(
            "openshell sandbox exec <name> --workdir /workspace --timeout 120 -- ls -la",
        ))
        .child(note(
            "面板内每个按钮都对应这类可复制的命令，行为能脱离 UI 复现。",
        )),
    );

    let sandboxes = snap.sandboxes();
    let batch = block(
        6.,
        "批量动作",
        "Batch Runner",
        "多沙箱 + 同一动作",
        Risk::Danger,
        v().child(if sandboxes.is_empty() {
            note("没有沙箱可批量。")
        } else {
            chips(
                &sandboxes
                    .iter()
                    .take(6)
                    .map(|sbx| sbx.name.as_str())
                    .collect::<Vec<_>>(),
            )
        })
        .child(command_box(
            "for sb in <name> <name>; do\n  openshell sandbox exec $sb -- git status\ndone",
        ))
        .child(warn_note(
            "破坏性动作不进批量；面板只给出预览，执行留在终端。",
        )),
    );

    let term = block(
        4.,
        "TUI 直通",
        "Term",
        "openshell term",
        Risk::Write,
        v().child(command_box("openshell term"))
            .child(kv(vec![
                ("theme", "auto / dark / light".into(), muted()),
                ("reason", "完整 CLI 体验不在面板里重造".into(), dim()),
            ]))
            .child(note("需要 TUI 时从面板复制这条命令到终端。")),
    );

    let keys = block(
        3.,
        "补全与别名",
        "Completions",
        "openshell completions bash",
        Risk::Read,
        v().child(command_box(
            "openshell completions bash\n  > ~/.local/share/bash-completion/completions/openshell",
        ))
        .child(chips(ALIASES))
        .child(note("别名与补全让 CLI 与面板说同一套词。")),
    );

    let fleet = block(
        6.,
        "机队健康矩阵",
        "Fleet Matrix",
        "（sandbox list 聚合）",
        Risk::Read,
        v().child(fleet_matrix(&sandboxes))
            .child(h().gap(px(14.)).mt(px(4.)).children(vec![
                legend("ready", ok()),
                legend("attention", warn()),
                legend("empty", alpha(0xffffff14)),
            ])),
    );

    let audit = block(
        6.,
        "危险动作审计",
        "Audit Trail",
        "（面板侧记录）",
        Risk::Read,
        if state.log.is_empty() {
            note("没有记录。DELETE / DETACH / STOP 只有在二次确认后才会写进这里。")
        } else {
            let rows = state
                .log
                .iter()
                .rev()
                .take(10)
                .map(|outcome| {
                    vec![
                        (
                            outcome.command.clone(),
                            if outcome.ok { muted() } else { accent() },
                        ),
                        (
                            if outcome.ok { "exit 0" } else { "exit != 0" }.to_string(),
                            if outcome.ok { dim() } else { accent() },
                        ),
                    ]
                })
                .collect();
            table(&["ACTION", "RESULT"], rows)
        },
    );

    v().w_full()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(rows(vec![compose, batch]))
        .child(rows(vec![term, keys, fleet]))
        .child(rows(vec![audit]))
}

fn fleet_matrix(sandboxes: &[openshell::Sandbox]) -> Div {
    if sandboxes.is_empty() {
        return note("没有沙箱：矩阵为空，不画占位方块。");
    }
    let mut list = v().gap(px(4.));
    for chunk in sandboxes.chunks(16) {
        let mut line = h().gap(px(4.));
        for sbx in chunk {
            let color = if matches!(sbx.phase.as_str(), "Ready" | "Running") {
                ok()
            } else {
                warn()
            };
            line = line.child(
                div()
                    .w(px(18.))
                    .h(px(10.))
                    .bg(color)
                    .id(format!("os-fleet-{}", sbx.name)),
            );
        }
        list = list.child(line);
    }
    list
}

fn legend(label: &str, color: Hsla) -> Div {
    h().items_center()
        .gap(px(5.))
        .child(div().size(px(6.)).bg(color))
        .child(mono(label.to_string(), 9., dim()))
}
