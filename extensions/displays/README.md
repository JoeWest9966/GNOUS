# 多屏幕（`displays`）

页面级扩展，入口在 **Extensions 分区 → DISPLAY**。它替代
`/home/root/GNOME/多屏幕/display-mode`（那只 bash + PyGObject 脚本 + 每 2 秒对账的
`display-mode.timer`）：同样的四档模式，但状态是结构化的、中继逻辑在 Rust 里、
换屏之前先在 mutter 的 verify 模式下校验一遍。

## 三个面

| 面 | 操作 | 会不会改变显示 |
| --- | --- | --- |
| 诊断 | `STATUS` / `DOCTOR` | 不会。`/sys/class/drm` + `GetCurrentState` + `systemctl show` |
| 布局 | `PREVIEW` / `APPLY` | `PREVIEW` 不会（`METHOD_VERIFY`）；`APPLY` 会，二次确认 |
| 接管 | `HANDOVER` / `REVERT` | 不会。前者只打印命令，后者只删 GNOUS 自己的两只单元文件 |

`APPLY` 的顺序是固定的：**verify → 持久化 → 重新读一次确认真实布局**。
verify 被 mutter 拒绝时绝不落地——真机上这条救过一次：把两块不同分辨率的屏塞进
同一个 logical monitor 时，mutter 回 `InvalidArgs: Monitors modes in logical monitor
not equal`，面板原样显示这句，屏幕一点没动。

## 为什么不用 `display-mode` 那只脚本

- 它依赖 PyGObject（Python + GLib），GNOUS 自带后不该再要求机器上有这个绑定；
- 它的 `status` 输出是给人看的文本，UI 只能猜；这里返回的是 facts；
- 它的 `both` 把两块屏放在同一个 `logical_monitor` 里（同一个坐标），不同模式时
  mutter 会直接拒绝，所以那个模式实际上从来没成功过；这里每块屏一个 logical
  monitor，横向依次排开；
- 它的 `auto` 只看 sysfs，不看 mutter 有没有可用模式；这里两个条件都要满足
  （mutter 不可用时会退回"按 sysfs 推断意图"，因为那种状态下本来也 apply 不了）。

## D-Bus：自己写的，约 700 行

`src/displays/dbus.rs` 是一个只想清楚做几件事的 D-Bus 客户端：`EXTERNAL` 认证、
`Hello`、方法调用、按签名解析回复。不新增任何 crate，`cargo build --offline` 可用。

踩过的四个坑，都写进了单测：

1. `EXTERNAL` 之后**必须先 `Hello`**，否则总线直接断开连接；
2. 变体里的类型签名是 `g`（1 字节长度、1 字节对齐），不是 `s`（4 字节长度）；
3. 头字段 PATH 的类型必须是 `o`，写成 `s` 会被 daemon 拒绝；
4. 数组长度**不含**最后一个元素的尾部填充——填充在数组之外。

`mutter.rs` 只包两件事：`GetCurrentState` 和 `ApplyMonitorsConfig`。真机集成测试
（有会话总线时才跑）会拿当前布局做一次 verify，并在两块屏都在时再验证一次
"扩展"布局，所以编码错位不会被静默放过。

## 配置

`$XDG_CONFIG_HOME/gnous/displays.json`（缺省 `~/.config/gnous/displays.json`），
原子写。缺省值刻意等于老脚本的行为：

```json
{
  "mode": "auto",
  "external": "HDMI-1",
  "internal": "eDP-1",
  "resolution": "preferred",
  "reconcileSecs": 2,
  "takeover": false
}
```

- `mode`：`auto`（有外接且 mutter 认它 → 外接，否则内置）/ `external` / `internal` / `both`
- `resolution`：`preferred`（mutter 标的 is-preferred）/ `max` / `current`
- `reconcileSecs`：GNOUS 自己接管时 timer 的周期，1..=60
- `takeover`：交棒之后置真，面板据此判断"现在归谁管"

## GNOUS 自己的单元

`APPLY` 只写两只文件，**从不 enable、从不 start**：

- `gnous-display.service`：`Type=oneshot`，`ExecStart=<gnous> --builtin-display-auto`
- `gnous-display.timer`：`OnBootSec=5s`，`OnUnitActiveSec=<reconcileSecs>s`

真正把控制权切过来的命令在 `HANDOVER` 里打印（含"后悔药"），由人执行：
先停/禁 `display-mode.timer`，`daemon-reload`，再 `enable --now gnous-display.timer`。
在那之前，`display-mode.timer` 依然是唯一在动屏幕的东西——所以面板常说
`ATTENTION：布局符合配置，但仍是 display-mode.service 在对账`。

## 测试

```bash
CARGO_NET_OFFLINE=true cargo test --offline displays::
```

40 项：D-Bus 编解码（含上面四个坑）、连接器名归一化与 sysfs 解析、配置校验与
原子写、单元渲染（不出现 `python3` / `/home/root/GNOME`）、纯函数 facts
（healthy / attention / drift / degraded 四种结论）、以及三条真机集成测试。
