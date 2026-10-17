<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'

import LoginDialog from './components/LoginDialog.vue'
import SaveTip from './components/SaveTip.vue'
import { refreshLogin } from './composables/useLogin'
import { useSaveTip } from './composables/useSaveTip'
import { checkUpdate } from './composables/useUpdate'
import AboutView from './views/AboutView.vue'
import DanmakuView from './views/DanmakuView.vue'
import RoomView from './views/RoomView.vue'
import StreamerView from './views/StreamerView.vue'
import TtsView from './views/TtsView.vue'

onMounted(() => {
  refreshLogin()
  // 启动后静默查一次新版本：延后 5s 避开启动时的配置读取与连接流程；
  // 结果不弹窗，由关于页的版本徽章展示（silent 失败不打扰）
  setTimeout(() => checkUpdate(true), 5000)
})

type NavKey = 'room' | 'danmaku' | 'tts' | 'streamer' | 'about'

const navs: { key: NavKey; label: string }[] = [
  { key: 'room', label: '直播间连接' },
  { key: 'danmaku', label: '弹幕设置' },
  { key: 'tts', label: '朗读设置' },
  { key: 'streamer', label: '主播分区' },
  { key: 'about', label: '关于软件' },
]

const current = ref<NavKey>('room')

// 切页回到顶部：5 个页面共用同一个滚动容器（v-show 常驻、不销毁），
// 不重置的话从「弹幕设置」滚到一半切到「主播分区」，会停在新页面中间（新页面更短时直接停在底部）。
const contentEl = ref<HTMLElement | null>(null)
watch(current, () => {
  if (contentEl.value) contentEl.value.scrollTop = 0
})

// 保存 / 操作失败提示是**全局单例**（`composables/useSaveTip`）：这里只挂一次浮层，
// 任何页面或子组件（弹幕窗 / 发送框这些标签页）直接调用 showSaved() / showError() 即可。
// 以前各页各挂一个、两个 fork 自有标签页干脆内联，导致「有的页面是浮层、有的内联、有的没有」。
const { savedTip, opError } = useSaveTip()
</script>

<template>
  <div class="layout">
    <aside class="sidebar">
      <div class="brand">
        <span class="brand-name">bili-danmu</span>
        <span class="brand-edition">streamer</span>
      </div>
      <nav>
        <button
          v-for="n in navs"
          :key="n.key"
          class="nav-item"
          :class="{ active: current === n.key }"
          @click="current = n.key">
          {{ n.label }}
        </button>
      </nav>
    </aside>
    <main ref="contentEl" class="content">
      <!-- v-show 常驻渲染：切换页面不销毁组件，连接状态/弹幕列表保留 -->
      <RoomView v-show="current === 'room'" />
      <DanmakuView v-show="current === 'danmaku'" />
      <TtsView v-show="current === 'tts'" />
      <StreamerView v-show="current === 'streamer'" />
      <AboutView v-show="current === 'about'" />
    </main>
  </div>
  <LoginDialog />
  <SaveTip :saved="savedTip" :error="opError" />
</template>

<style scoped>
.layout {
  display: flex;
  height: 100%;
}

.sidebar {
  width: 150px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  background: var(--bg-side);
}

/* 品牌区：品牌名在左、分支徽章在右，同一行。
   侧边栏只有 150px（左右 padding 各 11px → 内宽 128px），所以字号都得收紧：
   14px 的「bili-danmu」约 69px，徽章约 51px，加 4px 间隔共 124px，留 4px 余量。
   万一字体/缩放下仍超宽：徽章 flex-shrink:0 保持完整，品牌名退化成省略号。 */
.brand {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 4px;
  padding: 14px 11px;
  border-bottom: 1px solid var(--border);
}

.brand-name {
  font-weight: 600;
  font-size: 14px;
  line-height: 1.2;
  color: var(--accent);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 分支徽章：与「未开播」等状态徽章同一套视觉（1px 描边 + 4px 圆角 + 弱化文字色） */
.brand-edition {
  flex-shrink: 0;
  padding: 0 4px;
  font-size: 9px;
  font-weight: 400;
  line-height: 1.8;
  letter-spacing: 0.2px;
  color: var(--text-faint);
  border: 1px solid var(--border);
  border-radius: 4px;
}

nav {
  display: flex;
  flex-direction: column;
  padding: 8px;
  gap: 2px;
}

.nav-item {
  background: none;
  border: none;
  color: var(--text-dim);
  text-align: left;
  padding: 9px 12px;
  border-radius: 6px;
  font-size: 15px;
  font-weight: 600;
}

.nav-item:hover {
  background: var(--hover);
}

.nav-item.active {
  background: var(--accent-soft);
  color: var(--accent-strong-text);
}

.content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}
</style>
