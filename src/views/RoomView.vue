<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { onMounted, onUnmounted, ref, watch } from 'vue'

import StatusDashboard from '../components/StatusDashboard.vue'
import { refreshLogin, useLogin } from '../composables/useLogin'
import { useRoomConnection } from '../composables/useRoomConnection'

import type { FollowingLive, MyRoomInfo } from '../types/ipc'

onMounted(refreshLogin)

const { loggedIn, uid, uname, openLoginDialog, logout } = useLogin()
const { roomId, busy, status, errorMsg, recentRooms, locked, statusText, connect, disconnect } = useRoomConnection()

// ── streamer 分支：我的直播间（置顶优先展示，一键连自己房间）──
// 数据源 getRoomInfoOld?mid=<uid>：匿名可调，一次给出真实房间号 / 标题 / 开播状态 / 在线人数。
// 注意靓号主播 uid ≠ 房间号，所以不能拿 uid 当房间号查（上游 room_init 那条路走不通）。
const myRoom = ref<MyRoomInfo | null>(null)
const myRoomBusy = ref(false)
const myRoomErr = ref('')
const myRoomLoaded = ref(false)

async function loadMyRoom() {
  if (!loggedIn.value) return
  myRoomBusy.value = true
  myRoomErr.value = ''
  try {
    const info = await invoke<MyRoomInfo | null>('my_room_info')
    // 刷新期间退出登录：丢弃晚到的结果，避免登出后又冒出卡片内容
    if (!loggedIn.value) return
    myRoom.value = info
    myRoomLoaded.value = true
  } catch (e) {
    if (!loggedIn.value) return
    myRoomErr.value = String(e)
  } finally {
    myRoomBusy.value = false
  }
}

function connectMyRoom() {
  const id = myRoom.value?.room_id
  if (id) void connect(id)
}

function liveText(s: number) {
  return s === 1 ? '直播中' : s === 2 ? '轮播中' : '未开播'
}

// 卡片 60s 自动刷新开播状态与在线人数：只在「窗口可见 + 已登录」时真的发请求，
// 页面切到别的标签或最小化时不打扰（房间页是 v-show 常挂载的，所以要自己挡一层）
let myRoomTimer: number | undefined
onMounted(() => {
  myRoomTimer = window.setInterval(() => {
    if (document.visibilityState !== 'visible') return
    if (loggedIn.value) void loadMyRoom()
  }, 60_000)
})
onUnmounted(() => {
  if (myRoomTimer !== undefined) window.clearInterval(myRoomTimer)
})

// ── streamer 分支：最近连接 / 关注在播 折叠（默认收起，主播更常用的是上面那张卡片）──
const recentOpen = ref(false)
const followingOpen = ref(false)

/// 关注在播改为「展开时才查」：上游是登录即查，而streamer 分支默认用不到它，
/// 白翻最多 10 页关注列表不值当
async function toggleFollowing() {
  followingOpen.value = !followingOpen.value
  if (followingOpen.value && !followingLoaded.value && !followingBusy.value) {
    await loadFollowingLive()
  }
}

// 「关注在播」：只在点刷新时查（两个接口，不随页面挂载自动拉）
const following = ref<FollowingLive[]>([])
const followingBusy = ref(false)
const followingErr = ref('')
const followingLoaded = ref(false)

async function loadFollowingLive() {
  followingBusy.value = true
  followingErr.value = ''
  try {
    const list = await invoke<FollowingLive[]>('list_following_live')
    // 刷新期间退出登录：丢弃晚到的结果，避免登出后又冒出列表
    if (!loggedIn.value) return
    following.value = list
    followingLoaded.value = true
  } catch (e) {
    if (!loggedIn.value) return
    followingErr.value = String(e)
  } finally {
    followingBusy.value = false
  }
}

// 登录态变化：我的直播间跟着查一次；登出时清掉卡片数据（关注在播在 handleLogout 里清）
watch(loggedIn, on => {
  if (on) {
    void loadMyRoom()
  } else {
    myRoom.value = null
    myRoomLoaded.value = false
    myRoomErr.value = ''
  }
})

// 退出登录：先断开直播连接，再清登录态（登录是收弹幕的前提）
async function handleLogout() {
  if (status.value.state === 'connected' || status.value.state === 'connecting') {
    await disconnect()
  }
  await logout()
  // 关注在播是登录态数据，退出后清掉并回到「未刷新」
  following.value = []
  followingLoaded.value = false
  followingErr.value = ''
  myRoom.value = null
  myRoomLoaded.value = false
  myRoomErr.value = ''
  recentOpen.value = false
  followingOpen.value = false
}
</script>

