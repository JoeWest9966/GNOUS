# 代理工具扩展 · 开发期安全准则（本人强制约束）

> 适用范围：**只适用于 `proxy-tools`（代理工具）扩展的开发期**。
> 生效时间：分支 `feat/proxy-tools-ext` 从第一次提交开始，直到该扩展验收合并。
> 为什么需要它：本机的一切外网访问（包括正在跑这次对话的模型 API）都经过
> `127.0.0.1:31001` 这个中继；把它弄死 = 对话立刻中断、开发无法继续，
> 而且我掉线后没有任何补救手段，只能靠人手恢复。

---

## 1. 事实清单（2026-11-13 实测，变更后需重新核对）

| 项目 | 值 |
| --- | --- |
| 本机代理入口 | `127.0.0.1:31001`（HTTP / HTTPS / SOCKS5 同一端口） |
| 中继进程 | `python3 /home/root/GNOME/代理工具/jproxy.py serve`（当时 pid 440668，pid 会变） |
| 中继托管 | systemd **user** 单元 `j1900-proxy.service`，`Restart=always`、`RestartSec=3` |
| 上游 | `192.168.1.7:31001`（j1900 上 sing-box mixed 入站） |
| 控制面 | `gsettings org.gnome.system.proxy`（当时 `mode='manual'`）+ 环境变量 |
| 持久化环境文件 | `~/.config/environment.d/95-j1900-proxy.conf`、`~/.config/j1900-proxy/env.sh` |
| 运行中 user manager 环境 | `systemctl --user show-environment` 里 6 个 `*_proxy` 变量 |
| 中继日志 | `~/.cache/j1900-proxy/jproxy.log`（pidfile 存在时是 `jproxy.pid`） |
| 我的命脉 | 这次对话的 API 流量走上述中继；**它停 = 我死** |

推论（写进肌肉记忆）：

1. `j1900-proxy.service` 不是"待管理的对象"，是**承载我自己的基础设施**。
2. `systemctl --user unset-environment http_proxy ...` 会让我**下一条命令**就失去代理。
3. 任何 `pkill -f jproxy` / `killall python3` / `kill <pid>` 都可能直接掐断本对话。

---

## 2. 绝对禁止（硬红线，任何情况下都不执行）

以下字面量**不得出现在我构造的命令、脚本、测试、代码常量里**，除非是被
`safety_check` 显式标注为"仅展示/仅匹配"的字符串（且必须同时存在对应单测）。

```
systemctl --user stop|restart|kill|reload j1900-proxy.service
systemctl --user set-environment / unset-environment      # 会清掉本会话代理变量
systemctl --user daemon-reload                            # 可能连带重启中继
jproxy.py stop | jproxy.py serve                          # 停止/启动中继本体
gsettings set|reset org.gnome.system.proxy                # 全局控制面
rm|mv ~/.config/environment.d/95-j1900-proxy.conf
rm|mv ~/.config/j1900-proxy/
pkill | killall | kill -9 <pid>                           # 任何批量杀进程
iptables | nft | ip link | nmcli | resolvectl | rfkill     # 网络栈
```

另外禁止触达的端口（不监听、不连接、不探测端口占用）：

```
31001  代理入口（本对话命脉）
17670  openshell-gateway
8089   本地 python 服务
8765   本地 python 服务
8770   本地 python 服务
```

## 3. 允许的只读探测（白名单，且必须是"读"语义）

```
systemctl --user show j1900-proxy.service -p ActiveState,UnitFileState,ExecMainPID,SubState
systemctl --user cat j1900-proxy.service
systemctl --user show-environment                 # 只读；只看有没有 *_proxy
ss -ltn / ss -ltnp                                # 只读
cat /proc/<pid>/cmdline / cwd / status            # 只读
cat ~/.config/j1900-proxy/env.sh                  # 只读
gsettings get org.gnome.system.proxy              # 只读
journalctl --user -u j1900-proxy.service -n 50    # 只读
```

**不准跑**（会真的消耗隧道或触发限流）：

