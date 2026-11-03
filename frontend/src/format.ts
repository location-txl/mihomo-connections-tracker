/** 与后端 usage.rs 的 format_bytes 同口径：1024 进制，保留一位小数。 */
export function formatBytes(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
  if (bytes < 1024) return `${bytes} B`
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit++
  }
  return `${value.toFixed(1)} ${units[unit]}`
}

/** 机场维度 name 为 null 的行按 route_kind 显示中文标签。 */
export function routeKindLabel(routeKind: string | null): string {
  switch (routeKind) {
    case 'direct':
      return '直连'
    case 'reject':
      return '拒绝'
    case 'ambiguous':
      return '未明确归属'
    default:
      return ''
  }
}

/** YYYY-MM-DD，按浏览器本地时区。 */
export function toDateKey(date: Date): string {
  const month = `${date.getMonth() + 1}`.padStart(2, '0')
  const day = `${date.getDate()}`.padStart(2, '0')
  return `${date.getFullYear()}-${month}-${day}`
}

/** UTC 毫秒时间戳 → 本地 "MM-dd HH:mm"。 */
export function formatTime(ms: number): string {
  const date = new Date(ms)
  const pad = (value: number) => `${value}`.padStart(2, '0')
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

export function addDays(date: Date, days: number): Date {
  const next = new Date(date)
  next.setDate(next.getDate() + days)
  return next
}

export function monthStart(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), 1)
}

export function addMonths(date: Date, months: number): Date {
  return new Date(date.getFullYear(), date.getMonth() + months, 1)
}
