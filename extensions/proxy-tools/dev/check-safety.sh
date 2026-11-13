#!/usr/bin/env bash
# 代理工具扩展 · 提交前安全闸门
#
# 作用：扫描本次改动（默认 origin/main..HEAD + 工作区），命中"绝对禁止"字面量
# 就拒绝提交。这是 DEV-SAFETY.md 第 5 节的执行体。
#
# 用法：
#   extensions/proxy-tools/dev/check-safety.sh            # 查 origin/main..工作区
#   extensions/proxy-tools/dev/check-safety.sh --staged   # 只查暂存区
#
# 退出码：0 = 干净；1 = 命中红线
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." && cd ..
root=$(git rev-parse --show-toplevel)
cd "$root"

base=origin/main
if [[ "${1:-}" == "--staged" ]]; then
    diff_text=$(git diff --cached -U0 -- . || true)
else
    diff_text=$(git diff "$base" -U0 -- . 2>/dev/null || true)
    diff_text+=$'\n'"$(git diff -U0 -- . || true)"
fi

# 只检查新增行，避免被删除行里的历史字符串干扰。
added=$(printf '%s\n' "$diff_text" | grep -E '^\+' | grep -vE '^\+\+\+' || true)

untracked=$(git ls-files -o --exclude-standard 2>/dev/null || true)
if [[ -z "$added" && -z "$untracked" ]]; then
    echo "· 没有新增行，跳过"
    exit 0
fi

# 自己与文档里允许出现这些字符串（它们只是在描述规则）。
allow_files='DEV-SAFETY\.md|INVESTIGATION\.md|README\.md|dev/'

patterns=(
    'systemctl[[:space:]]+--user[[:space:]]+(stop|restart|kill|reload|set-environment|unset-environment|daemon-reload)'
    'jproxy\.py[[:space:]]+(stop|serve)'
    'gsettings[[:space:]]+(set|reset)[[:space:]]+org\.gnome\.system\.proxy'
    'environment\.d/95-j1900-proxy\.conf'
    'pkill|killall'
    'iptables|nft[[:space:]]|nmcli|resolvectl|rfkill|ip[[:space:]]+link'
    '31001|17670|8089|8765|8770'
)

hit=0
# 逐文件检查，便于放行白名单文件。
files=$(git diff "$base" --name-only 2>/dev/null; git diff --name-only 2>/dev/null; \
        git ls-files -o --exclude-standard 2>/dev/null)
files=$(printf '%s\n' "$files" | sort -u | grep -vE "$allow_files" || true)

for f in $files; do
    [[ -f "$f" ]] || continue
    if git ls-files --error-unmatch "$f" >/dev/null 2>&1; then
        file_added=$(printf '%s\n' "$diff_text" | awk -v want="b/$f" '
            /^diff --git / { cur=$0; in_file = (cur ~ (" " want "$")); next }
            in_file && /^\+/ && !/^\+\+\+/ { print }
        ')
    else
        # 未跟踪的新文件：全文都算新增
        file_added=$(sed 's/^/+/' "$f")
    fi
    [[ -z "$file_added" ]] && continue
    # 行内标记 `safety-allow` 才放行（例如常量定义、打印给操作员的文本）
    file_added=$(printf '%s\n' "$file_added" | grep -v 'safety-allow' || true)
    [[ -z "$file_added" ]] && continue
    for p in "${patterns[@]}"; do
        if printf '%s\n' "$file_added" | grep -Eq -- "$p"; then
            echo "✗ 红线命中：$f  ←  $p"
            printf '%s\n' "$file_added" | grep -En -- "$p" | sed 's/^/    /' | head -3
            hit=1
        fi
    done
done

if [[ $hit -ne 0 ]]; then
    echo
    echo "拒绝：以上改动触碰了 DEV-SAFETY.md 的红线。"
    echo "如果确实需要（例如只是文档引用），请把该文件加进本脚本的 allow_files。"
    exit 1
fi
echo "· 安全闸门通过（未触碰代理红线）"
