import { reactive } from 'vue'
import { api, isTauri } from '@/lib/api'
import { errorMessage } from '@/lib/format'
import type {
  ConfigDto,
  PipelineSummary,
  PollSnapshot,
  RunSummary,
  StatusChange,
} from '@/types'

export interface Notice {
  id: number
  kind: 'info' | 'success' | 'error' | 'warn'
  text: string
  at: number
}

interface StoreState {
  ready: boolean
  inTauri: boolean
  config: ConfigDto | null
  polling: boolean
  lastFetchedAt: number
  intervalSecs: number
  runningCount: number
  failedCount: number
  successCount: number
  pipelines: PipelineSummary[]
  pipelinesLoading: boolean
  keyword: string
  statusFilter: string
  page: number
  perPage: number
  total: number | null
  selectedPipelineId: number | null
  /** 每条流水线最近三次运行（展开时拉取并缓存） */
  recentRuns: Record<number, RunSummary[]>
  recentLoading: Record<number, boolean>
  /** 流水线展开状态，默认不展开 */
  expanded: Record<number, boolean>
  selectedRunId: number | null
  runDetail: RunSummary | null
  runDetailLoading: boolean
  notices: Notice[]
  busy: Record<string, boolean>
  settingsOpen: boolean
  detailError: string
}

const state = reactive<StoreState>({
  ready: false,
  inTauri: isTauri(),
  config: null,
  polling: false,
  lastFetchedAt: 0,
  intervalSecs: 0,
  runningCount: 0,
  failedCount: 0,
  successCount: 0,
  pipelines: [],
  pipelinesLoading: false,
  keyword: '',
  statusFilter: 'ALL',
  page: 1,
  perPage: 30,
  total: null,
  selectedPipelineId: null,
  recentRuns: {},
  recentLoading: {},
  expanded: {},
  selectedRunId: null,
  runDetail: null,
  runDetailLoading: false,
  notices: [],
  busy: {},
  settingsOpen: false,
  detailError: '',
})

let noticeSeq = 0
/** 详情请求序号：快速切换流水线/运行时只让最后一次请求落地 */
let runRequestSeq = 0

/**
 * 全局单一状态源。
 *
 * 直接返回 reactive 对象（而非 readonly 包装）：组件里存在 `v-model` 与
 * `state.page = 1` 这类就地写入，readonly 代理会在运行时静默拒绝写入。
 */
export function useStore(): { state: StoreState } {
  return { state }
}

export function notify(kind: Notice['kind'], text: string, ttl = 6000): void {
  const id = ++noticeSeq
  state.notices.push({ id, kind, text, at: Date.now() })
  // 只保留最近 30 条，避免长时间运行后内存膨胀
  if (state.notices.length > 30) state.notices.splice(0, state.notices.length - 30)
  if (ttl > 0) {
    window.setTimeout(() => dismissNotice(id), ttl)
  }
}

export function dismissNotice(id: number): void {
  const i = state.notices.findIndex((n) => n.id === id)
  if (i >= 0) state.notices.splice(i, 1)
}

function setBusy(key: string, value: boolean): void {
  state.busy = { ...state.busy, [key]: value }
}

/** 包装一次后端调用：统一 loading、错误提示。 */
async function guard<T>(key: string, fn: () => Promise<T>, silent = false): Promise<T | null> {
  setBusy(key, true)
  try {
    return await fn()
  } catch (e) {
    const msg = errorMessage(e)
    if (!silent) notify('error', msg, 9000)
    return null
  } finally {
    setBusy(key, false)
  }
}

// ---------------------------------------------------------------------------
// 动作
// ---------------------------------------------------------------------------

export async function loadConfig(): Promise<void> {
  const cfg = await guard('config', () => api.getConfig(), true)
  if (cfg) state.config = cfg
}

export async function loadPolling(): Promise<void> {
  const v = await guard('polling', () => api.getPolling(), true)
  if (typeof v === 'boolean') state.polling = v
}

export async function setPolling(enabled: boolean): Promise<void> {
  const v = await guard('polling', () => api.setPolling(enabled))
  state.polling = v ?? enabled
}

export async function refreshNow(): Promise<void> {
  await guard('refresh', () => api.refreshNow(), true)
  // 轮询关闭时后端不会推送快照，这里主动拉一次列表兜底
  if (!state.polling) await loadPipelines()
}

export async function loadPipelines(): Promise<void> {
  state.pipelinesLoading = true
  const res = await guard('pipelines', () =>
    api.listPipelines({
      page: state.page,
      perPage: state.perPage,
      name: state.keyword || undefined,
      statusList: state.statusFilter === 'ALL' ? undefined : state.statusFilter,
      withLatest: true,
    }),
  )
  state.pipelinesLoading = false
  if (res) {
    state.pipelines = res.items
    state.total = res.total
  }
}

