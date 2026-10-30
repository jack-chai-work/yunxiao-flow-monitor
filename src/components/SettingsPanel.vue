<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { api, isTauri } from '@/lib/api'
import { loadPipelines, notify, useStore } from '@/store'
import { errorMessage } from '@/lib/format'
import type { ConfigDto, ConfigInput, ConnectionTestResult } from '@/types'

const { state } = useStore()

const saving = ref(false)
const testing = ref(false)
const testResult = ref('')
const testFailed = ref(false)
/** 是否已设置开机自启（操作系统里的状态，不属于 config.json） */
const autostart = ref(false)
const autostartBusy = ref(false)
/** 空字符串表示不修改已保存的令牌 */
const tokenDraft = ref('')

const form = reactive<Omit<ConfigInput, 'token'>>({
  baseUrl: state.config?.baseUrl ?? 'https://openapi-rdc.aliyuncs.com',
  organizationId: state.config?.organizationId ?? '',
  pollIntervalSecs: state.config?.pollIntervalSecs ?? 10,
  fastPollIntervalSecs: state.config?.fastPollIntervalSecs ?? 3,
  watchedPipelineIds: [...(state.config?.watchedPipelineIds ?? [])],
  autoWatchLimit: state.config?.autoWatchLimit ?? 30,
  autoStartPolling: state.config?.autoStartPolling ?? true,
  notifyOnSuccess: state.config?.notifyOnSuccess ?? true,
  notifyOnFail: state.config?.notifyOnFail ?? true,
  notifyOnCanceled: state.config?.notifyOnCanceled ?? false,
  minimizeToTrayOnClose: state.config?.minimizeToTrayOnClose ?? true,
})

const watchedNames = computed(() =>
  form.watchedPipelineIds.map(
    (id) => state.pipelines.find((p) => p.pipelineId === id)?.name ?? `流水线 ${id}`,
  ),
)

const tokenPlaceholder = computed(() =>
  state.config?.hasToken ? `已保存：${state.config.tokenMasked}（留空则不修改）` : 'pt-xxxxxxxxxxxx',
)

const canSave = computed(() => form.organizationId.trim().length > 0)

function close(): void {
  state.settingsOpen = false
}

function payload(): ConfigInput {
  return { ...form, token: tokenDraft.value.trim() }
}

async function doTest(): Promise<void> {
  testing.value = true
  testResult.value = ''
  testFailed.value = false
  try {
    // 先落盘再测试，保证测试用的是表单里的最新值
    const saved = await api.saveConfig(payload())
    state.config = saved
    const res: ConnectionTestResult = await api.testConnection()
    testResult.value = res.samplePipelineName
      ? `${res.message}；示例流水线：${res.samplePipelineName}`
      : res.message
    notify('success', '连接测试通过')
  } catch (e) {
    testFailed.value = true
    testResult.value = errorMessage(e)
  } finally {
    testing.value = false
  }
}

async function doSave(): Promise<void> {
  saving.value = true
  try {
    const saved: ConfigDto = await api.saveConfig(payload())
    state.config = saved
    tokenDraft.value = ''
    notify('success', saved.hasToken ? '配置已保存' : '配置已保存（未设置令牌）')
    state.page = 1
    await loadPipelines()
    close()
  } catch (e) {
    notify('error', `保存失败：${errorMessage(e)}`, 9000)
  } finally {
    saving.value = false
  }
}

async function clearToken(): Promise<void> {
  try {
    const saved = await api.saveConfig({ ...form, token: '__CLEAR__' })
    state.config = saved
    tokenDraft.value = ''
    notify('warn', '已清除本地保存的令牌')
  } catch (e) {
    notify('error', errorMessage(e))
  }
}

function removeWatched(id: number): void {
  form.watchedPipelineIds = form.watchedPipelineIds.filter((x) => x !== id)
}

onMounted(async () => {
  if (!isTauri()) return
  try {
    autostart.value = await api.getAutostart()
  } catch {
    /* 读取失败时保持默认关闭 */
  }
})

async function toggleAutostart(enabled: boolean): Promise<void> {
  autostartBusy.value = true
  try {
    autostart.value = await api.setAutostart(enabled)
    notify('success', autostart.value ? '已开启开机自动启动' : '已关闭开机自动启动')
  } catch (e) {
    autostart.value = !enabled
    notify('error', `设置开机启动失败：${errorMessage(e)}`, 8000)
  } finally {
    autostartBusy.value = false
  }
}
</script>

