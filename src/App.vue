<script setup lang="ts">
import { onMounted, ref } from 'vue'

import LoginDialog from './components/LoginDialog.vue'
import { refreshLogin } from './composables/useLogin'
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
    <main class="content">
      <!-- v-show 常驻渲染：切换页面不销毁组件，连接状态/弹幕列表保留 -->
      <RoomView v-show="current === 'room'" />
      <DanmakuView v-show="current === 'danmaku'" />
      <TtsView v-show="current === 'tts'" />
      <StreamerView v-show="current === 'streamer'" />
      <AboutView v-show="current === 'about'" />
    </main>
  </div>
  <LoginDialog />
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

/* 品牌区：侧边栏只有 150px（内宽 122px），「bili-danmu + 徽章」一行放不下
   （17px 品牌名约 95px，徽章再小也塞不进去），所以品牌名一行、徽章另起一行 */
.brand {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 5px;
  padding: 14px 14px 12px;
  border-bottom: 1px solid var(--border);
}

.brand-name {
  font-weight: 600;
  font-size: 16px;
  line-height: 1.2;
  color: var(--accent);
}

/* 分支徽章：胶囊小卡片（柔和底色 + 对比色文字），比描边方框更像版本标签 */
.brand-edition {
  padding: 1px 7px 2px;
  font-size: 10px;
  font-weight: 500;
  line-height: 1.6;
  letter-spacing: 0.5px;
  color: var(--accent-strong-text);
  background: var(--accent-soft);
  border-radius: 999px;
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
  padding: 20px;
}
</style>