export function applySnapshot(snapshot: PollSnapshot): void {
  // 搜索 / 状态筛选生效时不用快照覆盖列表，避免用户正在筛选却被刷掉
  if (state.keyword.trim() || state.statusFilter !== 'ALL') {
    // 但仍然更新已存在条目的最近运行状态
    const map = new Map(snapshot.pipelines.map((p) => [p.pipelineId, p]))
    state.pipelines = state.pipelines.map((p) => map.get(p.pipelineId) ?? p)
  } else {
    state.pipelines = snapshot.pipelines
    state.total = snapshot.pipelines.length
  }
  state.lastFetchedAt = snapshot.fetchedAt
  state.intervalSecs = snapshot.intervalSecs
  state.runningCount = snapshot.runningCount
  state.failedCount = snapshot.failedCount
  state.successCount = snapshot.successCount
}

/** 点击流水线：详情展示它最近一次运行，运行历史默认保持折叠 */
export async function selectPipeline(id: number): Promise<void> {
  runRequestSeq += 1
  state.selectedPipelineId = id
  state.selectedRunId = null
  state.runDetail = null
  state.runDetailLoading = false
  state.detailError = ''
  await loadRecentRuns(id)
  if (state.selectedPipelineId !== id) return // 期间又点了别的流水线
  const latest =
    state.recentRuns[id]?.[0]?.pipelineRunId ??
    state.pipelines.find((p) => p.pipelineId === id)?.latestRun?.pipelineRunId
  if (latest) {
    await selectRun(latest)
  } else {
    // 列表里没有最近运行时，再单独查一次，确保「点流水线看最近一次」
    try {
      const run = await api.getLatestRun(id)
      if (state.selectedPipelineId === id && run?.pipelineRunId) await selectRun(run.pipelineRunId)
    } catch {
      /* 没有运行记录属正常情况 */
    }
  }
}

/** 展开/收起某条流水线的运行历史 */
export async function togglePipelineExpand(id: number): Promise<void> {
  const open = !state.expanded[id]
  state.expanded = { ...state.expanded, [id]: open }
  if (open) await loadRecentRuns(id)
}

/** 拉取某条流水线最近三次运行，作为流水线的子项展示 */
export async function loadRecentRuns(id: number, silent = false): Promise<void> {
  state.recentLoading = { ...state.recentLoading, [id]: true }
  try {
    const res = await api.listRuns({ pipelineId: id, page: 1, perPage: 3 })
    state.recentRuns = { ...state.recentRuns, [id]: (res.items ?? []).slice(0, 3) }
  } catch (e) {
    if (!silent) notify('error', `获取运行历史失败：${errorMessage(e)}`, 6000)
  } finally {
    state.recentLoading = { ...state.recentLoading, [id]: false }
  }
}

/** 选中某条流水线下的某次运行（侧栏子项点击） */
export async function selectPipelineRun(pipelineId: number, runId: number): Promise<void> {
  state.selectedPipelineId = pipelineId
  await selectRun(runId)
}

export async function selectRun(runId: number): Promise<void> {
  const pid = state.selectedPipelineId
  if (!pid) return
  const seq = ++runRequestSeq
  state.selectedRunId = runId
  state.runDetailLoading = true
  state.detailError = ''
  const run = await guard('runDetail', () => api.getRun(pid, runId))
  if (seq !== runRequestSeq) return // 期间已切到别的流水线/运行，丢弃本次结果
  state.runDetailLoading = false
  if (run) {
    state.runDetail = run
    state.selectedRunId = run.pipelineRunId || runId
  } else {
    state.runDetail = null
  }
}

/** 详情面板打开时静默刷新，保持阶段/任务状态跟随。 */
export async function refreshRunDetail(): Promise<void> {
  const pid = state.selectedPipelineId
  const rid = state.selectedRunId
  if (!pid || !rid) return
  const run = await guard('runDetail', () => api.getRun(pid, rid), true)
  if (run) state.runDetail = run
}

export async function toggleWatch(id: number): Promise<void> {
  const ids = await guard('watch', () => api.toggleWatch(id))
  if (ids && state.config) {
    state.config = { ...state.config, watchedPipelineIds: ids }
    state.pipelines = state.pipelines.map((p) => ({ ...p, watched: ids.includes(p.pipelineId) }))
    notify('info', ids.includes(id) ? '已加入关注，将进入实时轮询' : '已取消关注')
  }
}

export function handleStatusChange(change: StatusChange): void {
  const label = `${change.pipelineName} #${change.pipelineRunId}`
  const to = change.toStatus.toUpperCase()
  if (to === 'SUCCESS') notify('success', `${label} 运行成功`)
  else if (to === 'FAIL' || to === 'FAILED' || to === 'ERROR') notify('error', `${label} 运行失败`, 12000)
  else if (to === 'CANCELED' || to === 'CANCELLED') notify('warn', `${label} 已取消`)
  else notify('info', `${label} → ${change.toStatus}`, 4000)

  // 当前正在看这条流水线时，顺带刷新最近运行与详情
  if (state.selectedPipelineId === change.pipelineId) {
    void loadRecentRuns(change.pipelineId, true)
    if (state.selectedRunId === change.pipelineRunId) void refreshRunDetail()
  }
}
