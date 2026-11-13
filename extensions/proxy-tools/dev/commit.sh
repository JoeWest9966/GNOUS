#!/usr/bin/env bash
# 代理工具扩展专用提交入口：安全闸门 + 提交，一步到位。
# 用法：extensions/proxy-tools/dev/commit.sh -m "消息"
set -uo pipefail
root=$(git rev-parse --show-toplevel); cd "$root"
bash extensions/proxy-tools/dev/check-safety.sh || {
    echo "闸门未通过，拒绝提交"; exit 1; }
exec git -c core.hooksPath=extensions/proxy-tools/dev/hooks \
    -c user.name="JoeWest9966" \
    -c user.email="JoeWest9966@users.noreply.github.com" commit "$@"
