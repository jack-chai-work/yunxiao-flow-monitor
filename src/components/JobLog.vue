<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { api } from '@/lib/api'
import { notify } from '@/store'
import { parseAnsiLine, spanStyle, stripAnsi, type AnsiSpan } from '@/lib/ansi'
import { errorMessage, formatBytes } from '@/lib/format'
import type { JobSteps, StepInfo } from '@/types'

const props = defineProps<{
  pipelineId: number
  pipelineRunId: number
  jobId: number
  jobName: string
  /** 任务是否仍在运行，决定要不要继续增量拉日志 */
  jobActive: boolean
}>()

const emit = defineEmits<{ close: [] }>()

const stepsInfo = ref<JobSteps | null>(null)
const activeStep = ref<number | null>(null)
const loadingSteps = ref(false)
const loadingLog = ref(false)
const errorMsg = ref('')
const follow = ref(true)
/** 自动跟随到正在执行的步骤 */
const followStep = ref(true)
const wrap = ref(true)
const filter = ref('')
const raw = ref<string[]>([])
const box = ref<HTMLElement | null>(null)
const logUrl = ref('')

let timer = 0
/** 自动滚动的 requestAnimationFrame 句柄 */
let scrollRaf = 0
/** 并发拉取序号：快速切换步骤时只让最后一次请求的结果生效 */
let pullSeq = 0
/**
 * 云效日志接口的 offset 单位是「行号」（不是字符偏移），
 * limit 是本次最多返回的行数。这里记录下一次从第几行开始拉。
 */
let nextOffset = 0
/** 日志行数上限，超出后丢弃头部，避免长构建把内存打满 */
const MAX_LINES = 20000

const steps = computed<StepInfo[]>(() => stepsInfo.value?.steps ?? [])

const buildId = computed(() => stepsInfo.value?.buildId ?? 0)

/** 构建日志用 \r 刷同一行（进度条），展示时只保留最后一段，避免堆叠 */
function displayLine(text: string): string {
  const i = text.lastIndexOf('\r')
  return i >= 0 ? text.slice(i + 1) : text
}

type LogLevel = '' | 'error' | 'warn' | 'info' | 'debug' | 'success'

/** 各级别的强调色（整行淡色 + 级别标记本身用高亮色） */
const LEVEL_COLOR: Record<Exclude<LogLevel, ''>, string> = {
  error: '#f87171',
  warn: '#fbbf24',
  info: '#60a5fa',
  debug: '#9ca3af',
  success: '#4ade80',
}

const LEVEL_RE =
  /\[(ERROR|FAIL|FAILED|FATAL|WARN|WARNING|INFO|DEBUG|TRACE|SUCCESS|SUCCEED)\]|(?:^|\s)(ERROR|FAIL|FAILED|FATAL|WARN|WARNING|INFO|DEBUG|TRACE|SUCCESS|SUCCEED)\b/i

const LEVEL_MAP: Record<string, Exclude<LogLevel, ''>> = {
  error: 'error',
  fail: 'error',
  failed: 'error',
  fatal: 'error',
  warn: 'warn',
  warning: 'warn',
  info: 'info',
  debug: 'debug',
  trace: 'debug',
  success: 'success',
  succeed: 'success',
}

/** 识别日志级别：优先 [INFO]/[ERROR] 这类标记，其次常见行首关键字与构建结果 */
function detectLevel(text: string): LogLevel {
  const m = LEVEL_RE.exec(text)
  if (m) {
    const kw = (m[1] ?? m[2] ?? '').toLowerCase()
    return LEVEL_MAP[kw] ?? ''
  }
  if (/\bBUILD FAILED\b/i.test(text)) return 'error'
  if (/\bBUILD SUCCESS(?:FUL)?\b/i.test(text)) return 'success'
  if (/^\s*(error|fatal|exception)\b/i.test(text)) return 'error'
  if (/^\s*(warn|warning)\b/i.test(text)) return 'warn'
  return ''
}