- `jproxy.py check` —— 会真的对 github.com 建 SOCKS5 + TLS + CONNECT 三连。
- 任何 `curl`/`wget` 目标不是 `127.0.0.1` 的请求（包括"测一下通不通"）。
- `jproxy.py status` 允许，因为它只做 TCP connect + close（上限：每个动作一次，不循环）。

---

## 4. 代码级强规则（落进实现，不只写在文档里）

实现 `src/fixes_backend/proxy/guard.rs`，把红线变成**编译期常量 + 运行期断言 +
单测**，任何越过红线的能力都构造不出来：

1. `pub const PROTECTED_UNITS: &[&str] = &["j1900-proxy.service"]`
   `pub const PROTECTED_PORTS: &[u16] = &[31001, 17670, 8089, 8765, 8770]`
2. `pub fn assert_port_allowed(port: u16) -> Result<(), SafetyError>`
   —— 监听/连接前必须调用；测试里断言 31001 会被拒绝。
3. `pub fn assert_command_allowed(argv: &[String]) -> Result<(), SafetyError>`
   —— 唯一的外部命令出口是 `process::run_bounded`，在它里面**强制**过这个闸门；
   命中红线直接返回 `Err`，绝不 spawn。
4. `pub static ALL_COMMANDS: &[&[&str]]`（本扩展能执行的全部命令的**静态清单**），
   配一个单测：清单里每一条都必须 `assert_command_allowed` 通过；清单之外
   的命令在运行期被拒绝。这样"多写一条危险命令"会先把测试弄红。
5. `GNOUS_PROXY_DRY_RUN=1` 时，所有 mutating 动作（apply / revert）只打印
   "将要做什么"，**不 spawn、不写文件**。我开发期全程带着这个变量跑。
6. 集成测试只能绑 `127.0.0.1:0`（系统分配端口）或 `GNOUS_PROXY_TEST_PORT`
   指定的大于 20000 的端口，且上游一律是测试自己起的假 `TcpListener`；
   测试里断言 `PROTECTED_PORTS` 全部被 guard 拒绝。
7. 扩展的对外能力里**永远不存在**"停止中继 / 重启中继 / 改全局代理开关"，
   只有"读状态 + 写 GNOUS 自己的配置文件 + 生成给用户手动执行的命令文本"。

## 5. 构建与执行规则

1. 所有 cargo 命令都带 `CARGO_NET_OFFLINE=true`（或 `--offline`）：
   一旦需要下载，它会**立刻报错**而不是挂着等网络。
2. 不新增任何 `Cargo.toml` 依赖。确实需要时停下来问，绝不用 `cargo add`。
3. 每次 `git commit` 前必须跑 `extensions/proxy-tools/dev/check-safety.sh`：
   扫描本次 diff 里的禁止字面量，命中就拒绝提交。
4. 不执行仓库外 `/home/root/GNOME/代理工具/` 下的任何 `.sh`（`on.sh` / `off.sh`
   正是"会改控制面"的那两个）。

## 6. 事故恢复（我掉线后，人手执行）

按顺序试，第一条就够绝大多数情况：

```bash
# 1) 中继自愈（Restart=always，通常 3 秒内自己回来）
systemctl --user start j1900-proxy.service
ss -ltn | grep 31001 && echo 已恢复

# 2) 中继起不来 → 手工拉起
python3 /home/root/GNOME/代理工具/jproxy.py serve -d

# 3) 控制面被误关 → 恢复全局代理（会重新写 environment.d 与 gsettings）
bash /home/root/GNOME/代理工具/on.sh
```

仓库内也放一份 `extensions/proxy-tools/dev/restore-network.sh`（默认 dry-run，
`--yes` 才真做），与我掉线时你只需记住一句话：`bash` 上面第 1 条。

## 7. 我每次动手前的三问

1. 这条命令会不会碰 `31001` / `j1900-proxy` / `systemctl --user` / `gsettings set`？
2. 它会不会让**当前这个 shell 的环境变量**消失？
3. 如果它把中继弄死，我还能不能继续这个对话？

**任何一问答"可能"→ 不做，改写方案或停下来问。**
