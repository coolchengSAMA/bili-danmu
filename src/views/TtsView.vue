<script setup lang="ts">
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { onMounted, onUnmounted, ref } from 'vue'

import DanmakuFilterPanel from '../components/DanmakuFilterPanel.vue'
import SettingRow from '../components/SettingRow.vue'
import ToggleSwitch from '../components/ToggleSwitch.vue'
import { useSaveTip } from '../composables/useSaveTip'
import { useTtsConfig } from '../composables/useTtsConfig'

import type { DanmakuFilter } from '../types/ipc'

// 页内标签：engine / content / filter
const tab = ref<'engine' | 'content' | 'filter'>('engine')

const { showSaved, showError } = useSaveTip()
// 朗读配置与音色列表：与弹幕显示完全解耦，改动即时生效（下一段朗读开始用新参数）
const { config, voices, testing, loadingVoices, customVoice, save, testSpeak, refreshVoices } = useTtsConfig(
  showSaved,
  showError,
)

// 总开关 / 弹幕朗读还有第二个入口（直播间页「运行状态」里的快捷开关）：那边改了要同步过来，
// 否则这里显示的还是旧状态、再改一次配置就把对方刚改的值覆盖回去
let unlistenTts: UnlistenFn | undefined
onMounted(async () => {
  unlistenTts = await listen<{ master_enabled: boolean; enabled: boolean }>('tts-switches', e => {
    config.value.master_enabled = e.payload.master_enabled
    config.value.enabled = e.payload.enabled
  })
})

onUnmounted(() => unlistenTts?.())

function applyFilter(next: DanmakuFilter) {
  config.value.filter = next
  save()
}
</script>