/** 把行内的级别标记（[INFO] 等）单独染色加粗，其余 ANSI 样式保持不变 */
function paintLevelTag(spans: AnsiSpan[], level: LogLevel): AnsiSpan[] {
  if (!level) return spans
  for (let i = 0; i < spans.length; i++) {
    const span = spans[i]
    const m = LEVEL_RE.exec(span.text)
    if (!m) continue
    const before = span.text.slice(0, m.index)
    const after = span.text.slice(m.index + m[0].length)
    const replaced: AnsiSpan[] = []
    if (before) replaced.push({ ...span, text: before })
    replaced.push({ ...span, text: m[0], color: LEVEL_COLOR[level], bold: true })
    if (after) replaced.push({ ...span, text: after })
    return [...spans.slice(0, i), ...replaced, ...spans.slice(i + 1)]
  }
  return spans
}

/** 渲染用的行：先按关键词过滤，再做 ANSI 解析与级别着色 */
const lines = computed(() => {
  const kw = filter.value.trim().toLowerCase()
  const out: string[] = []
  for (const rawLine of raw.value) {
    const text = displayLine(rawLine)
    if (kw && !text.toLowerCase().includes(kw)) continue
    out.push(text)
  }
  return out.map((text) => {
    const level = detectLevel(text)
    return { text, level, spans: paintLevelTag(parseAnsiLine(text), level) }
  })
})

const sizeText = computed(() =>
  formatBytes(raw.value.reduce((acc, l) => acc + l.length + 1, 0)),
)

async function loadSteps(): Promise<void> {
  loadingSteps.value = true
  errorMsg.value = ''
  try {
    const res = await api.getJobSteps(props.pipelineId, props.pipelineRunId, props.jobId)
    stepsInfo.value = res
    if (!res.steps.length) {
      errorMsg.value = '该任务没有可用步骤（可能尚未开始调度，或为人工卡点类任务）'
      return
    }
    // 默认选中正在运行的步骤；还没开始跑就选第一个未完成的步骤，全部结束则选最后一个
    const running = res.steps.find((s) => s.running)
    const firstPending = res.steps.find((s) => !s.finish)
    activeStep.value = (running ?? firstPending ?? res.steps[res.steps.length - 1]).stepIndex
  } catch (e) {
    errorMsg.value = errorMessage(e)
  } finally {
    loadingSteps.value = false
  }
}

async function pullLog(reset = false): Promise<void> {
  if (activeStep.value === null) return
  if (!buildId.value) {
    errorMsg.value = errorMsg.value || '未取到 buildId，无法拉取日志'
    return
  }
  // 快速切换步骤时可能有多个请求并发，只让最后一次的结果落地
  const seq = ++pullSeq
  if (reset) {
    nextOffset = 0
    raw.value = []
    logUrl.value = ''
    errorMsg.value = ''
  }
  loadingLog.value = true
  try {
    // 多回退一行：运行中的最后一行可能还没写完，下一轮用完整内容覆盖它
    const reqOffset = nextOffset > 0 ? nextOffset - 1 : 0
    const chunk = await api.getJobStepLog({
      pipelineId: props.pipelineId,
      pipelineRunId: props.pipelineRunId,
      jobId: props.jobId,
      stepIndex: activeStep.value,
      buildId: buildId.value,
      offset: reqOffset,
      limit: 200000,
    })
    if (seq !== pullSeq) return // 已有更新的一次拉取，丢弃本次结果
    // 注意：不要先把 \r 换成 \n，否则行号会和服务端 offset 对不上；
    // 回车符只影响展示，交给 displayLine 处理。
    if (chunk.logs.length) {
      const newLines = chunk.logs.split('\n')
      if (nextOffset > 0) raw.value.pop() // 丢掉上一轮可能没写完的最后一行
      raw.value.push(...newLines)
      nextOffset = reqOffset + newLines.length
      if (raw.value.length > MAX_LINES) {
        raw.value.splice(0, raw.value.length - MAX_LINES)
      }
      if (follow.value) await scrollToBottom(reset)
    }
  } catch (e) {
    if (seq !== pullSeq) return
    errorMsg.value = errorMessage(e)
    stopTimer()
  } finally {
    loadingLog.value = false
  }
}

