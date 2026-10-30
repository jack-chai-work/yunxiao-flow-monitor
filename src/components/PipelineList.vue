<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  loadPipelines,
  loadRecentRuns,
  selectPipeline,
  selectPipelineRun,
  togglePipelineExpand,
  toggleWatch,
  useStore,
} from '@/store'
import { elapsedOf, formatRelative, triggerModeLabel } from '@/lib/format'
import StatusBadge from './StatusBadge.vue'

const { state } = useStore()
const searchDraft = ref(state.keyword)

const STATUS_OPTIONS = [
  { value: 'ALL', label: '全部状态' },
  { value: 'RUNNING', label: '运行中' },
  { value: 'SUCCESS', label: '成功' },
  { value: 'FAIL', label: '失败' },
  { value: 'CANCELED', label: '已取消' },
  { value: 'WAITING', label: '等待中' },
]

const pageCount = computed(() => {
  if (!state.total) return 1
  return Math.max(1, Math.ceil(state.total / state.perPage))
})

const sorted = computed(() => {
  // 运行中的排最前，其次按最近运行时间倒序
  return [...state.pipelines].sort((a, b) => {
    const ar = a.latestRun?.status === 'RUNNING' ? 1 : 0
    const br = b.latestRun?.status === 'RUNNING' ? 1 : 0
    if (ar !== br) return br - ar
    if (a.watched !== b.watched) return a.watched ? -1 : 1
    const at = a.latestRun?.startTime ?? a.latestRun?.createTime ?? 0
    const bt = b.latestRun?.startTime ?? b.latestRun?.createTime ?? 0
    return bt - at
  })
})

let searchTimer = 0
function onSearchInput(): void {
  window.clearTimeout(searchTimer)
  searchTimer = window.setTimeout(() => {
    state.keyword = searchDraft.value.trim()
    state.page = 1
    void loadPipelines()
  }, 400)
}

function onFilterChange(): void {
  state.page = 1
  void loadPipelines()
}

function prevPage(): void {
  if (state.page <= 1) return
  state.page -= 1
  void loadPipelines()
}

function nextPage(): void {
  if (state.page >= pageCount.value) return
  state.page += 1
  void loadPipelines()
}

function recentOf(id: number) {
  return (state.recentRuns[id] ?? []).slice(0, 3)
}
</script>

<template>
  <section class="pane">
    <div class="pane-head">
      <span class="pane-title">流水线</span>
      <div class="spacer"></div>
      <button class="btn btn-xs btn-ghost" :disabled="state.busy.pipelines" @click="loadPipelines">
        {{ state.busy.pipelines ? '…' : '刷新' }}
      </button>
    </div>

    <div class="filters">
      <input
        v-model="searchDraft"
        class="input"
        type="search"
        placeholder="搜索流水线名称…"
        @input="onSearchInput"
      />
      <select v-model="state.statusFilter" class="select" @change="onFilterChange">
        <option v-for="o in STATUS_OPTIONS" :key="o.value" :value="o.value">{{ o.label }}</option>
      </select>
    </div>

    <div class="pane-body">
      <div v-if="state.pipelinesLoading && !state.pipelines.length" class="empty">正在加载流水线…</div>
      <div v-else-if="!state.pipelines.length" class="empty">
        暂无数据<br />
        请先在「设置」中填写组织 ID 与令牌，<br />
        然后点击「立即刷新」
      </div>

      <ul v-else class="list">
        <li
          v-for="p in sorted"
          :key="p.pipelineId"
          class="pipe"
          :class="{ 'is-open': state.expanded[p.pipelineId] }"
        >
          <div
            class="item"
            :class="{ active: p.pipelineId === state.selectedPipelineId }"
            @click="selectPipeline(p.pipelineId)"
          >
            <button
              class="caret"
              :class="{ open: state.expanded[p.pipelineId] }"
              :title="state.expanded[p.pipelineId] ? '收起运行历史' : '展开最近 3 次运行'"
              @click.stop="togglePipelineExpand(p.pipelineId)"
            >
              ▶
            </button>
            <button
              class="star"
              :class="{ on: p.watched }"
              :title="p.watched ? '取消关注' : '关注（纳入实时轮询与桌面通知）'"
              @click.stop="toggleWatch(p.pipelineId)"
            >
              {{ p.watched ? '★' : '☆' }}
            </button>

            <div class="item-main">
              <div class="item-name" :title="p.name">{{ p.name }}</div>
              <div class="item-meta">
                <span class="mono">#{{ p.pipelineId }}</span>
                <span v-if="p.groupName">· {{ p.groupName }}</span>
                <span v-if="p.envName">· {{ p.envName }}</span>
              </div>
            </div>

            <div class="item-side">
              <StatusBadge :status="p.latestRun?.status ?? 'UNKNOWN'" />
              <div class="item-time">
                <template v-if="p.latestRun">
                  <span>#{{ p.latestRun.pipelineRunId }}</span>
                  <span v-if="p.latestRun.status === 'RUNNING'">
                    {{ elapsedOf(p.latestRun.startTime ?? p.latestRun.createTime) }}
                  </span>
                  <span v-else>{{ formatRelative(p.latestRun.endTime ?? p.latestRun.createTime) }}</span>
                </template>
                <template v-else>暂无运行</template>
              </div>
            </div>
          </div>

          <!-- 运行历史作为流水线子项，默认折叠，只展示最近三次 -->
          <ul v-if="state.expanded[p.pipelineId]" class="runs">
            <li v-if="state.recentLoading[p.pipelineId]" class="run-item loading">正在加载运行历史…</li>
            <li v-else-if="!recentOf(p.pipelineId).length" class="run-item loading">暂无运行记录</li>
            <li
              v-for="(r, ri) in recentOf(p.pipelineId)"
              :key="r.pipelineRunId"
              class="run-item"
              :class="{ active: r.pipelineRunId === state.selectedRunId }"
              @click.stop="selectPipelineRun(p.pipelineId, r.pipelineRunId)"
            >
              <span class="run-index">{{ ri + 1 }}</span>
              <span class="run-id mono">#{{ r.pipelineRunId }}</span>
              <StatusBadge :status="r.status" :dot="false" />
              <span class="run-trigger">{{ triggerModeLabel(r.triggerMode) }}</span>
              <span class="spacer"></span>
              <span v-if="r.status === 'RUNNING'" class="run-dur mono">
                {{ elapsedOf(r.startTime ?? r.createTime) }}
              </span>
              <span v-else class="run-dur mono">
                {{ elapsedOf(r.startTime ?? r.createTime, r.endTime) }}
              </span>
            </li>
            <li class="run-foot">
              仅展示最近 3 次
              <button class="link" @click.stop="loadRecentRuns(p.pipelineId)">刷新</button>
            </li>
          </ul>
        </li>
      </ul>
    </div>

    <div class="pager">
      <button class="btn btn-xs btn-ghost" :disabled="state.page <= 1" @click="prevPage">
        上一页
      </button>
      <span class="mono">{{ state.page }} / {{ pageCount }}</span>
      <button
        class="btn btn-xs btn-ghost"
        :disabled="state.page >= pageCount"
        @click="nextPage"
      >
        下一页
      </button>
    </div>
  </section>
