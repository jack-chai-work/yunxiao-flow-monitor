/** 与 Rust 侧 `src-tauri/src/api/models.rs` 的 serde 结构一一对应（camelCase）。 */

export type RunStatus =
  | 'SUCCESS'
  | 'FAIL'
  | 'RUNNING'
  | 'WAITING'
  | 'CANCELED'
  | 'INIT'
  | 'UNKNOWN'
  | string

export interface SourceInfo {
  sign?: string
  repoType?: string
  repo?: string
  branch?: string
  commitId?: string
  commitMessage?: string
}

export interface ActionInfo {
  type: string
  disable: boolean
  params?: unknown
}

export interface JobInfo {
  jobId: number
  name: string
  status: RunStatus
  startTime?: number
  endTime?: number
  actions: ActionInfo[]
}

export interface StageInfo {
  name: string
  status: RunStatus
  startTime?: number
  endTime?: number
  jobs: JobInfo[]
}

export interface RunSummary {
  pipelineId: number
  pipelineRunId: number
  status: RunStatus
  triggerMode?: number
  creatorAccountId?: string
  createTime?: number
  startTime?: number
  endTime?: number
  updateTime?: number
  sources: SourceInfo[]
  stages: StageInfo[]
  comment?: string
}

export interface PipelineSummary {
  pipelineId: number
  name: string
  creatorAccountId?: string
  createTime?: number
  groupId?: number
  groupName?: string
  envName?: string
  pipelineType?: string
  latestRun?: RunSummary
  watched: boolean
}

export interface StepInfo {
  stepIndex: number
  stepName: string
  nodeName?: string
  status: RunStatus
  running: boolean
  finish: boolean
  supportDebug: boolean
}

export interface JobSteps {
  jobId: number
  buildId?: number
  actionCode?: string
  actionName?: string
  steps: StepInfo[]
}

export interface LogChunk {
  logs: string
  last: number
  more: boolean
}

export interface PollSnapshot {
  pipelines: PipelineSummary[]
  fetchedAt: number
  runningCount: number
  failedCount: number
  successCount: number
  intervalSecs: number
}

export interface StatusChange {
  pipelineId: number
  pipelineName: string
  pipelineRunId: number
  fromStatus: RunStatus
  toStatus: RunStatus
  at: number
}

export interface ConfigDto {
  baseUrl: string
  organizationId: string
  pollIntervalSecs: number
  fastPollIntervalSecs: number
  watchedPipelineIds: number[]
  autoWatchLimit: number
  autoStartPolling: boolean
  notifyOnSuccess: boolean
  notifyOnFail: boolean
  notifyOnCanceled: boolean
  minimizeToTrayOnClose: boolean
  hasToken: boolean
  tokenMasked: string
  secureStoreEnabled: boolean
}

export interface ConfigInput {
  baseUrl: string
  organizationId: string
  pollIntervalSecs: number
  fastPollIntervalSecs: number
  watchedPipelineIds: number[]
  autoWatchLimit: number
  autoStartPolling: boolean
  notifyOnSuccess: boolean
  notifyOnFail: boolean
  notifyOnCanceled: boolean
  minimizeToTrayOnClose: boolean
  /** 空字符串 = 保留原令牌；`__CLEAR__` = 清除令牌 */
  token: string
}

export interface PipelinePage {
  items: PipelineSummary[]
  total: number | null
  page: number
  perPage: number
}

export interface RunPage {
  items: RunSummary[]
  total: number | null
  page: number
  perPage: number
}

export interface ConnectionTestResult {
  ok: boolean
  organizationId: string
  samplePipelineName?: string | null
  message: string
}

/** Rust 侧 `AppError` 的序列化形态。 */
export interface AppErrorPayload {
  code: string
  message: string
}
