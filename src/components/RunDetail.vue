<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { api } from '@/lib/api'
import { loadRecentRuns, notify, refreshRunDetail, selectRun, useStore } from '@/store'
import {
  elapsedOf,
  errorMessage,
  formatTime,
  isActiveStatus,
  statusClass,
  statusLabel,
  triggerModeLabel,
} from '@/lib/format'
import StatusBadge from './StatusBadge.vue'
import JobLog from './JobLog.vue'

const { state } = useStore()

const activeJob = ref<number | null>(null)
const confirming = ref('')
const showLaunch = ref(false)
const launching = ref(false)
const launchParams = ref('')
let detailTimer = 0

const run = computed(() => state.runDetail)
const pipelineName = computed(
  () => state.pipelines.find((p) => p.pipelineId === state.selectedPipelineId)?.name ?? '流水线',
)

const stages = computed(() => run.value?.stages ?? [])

const allJobs = computed(() => stages.value.flatMap((s) => s.jobs))

const isRunning = computed(() => isActiveStatus(run.value?.status))

// 运行中的实例每 5s 拉一次详情，让阶段/任务状态跟随；结束后自动停止
watch(
  () => [state.selectedRunId, isRunning.value] as const,
  ([rid, running]) => {
    window.clearInterval(detailTimer)
    if (!rid || !running) return
    detailTimer = window.setInterval(() => {
      void refreshRunDetail()
    }, 5000)
  },
  { immediate: true },
)

// 切换实例时默认选中第一个运行中或失败的任务，直接看日志
watch(
  () => state.selectedRunId,
  () => {
    activeJob.value = null
    confirming.value = ''
  },
)

watch(
  () => state.runDetail,
  (detail) => {
    if (!detail || activeJob.value !== null) return
    const jobs = detail.stages.flatMap((s) => s.jobs)
    const preferred =
      jobs.find((j) => isActiveStatus(j.status)) ?? jobs.find((j) => j.status === 'FAIL') ?? jobs[0]
    if (preferred) activeJob.value = preferred.jobId
  },
)

onBeforeUnmount(() => window.clearInterval(detailTimer))

function pickJob(jobId: number): void {
  activeJob.value = activeJob.value === jobId ? null : jobId
}

/** 统一处理写操作的成功 / 失败反馈 */
async function guarded(label: string, fn: () => Promise<unknown>): Promise<void> {
  try {
    await fn()
    notify('success', `${label}成功`)
    await refreshRunDetail()
  } catch (e) {
    notify('error', `${label}失败：${e instanceof Error ? e.message : String(e)}`, 9000)
  } finally {
    confirming.value = ''
  }
}

function askStopRun(): void {
  const rid = state.selectedRunId
  const pid = state.selectedPipelineId
  if (!rid || !pid) return
  confirming.value = `stop-run-${rid}`
}

function doStopRun(): void {
  const rid = state.selectedRunId
  const pid = state.selectedPipelineId
  if (!rid || !pid) return
  void guarded('终止运行', () => api.stopRun(pid, rid))
}

function doJobAction(action: 'start' | 'stop' | 'skip', jobId: number): void {
  const rid = state.selectedRunId
  const pid = state.selectedPipelineId
  if (!rid || !pid) return
  const label = { start: '重跑任务', stop: '终止任务', skip: '跳过任务' }[action]
  void guarded(label, () => {
    if (action === 'start') return api.startJob(pid, rid, jobId)
    if (action === 'stop') return api.stopJob(pid, rid, jobId)
    return api.skipJob(pid, rid, jobId)
  })
}

async function doLaunch(): Promise<void> {
  const pid = state.selectedPipelineId
  if (!pid) return

  let params: unknown = null
  const raw = launchParams.value.trim()
  if (raw) {
    try {
      params = JSON.parse(raw)
    } catch {
      notify('error', '运行参数不是合法 JSON，请检查后再试')
      return
    }
  }

  launching.value = true
  try {
    const runId = await api.startRun(pid, params)
    notify('success', `已触发运行，实例 #${runId}`)
    showLaunch.value = false
    launchParams.value = ''
    // 触发后展开该流水线的运行历史并选中新实例
    state.expanded = { ...state.expanded, [pid]: true }
    await loadRecentRuns(pid)
    await selectRun(runId)
  } catch (e) {
    notify('error', `触发失败：${errorMessage(e)}`, 9000)
  } finally {
    launching.value = false
  }
}

function jobDuration(j: { startTime?: number; endTime?: number; status: string }): string {
  if (!j.startTime) return ''
  return isActiveStatus(j.status)
    ? elapsedOf(j.startTime)
    : elapsedOf(j.startTime, j.endTime)
}
</script>

