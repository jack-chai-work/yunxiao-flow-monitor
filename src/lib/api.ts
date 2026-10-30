import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AppErrorPayload,
  ConfigDto,
  ConfigInput,
  ConnectionTestResult,
  JobSteps,
  LogChunk,
  PipelinePage,
  PipelineSummary,
  PollSnapshot,
  RunPage,
  RunSummary,
  StatusChange,
} from '@/types'

/**
 * 判断当前是否运行在 Tauri 容器内。
 * 纯 `npm run dev` 打开浏览器时没有 IPC，用它给出可读提示而不是抛异常。
 */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

/** 把 Rust 侧返回的错误统一成 Error，message 直接可展示。 */
function normalize(e: unknown): Error {
  if (e && typeof e === 'object') {
    const p = e as Partial<AppErrorPayload>
    if (typeof p.message === 'string') return new Error(p.message)
  }
  return new Error(String(e))
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new Error('当前不在桌面应用中运行，请通过 npm run app:dev 启动')
  }
  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    throw normalize(e)
  }
}

export const api = {
  // 配置
  getConfig: () => call<ConfigDto>('get_config'),
  saveConfig: (input: ConfigInput) => call<ConfigDto>('save_config', { input }),
  testConnection: () => call<ConnectionTestResult>('test_connection'),

  // 开机自启（写入操作系统的登录启动项）
  getAutostart: () => call<boolean>('get_autostart'),
  setAutostart: (enabled: boolean) => call<boolean>('set_autostart', { enabled }),

  // 只读查询
  listPipelines: (opts: {
    page?: number
    perPage?: number
    name?: string
    statusList?: string
    withLatest?: boolean
  }) => call<PipelinePage>('list_pipelines', opts),

  getPipeline: (pipelineId: number) =>
    call<Record<string, unknown>>('get_pipeline', { pipelineId }),

  listRuns: (opts: {
    pipelineId: number
    page?: number
    perPage?: number
    status?: string
    triggerMode?: number
  }) => call<RunPage>('list_runs', opts),

  getRun: (pipelineId: number, pipelineRunId: number) =>
    call<RunSummary>('get_run', { pipelineId, pipelineRunId }),

  getLatestRun: (pipelineId: number) => call<RunSummary>('get_latest_run', { pipelineId }),

  getJobSteps: (pipelineId: number, pipelineRunId: number, jobId: number) =>
    call<JobSteps>('get_job_steps', { pipelineId, pipelineRunId, jobId }),

  getJobStepLog: (opts: {
    pipelineId: number
    pipelineRunId: number
    jobId: number
    stepIndex: number
    buildId: number
    offset?: number
    limit?: number
  }) => call<LogChunk>('get_job_step_log', opts),

  getJobStepLogUrl: (opts: {
    pipelineId: number
    pipelineRunId: number
    jobId: number
    stepIndex: number
    buildId: number
  }) => call<unknown>('get_job_step_log_url', opts),

  // 运行控制
  startRun: (pipelineId: number, params?: unknown) =>
    call<number>('start_run', { pipelineId, params: params ?? null }),
  stopRun: (pipelineId: number, pipelineRunId: number) =>
    call<boolean>('stop_run', { pipelineId, pipelineRunId }),
  startJob: (pipelineId: number, pipelineRunId: number, jobId: number) =>
    call<boolean>('start_job', { pipelineId, pipelineRunId, jobId }),
  stopJob: (pipelineId: number, pipelineRunId: number, jobId: number) =>
    call<boolean>('stop_job', { pipelineId, pipelineRunId, jobId }),
  skipJob: (pipelineId: number, pipelineRunId: number, jobId: number) =>
    call<boolean>('skip_job', { pipelineId, pipelineRunId, jobId }),

  // 轮询与关注
  setPolling: (enabled: boolean) => call<boolean>('set_polling', { enabled }),
  getPolling: () => call<boolean>('get_polling'),
  refreshNow: () => call<void>('refresh_now'),
  getSnapshot: () => call<PipelineSummary[]>('get_snapshot'),
  toggleWatch: (pipelineId: number) => call<number[]>('toggle_watch', { pipelineId }),
}

/** 后端事件名，与 `src-tauri/src/poller.rs` 保持一致。 */
export const EVENTS = {
  snapshot: 'yunxiao://snapshot',
  statusChanged: 'yunxiao://status-changed',
  pollError: 'yunxiao://poll-error',
  pollState: 'yunxiao://poll-state',
} as const

export function onSnapshot(cb: (p: PollSnapshot) => void): Promise<UnlistenFn> {
  return listen<PollSnapshot>(EVENTS.snapshot, (e) => cb(e.payload))
}

export function onStatusChanged(cb: (p: StatusChange) => void): Promise<UnlistenFn> {
  return listen<StatusChange>(EVENTS.statusChanged, (e) => cb(e.payload))
}

export function onPollError(cb: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>(EVENTS.pollError, (e) => cb(e.payload))
}

export function onPollState(cb: (running: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>(EVENTS.pollState, (e) => cb(e.payload))
}
