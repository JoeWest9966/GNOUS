# 终端字体（`termfont`）

页面级扩展，入口在 **Extensions 分区 → TYPE**。它替代
`/home/root/GNOME/终端字体/termfont`（那只一次性 CLI）：
同样把 GNOME 终端和 FinalShell 的字号调到一处，但状态是结构化的、
写前先备份、改完重新读一次再报结论。

## 为什么两台终端要一起管

两者字号来源完全不同，所以平时"各改各的、还常常找不到"：

| 程序 | 来源 | 单位 | 生效方式 |
| --- | --- | --- | --- |
| GNOME 终端 | gsettings `org.gnome.Terminal.Legacy.Profile` 的 `font` | **pt** | 改完立即生效 |
| FinalShell | `~/.finalshell/config.json` 的 `font_size` | **px** | 只在启动时读取 |

这就是 FinalShell 显得特别小的原因：默认 `12 px` 只有 `9 pt`，比终端里常见的
`11~12 pt` 小三分之一。换算关系固定：**1 pt = 4/3 px**。

## 六个动作

| 标签 | 会改机器 | 说明 |
| --- | --- | --- |
| `STATUS` | 否 | 读系统等宽字体、终端 profile、FinalShell 配置，给出结论 |
| `DOCTOR` | 否 | 八项自检，每项带真实细节 |
| `TERM` | 是 | 把终端设成目标 pt（字体族沿用当前自己选的，跟随系统则用系统族名） |
| `FS` | 是 | 定点改 `font_size`，**写前备份**成 `config.json.bak` |
| `ALL` | 是 | 两者一起改，px 由 pt 换算（除非关掉换算） |
| `FOLLOW` | 是 | 终端改回"跟随系统等宽字体"（等价 `termfont term system`） |
| `RESTART` | 是，会断 SSH | 重启 FinalShell 让它读新字号 |

`TERM` / `FS` / `ALL` / `FOLLOW` / `RESTART` 全部需要**二次确认**；`RESTART` 在
按钮和标签上都标红，因为它会断掉正在跑的 SSH 会话。

## 目标值（页面底部的步进器）

`~/.config/gnous/termfont.json`：

```json
{ "termPt": 12.0, "fsPx": 16, "autoConvert": true }
```

- `termPt`：终端目标字号，`−/+` 一次 0.5 pt，范围 6..48
- `fsPx`：FinalShell 目标字号，一次 1 px，范围 6..200
- `autoConvert`：为真时 `ALL` 用 `termPt × 4/3` 覆盖 px；手动拧 px 会自动关掉它

改目标值只写这一个文件，**不动机器**；要生效必须再点 `TERM` / `FS` / `ALL`。
缺省值刻意等于本机此刻的真实状态，装上 GNOUS 不会悄悄改变任何东西。

## 安全边界（代码级）

1. `src/termfont/exec.rs` 是唯一执行缝，只允许 `gsettings`，并且**按键**收窄：
   `org.gnome.desktop.interface` 只能读 `monospace-font-name`；
   `org.gnome.Terminal.ProfilesList` 只能读 `default`；
   终端 profile 只能改 `font` / `use-system-font`。
   其它 schema、其它键、多余参数、形状不对的 `set`（缺值）一律拒绝。
2. FinalShell 的配置**不重新序列化**：文件里有 `huw` 这类加密字段，只有定点替换
   `"font_size":N` 这一段，其余字节一个不动。
3. 写之前备份 `config.json.bak`；值没变就不写、也不覆盖备份（幂等）。
4. 认进程只看 `/proc/<pid>/cmdline` 的 **argv[0]**。曾经用"整条命令行包含这个
   路径"来判断，结果把一条只是提到该路径的 bash 命令当成 FinalShell 发了 SIGTERM
   ——测试里踩到了，现在有回归测试守着。
5. `RESTART` 永远不由 `FS` / `ALL` 自动触发：改完只提示"下次启动生效"或
   "需要 RESTART"。重启用 SIGTERM → 最多等 4 秒 → SIGKILL → `setsid` 重新拉起。

## 调试

```bash
GNOUS_TERMFONT_DRY_RUN=1 ...      # 写操作只打印，不落盘、不重启
GNOUS_TERMFONT_CONFIG=/tmp/x.json # 换配置文件位置
GNOUS_TERMFONT_FINALSHELL_CONFIG=/tmp/c.json
GNOUS_TERMFONT_FINALSHELL_BIN=/usr/bin/sleep  # 把"重启"指到无害程序再测
```

## 测试

```bash
CARGO_NET_OFFLINE=true cargo test --offline termfont::
```

40+ 项：字号解析/格式化、profile schema 拼装、允许清单（含"写只读键"与
"路径冒用"的反例）、FinalShell 定点替换与备份幂等、facts 四档结论、
步进与换算、以及三条真机集成测试（真实 gsettings 读取、真实进程识别与重启
——后者用 `/usr/bin/sleep` 代替 FinalShell，不会弹窗口）。
