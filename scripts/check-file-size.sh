#!/usr/bin/env bash
# 文件规模闸门：对 base..HEAD **新增**的源码文件检查行数上限（800 行），
# 超限直接失败——dev-habits『单一职责』的机械兜底（约 300 行是拆分提醒线，
# 800 是硬闸）。只管新增文件：存量超限文件不在此列，随结构拆分逐批销号。
#
# 存量超限销号清单（GRANDFATHERED）：在 dev 上早已存在、随**整壳并入 main**
# 时才第一次「看起来像新增」的文件——它们是待拆分的存量，不是新违规。
# 每拆完一个就从清单划掉一行；清单只允许变短。
#
# 用法：scripts/check-file-size.sh <base-ref-or-sha>
# CI 里 PR 取 base 分支、push 取 github.event.before（首个推送 before 为
# 全 0，直接放行）。
set -euo pipefail
cd "$(dirname "$0")/.."

BASE=${1:?用法: check-file-size.sh <base-ref-or-sha>}
LIMIT=800
# 销号清单：从 dev 合入 main 时（2026-10-15，v0.4.5）首次暴露的 GPUI 存量。
# 2026-10-20 全部销号（`docs/gpui-shell-file-split.md`）：GPUI 壳已无 >800 行
# 文件，清单清空——`is_grandfathered` 保留，留给以后真的出现存量豁免时再用。
GRANDFATHERED=()
is_grandfathered() {
  local f=$1
  # `${arr[@]:-}` 让空清单在 bash 3.2 / `set -u` 下也不炸（macOS 自带 bash）。
  for g in "${GRANDFATHERED[@]:-}"; do
    [ "$f" = "$g" ] && return 0
  done
  return 1
}
fail=0
while IFS= read -r f; do
  [ -f "$f" ] || continue
  lines=$(wc -l < "$f" | tr -d ' ')
  if [ "$lines" -gt "$LIMIT" ]; then
    if is_grandfathered "$f"; then
      echo "存量超限（销号清单内，不计失败）: $f (${lines} 行)"
    else
      echo "超限新增文件: $f (${lines} 行 > ${LIMIT} 上限，先拆分再合入)"
      fail=1
    fi
  fi
done < <(git diff --name-only --diff-filter=A "${BASE}...HEAD" -- '*.rs' '*.ts' '*.tsx')
if [ "$fail" -eq 0 ]; then
  echo "文件规模闸门通过（新增文件均 ≤ ${LIMIT} 行）"
fi
exit "$fail"
