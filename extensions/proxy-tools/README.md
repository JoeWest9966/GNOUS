# 代理工具（`proxy-tools`）

页面级扩展，入口在 **Extensions 分区 → PROXY**。它管理的是本机那套
`127.0.0.1:31001 → 192.168.1.7:31001` 代理中继与控制面（`gsettings` +
`environment.d`），也就是原来 `/home/root/GNOME/代理工具/` 下那套 bash + python 脚本。

## 为什么这个扩展格外小心

本机所有外网访问——包括驱动这次开发的模型流量——都经过 `31001`。
把中继弄死不是"测试失败"，而是**开发本身当场中断**，且掉线后没有任何自动化手段
可以把它救回来。所以这个扩展的边界不是"尽量别碰"，而是**结构上碰不到**：

- 旧单元 `j1900-proxy.service` 是只读对象，GNOUS 只 `show` / `cat` / `is-active`；
- `systemctl --user set-environment` / `unset-environment` / `daemon-reload` /
  `restart` / `kill` 这些动词在接口层就不存在（`src/proxy_tools/guard.rs`）；
- 绑定受保护端口（`31001` 等）必须显式带 `GNOUS_PROXY_ALLOW_PROTECTED_PORT=1`，
  而这个变量只在**交棒之后**由 GNOUS 自己的单元设置；
- 交棒（把端口从旧中继交给 GNOUS）只打印命令，GNOUS 永不执行。

开发期的完整约束（事实清单、红线、只读白名单、事故恢复）见
[`DEV-SAFETY.md`](./DEV-SAFETY.md)，现状取证见 [`INVESTIGATION.md`](./INVESTIGATION.md)。

## 三个面

| 面 | 动作 | 会做什么 |
| --- | --- | --- |
| 诊断 | `STATUS` / `DOCTOR` | 只读：`/proc/net/tcp*` 判监听、`systemctl --user show`、环境文件、`gsettings get`、上游 LAN 握手、日志尾部异常统计 |
| 数据面 | `APPLY` / `REVERT` | 只写/删 GNOUS 自己的东西：`~/.config/gnous/proxy-tools.json` 与 `~/.config/systemd/user/gnous-proxy-relay.service`；**不 enable、不 start、不 reload** |
| 交棒 | `HANDOVER` | 只打印人手执行的命令（含"后悔药"回旧中继），`facts.executed = false` |

`APPLY` / `REVERT` 需要点第二次确认；成功后会自动重跑一次只读 `STATUS`。

## 数据面：中继自己实现

`jproxy.py` 的行为用标准库重写进 `src/proxy_tools/relay.rs`，通过
`gnous --builtin-proxy-relay --listen … --upstream …` 运行，由 GNOUS 自己的
user 单元 `gnous-proxy-relay.service` 托管：

- 一个线程处理一个连接，两个方向各一个 `pump`，读到 EOF 后对另一侧 `shutdown(Write)`；
- 上游连接有重试（默认 2 次）与 `connect_timeout`（默认 8s），两侧都有空闲超时（默认 300s）；
- `SIGTERM` 让 accept 循环在 80ms 内退出，不做 `wait_closed` 那种会挂住的事；
- 不引入任何新 crate：`std::net` + 线程 + `libc` 的信号，保证 `cargo build --offline` 可用。

## 状态文件与配置

- 配置：`$XDG_CONFIG_HOME/gnous/proxy-tools.json`（缺省 `~/.config/gnous/proxy-tools.json`），
  原子写（临时文件 + rename）。
- 单元：`$XDG_CONFIG_HOME/systemd/user/gnous-proxy-relay.service`。
- 日志：`~/.cache/gnous/proxy-relay.log`。
- 字段：`listen` / `upstream` / `idleTimeoutSecs` / `connectTimeoutSecs` / `retries` /
  `noProxyMode`（`exact24` 整段精确 IP、`minimal` 仅本机+网关、`manual` 不生成）/
  `adopted`（交棒后置真，决定单元里是否带 `GNOUS_PROXY_ALLOW_PROTECTED_PORT=1`）。

## `no_proxy` 为什么枚举 254 个 IP

Node / Bun 的 `no_proxy` 只做精确主机名与域名后缀匹配，**完全不认 CIDR**：
`192.168.0.0/16`、`192.168.1.0/24`、`192.168.1.` 全部无效，只有 `192.168.1.7`
这种精确 IP 才生效。于是 `exact24` 模式按真实网卡枚举整个 /24，并能在
`DOCTOR` 里直接报出"缺 N 个精确 IP"。curl 认 CIDR 所以用 curl 测永远正常，
这正是"命令行能上网、pi 连不上内网 API"的根因。

## 测试

```bash
CARGO_NET_OFFLINE=true cargo test --offline proxy_tools
```

46 项：guard 的红线（旧单元只读、会话级动词被拒、只读动词放行、端口 opt-in、
允许清单自证）、`no_proxy` 解析与缺口、`/proc/net/tcp` 解析、事实装配（三种健康结论）、
relay 的回环端到端与"上游不可达时不挂住"、单元渲染（不出现 `jproxy` / `python3`）、
以及 `run()` 的 op 语义。测试只绑 `127.0.0.1:0` 或高位端口，从不连真实上游。

## 开发期工具

```bash
extensions/proxy-tools/dev/check-safety.sh    # 提交前红线扫描（含未跟踪文件）
extensions/proxy-tools/dev/commit.sh -m "…"   # 闸门 + 提交一步到位
extensions/proxy-tools/dev/restore-network.sh # 人手救命脚本，默认 dry-run
```
