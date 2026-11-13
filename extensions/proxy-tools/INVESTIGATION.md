# 代理工具扩展 · 现状调查报告（只读取证）

调查时间：2026-11-13 18:26–18:40，全程只读，未执行任何 `on.sh` / `off.sh` /
`systemctl --user` 写操作。原始文件备份在 `/tmp/proxy-backup-20261113-182652/`。

## 1. 现有实现构成

| 层 | 文件 | 作用 |
| --- | --- | --- |
| 数据面 | `jproxy.py`（393 行） | asyncio TCP 中继：`127.0.0.1:31001` → `192.168.1.7:31001` |
| 数据面托管 | `j1900-proxy.service`（user 单元，enabled + active） | `ExecStart=/usr/bin/python3 /home/root/GNOME/代理工具/jproxy.py serve`，`Restart=always`、`RestartSec=3` |
| 控制面 | `on.sh` / `off.sh` + `lib.sh` | gsettings（GNOME 代理）+ `~/.config/environment.d/95-j1900-proxy.conf` + `~/.config/j1900-proxy/env.sh` |
| 运行态 | `~/.cache/j1900-proxy/{jproxy.log, jproxy.pid}` | 日志 10365 行 / 816 KiB，pidfile 当前不存在（跑的是 systemd 实例） |

实测运行态：`ExecMainPID=440668`、`SubState=running`、`NRestarts=0`、
`gsettings org.gnome.system.proxy mode = 'manual'`、
user manager 里 8 条含 proxy 的环境变量、`wlp1s0 192.168.1.3/24`。

## 2. 现有实现的真实行为

- `jproxy.py serve`：单端口同时说 SOCKS5 与 HTTP CONNECT（上游 sing-box mixed 入站），
  双向 pump、空闲 300s 超时、上游重试 2 次、`SIGTERM` 时**故意不 await
  server.wait_closed()**（否则 keep-alive 会让 systemd 判定 failed）。
  已修过的坑：`%h` 展开成 `/root/...` 导致 unit 从未成功启动，现改绝对路径。
- `check`：真的对 `github.com:443` 做 SOCKS5 建链 + TLS + HTTP HEAD + HTTP CONNECT
  四步，**会消耗隧道并对公网发请求**。
- `status`：读 pidfile + `port_open` 本地端口 + `port_open` 上游端口（TCP connect 即断开）。
- `on.sh`：先确认中继活着（没有就 `systemctl --user start` 或起裸进程），
  跑 `check` 通过后才改控制面；`no_proxy` 枚举**本机 /24 的全部 254 个精确 IP**
  （Node/Bun 不认 CIDR）。
- `off.sh`：停中继（unit + 裸进程）→ `gsettings mode none` → 删两个环境文件 +
  `/run/user/<uid>/systemd/environment.d/` 副本 → `systemctl --user unset-environment`
  → 打印五项自检。

## 3. 现有实现的缺陷（本扩展要修的）

1. **数据面与控制面耦合在脚本里**：`off.sh` 一口气把中继停掉 + 清环境变量，
   没有"只关控制面、保留中继"的中间档；而两者风险等级完全不同。
2. **没有结构化状态**：全部是人类可读 stdout，UI 只能靠正则，无法判健康。
3. **`check` 太贵**：跑一次会对公网建 4 条连接；面板不能按这个频率采样。
4. **`no_proxy` 硬枚举 254 个 IP**：换网段/多网卡/临时网段就漏；且 7216 字节的
   environment.d 每次开机都要解析。
5. **路径写死在 `/home/root/GNOME/代理工具/`**：靠外部目录存在才能工作，
   和 GNOUS 版本化安装脱节（正是这次要内嵌的原因）。
6. **没有审计**：谁在什么时候改了控制面、改前改后是什么，无记录。
7. **`stop` 不区分来源**：`jproxy.py stop` 按 pidfile 杀进程，与 systemd 托管
   两套进程管理并存，容易互相打架。

## 4. 关键约束（决定实现形态）

- **不能新增 Cargo 依赖**：`cargo metadata --offline` 已验证会去联网（
  `accesskit_macos` 之类非 Linux 目标依赖不在本地缓存），一旦新增 crate 就必须
  走代理下载。所以中继要用 `std::net` + 线程实现，不引入 tokio。
- **`cargo check/test --offline` 已验证可行**（1m28s / 2m16s，48 测试绿），
  开发全程离线。
- **切换 31001 的主人是用户**：把活的中继换掉必然有几秒断流，
  这一步不能由 agent 自动执行，只能由用户点/手工执行。

## 5. 建议的接入形态（待确认）

页面级扩展 `proxy-tools`，三个互相独立、风险分级的面：

- **A. 诊断（只读）**：中继单位状态、端口监听、上游可达、控制面开关、
  `no_proxy` 是否覆盖当前网段、environment.d 与实际环境是否一致、日志尾部错误率。
- **B. 数据面（GNOUS 自己的中继）**：Rust `std` 实现中继，作为
  `gnous --builtin-proxy-relay` 子命令 + GNOUS 自己的 user 单元
  `gnous-proxy-relay.service`；只启停自己的 unit，把 `j1900-proxy.service`
  当只读的遗留对象；"迁移到 GNOUS 中继"是一次性、需二次确认、且提示会短暂断流的动作。
- **C. 控制面（gsettings + 环境变量）**：只在用户点击时写，写前展示 diff，
  默认 dry-run，提供"回到直连"回滚；`systemctl --user unset-environment` 这类
  会掐断当前会话的操作不由 GNOUS 自动执行，只生成命令给用户。