</template>

<style scoped>
.filters {
  display: flex;
  gap: 6px;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border-soft);
  flex: none;
}

.filters .input {
  flex: 1;
  min-width: 0;
}

.filters .select {
  width: 96px;
  flex: none;
}

.list {
  list-style: none;
  margin: 0;
  padding: 0;
}

.pipe {
  border-bottom: 1px solid var(--border-soft);
}

.item {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding: 9px 10px;
  cursor: pointer;
  transition: background 0.12s;
}

.item:hover {
  background: var(--bg-panel);
}

.item.active {
  background: var(--accent-soft);
  box-shadow: inset 2px 0 0 var(--accent);
}

.caret {
  background: transparent;
  border: none;
  color: var(--text-dim);
  font-size: 9px;
  line-height: 1;
  padding: 5px 1px 0;
  flex: none;
  transition: transform 0.18s ease, color 0.15s;
  transform-origin: 50% 45%;
}

.caret.open {
  transform: rotate(90deg);
  color: var(--accent);
}

.caret:hover {
  color: var(--text);
}

.star {
  background: transparent;
  border: none;
  color: var(--text-dim);
  font-size: 14px;
  line-height: 1;
  padding: 2px 0 0;
  flex: none;
}

.star.on {
  color: var(--warn);
}

.item-main {
  flex: 1;
  min-width: 0;
}

.item-name {
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.item-meta {
  color: var(--text-dim);
  font-size: 11.5px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  margin-top: 2px;
}

.item-side {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 4px;
  flex: none;
}

.item-time {
  color: var(--text-dim);
  font-size: 11px;
  display: flex;
  gap: 4px;
  font-variant-numeric: tabular-nums;
}

/* ---- 运行历史子项 ---- */

.runs {
  list-style: none;
  margin: 0 0 6px;
  padding: 2px 8px 0 26px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  animation: runs-in 0.16s ease-out;
}

@keyframes runs-in {
  from {
    opacity: 0;
    transform: translateY(-3px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.run-item {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 5px 8px;
  border-radius: 5px;
  border-left: 2px solid transparent;
  cursor: pointer;
  font-size: 12px;
  transition: background 0.12s, border-color 0.12s;
}

.run-item:hover {
  background: var(--bg-panel);
}

.run-item.active {
  background: var(--accent-soft);
  border-left-color: var(--accent);
}

.run-item.loading {
  color: var(--text-dim);
  cursor: default;
  font-size: 11.5px;
  padding-left: 10px;
}

.run-index {
  color: var(--text-dim);
  font-size: 10.5px;
  width: 10px;
  text-align: center;
  flex: none;
}

.run-id {
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
}

.run-trigger {
  color: var(--text-dim);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.run-dur {
  color: var(--text-dim);
  font-size: 11px;
  flex: none;
}

.run-foot {
  padding: 3px 10px 4px;
  color: var(--text-dim);
  font-size: 10.5px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.link {
  background: transparent;
  border: none;
  color: var(--accent);
  font-size: 10.5px;
  padding: 0;
}

.link:hover {
  text-decoration: underline;
}

.pager {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 10px;
  border-top: 1px solid var(--border-soft);
  background: var(--bg-panel);
  color: var(--text-muted);
  font-size: 12px;
  flex: none;
}
</style>