<template>
  <div class="room-page">
    <div class="login-bar" :class="{ logged: loggedIn }">
      <template v-if="loggedIn">
        <span class="ok">
          ✓ 已登录
          <span v-if="uname">：{{ uname }}</span>
          <span v-else>（UID {{ uid }}）</span>
        </span>
        <button class="link-btn" @click="handleLogout">退出登录</button>
      </template>
      <template v-else>
        <span class="warn">⚠ 未登录，B 站需要登录才能接收弹幕</span>
        <button class="link-btn primary" @click="openLoginDialog()">扫码登录</button>
      </template>
    </div>

    <h2>直播间</h2>

    <!-- streamer 分支：我的直播间（置顶优先展示，一键连自己房间） -->
    <div v-if="loggedIn" class="my-room-card" :class="{ live: myRoom?.live_status === 1 }">
      <div class="my-room-info">
        <span class="my-room-label">我的直播间</span>
        <span v-if="myRoomBusy && !myRoomLoaded" class="recent-label">查询中…</span>
        <span v-else-if="myRoomErr" class="error inline">{{ myRoomErr }}</span>
        <span v-else-if="myRoomLoaded && !myRoom" class="recent-label">这个账号还没有直播间（从未开播过）</span>
        <template v-else-if="myRoom">
          <span class="state-badge" :class="myRoom.live_status === 1 ? 'on' : 'off'">
            {{ liveText(myRoom.live_status) }}
          </span>
          <span class="my-room-title">{{ myRoom.title || '（未设置标题）' }}</span>
          <span class="recent-label">房间 {{ myRoom.room_id }}</span>
          <span v-if="myRoom.online_hidden" class="recent-label">人数已对观众隐藏</span>
          <span v-else class="recent-label">在线 {{ myRoom.online }}</span>
        </template>
      </div>
      <div class="my-room-actions">
        <button class="link-btn" :disabled="myRoomBusy" @click="loadMyRoom">
          {{ myRoomBusy ? '刷新中…' : '刷新' }}
        </button>
        <button
          v-if="myRoom"
          class="btn primary small"
          :disabled="busy || locked || status.state === 'connecting'"
          @click="connectMyRoom">
          连自己房间
        </button>
      </div>
    </div>

    <div class="row">
      <input
        v-model="roomId"
        class="room-input"
        type="text"
        placeholder="输入直播间 ID 或直播间链接，如 22312451"
        :disabled="locked"
        @keyup.enter="connect()" />
      <button v-if="status.state === 'connected'" class="btn danger" :disabled="busy" @click="disconnect">断开</button>
      <button v-else class="btn primary" :disabled="busy || status.state === 'connecting'" @click="connect()">
        连接
      </button>
    </div>

    <!-- 失败提示紧贴输入框：原来放在折叠区之后，离刚操作的地方隔了上百像素 -->
    <p v-if="errorMsg" class="error">{{ errorMsg }}</p>

    <!-- streamer 分支：最近连接与关注在播都折叠（默认收起，需要时展开） -->
    <div v-if="recentRooms.length" class="fold-bar">
      <button class="fold-head" @click="recentOpen = !recentOpen">
        <span class="fold-arrow">{{ recentOpen ? '▾' : '▸' }}</span>
        最近连接
        <span class="recent-label">（{{ recentRooms.length }}）</span>
      </button>
      <div v-if="recentOpen" class="fold-body">
        <button
          v-for="r in recentRooms"
          :key="r.room_id"
          class="crumb"
          :title="`房间 ${r.room_id}`"
          :disabled="busy || locked"
          @click="connect(r.room_id)">
          {{ r.uname || r.room_id }}
        </button>
      </div>
    </div>

    <div v-if="loggedIn" class="fold-bar">
      <button class="fold-head" @click="toggleFollowing()">
        <span class="fold-arrow">{{ followingOpen ? '▾' : '▸' }}</span>
        关注在播
        <span v-if="followingOpen && following.length" class="recent-label">（{{ following.length }}）</span>
      </button>
      <div v-if="followingOpen" class="fold-body">
        <button class="link-btn" :disabled="followingBusy || !loggedIn" @click="loadFollowingLive">
          {{ followingBusy ? '查询中…' : '刷新' }}
        </button>
        <span v-if="followingErr" class="error inline">{{ followingErr }}</span>
        <span v-else-if="followingLoaded && !following.length" class="recent-label">（当前没有关注的主播在播）</span>
        <button
          v-for="f in following"
          :key="f.uid"
          class="crumb"
          :title="`房间 ${f.room_id}`"
          :disabled="busy || locked"
          @click="connect(f.room_id)">
          {{ f.uname }}
        </button>
      </div>
    </div>

    <div class="status-box">
      <span
        class="dot"
        :class="{
          green: status.state === 'connected',
          yellow: status.state === 'connecting',
          red: status.state === 'error',
          gray: status.state === 'disconnected',
        }"></span>
      <span>{{ statusText }}</span>
      <span v-if="status.state === 'connected'" class="room-tag">房间 {{ status.roomId }}（30s 心跳保活中）</span>
      <span v-if="status.state === 'error' && status.message" class="room-tag">
        {{ status.message }}
      </span>
    </div>

    <StatusDashboard />
  </div>
