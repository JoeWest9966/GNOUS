#!/usr/bin/env bash
# 代理工具扩展 · 断网自救脚本（给人手用，不是给 agent 自动跑）
#
# 背景：GNOUS 代理工具扩展开发期间，agent 的模型流量也走 127.0.0.1:31001。
# 万一中继被弄停，agent 立刻掉线，只能由人手跑本脚本恢复。
#
# 用法：
#   bash extensions/proxy-tools/dev/restore-network.sh          # 只看计划（默认 dry-run）
#   bash extensions/proxy-tools/dev/restore-network.sh --yes    # 真的执行
set -uo pipefail

LOCAL_PORT="${LOCAL_PORT:-31001}"
UPSTREAM="${UPSTREAM:-192.168.1.7:31001}"
UNIT="j1900-proxy.service"
TOOL_DIR="${TOOL_DIR:-/home/root/GNOME/代理工具}"

apply=0
[[ "${1:-}" == "--yes" ]] && apply=1

run() {
    if (( apply )); then
        echo "+ $*"
        "$@"
    else
        echo "[dry-run] $*"
    fi
}

echo "== 检查当前状态 =="
printf '31001 监听           : '; (ss -ltn 2>/dev/null | grep -q ":${LOCAL_PORT}[[:space:]]") && echo "有" || echo "无"
printf 'unit ActiveState     : '; systemctl --user show "$UNIT" -p ActiveState --value 2>/dev/null || echo "未知"
printf 'GNOME proxy mode     : '; gsettings get org.gnome.system.proxy mode 2>/dev/null || echo "未知"
printf 'user manager 代理变量: '; systemctl --user show-environment 2>/dev/null | grep -ci proxy || echo 0

echo
echo "== 恢复步骤（先轻后重）=="
echo "-- 1. 让 systemd 把中继拉起来（Restart=always，通常 3 秒内自愈）"
run systemctl --user start "$UNIT"
if (( apply )); then
    for _ in $(seq 1 30); do
        ss -ltn 2>/dev/null | grep -q ":${LOCAL_PORT}[[:space:]]" && break
        sleep 0.2
    done
fi

if ss -ltn 2>/dev/null | grep -q ":${LOCAL_PORT}[[:space:]]"; then
    echo "· 中继已在监听 ${LOCAL_PORT}"
else
    echo "-- 2. unit 起不来，退回裸进程"
    run python3 "$TOOL_DIR/jproxy.py" serve --daemon --listen "127.0.0.1:${LOCAL_PORT}" --upstream "$UPSTREAM"
fi

echo
echo "== 如果浏览器 / GUI 也上不了网，再恢复控制面 =="
run bash "$TOOL_DIR/on.sh"

if (( ! apply )); then
    echo
    echo "以上都是 dry-run。确认要执行请加 --yes。"
fi
