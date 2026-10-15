<script setup lang="ts">
import { getVersion } from '@tauri-apps/api/app'
import { invoke } from '@tauri-apps/api/core'
import { computed, onMounted, onUnmounted, ref } from 'vue'

import { checkUpdate, openUpdatePage, useUpdate } from '../composables/useUpdate'

// 软件版本号：取自 Tauri 包信息（与 src-tauri/Cargo.toml / tauri.conf.json 同源），
// 不在前端再抄一份；浏览器里跑（pnpm dev 无 Tauri 注入）时拿不到，徽章自行隐藏
const version = ref('')

// 仓库地址：streamer 分支自己的仓库。页面展示短形式，复制到剪贴板的是带协议的规范 URL。
// 上游仓库地址与原作者的支持入口在这里不再展示（出处与许可仍完整保留在仓库的
// README / STREAMER.md / LICENSE 里，GPL-3.0 要求的部分不能省）。
const REPO_LABEL = 'github.com/coolchengSAMA/bili-danmu'
const REPO_URL = `https://${REPO_LABEL}`

const copied = ref(false)
const copyError = ref('')
const logError = ref('')
let tipTimer: ReturnType<typeof setTimeout> | undefined

// 不用 <a target="_blank">：Tauri 默认不处理 WebView2 的新窗口请求（wry 会直接
// SetHandled(true) 吞掉），链接点了没反应；剪贴板是纯 Web API，无需插件。
async function copyRepoUrl() {
  copyError.value = ''
  try {
    await navigator.clipboard.writeText(REPO_URL)
    copied.value = true
    if (tipTimer) clearTimeout(tipTimer)
    tipTimer = setTimeout(() => (copied.value = false), 1500)
  } catch {
    // 剪贴板被拒（罕见）：地址本身可选中，退化成手动复制
    copyError.value = '复制失败，请手动选中地址复制'
  }
}

// ---- 日志目录 ----
// 「朗读不出声 / 收不到弹幕」这类问题只能靠日志定位，按钮放在标题行（不另占一行，
// 关于页总高已接近内容区上限）；目录由 Rust 侧建好后用资源管理器打开。
async function openLogDir() {
  logError.value = ''
  try {
    await invoke('open_log_dir')
  } catch (e) {
    logError.value = String(e)
  }
}

// ---- 版本更新提醒 ----
// 结果来自模块级单例（App 启动 5s 后已静默查过一次），这里只负责展示与手动重试
const { checking, info, error: updateError } = useUpdate()

// 只有手动点过才显示「已是最新」：启动自动检查无结果时不该多一行无用文案
const checked = ref(false)

const hasUpdate = computed(() => Boolean(info.value?.has_update))

// 一个按钮走完「检查更新 → 检查中… → 发现新版本 / 已是最新 / 重试」全部状态，不另占一行
const updateLabel = computed(() => {
  if (checking.value) return '检查中…'
  if (updateError.value) return '重试'
  if (hasUpdate.value) return `发现新版本 v${info.value!.latest}`
  return checked.value ? '已是最新' : '检查更新'
})

async function onUpdateClick() {
  if (hasUpdate.value) {
    await openUpdatePage()
    return
  }
  checked.value = true
  await checkUpdate()
}

// 按钮提示：有新版说明点按去向，失败说明原因——两者都用 hover 提示，不占页面高度
const updateTitle = computed(() => (hasUpdate.value ? '在浏览器打开下载页' : updateError.value))

onMounted(async () => {
  try {
    version.value = await getVersion()
  } catch {
    // 拿不到版本号就不显示徽章，不影响页面其它内容
  }
})

onUnmounted(() => {
  if (tipTimer) clearTimeout(tipTimer)
})
</script>