function startTimer(): void {
  stopTimer()
  if (!props.jobActive) return
  timer = window.setInterval(() => void tick(), 2000)
}

function stopTimer(): void {
  if (timer) window.clearInterval(timer)
  timer = 0
}

/** 每 2 秒：先同步步骤状态（自动跟阶段），再增量拉日志 */
async function tick(): Promise<void> {
  const switched = await syncSteps()
  if (!switched) await pullLog()
}

/**
 * 任务运行中刷新步骤列表；当前步骤结束后自动切到下一个还没结束的步骤，
 * 这样「步骤 1 完成、步骤 2 开始」时日志会自己跟过去。
 */
async function syncSteps(): Promise<boolean> {
  if (!props.jobActive || !followStep.value) return false
  try {
    const res = await api.getJobSteps(props.pipelineId, props.pipelineRunId, props.jobId)
    stepsInfo.value = res
    const running = res.steps.find((s) => s.running)
    let target = running?.stepIndex ?? null
    if (target === null) {
      const current = res.steps.find((s) => s.stepIndex === activeStep.value)
      if (current?.finish) {
        // 当前步骤已结束，切到后面第一个还没结束的步骤
        target = res.steps.find((s) => s.stepIndex > current.stepIndex && !s.finish)?.stepIndex ?? null
      }
    }
    if (target !== null && target !== activeStep.value) {
      activeStep.value = target
      return true
    }
  } catch {
    /* 偶发失败忽略，下一轮再试 */
  }
  return false
}

/** 手动切换步骤时暂停自动跟随，避免下一轮又被切走 */
function onPickStep(event: Event): void {
  activeStep.value = Number((event.target as HTMLSelectElement).value)
  followStep.value = false
}

let autoScrolling = false

/** 平滑滚动到底部（requestAnimationFrame + easeOutCubic） */
async function scrollToBottom(instant = false): Promise<void> {
  await nextTick()
  const el = box.value
  if (!el) return
  const target = el.scrollHeight - el.clientHeight
  if (target <= 0) return
  const start = el.scrollTop

  if (instant || Math.abs(target - start) < 4) {
    cancelAnimationFrame(scrollRaf)
    autoScrolling = false
    el.scrollTop = target
    return
  }

  cancelAnimationFrame(scrollRaf)
  autoScrolling = true
  const distance = target - start
  const duration = Math.min(420, Math.max(160, Math.abs(distance) * 0.35))
  const t0 = performance.now()

  const step = (now: number) => {
    const p = Math.min(1, (now - t0) / duration)
    const eased = 1 - Math.pow(1 - p, 3)
    el.scrollTop = start + distance * eased
    if (p < 1) {
      scrollRaf = requestAnimationFrame(step)
    } else {
      autoScrolling = false
      el.scrollTop = target
    }
  }
  scrollRaf = requestAnimationFrame(step)
}

/** 用户主动滚轮时打断自动滚动动画 */
function onWheel(): void {
  if (scrollRaf) cancelAnimationFrame(scrollRaf)
  scrollRaf = 0
  autoScrolling = false
}

function onScroll(): void {
  const el = box.value
  if (!el || autoScrolling) return
  // 用户手动往上翻时自动关闭跟随，回到底部再打开
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40
  if (!atBottom && follow.value) follow.value = false
  else if (atBottom && !follow.value) follow.value = true
}

async function copyLog(): Promise<void> {
  const text = stripAnsi(raw.value.map(displayLine).join('\n'))
  try {
    await navigator.clipboard.writeText(text)
    notify('success', `已复制 ${raw.value.length} 行日志`)
  } catch {
    notify('error', '复制失败，请改用「下载日志」')
  }
}

