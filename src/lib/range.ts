/**
 * 选集区间语法解析。
 *
 * 支持 `1-50`、`1-10, 25, 30-45`、`1, 3, 5`。空串返回空数组（不是全部），
 * 这样调用方能区分「没填」与「全选」。
 */
export function parseRange(input: string, max = Number.MAX_SAFE_INTEGER): number[] {
  const trimmed = input.trim();
  if (!trimmed) return [];

  const out = new Set<number>();
  for (const part of trimmed.split(/[,，]/)) {
    const seg = part.trim();
    if (!seg) continue;

    const dash = seg.match(/^(\d+)\s*-\s*(\d+)$/);
    if (dash) {
      const from = Number(dash[1]);
      const to = Number(dash[2]);
      if (from < 1 || to < from) continue;
      // 上限夹到 max，避免用户填 1-99999 把整部剧都选上
      const end = Math.min(to, max);
      for (let i = from; i <= end; i += 1) out.add(i);
      continue;
    }

    if (/^\d+$/.test(seg)) {
      const n = Number(seg);
      if (n >= 1 && n <= max) out.add(n);
    }
  }

  return [...out].sort((a, b) => a - b);
}

/** 把选中的集号序列化成紧凑区间串，如 `1-5, 8, 12-14`。 */
export function formatRange(indices: number[]): string {
  if (indices.length === 0) return '';
  const sorted = [...new Set(indices)].sort((a, b) => a - b);

  const parts: string[] = [];
  let start = sorted[0]!;
  let prev = sorted[0]!;

  for (const n of sorted.slice(1)) {
    if (n === prev + 1) {
      prev = n;
      continue;
    }
    parts.push(start === prev ? `${start}` : `${start}-${prev}`);
    start = n;
    prev = n;
  }
  parts.push(start === prev ? `${start}` : `${start}-${prev}`);
  return parts.join(', ');
}

/** 取前 n 集。 */
export function firstN(total: number, n: number): number[] {
  const count = Math.max(0, Math.min(n, total));
  return Array.from({ length: count }, (_, i) => i + 1);
}

/** 取后 n 集。 */
export function lastN(total: number, n: number): number[] {
  const count = Math.max(0, Math.min(n, total));
  return Array.from({ length: count }, (_, i) => total - count + i + 1);
}