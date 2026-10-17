<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { computed, onMounted, onUnmounted, ref } from 'vue'

import { useSaveTip } from '../composables/useSaveTip'
import ToggleSwitch from './ToggleSwitch.vue'

import type { DanmakuFilter, DashboardStatus } from '../types/ipc'

// 运行状态 + 快捷开关。
//
// 状态按秒轮询 Rust 的 `get_dashboard_status`（单一快照），开关命令成功后由广播 / 下一次轮询
// 回写；**不在前端各存一份开关状态**——同一组开关在设置页还有入口，各存一份必然漂移
// （这正是以前这块只敢做只读的原因）。
const { showError } = useSaveTip()

const status = ref<DashboardStatus | null>(null)
let timer: ReturnType<typeof setInterval> | undefined
const unlistens: UnlistenFn[] = []

/** 总开关关掉时，三个分类开关置灰（仍显示当前值，方便看出重新打开后会是什么状态） */
const masterOn = computed(() => status.value?.tts.master_enabled ?? false)

async function refresh() {
  try {
    status.value = await invoke<DashboardStatus>('get_dashboard_status')
  } catch (e) {
    console.error('读取仪表盘状态失败', e)
  }
}

/**
 * 快捷开关：先乐观更新（点下去立刻有反馈），失败回滚并提示
 *
 * 状态最终仍以 Rust 为准（广播 / 下一次轮询会覆盖乐观值）。
 */
async function toggleOverlay(key: 'visible' | 'clickthrough' | 'alwaysOnTop') {
  const overlay = status.value?.overlay
  if (!overlay) return
  const next = !overlay[key]
  overlay[key] = next
  try {
    if (key === 'visible') {
      await invoke('overlay_set_visible', { visible: next })
    } else if (key === 'clickthrough') {
      await invoke('overlay_set_clickthrough', { enabled: next })
    } else {
      await invoke('overlay_set_always_on_top', { enabled: next })
    }
  } catch (e) {
    overlay[key] = !next
    showError(e)
  }
}

type TtsSwitch = 'master' | 'danmaku' | 'gift' | 'welcome'

/**
 * 朗读开关（四个：总开关 + 弹幕 / 礼物 / 欢迎）
 *
 * 两维模型：总开关管「要不要出声」，三个分类开关管「哪一类出声」，分类开关只在总开关
 * 打开时生效。命令都是单字段的，避免整份 `TtsConfig` 回传覆盖别处刚改的配置。
 */
async function toggleTts(which: TtsSwitch) {
  const st = status.value
  if (!st) return
  const read = () =>
    which === 'master'
      ? st.tts.master_enabled
      : which === 'danmaku'
        ? st.tts.enabled
        : which === 'gift'
          ? st.gift_tts.enabled
          : st.welcome.tts_guard
  const write = (v: boolean) => {
    if (which === 'master') st.tts.master_enabled = v
    else if (which === 'danmaku') st.tts.enabled = v
    else if (which === 'gift') st.gift_tts.enabled = v
    else st.welcome.tts_guard = v
  }
  const prev = read()
  write(!prev)
  try {
    if (which === 'master') await invoke('tts_set_master_enabled', { enabled: !prev })
    else if (which === 'danmaku') await invoke('tts_set_danmaku_enabled', { enabled: !prev })
    else if (which === 'gift') await invoke('gift_tts_set_enabled', { enabled: !prev })
    else await invoke('welcome_tts_set_enabled', { enabled: !prev })
  } catch (e) {
    write(prev)
    showError(e)
  }
}

onMounted(async () => {
  void refresh()
  timer = setInterval(refresh, 1000)
  // 广播优先于轮询：别的入口（设置页 / 托盘 / 快捷键）改了开关时这边立刻跟上
  unlistens.push(
    await listen<boolean>('overlay-visible', e => {
      if (status.value) status.value.overlay.visible = e.payload
    }),
    await listen<boolean>('overlay-clickthrough', e => {
      if (status.value) status.value.overlay.clickthrough = e.payload
    }),
    await listen<boolean>('overlay-always-on-top', e => {
      if (status.value) status.value.overlay.always_on_top = e.payload
    }),
    await listen<{ master_enabled: boolean; enabled: boolean }>('tts-switches', e => {
      if (!status.value) return
      status.value.tts.master_enabled = e.payload.master_enabled
      status.value.tts.enabled = e.payload.enabled
    }),
    await listen<boolean>('gift-tts-enabled', e => {
      if (status.value) status.value.gift_tts.enabled = e.payload
    }),
    await listen<boolean>('welcome-tts-enabled', e => {
      if (status.value) status.value.welcome.tts_guard = e.payload
    }),
  )
})

onUnmounted(() => {
  if (timer) clearInterval(timer)
  unlistens.forEach(fn => fn())
})