function downloadLog(): void {
  const text = stripAnsi(raw.value.map(displayLine).join('\n'))
  const blob = new Blob([text], { type: 'text/plain;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `pipeline-${props.pipelineId}-run-${props.pipelineRunId}-job-${props.jobId}-step-${activeStep.value}.log`
  a.click()
  URL.revokeObjectURL(url)
}

async function fetchLogUrl(): Promise<void> {
  if (activeStep.value === null || !buildId.value) return
  try {
    const res = (await api.getJobStepLogUrl({
      pipelineId: props.pipelineId,
      pipelineRunId: props.pipelineRunId,
      jobId: props.jobId,
      stepIndex: activeStep.value,
      buildId: buildId.value,
    })) as Record<string, unknown> | string
    const url =
      typeof res === 'string'
        ? res
        : ((res?.url ?? res?.logUrl ?? res?.downloadUrl) as string | undefined) ?? ''
    logUrl.value = url
    if (url) {
      await navigator.clipboard.writeText(url)
      notify('success', '日志下载地址已复制到剪贴板')
    } else {
      notify('warn', '接口未返回可用的下载地址')
    }
  } catch (e) {
    notify('error', `获取下载地址失败：${errorMessage(e)}`)
  }
}

watch(activeStep, () => {
  errorMsg.value = ''
  void pullLog(true)
  startTimer()
})

// 重新打开「跟阶段」后立刻对齐到正在执行的步骤，不用等下一轮轮询
watch(followStep, (on) => {
  if (on) void tick()
})

watch(
  () => props.jobActive,
  (active) => {
    if (active) {
      startTimer()
    } else {
      stopTimer()
      // 任务刚结束：补拉一次，把运行中可能没写完的最后一行补全
      void pullLog()
    }
  },
)

onMounted(async () => {
  // 只加载步骤列表；首屏日志由 watch(activeStep) 统一负责，避免重复请求
  await loadSteps()
})

onBeforeUnmount(() => {
  stopTimer()
  if (scrollRaf) cancelAnimationFrame(scrollRaf)
})
</script>

<template>
  <div class="log-panel">
    <div class="log-head">
      <span class="log-title">日志 · {{ props.jobName }}</span>

      <select
        v-if="steps.length"
        :value="activeStep ?? ''"
        class="select step-select"
        @change="onPickStep"
      >
        <option v-for="s in steps" :key="s.stepIndex" :value="s.stepIndex">
          {{ s.stepIndex }} · {{ s.stepName }}{{ s.running ? '（运行中）' : '' }}
        </option>
      </select>
      <span v-else class="hint">无步骤</span>

      <label class="toggle" title="任务运行中自动跟随正在执行的步骤">
        <input v-model="followStep" type="checkbox" />跟阶段
      </label>

      <div class="spacer"></div>

      <input v-model="filter" class="input filter" type="search" placeholder="过滤当前日志…" />

      <label class="toggle" title="自动滚动到最新输出">
        <input v-model="follow" type="checkbox" />跟随
      </label>
      <label class="toggle" title="长行折行显示">
        <input v-model="wrap" type="checkbox" />折行
      </label>

      <button class="btn btn-xs" :disabled="loadingLog" @click="pullLog()">追加</button>
      <button class="btn btn-xs" :disabled="loadingLog" @click="pullLog(true)">重拉</button>
      <button class="btn btn-xs" :disabled="!raw.length" @click="copyLog">复制</button>
      <button class="btn btn-xs" :disabled="!raw.length" @click="downloadLog">下载</button>
      <button class="btn btn-xs" :disabled="!buildId" @click="fetchLogUrl">地址</button>
      <button class="btn btn-xs btn-ghost" @click="emit('close')">收起</button>
    </div>

    <div v-if="errorMsg" class="error-inline">{{ errorMsg }}</div>
    <div v-if="logUrl" class="log-url mono" @click="fetchLogUrl">{{ logUrl }}</div>

    <div ref="box" class="log-box" :class="{ wrap }" @scroll="onScroll" @wheel="onWheel">
      <div v-if="loadingSteps" class="empty">正在获取任务步骤…</div>
      <div v-else-if="!lines.length" class="empty">
        暂无日志输出<br />
        <span v-if="props.jobActive">任务运行中，每 2 秒自动追加…</span>
        <span v-else>该步骤没有产生日志，可切换上方步骤查看</span>
      </div>

      <table v-else class="log-table">
        <tbody>
          <tr v-for="(line, i) in lines" :key="i" :class="line.level ? 'ln-' + line.level : ''">
            <td class="ln">{{ i + 1 }}</td>
            <td class="lt">
              <span v-for="(sp, j) in line.spans" :key="j" :style="spanStyle(sp)">{{
                sp.text
              }}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <div class="log-foot">
      <span>
        {{ lines.length }} 行<template v-if="filter">（已过滤，共 {{ raw.length }} 行）</template>
        · {{ sizeText }}
      </span>
      <span v-if="props.jobActive" class="live">● 实时跟踪中</span>
      <span v-else>任务已结束</span>
      <div class="spacer"></div>
      <span v-if="!follow" class="hint">已暂停跟随，滚动到底部可恢复</span>
    </div>
  </div>
</template>

<style scoped>
.log-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-top: 1px solid var(--border);
  background: #0b0d11;
}

.log-head {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 7px 10px;
  background: var(--bg-panel);
  border-bottom: 1px solid var(--border-soft);
  flex: none;
  flex-wrap: wrap;
}

.log-title {
  font-weight: 600;
  font-size: 12.5px;
  white-space: nowrap;
}

.step-select {
  max-width: 220px;
  padding: 4px 7px;
}

.filter {
  width: 150px;
  padding: 4px 8px;
}

.toggle {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--text-muted);
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}