<template>
  <div class="tts-page">
    <!-- 保存 / 错误提示：右下角浮层，不占文档流（出现在卡片内会把页面顶得抖动） -->

    <div class="tabs">
      <button
        v-for="t in [
          { key: 'engine', label: '引擎与音色' },
          { key: 'content', label: '朗读内容' },
          { key: 'filter', label: '朗读筛选' },
        ]"
        :key="t.key"
        class="tab-btn"
        :class="{ active: tab === t.key }"
        @click="tab = t.key as 'engine' | 'content' | 'filter'">
        {{ t.label }}
      </button>
    </div>

    <section v-show="tab === 'engine'" class="tab-pane">
      <div class="setting-card">
        <SettingRow label="全部朗读（总开关）">
          <ToggleSwitch v-model="config.master_enabled" @change="save()" />
        </SettingRow>

        <p class="tip">
          <b>总开关</b>
          管住一切自动朗读：关掉后弹幕、礼物打赏、欢迎信息都不再出声（队列里排着的也一并丢掉、正在念的立刻停）。
          下面（以及「主播分区」里的礼物 / 欢迎朗读）是分类开关，只在总开关打开时生效。
          下面的「试听」不受总开关影响——调音色时随时能听。
        </p>

        <SettingRow label="弹幕朗读">
          <ToggleSwitch v-model="config.enabled" :disabled="!config.master_enabled" @change="save()" />
        </SettingRow>

        <p class="tip">
          关掉只停弹幕，
          <b>礼物朗读与欢迎朗读照常念</b>
          （它们的开关在「主播分区」）。这是「只读打赏、不读弹幕」的用法。
        </p>

        <SettingRow label="音色">
          <div class="voice-control">
            <select v-model="config.voice" class="select" @change="save()">
              <option v-for="v in voices" :key="v.id" :value="v.id">
                {{ v.label }}
              </option>
              <option v-if="customVoice" :value="customVoice">{{ customVoice }}（不在列表中，可点「刷新」重试）</option>
            </select>
            <button class="ghost-btn" :disabled="loadingVoices" @click="refreshVoices()">
              {{ loadingVoices ? '获取中…' : '刷新' }}
            </button>
          </div>
        </SettingRow>

        <SettingRow label="语速">
          <div class="size-control">
            <input v-model.number="config.rate_pct" type="range" min="-50" max="100" step="5" @change="save()" />
            <span class="value">{{ (1 + config.rate_pct / 100).toFixed(2) }}x</span>
          </div>
        </SettingRow>

        <SettingRow label="音量">
          <div class="size-control">
            <input v-model.number="config.volume_pct" type="range" min="-100" max="100" step="5" @change="save()" />
            <span class="value">{{ config.volume_pct > 0 ? '+' : '' }}{{ config.volume_pct }}%</span>
          </div>
        </SettingRow>

        <SettingRow label="试听当前设置">
          <button class="save-btn" :disabled="testing" @click="testSpeak()">
            {{ testing ? '朗读中…' : '试听' }}
          </button>
        </SettingRow>

        <p class="tip">
          使用微软 Edge 的在线语音，共 {{ voices.length }} 个中文音色， 普通话排在最前，其后是方言与粤语 /
          台湾。需要联网：断网或微软忙的时候这一段会没声音，不影响下一条。
        </p>
      </div>
    </section>

    <section v-show="tab === 'content'" class="tab-pane">
      <div class="setting-card">
        <SettingRow label="朗读用户名">
          <ToggleSwitch v-model="config.read_username" @change="save()" />
        </SettingRow>

        <SettingRow label="朗读身份前缀（房管 / 舰长）">
          <ToggleSwitch v-model="config.read_role" @change="save()" />
        </SettingRow>

        <SettingRow label="弹幕堆积时只念最新的（当前这条念完）">
          <ToggleSwitch v-model="config.interrupt_on_backlog" @change="save()" />
        </SettingRow>

        <SettingRow label="弹幕内容最大朗读字数（0 = 不限制）">
          <input v-model.number="config.max_len" type="number" min="0" max="500" class="num" @change="save()" />
        </SettingRow>

        <SettingRow label="最多排队等念的条数（超出丢弃最旧的）">
          <input v-model.number="config.max_queue" type="number" min="1" max="200" class="num" @change="save()" />
        </SettingRow>

        <p class="tip">
          关掉用户名和身份前缀，就只念弹幕内容（默认）。 最大字数只算弹幕正文，用户名和身份前缀不算在内。
          弹幕特别多的直播间，建议同时打开「弹幕堆积时只念最新的」：这样念的总是最新那条，
          正在念的不会被打断（否则可能只听到半句用户名），最多晚一条的时间。
          「最多排队等念的条数」调小，效果类似：念的都是最新弹幕。
        </p>
      </div>
    </section>

    <section v-show="tab === 'filter'" class="tab-pane">
      <p class="tip">这里的规则只影响朗读，与「弹幕 → 弹幕过滤」的显示筛选各自独立。</p>
      <div class="setting-card">
        <SettingRow label="朗读表情弹幕">
          <ToggleSwitch v-model="config.read_emote" @change="save()" />
        </SettingRow>
        <p class="tip">默认关：表情不参与朗读（「今天真好[花]」只念「今天真好」）。打开后表情照念。</p>
      </div>
      <DanmakuFilterPanel :model-value="config.filter" verb="朗读" @change="applyFilter" />
    </section>
  </div>
</template>

<style scoped>
.tts-page {
  display: flex;
  flex-direction: column;
}

/* 设置卡片 / 标签栏 / 开关 / 下拉 / 提示 / 主按钮见全局 src/styles/settings.css */
.select {
  min-width: 220px;
}

.voice-control {
  display: flex;
  align-items: center;
  gap: 8px;
}

.ghost-btn {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 12px;
  font-size: 14px;
  color: var(--text-dim);
  background: none;
  cursor: pointer;
  white-space: nowrap;
}

.ghost-btn:hover:not(:disabled) {
  color: var(--text);
  border-color: var(--accent);
}

.ghost-btn:disabled {
  opacity: 0.6;
  cursor: default;
}

.value {
  /* 数值带百分号 / 倍速后缀，比全局默认宽一点，避免拖动时数字宽度变化挤动滑块 */
  min-width: 52px;
}

.num {
  width: 70px;
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 5px;
  color: var(--text);
  padding: 4px 8px;
  font-size: 15px;
  outline: none;
  text-align: center;
}

.num:focus {
  border-color: var(--accent);
}
</style>