<template>
  <div class="about-page">
    <!-- 「关于」包一层 span 是为了让版本徽章与文字之间只隔空白节点：
         纯空白节点（含换行）会被 Vue 丢掉，而文本节点里的换行会变成一格可见空格，
         把标题顶出一格。间距交给 .version 的 margin-left。 -->
    <h2>
      <span>关于</span>
      <span v-if="version" class="version">v{{ version }}</span>
      <!-- 更新提醒与版本徽章同行：一个按钮承载全部状态，不另占一行 -->
      <button
        class="pill-btn"
        :class="{ 'has-update': hasUpdate }"
        :disabled="checking"
        :title="updateTitle"
        @click="onUpdateClick()">
        {{ updateLabel }}
      </button>
      <!-- 日志按钮与更新徽章同行：排障时用户要能自己找到日志文件 -->
      <button
        class="pill-btn"
        :title="logError || '打开日志目录（反馈问题时请附上最新的 bili-danmu.log）'"
        @click="openLogDir()">
        日志
      </button>
    </h2>

    <p class="intro">
      bili-danmu-streamer：轻量级 B 站直播弹幕助手（个人自用分支）。连接直播间，弹幕实时显示在桌面透明悬浮层， 给用 OBS
      推流的主播一个干净、不挡画面的看弹幕工具。
    </p>

    <p class="intro">
      我在上游基础上按自己的使用体验加了：置顶「我的直播间」一键直连、发送框整条隐藏 +
      全局快捷键、测试弹幕、弹幕窗边界提示与缩放手柄； 差异清单与出处见仓库 README 与 STREAMER.md。
    </p>

    <div class="card">
      <h3>项目地址</h3>
      <div class="repo-row">
        <span class="repo-url">{{ REPO_LABEL }}</span>
        <button class="copy-btn" @click="copyRepoUrl()">
          {{ copied ? '✓ 已复制' : '复制' }}
        </button>
      </div>
      <p v-if="copyError" class="repo-error">{{ copyError }}</p>
      <p v-if="logError" class="repo-error">{{ logError }}</p>
    </div>

    <div class="card">
      <h3>致谢开源</h3>
      <ul class="thanks">
        <li>B 站弹幕协议相关接口的公开分析与实现</li>
        <li>
          <a href="https://github.com/SoraYjy/DanmuFree" target="_blank" rel="noreferrer">DanmuFree</a>
          （认证流程与 WBI 签名对齐的参考实现）
        </li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.about-page {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

h2 {
  font-size: 18px;
  margin-bottom: 6px;
}

/* 版本号徽章：与标题同行，不另占一行（关于页总高要压在内容区内） */
.version {
  margin-left: 6px;
  font-size: 13px;
  font-weight: 400;
  color: var(--text-faint);
}

.intro {
  font-size: 15px;
  line-height: 1.8;
  color: var(--text-dim);
}

.card {
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 10px 12px;
}

.card h3 {
  font-size: 15px;
  color: var(--accent);
  margin-bottom: 8px;
}

.thanks {
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 15px;
  line-height: 1.7;
}

.thanks a {
  color: var(--accent);
  text-decoration: none;
}

.thanks a:hover {
  text-decoration: underline;
}

.repo-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.repo-url {
  font-size: 15px;
  color: var(--text-dim);
  /* 全局禁了选中，这里放开：复制失败时还能手动选中 */
  user-select: text;
  overflow-wrap: anywhere;
}

.copy-btn {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 5px 12px;
  font-size: 14px;
  color: var(--text-dim);
  background: none;
  cursor: pointer;
  white-space: nowrap;
}

.copy-btn:hover {
  color: var(--text);
  border-color: var(--accent);
}

.repo-error {
  font-size: 14px;
  color: var(--red);
  padding-top: 6px;
}

/* 页头小胶囊（更新徽章 + 日志按钮）：与版本徽章同行，13px 不撑高 h2。更新按钮一个
   按钮承载「检查更新 / 检查中… / 发现新版本 / 已是最新 / 重试」五态（见 updateLabel），
   避免再添一行提示文字。 */
.pill-btn {
  margin-left: 8px;
  padding: 2px 10px;
  font-size: 13px;
  font-weight: 400;
  color: var(--text-faint);
  background: none;
  border: 1px solid var(--border);
  border-radius: 999px;
}

.pill-btn:hover:not(:disabled) {
  color: var(--text);
  border-color: var(--accent);
}

.pill-btn:disabled {
  cursor: default;
}

/* 有新版：换成强调色胶囊，比灰色的「检查更新」显眼 */
.pill-btn.has-update {
  color: var(--accent-strong-text);
  background: var(--accent-soft);
  border-color: var(--accent);
}
</style>
