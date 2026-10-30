import type { RunStatus } from '@/types'

/** 状态 → 中文标签。与 Rust 侧 `poller::status_label` 保持一致。 */
export function statusLabel(status?: string | null): string {
  switch ((status ?? '').toUpperCase()) {
    case 'SUCCESS':
      return '运行成功'
    case 'FAIL':
    case 'FAILED':
    case 'ERROR':
      return '运行失败'
    case 'RUNNING':
      return '运行中'
    case 'WAITING':
      return '等待中'
    case 'CANCELED':
    case 'CANCELLED':
      return '已取消'
    case 'INIT':
      return '初始化'
    case 'QUEUED':
    case 'PENDING':
    case 'READY':
      return '排队中'
    case '':
    case 'UNKNOWN':
      return '未知'
    default:
      return String(status)
  }
}

/** 状态 → CSS 类名（配色见 styles/main.css）。 */
export function statusClass(status?: string | null): string {
  switch ((status ?? '').toUpperCase()) {
    case 'SUCCESS':
      return 'st-success'
    case 'FAIL':
    case 'FAILED':
    case 'ERROR':
      return 'st-fail'
    case 'RUNNING':
      return 'st-running'
    case 'WAITING':
      return 'st-waiting'
    case 'CANCELED':
    case 'CANCELLED':
      return 'st-canceled'
    case 'INIT':
    case 'QUEUED':
    case 'PENDING':
    case 'READY':
      return 'st-queued'
    default:
      return 'st-unknown'
  }
}

/** 是否处于「尚未结束」的状态，决定要不要继续追日志。 */
export function isActiveStatus(status?: string | null): boolean {
  switch ((status ?? '').toUpperCase()) {
    case 'RUNNING':
    case 'WAITING':
    case 'INIT':
    case 'QUEUED':
    case 'PENDING':
    case 'READY':
      return true
    default:
      return false
  }
}

export function isRunStatus(s: string): s is RunStatus {
  return typeof s === 'string'
}

/** 触发方式：1 人工 2 定时 3 代码提交 5 流水线触发 6 WEBHOOK。 */
export function triggerModeLabel(mode?: number | null): string {
  switch (mode) {
    case 1:
      return '人工触发'
    case 2:
      return '定时触发'
    case 3:
      return '代码提交'
    case 4:
      return 'API 触发'
    case 5:
      return '流水线触发'
    case 6:
      return 'Webhook'
    case undefined:
    case null:
      return '—'
    default:
      return `方式 ${mode}`
  }
}

const pad = (n: number) => String(n).padStart(2, '0')

/** 毫秒时间戳 → `YYYY-MM-DD HH:mm:ss`（本地时区）。 */
export function formatTime(ms?: number | null): string {
  if (!ms) return '—'
  const d = new Date(ms)
  if (Number.isNaN(d.getTime())) return '—'
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(
    d.getMinutes(),
  )}:${pad(d.getSeconds())}`
}

/** 毫秒时间戳 → `HH:mm:ss`。 */
export function formatClock(ms?: number | null): string {
  if (!ms) return '—'
  const d = new Date(ms)
  if (Number.isNaN(d.getTime())) return '—'
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

/** 相对时间：刚刚 / x 分钟前 / x 小时前 / 具体日期。 */
export function formatRelative(ms?: number | null): string {
  if (!ms) return '—'
  const diff = Date.now() - ms
  if (diff < 0) return formatTime(ms)
  const sec = Math.floor(diff / 1000)
  if (sec < 10) return '刚刚'
  if (sec < 60) return `${sec} 秒前`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min} 分钟前`
  const hour = Math.floor(min / 60)
  if (hour < 24) return `${hour} 小时前`
  const day = Math.floor(hour / 24)
  if (day < 7) return `${day} 天前`
  return formatTime(ms)
}

/** 时长：`1h 02m 03s` / `2m 03s` / `3s`。 */
export function formatDuration(ms?: number | null): string {
  if (!ms || ms < 0) return '—'
  const total = Math.floor(ms / 1000)
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  if (h > 0) return `${h}h ${pad(m)}m ${pad(s)}s`
  if (m > 0) return `${m}m ${pad(s)}s`
  return `${s}s`
}

/** 运行中的实例按「当前时间 - 开始时间」实时计算耗时。 */
export function elapsedOf(start?: number | null, end?: number | null, now = Date.now()): string {
  if (!start) return '—'
  return formatDuration((end ?? now) - start)
}

/** 字节数 → 人类可读。 */
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(2)} MB`
}

/** 把云效返回的错误对象/字符串统一成可展示文本。 */
export function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message
  if (typeof e === 'string') return e
  try {
    return JSON.stringify(e)
  } catch {
    return String(e)
  }
}