// 朗读条件人话摘要：身份规则之间是「或」，敏感词屏蔽独立叠加，全关 = 不限身份
function filterSummary(f: DanmakuFilter): string {
  const roles: string[] = []
  if (f.enable_guard_admin) roles.push('舰长/房管')
  if (f.enable_medal) roles.push('粉丝牌')
  if (f.enable_wealth) roles.push(`荣耀 ≥ ${f.wealth_min}`)
  const base = roles.length ? `只读 ${roles.join(' / ')}` : '全部弹幕'
  const words = f.enable_sensitive ? f.sensitive_words.filter(w => w.trim() !== '').length : 0
  return words > 0 ? `${base}，屏蔽 ${words} 词` : base
}
</script>

<template>
  <div class="dashboard">
    <h3>运行状态</h3>
    <div class="cards">
      <div class="card">
        <div class="card-label">弹幕速率</div>
        <div class="card-value rate" :class="{ live: (status?.danmaku_rate ?? 0) > 0 }">
          {{ status ? status.danmaku_rate : '—' }}
        </div>
        <div class="card-sub">条 / 10 秒</div>
      </div>

      <div class="card">
        <div class="card-label">弹幕窗</div>
        <div class="quick">
          <div class="quick-row">
            <span class="quick-label">显示</span>
            <ToggleSwitch :model-value="status?.overlay.visible ?? false" @change="toggleOverlay('visible')" />
          </div>
          <div class="quick-row">
            <span class="quick-label">穿透</span>
            <ToggleSwitch
              :model-value="status?.overlay.clickthrough ?? false"
              @change="toggleOverlay('clickthrough')" />
          </div>
          <div class="quick-row">
            <span class="quick-label">置顶</span>
            <ToggleSwitch
              :model-value="status?.overlay.always_on_top ?? false"
              @change="toggleOverlay('alwaysOnTop')" />
          </div>
        </div>
      </div>

      <div class="card">
        <div class="card-label">朗读</div>
        <div class="quick">
          <div class="quick-row">
            <span class="quick-label">全部朗读</span>
            <ToggleSwitch :model-value="status?.tts.master_enabled ?? false" @change="toggleTts('master')" />
          </div>
          <div class="quick-row">
            <span class="quick-label">弹幕朗读</span>
            <ToggleSwitch
              :disabled="!masterOn"
              :model-value="status?.tts.enabled ?? false"
              @change="toggleTts('danmaku')" />
          </div>
          <div class="quick-row">
            <span class="quick-label">礼物朗读</span>
            <ToggleSwitch
              :disabled="!masterOn"
              :model-value="status?.gift_tts.enabled ?? false"
              @change="toggleTts('gift')" />
          </div>
          <div class="quick-row">
            <span class="quick-label">欢迎朗读</span>
            <ToggleSwitch
              :disabled="!masterOn"
              :model-value="status?.welcome.tts_guard ?? false"
              @change="toggleTts('welcome')" />
          </div>
        </div>
        <div class="card-sub">
          {{
            masterOn
              ? status && status.tts.enabled
                ? filterSummary(status.tts.filter)
                : '弹幕朗读已关'
              : '全部朗读已关闭（设置页试听除外）'
          }}
        </div>
      </div>
    </div>
    <p class="dashboard-tip">
      快捷开关与「朗读设置」「主播分区」页是同一份状态。「全部朗读」是总开关，下面三个是分类开关（关掉弹幕朗读不影响礼物
      / 欢迎朗读）。试听不受这里影响；穿透与置顶只对本次运行生效，重启回到默认（不穿透、置顶开）。
    </p>
  </div>
</template>

<style scoped>
.dashboard {
  margin-top: 22px;
}

h3 {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-dim);
  margin-bottom: 10px;
}

/* 窗口变窄时自动折行，避免三列被压扁 */
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 10px;
}

.card {
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px 12px;
  min-width: 0;
}

.card-label {
  font-size: 13px;
  color: var(--text-faint);
  margin-bottom: 6px;
}

.card-value {
  font-size: 17px;
  font-weight: 600;
  color: var(--text);
  line-height: 1.2;
}

/* 速率是这块的视觉重心：大号 + 等宽数字，跳动时宽度不抖 */
.card-value.rate {
  font-size: 30px;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.5px;
}

.card-value.live {
  color: var(--accent);
}

/* 快捷开关：标签在左、开关在右，行高与设置页的开关行接近但不占满整卡 */
.quick {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.quick-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.quick-label {
  font-size: 13px;
  color: var(--text);
}

.card-sub {
  font-size: 12px;
  color: var(--text-dim);
  margin-top: 8px;
  line-height: 1.4;
  overflow-wrap: anywhere;
}

/* 页面自己的说明文字（全局 .tip 是设置页的，字号与间距不同，故另起一个类） */
.dashboard-tip {
  font-size: 13px;
  color: var(--text-faint);
  padding-top: 8px;
  line-height: 1.5;
}
</style>
