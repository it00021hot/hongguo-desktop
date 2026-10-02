/** 体积格式化：字节 → 可读字符串。 */
export function formatBytes(bytes: number, decimals = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** i;
  return `${value.toFixed(i === 0 ? 0 : decimals)} ${units[i]}`;
}

/** 秒 → 时分秒。 */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '00:00';
  const total = Math.floor(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** 速率格式化。 */
export function formatSpeed(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond, 0)}/s`;
}

// 路径分隔符、Windows 保留字符，以及控制字符（0x00-0x1f）。
// 用构造函数逐个判断而不是正则：字符集里含控制字符，字面量写出来会触发
// no-control-regex，读起来也不直观。
const ILLEGAL_CHARS = new Set([
  String.fromCharCode(92), // 反斜杠
  '/',
  ':',
  '*',
  '?',
  String.fromCharCode(34), // 双引号
  '<',
  '>',
  '|',
]);

function isIllegal(ch: string): boolean {
  const code = ch.charCodeAt(0);
  return ILLEGAL_CHARS.has(ch) || (code >= 0 && code <= 0x1f);
}

/** 清洗文件名，与 Rust 侧 `sanitize_file_name` 规则保持一致。 */
export function sanitizeFileName(name: string): string {
  const cleaned = [...name]
    .map((ch) => (isIllegal(ch) ? '_' : ch))
    .join('')
    .trim()
    .replace(/[. ]+$/, '');
  return cleaned || '未命名';
}

/** 清洗目录名。 */
export function sanitizeFolderName(name: string): string {
  return sanitizeFileName(name).slice(0, 80);
}

/** 按命名模板渲染文件名，与 Rust 侧 `Settings::render_file_name` 保持一致。 */
export function renderFileName(
  naming: 'titleIndex' | 'titleIndexEpisode' | 'onlyTitle',
  seriesTitle: string,
  vidIndex: number,
  epTitle: string,
): string {
  const index = String(vidIndex).padStart(3, '0');
  switch (naming) {
    case 'titleIndex':
      return sanitizeFileName(`${seriesTitle} ${index}`);
    case 'titleIndexEpisode':
      return epTitle.trim()
        ? sanitizeFileName(`${seriesTitle} ${index} ${epTitle}`)
        : sanitizeFileName(`${seriesTitle} ${index}`);
    case 'onlyTitle':
      return sanitizeFileName(seriesTitle);
  }
}