</template>

<style scoped>
.room-page {
  display: flex;
  flex-direction: column;
}

.login-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 10px;
  margin-bottom: 16px;
  font-size: 15px;
}

.login-bar .ok {
  color: var(--green);
}

.login-bar .warn {
  color: var(--yellow);
}

.link-btn {
  background: none;
  border: none;
  color: var(--accent);
  font-size: 15px;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 4px;
}

.link-btn:hover {
  background: var(--hover);
}

h2 {
  font-size: 18px;
  margin-bottom: 14px;
}

.row {
  display: flex;
  gap: 8px;
}

.room-input {
  flex: 1;
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  padding: 8px 10px;
  font-size: 16px;
  outline: none;
}

.room-input:focus {
  border-color: var(--accent);
}

/* 锁定态与按钮禁用观感统一 */
.room-input:disabled {
  color: var(--text-faint);
  cursor: not-allowed;
  opacity: 0.7;
}

.btn {
  border: none;
  border-radius: 6px;
  padding: 8px 18px;
  font-size: 16px;
  color: #fff;
}

.btn.primary {
  background: var(--accent);
}

.btn.primary:hover {
  background: var(--accent-hover);
}

.btn.danger {
  background: var(--danger);
}

.btn.danger:hover {
  background: var(--danger-hover);
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.error {
  color: var(--red);
  font-size: 15px;
  margin-top: 8px;
}

/* streamer 分支：我的直播间卡片（置顶）；左侧竖条在开播时变绿，一眼看出「我是不是在播」 */
.my-room-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-left: 3px solid var(--gray);
  border-radius: 6px;
  padding: 8px 10px;
  margin-bottom: 14px;
}

.my-room-card.live {
  border-left-color: var(--green);
}

.my-room-info {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px 10px;
  min-width: 0;
}

.my-room-label {
  color: var(--text);
  font-size: 15px;
  font-weight: 600;
}

.my-room-title {
  font-size: 15px;
  max-width: 18em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.state-badge {
  font-size: 13px;
  border-radius: 4px;
  padding: 1px 6px;
  border: 1px solid var(--border);
  color: var(--text-faint);
}

.state-badge.on {
  color: var(--green);
  border-color: var(--green);
}

.my-room-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.btn.small {
  padding: 4px 10px;
  font-size: 14px;
}

/* 折叠区（最近连接 / 关注在播）：标题一行，展开才显示内容 */
.fold-bar {
  margin-top: 10px;
}

.fold-head {
  background: none;
  border: none;
  /* 命中区比文字大一圈：原来只有 2px 竖向 padding + 14px 小字，
     鼠标容易点不中（折叠区是这两块唯一入口） */
  padding: 6px 2px;
  border-radius: 4px;
  color: var(--text-faint);
  font-size: 14px;
  cursor: pointer;
}

.fold-head:hover {
  color: var(--text);
}

.fold-arrow {
  display: inline-block;
  width: 1em;
}

.fold-body {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px 8px;
  margin-top: 6px;
  padding-left: 1em;
}

/* 折叠区第一个名字前面不是 .crumb，分隔符单独补一条 */
.fold-body .crumb::before {
  content: '›';
  color: var(--text-faint);
  margin-right: 8px;
}

.error.inline {
  margin: 0;
  font-size: 14px;
}

.recent-label {
  color: var(--text-faint);
  font-size: 14px;
}

.crumb {
  background: none;
  border: none;
  padding: 2px 0;
  color: var(--accent);
  font-size: 14px;
  cursor: pointer;
  max-width: 12em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.crumb + .crumb::before {
  content: '›';
  color: var(--text-faint);
  margin-right: 8px;
}

.crumb:hover:not(:disabled) {
  text-decoration: underline;
}

.crumb:disabled {
  color: var(--text-faint);
  cursor: not-allowed;
}

.status-box {
  margin-top: 18px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 16px;
}

.dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
}

.dot.green {
  background: var(--green);
}
.dot.yellow {
  background: var(--yellow);
}
.dot.red {
  background: var(--red);
}
.dot.gray {
  background: var(--gray);
}

.room-tag {
  color: var(--text-faint);
  font-size: 14px;
}
</style>