<template>
  <Teleport to="body">
    <div class="modal-mask" @click.self="close">
      <div class="modal">
        <div class="modal-head">
          设置
          <div class="spacer"></div>
          <button class="btn btn-xs btn-ghost" @click="close">✕</button>
        </div>

        <div class="modal-body">
          <section>
            <div class="section-title">云效接入（中心版）</div>
            <div class="grid-2">
              <div class="field">
                <label for="f-base">服务接入点 domain</label>
                <input id="f-base" v-model="form.baseUrl" class="input mono" spellcheck="false" />
                <div class="hint">公共云中心版固定为 https://openapi-rdc.aliyuncs.com</div>
              </div>
              <div class="field">
                <label for="f-org">组织 ID（organizationId）</label>
                <input
                  id="f-org"
                  v-model="form.organizationId"
                  class="input mono"
                  spellcheck="false"
                  placeholder="5ebbc0228123212b59xxxxx"
                />
                <div class="hint">云效 → 组织管理后台 → 基本信息 页面可获取</div>
              </div>
            </div>

            <div class="field" style="margin-top: 12px">
              <label for="f-token">个人访问令牌（x-yunxiao-token）</label>
              <div class="token-row">
                <input
                  id="f-token"
                  v-model="tokenDraft"
                  class="input mono"
                  type="password"
                  spellcheck="false"
                  autocomplete="off"
                  :placeholder="tokenPlaceholder"
                />
                <button
                  v-if="state.config?.hasToken"
                  class="btn btn-danger"
                  title="删除本地保存的令牌"
                  @click="clearToken"
                >
                  清除
                </button>
              </div>
              <div class="hint">
                云效 → 个人设置 → 个人访问令牌。需勾选「流水线」下
                <b>流水线（只读）</b>、<b>流水线运行实例（读写）</b>、<b>流水线运行任务（读写）</b>。
                令牌仅首次创建时可见，请妥善保存。<br />
                当前存储方式：<b>{{
                  state.config?.secureStoreEnabled
                    ? '系统钥匙串（已启用 secure-store）'
                    : '本地混淆文件（编译时加 --features secure-store 可改用系统钥匙串）'
                }}</b>
              </div>
            </div>
          </section>

          <section>
            <div class="section-title">轮询</div>
            <div class="grid-3">
              <div class="field">
                <label for="f-poll">常规间隔（秒）</label>
                <input
                  id="f-poll"
                  v-model.number="form.pollIntervalSecs"
                  class="input"
                  type="number"
                  min="3"
                  max="600"
                />
              </div>
              <div class="field">
                <label for="f-fast">运行中加速间隔（秒）</label>
                <input
                  id="f-fast"
                  v-model.number="form.fastPollIntervalSecs"
                  class="input"
                  type="number"
                  min="1"
                  max="60"
                />
              </div>
              <div class="field">
                <label for="f-limit">未指定关注时，自动盯首页条数</label>
                <input
                  id="f-limit"
                  v-model.number="form.autoWatchLimit"
                  class="input"
                  type="number"
                  min="1"
                  max="30"
                />
              </div>
            </div>
            <div class="hint" style="margin-top: 6px">
              间隔越小请求越频繁，云效接口有限流（HTTP 429）。常规建议 ≥ 10 秒，加速 ≥ 3 秒。
            </div>
            <label class="check-row">
              <input v-model="form.autoStartPolling" type="checkbox" />
              应用启动后自动开始轮询
            </label>
          </section>

          <section>
            <div class="section-title">启动</div>
            <label class="check-row">
              <input
                type="checkbox"
                :checked="autostart"
                :disabled="autostartBusy"
                @change="toggleAutostart(($event.target as HTMLInputElement).checked)"
              />
              开机自动启动（登录系统后自动运行本程序）
            </label>
            <div class="hint">
              通过操作系统登录启动项实现：Windows 下写入当前用户的 Run 注册表项，只对当前用户生效。
              开发模式下注册的是调试用可执行文件，安装版会注册安装目录下的程序。
            </div>
          </section>

          <section>
            <div class="section-title">桌面通知</div>
            <label class="check-row">
              <input v-model="form.notifyOnSuccess" type="checkbox" />运行成功时通知
            </label>
            <label class="check-row">
              <input v-model="form.notifyOnFail" type="checkbox" />运行失败时通知
            </label>
            <label class="check-row">
              <input v-model="form.notifyOnCanceled" type="checkbox" />运行被取消时通知
            </label>
            <label class="check-row">
              <input v-model="form.minimizeToTrayOnClose" type="checkbox" />
              关闭窗口时最小化到系统托盘（关闭则直接退出程序）
            </label>
            <div class="hint">
              通知在状态发生跃迁时才发送（例如 运行中 → 运行成功），首次轮询只记录基线不通知，避免启动刷屏。
            </div>
          </section>

          <section>
            <div class="section-title">
              关注的流水线（{{ form.watchedPipelineIds.length }}）
            </div>
            <div v-if="!form.watchedPipelineIds.length" class="hint">
              未指定关注项时，轮询会自动盯「流水线列表首页」的前
              {{ form.autoWatchLimit }} 条。在左侧列表点 ☆ 可加入关注。
            </div>
            <div v-else class="chips">
              <span v-for="(id, i) in form.watchedPipelineIds" :key="id" class="chip">
                {{ watchedNames[i] }}
                <button class="chip-x" @click="removeWatched(id)">✕</button>
              </span>
            </div>
          </section>

          <div v-if="testResult" class="test-box" :class="{ fail: testFailed }">
            {{ testResult }}
          </div>
        </div>

        <div class="modal-foot">
          <button class="btn" :disabled="testing" @click="doTest">
            {{ testing ? '测试中…' : '测试连接' }}
          </button>
          <div class="spacer"></div>
          <button class="btn" @click="close">取消</button>
          <button class="btn btn-primary" :disabled="saving || !canSave" @click="doSave">
            {{ saving ? '保存中…' : '保存' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
section {
  border-bottom: 1px solid var(--border-soft);
  padding-bottom: 14px;
}

section:last-of-type {
  border-bottom: none;
}

.token-row {
  display: flex;
  gap: 8px;
}

.token-row .input {
  flex: 1;
  min-width: 0;
}

.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 3px 8px;
  border-radius: 999px;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  font-size: 12px;
}

.chip-x {
  background: transparent;
  border: none;
  color: var(--text-dim);
  font-size: 11px;
  padding: 0;
  line-height: 1;
}

.chip-x:hover {
  color: var(--fail);
}

.test-box {
  padding: 9px 11px;
  border-radius: 6px;
  font-size: 12.5px;
  line-height: 1.6;
  background: rgba(34, 197, 94, 0.1);
  border: 1px solid rgba(34, 197, 94, 0.3);
  color: #86efac;
  user-select: text;
  word-break: break-all;
}

.test-box.fail {
  background: rgba(239, 68, 68, 0.1);
  border-color: rgba(239, 68, 68, 0.32);
  color: #fca5a5;
}
</style>
