#!/usr/bin/env bash
# 文件规模闸门：对 base..HEAD **新增**的源码文件检查行数上限（800 行），
# 超限直接失败——dev-habits『单一职责』的机械兜底（约 300 行是拆分提醒线，
# 800 是硬闸）。只管新增文件：存量超限文件不在此列，随结构拆分逐批销号。
#
# 用法：scripts/check-file-size.sh <base-ref-or-sha>
# CI 里 PR 取 base 分支、push 取 github.event.before（首个推送 before 为
# 全 0，直接放行）。
set -euo pipefail
cd "$(dirname "$0")/.."

BASE=${1:?用法: check-file-size.sh <base-ref-or-sha>}
LIMIT=800
fail=0
while IFS= read -r f; do
  case "$f" in vendor/* | crates/hongguo-vvdec-sys/vendor/*) continue ;; esac
  [ -f "$f" ] || continue
  lines=$(wc -l < "$f" | tr -d ' ')
  if [ "$lines" -gt "$LIMIT" ]; then
    echo "超限新增文件: $f (${lines} 行 > ${LIMIT} 上限，先拆分再合入)"
    fail=1
  fi
done < <(git diff --name-only --diff-filter=A "${BASE}...HEAD" -- '*.rs' '*.ts' '*.tsx')
if [ "$fail" -eq 0 ]; then
  echo "文件规模闸门通过（新增文件均 ≤ ${LIMIT} 行）"
fi
exit "$fail"