<template>
  <section class="pane detail">
    <template v-if="!state.selectedRunId">
      <div class="empty detail-empty">
        <template v-if="state.runDetailLoading">正在加载运行详情…</template>
        <template v-else-if="state.selectedPipelineId">
          该流水线暂无运行记录<br />
          <button class="btn btn-xs btn-primary" style="margin-top: 10px" @click="showLaunch = true">
            运行流水线
          </button>
        </template>
        <template v-else>
          选择左侧流水线<br />
          详情会自动展示它最近一次运行；<br />
          展开流水线可以看到最近 3 次运行历史
        </template>
      </div>
    </template>

    <template v-else>
      <div class="pane-head detail-head">
        <div class="head-main">
          <div class="head-title">
            <span class="mono">#{{ state.selectedRunId }}</span>
            <span class="pname">{{ pipelineName }}</span>
            <StatusBadge :status="run?.status" />
          </div>
          <div class="head-meta">
            <span>{{ triggerModeLabel(run?.triggerMode) }}</span>
            <span>开始 {{ formatTime(run?.startTime ?? run?.createTime) }}</span>
            <span>
              耗时
              <b class="mono">{{
                isRunning
                  ? elapsedOf(run?.startTime ?? run?.createTime)
                  : elapsedOf(run?.startTime ?? run?.createTime, run?.endTime)
              }}</b>
            </span>
            <span v-if="run?.sources?.length">
              分支
              <b class="mono">{{ run.sources.map((s) => s.branch).filter(Boolean).join(', ') }}</b>
            </span>
            <span v-if="run?.sources?.[0]?.commitId" class="mono">
              {{ run.sources[0].commitId.slice(0, 8) }}
            </span>
          </div>
        </div>
        <div class="spacer"></div>
        <div class="head-actions">
          <button
            class="btn btn-xs btn-primary"
            :disabled="!state.selectedPipelineId || launching"
            title="触发一次新的流水线运行"
            @click="showLaunch = true"
          >
            运行
          </button>
          <button class="btn btn-xs" :disabled="state.busy.runDetail" @click="refreshRunDetail">
            刷新
          </button>
          <button
            v-if="isRunning"
            class="btn btn-xs btn-danger"
            @click="confirming === `stop-run-${state.selectedRunId}` ? doStopRun() : askStopRun()"
          >
            {{ confirming === `stop-run-${state.selectedRunId}` ? '确认终止？' : '终止运行' }}
          </button>
        </div>
      </div>

      <div v-if="state.detailError" class="error-inline">{{ state.detailError }}</div>

      <div class="detail-body">
        <div v-if="state.runDetailLoading && !run" class="empty">正在加载运行详情…</div>

        <template v-else-if="run">
          <div class="stages">
            <div v-if="!stages.length" class="empty">
              该接口未返回阶段拓扑信息。<br />
              可能原因：流水线尚未开始调度，或组织返回结构较旧。
            </div>

            <div v-for="(stage, si) in stages" :key="`${si}-${stage.name}`" class="stage">
              <div class="stage-head" :class="statusClass(stage.status)">
                <span class="stage-index">{{ si + 1 }}</span>
                <span class="stage-name">{{ stage.name }}</span>
                <StatusBadge :status="stage.status" :dot="false" />
                <span class="stage-dur mono">
                  {{ elapsedOf(stage.startTime, stage.endTime) }}
                </span>
              </div>

              <div class="jobs">
                <div
                  v-for="job in stage.jobs"
                  :key="job.jobId"
                  class="job"
                  :class="[statusClass(job.status), { active: activeJob === job.jobId }]"
                >
                  <button class="job-main" @click="pickJob(job.jobId)">
                    <span class="job-dot"></span>
                    <span class="job-name">{{ job.name }}</span>
                    <span class="job-status">{{ statusLabel(job.status) }}</span>
                    <span class="job-dur mono">{{ jobDuration(job) }}</span>
                  </button>

                  <div class="job-actions">
                    <button
                      class="btn btn-xs btn-ghost"
                      title="ExecutePipelineJobRun：手动运行 / 重跑该任务"
                      @click.stop="doJobAction('start', job.jobId)"
                    >
                      重跑
                    </button>
                    <button
                      v-if="isActiveStatus(job.status)"
                      class="btn btn-xs btn-ghost"
                      title="StopPipelineJobRun：终止该任务"
                      @click.stop="doJobAction('stop', job.jobId)"
                    >
                      终止
                    </button>
                    <button
                      class="btn btn-xs btn-ghost"
                      title="SkipPipelineJobRun：跳过该任务"
                      @click.stop="doJobAction('skip', job.jobId)"
                    >
                      跳过
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <JobLog
            v-if="activeJob !== null"
            :key="`${state.selectedRunId}-${activeJob}`"
            :pipeline-id="run.pipelineId || state.selectedPipelineId || 0"
            :pipeline-run-id="state.selectedRunId"
            :job-id="activeJob"
            :job-name="allJobs.find((j) => j.jobId === activeJob)?.name ?? `任务 ${activeJob}`"
            :job-active="isActiveStatus(allJobs.find((j) => j.jobId === activeJob)?.status)"
            @close="activeJob = null"
          />
        </template>
      </div>
    </template>

    <Teleport to="body">
      <div v-if="showLaunch" class="modal-mask" @click.self="showLaunch = false">
        <div class="modal" style="width: min(560px, 100%)">
          <div class="modal-head">
            运行流水线：{{ pipelineName }}
            <div class="spacer"></div>
            <button class="btn btn-xs btn-ghost" @click="showLaunch = false">✕</button>
          </div>
          <div class="modal-body">
            <div class="field">
              <label for="launch-params">运行参数（JSON，可留空使用默认配置）</label>
              <textarea
                id="launch-params"
                v-model="launchParams"
                class="input mono params"
                spellcheck="false"
                placeholder='{&#10;  "runningBranchs": { "https://codeup.aliyun.com/xxx/demo.git": "master" },&#10;  "envs": { "KEY": "VALUE" },&#10;  "comment": "手动触发"&#10;}'
              ></textarea>
              <div class="hint">
                对应云效 CreatePipelineRun 的 <code>params</code> 字段，支持
                <code>branchModeBranchs</code>、<code>envs</code>、<code>runningBranchs</code>、
                <code>runningTags</code>、<code>comment</code>。留空即以 <code>{}</code> 触发。
              </div>
            </div>
          </div>
          <div class="modal-foot">
            <button class="btn" @click="showLaunch = false">取消</button>
            <button class="btn btn-primary" :disabled="launching" @click="doLaunch">
              {{ launching ? '触发中…' : '确认运行' }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </section>
</template>

<style scoped>
.detail {
  background: var(--bg);
}

.detail-empty {
  margin: auto;
}

.detail-head {
  align-items: flex-start;
  gap: 12px;
  padding: 10px 14px;
}

.head-main {
  min-width: 0;
  flex: 1;
}

.head-title {
  display: flex;
  align-items: center;
  gap: 9px;
  font-size: 14px;
}

.head-title .mono {
  font-weight: 700;
  color: var(--accent);
}

.pname {
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.head-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 14px;
  margin-top: 5px;
  color: var(--text-dim);
  font-size: 11.5px;
}

.head-meta b {
  color: var(--text-muted);
  font-weight: 500;
}

.head-actions {
  display: flex;
  gap: 6px;
  flex: none;
}

.detail-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

.stages {
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow-y: auto;
  flex: none;
  max-height: 46%;
}

.stage {
  border: 1px solid var(--border-soft);
  border-radius: var(--radius);
  background: var(--bg-panel);
  overflow: hidden;
}

.stage-head {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 7px 11px;
  border-bottom: 1px solid var(--border-soft);
  border-left: 3px solid var(--st, var(--border));
}

.stage-index {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--bg-elevated);
  color: var(--text-muted);
  display: grid;
  place-items: center;
  font-size: 11px;
  flex: none;
}

.stage-name {
  font-weight: 600;
  font-size: 12.5px;
}

.stage-dur {
  margin-left: auto;
  color: var(--text-dim);
  font-size: 11px;
}

.jobs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding: 10px 11px;
}

.job {
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg-elevated);
  min-width: 210px;
  flex: 1 1 240px;
  max-width: 420px;
  overflow: hidden;
}

.job.active {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--accent-soft);
}

.job-main {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  background: transparent;
  border: none;
  text-align: left;
}

.job-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--st, var(--text-dim));
  flex: none;
}

.st-running .job-dot {
  animation: pulse 1.3s ease-in-out infinite;
}

.job-name {
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.job-status {
  color: var(--st, var(--text-muted));
  font-size: 11.5px;
  flex: none;
}

.job-dur {
  color: var(--text-dim);
  font-size: 11px;
  flex: none;
}

.job-actions {
  display: flex;
  gap: 4px;
  padding: 0 8px 7px;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.3;
  }
}

.params {
  min-height: 132px;
  resize: vertical;
  line-height: 1.6;
  user-select: text;
}

code {
  background: var(--bg-elevated);
  padding: 1px 4px;
  border-radius: 3px;
  font-family: var(--mono);
  font-size: 11.5px;
}
</style>
