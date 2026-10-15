import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { computed, onMounted, onUnmounted, ref } from 'vue'

import type { RecentRoom, RoomStatusEvent } from '../types/ipc'

const STATUS_TEXT: Record<string, string> = {
  disconnected: '未连接',
  connecting: '连接中…',
  connected: '已连接',
  error: '连接失败',
}

/**
 * 从用户输入里取房间号
 *
 * 除了直接输房间号，也支持把直播间链接整条粘进来（主播日常就是这么复制地址的）：
 * - 纯数字：原样用（不限制位数，历史上有很短的房间号）
 * - 直播间链接：先按域名 + 路径抠（`live.bilibili.com/22312451`、`/blanc/123` 都能认），
 *   避免被查询串里的数字（`?visit_id=…`）带偏
 * - 其它：退化成「取最后一段 3 位以上数字」
 *
 * 取不到合法房间号时返回 0，由调用方给提示。
 */
function parseRoomId(raw: string): number {
  const text = raw.trim()
  if (!text) return 0
  const picked = /^\d+$/.test(text)
    ? text
    : (/live\.bilibili\.com\/(?:blanc\/|blackboard\/)?(\d+)/.exec(text)?.[1] ?? text.match(/\d{3,}/g)?.pop() ?? '')
  const id = Number(picked)
  return Number.isSafeInteger(id) && id > 0 ? id : 0
}

/**
 * 直播间连接：连接 / 断开、状态订阅与最近房间面包屑。
 *
 * 状态来源有两条：room-status 事件（主）与 get_connection_status 轮询由 Overlay 负责；
 * 这里只订阅事件，并在挂载时补拉一次最近房间。卸载时自动退订。
 */
export function useRoomConnection() {
  const roomId = ref('')
  const busy = ref(false) // 连接/断开操作中
  const status = ref<RoomStatusEvent>({ state: 'disconnected' })
  const errorMsg = ref('')
  // 最近连接过的直播间（Rust 持久化，输入框下方面包屑）
  const recentRooms = ref<RecentRoom[]>([])

  let unlistenStatus: UnlistenFn | undefined
  let unlistenRecent: UnlistenFn | undefined

  // 连接中/已连接时锁定房间号输入框与最近面包屑：改号需先断开，避免输入框与实际连接目标不一致
  const locked = computed(() => status.value.state === 'connecting' || status.value.state === 'connected')

  const statusText = computed(() => STATUS_TEXT[status.value.state] ?? status.value.state)

  /**
   * 连接直播间
   *
   * `target` 为面包屑 / 自己房间卡片直连的房间号，会回填输入框保证与连接目标一致；
   * 从输入框连接时允许粘贴直播间链接（见 `parseRoomId`），并把规范化后的房间号写回输入框。
   */
  async function connect(target?: number) {
    if (target !== undefined) {
      roomId.value = String(target)
    }
    const id = parseRoomId(roomId.value)
    if (!id) {
      errorMsg.value = '请输入有效的直播间 ID（也可以直接粘贴直播间链接）'
      return
    }
    roomId.value = String(id)
    errorMsg.value = ''
    busy.value = true
    try {
      await invoke('connect_room', { roomId: id })
    } catch (e) {
      status.value = { state: 'error', message: String(e) }
    } finally {
      busy.value = false
    }
  }

  async function disconnect() {
    busy.value = true
    try {
      await invoke('disconnect_room')
    } catch (e) {
      errorMsg.value = String(e)
    } finally {
      busy.value = false
    }
  }

  // 读取最近房间（连接成功后 Rust 侧已写入，含主播名）
  async function loadRecentRooms() {
    try {
      recentRooms.value = await invoke<RecentRoom[]>('get_recent_rooms')
    } catch {
      // 读取失败不影响连接功能，保持空列表
    }
  }

  onMounted(async () => {
    await loadRecentRooms()
    unlistenStatus = await listen<RoomStatusEvent>('room-status', e => {
      status.value = e.payload
      if (e.payload.state === 'connected') {
        errorMsg.value = ''
      }
    })
    // 最近房间由 Rust 后台任务抽取（与连接并行，不阻塞弹幕会话），写完广播一次
    unlistenRecent = await listen('recent-rooms-changed', () => {
      void loadRecentRooms()
    })
  })

  onUnmounted(() => {
    unlistenStatus?.()
    unlistenRecent?.()
  })

  return {
    roomId,
    busy,
    status,
    errorMsg,
    recentRooms,
    locked,
    statusText,
    connect,
    disconnect,
  }
}