.toggle input {
  accent-color: var(--accent);
}

.log-url {
  padding: 5px 10px;
  font-size: 11px;
  color: var(--accent);
  background: var(--accent-soft);
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  user-select: text;
  flex: none;
}

.log-box {
  flex: 1;
  overflow: auto;
  min-height: 0;
  padding: 6px 0;
  user-select: text;
}

.log-table {
  border-collapse: collapse;
  width: 100%;
  font-family: var(--mono);
  font-size: 12px;
  line-height: 1.55;
  table-layout: fixed;
}

.log-table td {
  padding: 0 10px;
  vertical-align: top;
}

.ln {
  width: 56px;
  text-align: right;
  color: #4b5563;
  user-select: none;
  border-right: 1px solid var(--border-soft);
  font-variant-numeric: tabular-nums;
}

.lt {
  white-space: pre;
  color: #d1d5db;
  word-break: normal;
}

.log-box.wrap .lt {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.ln-error .lt {
  color: #fca5a5;
}

.ln-error .ln {
  color: #7f1d1d;
}

.ln-error {
  background: rgba(239, 68, 68, 0.07);
}

.ln-warn .lt {
  color: #fcd34d;
}

.ln-warn .ln {
  color: #78350f;
}

.ln-warn {
  background: rgba(245, 158, 11, 0.06);
}

.ln-info .lt {
  color: #bfdbfe;
}

.ln-info .ln {
  color: #1e3a8a;
}

.ln-debug .lt {
  color: #9ca3af;
}

.ln-success .lt {
  color: #86efac;
}

.ln-success .ln {
  color: #14532d;
}

.log-foot {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 4px 10px;
  border-top: 1px solid var(--border-soft);
  background: var(--bg-panel);
  color: var(--text-dim);
  font-size: 11.5px;
  flex: none;
}

.live {
  color: var(--running);
}
</style>
