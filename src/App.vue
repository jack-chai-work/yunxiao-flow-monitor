<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import {
  dismissNotice,
  handleStatusChange,
  applySnapshot,
  loadConfig,
  loadPipelines,
  loadPolling,
  notify,
  refreshNow,
  setPolling,
  useStore,
} from '@/store'
import { api, isTauri, onPollError, onPollState, onSnapshot, onStatusChanged } from '@/lib/api'
import { formatClock, statusLabel } from '@/lib/format'
import PipelineList from '@/components/PipelineList.vue'
import RunDetail from '@/components/RunDetail.vue'
import SettingsPanel from '@/components/SettingsPanel.vue'

const { state } = useStore()
const unlisteners = ref<Array<() => void>>([])
const now = ref(Date.now())
let clockTimer = 0

const pollingLabel = computed(() => {
  if (!state.config?.hasToken || !state.config?.organizationId) return '未配置'
  return state.polling ? `轮询中 · ${state.intervalSecs || '—'}s` : '已暂停'
})

const canOperate = computed(() => Boolean(state.config?.hasToken && state.config?.organizationId))

async function togglePolling(): Promise<void> {
  if (!canOperate.value) {
    state.settingsOpen = true
    notify('warn', '请先在设置中填写组织 ID 与个人访问令牌')
    return
  }
  await setPolling(!state.polling)
}

onMounted(async () => {
  clockTimer = window.setInterval(() => {
    now.value = Date.now()
  }, 1000)

  if (!isTauri()) {
    notify(
      'error',
      '当前运行在浏览器中，无法调用桌面端能力。请使用 npm run app:dev 启动 Tauri 应用。',
      0,
    )
    return
  }

  await loadConfig()
  await loadPolling()

  // 先用后端缓存的快照渲染，避免首屏空白
  try {
    const snap = await api.getSnapshot()
    if (snap && snap.length) {
      applySnapshot({
        pipelines: snap,
        fetchedAt: Date.now(),
        runningCount: snap.filter((p) => p.latestRun?.status === 'RUNNING').length,
        failedCount: 0,
        successCount: 0,
        intervalSecs: 0,
      })
    }
  } catch {
    /* 首次启动没有快照属正常情况 */
  }

  if (canOperate.value) await loadPipelines()

  const offs = await Promise.all([
    onSnapshot((p) => applySnapshot(p)),
    onStatusChanged((c) => handleStatusChange(c)),
    onPollState((running) => {
      state.polling = running
    }),
    onPollError((msg) => notify('error', `轮询异常：${msg}`, 9000)),
  ])
  unlisteners.value = offs
})

onBeforeUnmount(() => {
  window.clearInterval(clockTimer)
  unlisteners.value.forEach((off) => off())
})
</script>

<template>
  <header class="app-header">
    <div class="app-title">
      <span class="logo">流</span>
      <span>云效流水线实时监控</span>
    </div>

    <span class="badge" :class="state.polling ? 'st-running' : 'st-unknown'">
      <span class="dot"></span>{{ pollingLabel }}
    </span>

    <div class="stat-group">
      <span>运行中 <b>{{ state.runningCount }}</b></span>
      <span>成功 <b>{{ state.successCount }}</b></span>
      <span>失败 <b>{{ state.failedCount }}</b></span>
      <span v-if="state.lastFetchedAt">更新于 {{ formatClock(state.lastFetchedAt) }}</span>
    </div>

    <div class="spacer"></div>

    <button class="btn" :disabled="state.busy.refresh" @click="refreshNow">
      {{ state.busy.refresh ? '刷新中…' : '立即刷新' }}
    </button>
    <button class="btn" :class="{ 'btn-primary': !state.polling }" @click="togglePolling">
      {{ state.polling ? '暂停轮询' : '开始轮询' }}
    </button>
    <button class="btn btn-ghost" @click="state.settingsOpen = true">设置</button>
  </header>

  <main class="app-body">
    <PipelineList />
    <RunDetail />
  </main>

  <footer class="app-footer">
    <span v-if="!canOperate" class="foot-warn">
      尚未完成配置 —— 点击右上角「设置」填写组织 ID 与个人访问令牌
    </span>
    <span v-else>
      组织 {{ state.config?.organizationId }} · 接入点 {{ state.config?.baseUrl }}
      <template v-if="state.config?.secureStoreEnabled">· 令牌存于系统钥匙串</template>
    </span>
    <div class="spacer"></div>
    <span class="hint">
      关闭窗口将最小化到托盘继续监控，退出请右键托盘图标
      <template v-if="state.selectedRunId">
        · 当前实例 #{{ state.selectedRunId }}
        {{ state.runDetail ? statusLabel(state.runDetail.status) : '' }}
      </template>
    </span>
  </footer>

  <div class="toast-stack">
    <div
      v-for="n in state.notices"
      :key="n.id"
      class="toast"
      :class="n.kind"
      @click="dismissNotice(n.id)"
    >
      {{ n.text }}
    </div>
  </div>

  <SettingsPanel v-if="state.settingsOpen" />
</template>

<style scoped>
.app-footer {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 5px 14px;
  border-top: 1px solid var(--border);
  background: var(--bg-panel);
  color: var(--text-muted);
  font-size: 12px;
  flex: none;
  overflow: hidden;
  white-space: nowrap;
}

.foot-warn {
  color: var(--warn);
}
</style>
