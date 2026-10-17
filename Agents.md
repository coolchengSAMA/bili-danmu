# bili-danmu 开发指南

## 项目概览

轻量级 B 站直播弹幕桌面助手（Windows）：连接 B 站直播间 → 获取实时弹幕 → 在桌面透明悬浮层（Overlay）显示 + 可选 Edge TTS 朗读，供 OBS 推流主播看/听弹幕。Tauri 2 多窗口架构：主窗口（配置）+ 透明悬浮窗（Overlay）+ 发送弹幕框（Sender，始终吸附 Overlay 下方）。

## 礼物系统与欢迎信息（均已上线）

打赏（礼物 / SC / 上舰）的**弹幕窗渲染**与**朗读**都已上线：解析 → 金额门槛 + 连击合并（`src-tauri/src/gift.rs`）→ 弹幕窗内独立礼物区（弹幕列表上方）；朗读侧另算一遍门槛与连击（`src-tauri/src/tts/gift.rs`）→ 插队送进 Edge TTS 队列。

- 解析：`parser/gift.rs` 把 `SEND_GIFT` / `COMBO_SEND` / `SUPER_CHAT_MESSAGE` / `GUARD_BUY` 解成 `BilibiliEvent::Backing`；金额统一折算成整数「分」（`amount_fen`，金瓜子 1000 = 1 元），免费礼物（银瓜子 / 零价）直接返回 None
- `COMBO_SEND` **只当连击仍在继续的信号**（延长合并窗口），数量与金额一律以 `SEND_GIFT` 为准——两路事件同时下发，两边都计就重复；它是否带价格字段未实测（用 `scripts/ws-probe.ps1` 验证）
- 门槛与连击合并都在 Rust（`gift::on_backing`）：门槛按**累加后的总额**判定，未过门槛的连击先攒在分组里；同人同礼物在 `combo_window_secs` 内合并成一行，行 id = 分组键 + 首次时间戳，前端按 id **覆盖**更新（位置不变）
- 渲染：`GiftRow.vue` + `MetaBadges.vue`。徽章列抽成 `MetaBadges` 是**故意共用**的——`role-slot` 宽度与 `--role-indent` / 发送框左缩进（`window.rs` `sender_layout_metrics`）同源，复制一份 CSS 后改单侧就会错位；`row-style.ts` 同理（两行共用描边计算）。**礼物行不显示粉丝牌**：粉丝牌由弹幕行组装成 `medal` 对象传入，礼物行不传就不渲染（打赏行已有用户名与礼物名）；打赏事件（`Backing`）因此不再下发 `medal_*` 字段，前端也不再有 `DisplayBacking`，礼物行直接用 `BackingEvent`
- 设置项在「主播分区 → 礼物渲染」标签：开关 / 金额门槛 / 条数上限 / 合并窗口
- 礼物朗读（`tts/gift.rs` + `tts::on_backing`）：文案「感谢老板A送的5个辣条」/ SC 念留言正文（超 50 字截断）/ 上舰念档位；**插队**走 `TtsState::push_front`（排到待朗读弹幕之前），语义是「当前这条念完马上接」，**不**掐正在念的那条
- 礼物朗读**自己**按「用户 + 礼物」累加一遍，**不复用** `gift.rs` 的分组表：渲染侧只在过渲染门槛时才产出累计行，而朗读门槛与它独立（`GiftTtsConfig`，可更低）；连击静默满一个窗口才念一次，窗口沿用 `GiftConfig::combo_window_secs`（`tts::combo_window` 真接读 `OverlayState.gift`，改窗口只需改一处）；定时器是常驻轮询任务（`spawn_gift_timer`，1 秒一跳：按窗口整跳会让汇总最多晚一个窗口）
- 队列项有两个布尔位，语义别混：`force` = **压力下别丢**（不被新弹幕挤掉、不被「积压打断」作废、熔断清积压时保留；礼物 / 欢迎 / 试听都带）；`manual` = **用户当场点的试听**，是唯一能绕过朗读总开关的项
- **朗读总开关（`tts.enabled`）管一切自动朗读**：弹幕、礼物打赏、欢迎信息都由它管住。关掉时 `set_config` 只 `retain(manual)`（连礼物一起清）、`epoch++` 作废在途合成，播放端 `stop_now` 与播放前的检查都按「非 manual + 总开关关」立刻停/丢弃。**只有设置页「试听」例外**（`manual = true`，调音色时总开关关着也要能听见）。各功能自己的开关（`gift_tts.enabled` / `welcome.tts_guard` / 朗读筛选）都是**第二道闸**，别再把它们写成「独立于总开关」
- 队列丢弃只丢**弹幕**（`tts/mod.rs` `push` 找最前的非 force 项）：插队的礼物在队首，直接 `pop_front` 会被下一条弹幕顶掉——高频直播间每秒几十条，上舰/醒目留言这种只来一次的大额插队后一条都活不到开播；熔断退避只 retain force 项（总开关关闭则只 retain manual，见上条）
- 礼物朗读：进队带 `force = true`、`push_front` 插队；`GiftTtsConfig` 独立开关与金额门槛是第二道闸，总开关关着时不念。设置项在「主播分区 → 礼物朗读」标签
- 未做：礼物图标（渲染层已留扩展位）
- 待验证：`rnd` 去重未做——重连后服务端若重推同一条 `SEND_GIFT`，窗口内会重复累加，数量与金额翻倍

## 欢迎信息（进房 / 关注 / 分享 / 点赞 / 舰长进场）

五个 cmd 解成 `BilibiliEvent::Welcome` → `welcome.rs` 去重限速 → 两个独立消费者（渲染 / 朗读）。

- 解析（`parser/welcome.rs`，字段号全部实测得出，见 `Task/findings.md`）：
  - `INTERACT_WORD_V2` —— **不是 JSON 是 base64 protobuf**（`1`=uid、`2`=用户名、`5`=msg_type、`7`=时间戳秒），msg_type `1/2/3` → 进房 / 关注 / 分享，其余取值不猜语义直接丢。`INTERACT_WORD`（V1）已不存在
  - `LIKE_INFO_V3_CLICK` —— 单次点赞，纯 JSON，`uinfo.guard.level` 直接可读
  - `LIKE_INFO_V3_UPDATE` —— 只有累计点赞总数（`click_count`），**不是事件**，故意不解析
  - `ENTRY_EFFECT` —— 舰长 / 提督 / 总督进场，**按 `uinfo.guard.level > 0` 过滤**：实测荣耀等级高的普通观众也会触发这条，不过滤会把「上舰欢迎」做成「高等级观众欢迎」
- **进房事件里没有 guard_level**（60 个样本全字段扫描确认）：`22.4.1` 那个 1..33 的值是等级不是舰队，所以「只看舰长以上」不能挂进房，只能挂 `ENTRY_EFFECT`
- 限速（`welcome.rs`）：同人同类 60 秒去重 + **每 30 秒最多 1 条**；额度是「普通形态一个大池 + 舰长进场一个小池」，**不分形态各一份**——分形态等于 4 类各一份（二十几条/分钟），比中位房间的弹幕（实测 10 条/分钟）还密，那就是刷屏。舰长进场也没真豁免：热度房里实测 `ENTRY_EFFECT` 有 10 条/分钟，真豁免会反超弹幕
- 量级实测：进房 : 弹幕在 **1.5 : 1 ~ 10 : 1** 之间（狼人杀 58 万在线是 160 : 103，另一间 1.1 万在线的是 103 : 10——后者弹幕特别稀，比值被拉大）。**占比 ∝ 1/弹幕速率**：房间越热，欢迎信息占比越小，所以「热度房怕刷屏」这个直觉是反的；真正的代价是限速后只剩 2% 左右的采样，读不出「谁来了」
- 去重限速**与开关无关地先跑**（`welcome::on_welcome`）：开关只决定「谁来消费」。若让开关去挡限速，关掉渲染时舰长进场就能绕过去重表，同一个舰长每次进出都被重复念
- 渲染：复用**系统提示行**形态（`OverlayApp` 的 `pushRow`，与「已进入直播间…」同一形态）——不显示用户名与徽章，不动 `row-style.ts` / `MetaBadges` 的共用契约，也不影响发送框吸附。正文颜色由「用户名颜色」偏移 30° 色相派生（`row-style.ts::shiftHue`）：颜色跟着主题走又与弹幕用户名不同；**只动色相不降亮度**（窗口背景多是游戏画面，降亮度会糊），也不用 CSS `filter: hue-rotate()`（会给每行开合成层）
- 朗读（`tts::on_welcome` + `tts/text.rs::build_welcome_text`）：只念舰长进场，文案「欢迎舰长 老板A进入直播间」。**档位要念**（不念的话听起来就是「欢迎某某进入直播间」，分不出总督和舰长），档位与用户名之间留一个空格——与弹幕朗读的身份前缀同一种分隔做法。排到**队尾**（不像礼物那样插队、不抢弹幕额度），但 `force = true`（压力下别丢）；欢迎信息自己的开关是第二道闸，**朗读总开关**关着时不念
- 设置项在「主播分区 → 欢迎信息」标签，显示与朗读**均默认关**：这类事件量级比弹幕大，且在热度房限速后信息量很低，由主播按房间情况自己开

## 关注在播（主界面手动刷新）

「直播间」页在「最近」位置多一行「关注在播」+ 刷新按钮（「最近房间」面包屑已暂时隐藏，代码保留）：列出已关注且正在开播的主播名，点击直连。

- 触发时机：`loggedIn` 变 true 自动查一次（打开软件已是登录态 / 扫码登录成功后各走一次，同一个 `watch`），之后手动点「刷新」；登出时清空列表

- 数据源 `bilibili/live.rs` → `commands::list_following_live`：① `x/relation/followings` 拉关注（uid + 昵称，最多 10 页 / 500 人）；② `room/v1/Room/get_status_info_by_uids` 每批 100 个 uid 查开播状态，只取 `live_status == 1`
- 实测：关注列表带现有登录 Cookie 即可，**无需 WBI 签名、无需 buvid3**（Referer 用 live / space 都通）；批量状态接口匿名可调，且**没有直播间的 uid 直接不返回 key**（不是报错），按缺失跳过
- 未登录时关注列表返回 `code:-101`：命令层直接报错「请先扫码登录」，退出登录时前端清空列表
- 没有面向关注列表的开播推送长连接，只能轮询批量接口；上游不做后台轮询（手动刷新 + 登录后自动一次）
- **streamer 分支差异**：整块折叠（默认收起），并改成**展开时才查**——我自己日常用不到它，登录即查会白翻最多 10 页关注列表；展开后仍需点「刷新」再查一次

## streamer 分支（fork 专属，见 `STREAMER.md`）

本仓库是 [bd-dxg/bili-danmu](https://github.com/bd-dxg/bili-danmu) 的 fork，长期分支 `streamer` 是**个人自用分支**——按我自己使用中的体验来改与加功能（天天连自己房间、要避开游戏误点、想判断观众留存）。与上游的差异集中在「默认值与优先级」，代码差异要保持薄、可枚举、可回退：

- **我的直播间（置顶卡片）**：按登录 uid 查自己房间，显示房间号 / 标题 / 开播状态 / 在线人数，右侧「连自己房间」一键直连。数据源 `bilibili/live.rs::fetch_my_room` → `room/v1/Room/getRoomInfoOld?mid=<uid>`（**匿名可调，不传登录 Cookie**，少一处凭据外发）。
  - **坑**：别用 `room_init?id=<uid>` 找自己房间——**靓号主播的 uid ≠ 房间号**（uid `391365058` 的房间是 `25627030`），拿 uid 查会得到 `code=60004 房间不存在`
  - 字段实测是 camelCase（`liveStatus`、`online_hidden`），解析两种写法都兼容；`online_hidden` 为真时界面换成「人数已对观众隐藏」（接口仍给数值）
  - 从未开播过的账号接口不给 `roomid` → 命令返回 `None`，界面提示「这个账号还没有直播间」，**不算错误**
  - 卡片 60s 自动刷新，但**只在窗口可见且已登录时**才发请求（房间页是 v-show 常挂载的，必须自己挡一层，否则后台也一直轮询）
- **最近连接 / 关注在播 折叠**：上游把「最近连接」硬编码隐藏（`SHOW_RECENT_ROOMS = false`）、「关注在播」直接平铺；streamer 分支两个都做成默认收起的折叠区，展开即用（偶尔也要帮别人看弹幕）。
- **关于页的出处口径**：同时列出**本仓库**与**上游原始项目**两个地址（各带复制按钮），并写明「本分支基于上游开发；支持请优先给上游作者」；**原作者的赞赏码不搬进本 App**——它不是本分支的支持入口，混在一起会让人误以为赞赏是给本分支的，上游仓库里能找到。`LICENSE` 与版权声明不动（GPL-3.0 要求的部分不能省）。
- **弹幕窗右侧在线列（在看人数 + 实时在线榜）**：与左侧头衔列对称的一条右列，主播直播时扫一眼就知道「有没有人、谁在互动」。
  - 数据源 `bilibili/audience.rs`：`getOnlineGoldRank`（**匿名可调，不传 Cookie**），一次请求同时拿 `onlineNum`（人气房会饱和成 `9999+`，照实显示）与榜单前 [`RANK_SIZE`=5] 名；主播 uid 走 `room_init` 并按房间缓存在 `AudienceState::anchor`
  - **榜单语义（实测）**：= 此刻在房间里 ∩ 今天有贡献值的人，人一离开直播间 **≤20 秒**就掉榜，所以它是「实时在线榜」而不是当日累计榜；**潜水观众永远不在榜上**（B 站隐私边界），界面文案必须写清这一点
  - 节奏：复用 lib.rs 里那个 2 秒周期任务，每 15 跳（=30 秒）拉一次；**换房立刻拉一次**（`audience_room_changed`），失败静默 + 最短 5 秒重试（`AUDIENCE_RETRY_MIN_SECS`，避免接口风控时变成每 2 秒一次请求），成功后广播 `audience-update`
  - **布局契约**：`--audience-indent: 6.5em`（右侧整列宽）、`--audience-inset = --audience-indent − 0.35em`（面板底色让出的宽度）。`.panel` 用 `margin-right: var(--audience-inset)` 让出右列，内部两区**不做**反向拉回（左侧头衔列要拉回、右侧不拉回，这是两者的关键差别）；`window.rs` 的 `AUDIENCE_INDENT_EM` 与 `sender_layout_metrics` 必须同步（右侧列出现时面板变窄，发送框宽度按新面板宽度重算）
  - **右列底色与面板同一份 `bgColor`**（即同一个 `bg_opacity` 设置）：`.audience-bg` 右边圆角、左边直角，同时 `.overlay-root.with-audience .panel` 去掉右侧圆角 → 两块拼成**一个连续圆角矩形**，接缝处只留 1px 淡分隔线；右列显示时弹幕/礼物区加 0.35em 右边距，正文不贴分隔线
  - **两个开关**（`OverlayStyle::show_online` / `show_rank`，默认都开）在「弹幕窗」标签页，走 `overlay_set_audience` 做 load-modify-save（**不要**提交整份 style：会覆盖「弹幕样式」页正在改的其它字段）；两个都关时 `.with-audience` 不生效 → inset 为 0、面板恢复满宽
  - 右列边界提示与缩放手柄：`.audience-band`（淡底 + 左虚线，鼠标移入才浮出，做法同 `.title-band`）+ `.resize-handle-east`。手柄放**窗口右缘**（9px，系统热区只有 3px）而**不是**虚线处：虚线那边压着「在看 123」，点文字会变成缩放
  - 舰队徽章抽成 `overlay/GuardChip.vue`：弹幕行 `MetaBadges` 与在线榜共用，**三档配色只留一处**（复制一份必然漂移，同「徽章列共用契约」）
  - 「发送测试弹幕」一并预览在线列（`buildPreviewAudience()`，覆盖超长昵称/无舰长牌/三档舰队）：带 `preview: true` 的快照盖住真实数据，`preview-reset` 时回落；**有预览数据时未连接也显示**（否则不上播就验证不了样式）
- 差异清单、回退方式与「同步上游」的纪律写在 `STREAMER.md`；改这里的行为时要同步更新它。

## 表情包（渲染 + 朗读均已上线）

弹幕里的表情包看 `DANMU_MSG.info[0][13]`：有表情时是对象（`emoticon_unique` / `url` / `width` / `height` / `bulge_display`），无表情时是**字符串 `"{}"`**。解析在 `emote.rs::parse_emote` → `event::Emote`，随弹幕事件下发（渲染与朗读同一个字段）。

- **判断依据不是「有没有中括号」**：`[委屈]` 这类只是全站小表情；`room_*`（UP主大表情 / 房间专属）、`official_*`（通用表情，你举的「冲鸭」`official_332`、「upup」`official_348`）、`upower_[包名_表情名]`（收藏集装扮）在正文里是**纯词、没有中括号** —— 它们全靠 `info[0][13].emoticon_unique` 非空来认（实测 147 条样本 0 例外，且表情弹幕的正文全部等于表情名）
- 渲染在 `overlay/DanmakuRow.vue`：表情弹幕的正文**补上中括号**显示（「冲鸭」→「[冲鸭]」）—— 不拉外部图（桌面壳里图片很容易瘸：CSP 默认不只是 `'self' data:`，表情图地址又有 `http://` 与协议相对 `//` 两种形态，一失败就是破图/裸文字，不如文字好认）
- 报文里的 `url` / `width` / `height` / `bulge` 照实解析并下发（`emote.rs::normalize_url` 把地址统一升到 `https://`），但**不用于渲染**：保留是协议建模 + 日志诊断用（日志会打 unique 与尺寸）
- 朗读（`tts/text.rs::build_text`）：带表情字段的**整条不念**；无字段的老式小表情**剥掉 `[名字]` 只念文字**（「今天真好[花]」→ 念「今天真好」，整条都是表情则不念）；开关在「朗读 → 朗读筛选 → 朗读表情弹幕」（`TtsConfig::read_emote`，开启后两类都照常念）
- `info[0][13]` 无表情时是字符串 `"{}"` 而不是空对象：`as_object()` 直接 None，**别当对象硬解**；图片地址必须在 Rust 侧升到 `https://`（老式小表情给协议相对 `//i0.hdslb.com/…`，通用表情「冲鸭」给的是 `http://…`）
- 判据（`emote.rs::parse_emote`）：`info[0][13]` 能当对象解且 `emoticon_unique` 非空 → 表情，**图片地址可有可无**（早期版本要求地址非空，把没地址的表情漏成普通文字、还被朗读）
- `info[0][13]` 无表情时是字符串 `"{}"` 而不是空对象：`as_object()` 直接 None，**别当对象硬解**；图片地址仍会归一化到 `https://`（老式小表情给协议相对 `//i0.hdslb.com/…`，通用表情「冲鸭」给的是 `http://…`）
- 待验证：**文字 + 表情混合**弹幕（本次 147 条里没有）。若 B 站允许，`info[0][13]` 也只带一个表情对象，当前实现会整行换图 / 整条不念，会丢掉其中的文字。旁证字段：`info[0][12]`（0/1 标记）、`info[0][15].extra`（冗余副本，含 `dm_type=1`）
- 排障：逐条弹幕 / 表情日志是 **debug 级**（`lib.rs` 里 level 默认 Info，平时不落盘、不刷屏，避免高频房把 5MB 轮转刷满、冲掉故障日志）—— 需要时把那一行临时改成 `Debug` 再复现一次；日志目录 `%LOCALAPPDATA%\com.bilidanmu.app\logs\bili-danmu.log`（关于页可一键打开）
- 实测脚本：`Task/probe-emote.ps1`（表情清单）、`Task/probe-emote-map.ps1`（unique→名字 逐条校验）、`Task/analyze-dump.ps1`。**注意抓包抽样不可靠**（180 秒只有几十条 DANMU_MSG，容易漏掉低频形态），房间弹幕历史接口 `xlive/web-room/v1/dM/gethistory` 能直接看到自己发出去的原文，排「发出去的到底是什么」很快
- **两类表情都要认**：① 报文带字段的（`d.emote`：`room_` / `official_` / `upower_`）；② **无字段的按正文里的 `[名字]` 片段认**（`emote.rs::strip_bracket_emotes`，重剥离不重整体匹配）。三个实测形态：连点表情拼成一条（`[比心][比心][比心]`）、文字 + 表情混在同一条（`今天真好[花]` → 只念「今天真好」）、纯表情（`[花]` → 不念）。畸形中括号（未闭合 / 内部还有 `[` / 名字为空或 > 30 字）一律不剥
- 代价（已知且接受）：手打方括号文字（「价格[10]元」）也会被当表情剥掉

## 技术栈

- 前端：Vue 3（`<script setup>`）+ Vite + TypeScript，包管理 pnpm
- 桌面壳：Tauri 2（Rust，edition 2021）
- Rust 关键依赖：tokio / reqwest（json、gzip；登录 Cookie 全程显式 header 传递）/ tokio-tungstenite（弹幕 WebSocket + Edge TTS WebSocket）/ native-tls（Edge TTS 自建 TCP 后显式包 TLS）/ futures-util（WS 收发）/ brotli-decompressor + flate2（弹幕包解压）/ md-5（WBI 签名）/ sha2（Edge TTS 的 Sec-MS-GEC 令牌）/ windows-sys（登录 Cookie 的 Windows DPAPI 加密、TTS 的 winmm MCI 播放）/ tauri-plugin-log（统一日志落盘，业务代码经 `log` 门面宏调用）/ tauri-plugin-global-shortcut（发送框显示/隐藏的全局快捷键，钉 `~2.3`——2.4 要求 tauri ^2.12 会把 lock 顶到 2.12 而 npm 侧 `@tauri-apps/api` 还是 2.11，`tauri build` 会拒绝打包）
- 授权：GPL-3.0

## 常用命令

- 安装依赖：`pnpm install`（根目录，会同时装 Rust 依赖需本机有 Rust 工具链）
- 开发运行：`pnpm tauri dev`（Vite 热更 + Rust 调试一起跑）
- 仅前端开发：`pnpm dev`
- 前端构建：`pnpm build`
- 打包桌面应用：`pnpm tauri build`（产物在 `src-tauri/target/release/bundle/nsis/`）
- 纯 Rust 编译检查：在 `src-tauri/` 下 `cargo check`
- 纯 Rust 单测：在 `src-tauri/` 下 `cargo test --lib`（含 `--ignored` 的联网测试：Edge TTS 合成、音色列表、突发失败率、更新检查）
- PowerShell 辅助脚本：`scripts/`（`ui.ps1` 启动、`ws-probe.ps1` 弹幕协议探测、`edge-tts-probe.ps1` Edge TTS 协议与延迟/音色探测、`gen-icon.ps1` 图标生成）
  - 含中文的 `.ps1` 必须用 `pwsh` 跑（Windows PowerShell 5.1 会按 GBK 解码无 BOM 的 UTF-8 文件，中文被拆坏后连引号都会解析出错）

## 代码约定

- 目录结构：
  - `src/` 前端：
    - `views/` 页面（RoomView 房间、DanmakuView 弹幕、TtsView 朗读、AboutView 关于，后三者均为页内标签式）
    - `components/` 复用组件（`LoginDialog.vue` 登录二维码弹窗、`DanmakuFilterPanel.vue` 弹幕筛选表单（显示/朗读共用）、`StatusDashboard.vue` 运行状态仪表盘、`SettingRow.vue` 设置行、`OverlayWindowPanel.vue` 弹幕窗标签页、`SenderWindowPanel.vue` 发送框标签页（显示开关 + 全局快捷键录制）、`SaveTip.vue` 保存/错误浮层提示（右下角，不占文档流）、`FollowingLiveRooms.vue` 关注在播一行（**上游抽出的组件，本分支未使用**：streamer 的关注在播仍是 RoomView 里的折叠实现，见 `STREAMER.md`；同步上游时不要删这个文件））
    - `composables/` 逻辑（`useLogin` 登录态、`useRoomConnection` 连接与最近房间、`useTtsConfig` 朗读配置与音色列表与校验、`useSaveTip` 设置页提示、`useUpdate` 更新检查结果）
    - `overlay/OverlayApp.vue` + `overlay/DanmakuRow.vue` 悬浮层入口与单行渲染、`overlay/GiftRow.vue` 礼物行、`overlay/MetaBadges.vue` 两区共用的身份徽章列、`overlay/row-style.ts` 共用的描边计算、`sender/SenderApp.vue` 发送弹幕框入口
    - `styles/settings.css` 全局设置页样式（约束见「注意事项」）、`types/ipc.ts` IPC 类型定义、`overlay.ts` / `sender.ts` 独立入口（对应 `overlay.html` / `sender.html`）、`context-menu.ts` 两个悬浮窗入口共用的右键菜单屏蔽
  - `src-tauri/src/` Rust 核心：
    - `lib.rs` 组装（状态托管、窗口创建、托盘、命令注册）、`state.rs` 内存状态、`commands.rs` IPC 命令、`connection.rs` 连接与断线重连、`window.rs` 窗口辅助与发送框吸附、`shortcut.rs` 发送框全局快捷键（串解析 / 注册 / 注销）
    - `bilibili/`：`protocol.rs` 协议包、`parser.rs` 解压/展开 + 命令分发 + 共用小工具（命令解析按域拆在 `parser/`：`danmaku.rs` 弹幕 / `gift.rs` 打赏 / `welcome.rs` 欢迎）、`emote.rs` 表情包（报文表情字段解析 + 正文 `[名字]` 剥离）、`api.rs` HTTP 接口（房间解析、弹幕配置、主播名）、`ws.rs` WebSocket 会话、`wbi.rs` 签名、`login.rs` 扫码登录、`send.rs` 发送弹幕、`live.rs` 关注在播与我的直播间（关注列表 + 批量开播状态 + 按 uid 查自己的房间）、`audience.rs` 在线人数与实时在线榜（**streamer 分支**）、`event.rs` 事件
    - `tts/`：`mod.rs` 队列状态与入口钩子（`on_danmaku` / `on_backing`）、`clean.rs` 朗读前清洗（噪声字符 / 重复折叠 / 链接）、`text.rs` 筛选与组装文案（弹幕与打赏各一个组装函数）、`gift.rs` 礼物朗读的连击聚合与静默窗口、`worker.rs` 合成与播放流水线、`player.rs` winmm MCI 播放、`edge.rs` Edge TTS 协议、`edge/drm.rs` 的 `Sec-MS-GEC` 令牌生成（v2 自定义哈希）、`edge/voices.rs` 音色表、`edge/ssml.rs` SSML 构造、`edge/util.rs` 时间与编码工具
    - `config.rs` 配置读写与全局互斥、`config/types.rs` 结构体与默认值、`config/crypto.rs` DPAPI 加解密
    - `gift.rs` 礼物列表：金额门槛与连击合并（唯一判定处，前端只负责按 id 覆盖与条数上限）
    - `update.rs` 版本更新提醒：请求 GitHub Releases 比对版本（只提醒，不下载安装；**streamer 分支查本仓库的 Release**，版本比较支持 `-streamer.N` 后缀）、ShellExecuteW 打开下载页
- 多窗口：根目录 `index.html`（主窗口）+ `overlay.html`（透明悬浮窗）+ `sender.html`（发送弹幕框），Tauri 配置见 `src-tauri/tauri.conf.json` 与 `capabilities/default.json`
- IPC 双向类型约定：Rust command 与 `src/types/ipc.ts` 保持一致，改动协议时两端同步
- 注释、commit 一律简体中文；PRD（`prd.md`）已停止维护（见下）
- 功能状态与规划以 `docs/技术说明.md` 表格为准（未做项：礼物图标、滚动弹幕、顶弹、用户屏蔽、Windows 系统 TTS、全局快捷键的 TTS / 穿透部分（发送框显示/隐藏已做）、开机自启、自动更新）；`README.md` 只写面向用户的产品介绍，技术细节一律放 `docs/技术说明.md`；`prd.md` 仅作历史设计参考，新需求不要再往上写

## 版本号与发布

- 禁止在 `main` 上直接开发：功能 / 修复 / 重构一律先切分支（沿用 `feature/` / `fix/` / `refactor/` 前缀，见 `git branch -a`），完成后开 PR、squash 合并回 `main`；只有版本号提升、README / 文档这类发布收尾可直接提交 `main`
- 版本号要同步改 3 处：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`；`src-tauri/Cargo.lock` 由 `cargo check` / 构建自动更新
- **本仓库（streamer 分支）的版本线**：版本号形如 `1.5.0-streamer.N`（前三位 = 所基于的上游版本，`N` = 本版发布序号），Release tag 同形（`v1.5.0-streamer.1`）。更新检查（`update.rs`）**查本仓库的 Release**，其版本比较已支持这段后缀（第四段取预发布段里最后一个数字）——否则同一个 `1.5.0` 下 `-streamer.2` 会被判成与 `-streamer.1` 相等，更新提示永远不会出现。
- **名称**：产品名 `bili-danmu-streamer`（`tauri.conf.json` 的 `productName`：决定安装目录、开始菜单项与安装包文件名），界面显示名「bili-danmu-streamer」（主窗口标题、弹幕窗/发送框标题、托盘提示）。**`identifier` 必须保持 `com.bilidanmu.app`**：配置目录与登录态都挂在它下面，改掉等于换一个应用（要重新扫码登录）。
- 流程：改版本 → commit（`🚀 应用版本号升至 X.Y.Z（…）`）→ 推 `main` → `pnpm tauri build` → `gh release create`
- 安装包路径：`src-tauri/target/release/bundle/nsis/bili-danmu_<版本>_x64-setup.exe`
- 打包前确认应用没在运行，否则 NSIS 写 exe 会失败
- `gh release create` 的 tag 只建在远端，本地要 `git fetch --tags` 才看得到
- 仓库历史一律 squash 合并（main 上每个 PR 一个带 `(#N)` 的 commit）：`gh pr merge --squash` 时**别传 `--subject`**，否则 GitHub 不会自动追加 `(#N)`，破坏标题格式

## 注意事项

- 禁止提交：`node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/schemas/`、`Task/`、`dev.log`、根目录截图（`.gitignore` 已配）
- 弹幕必须登录态：B 站 2025+ 不向游客推送 `DANMU_MSG`，游客级 token 只是登录路径失败时的兜底（能连上但收不到弹幕）。登录 Cookie 本地持久化；改动协议时用 `scripts/ws-probe.ps1` 验证
- 弹幕窗口性能敏感（120 条上限、透明层重绘），前端改动注意不要引入高频重排；入场动画（`overlay.css` 的 `.row-enter`）只动 `transform` / `opacity`（合成层）且只在新节点挂载时播一次——**不要**给行加 `will-change`（120 行会常驻独立图层），也不要改成逐帧改 height / margin 的效果（每帧重排）
- 登录 Cookie 在 `config.rs` 的 `save_config_unlocked`（唯一写盘边界）统一 **DPAPI 加密**（`dpapi:` 前缀 + hex）落盘，加解密在 `config/crypto.rs`，读取自动解密（旧版明文无前缀兼容）：任何新增的敏感字段必须走同类加密，勿明文落盘；内存态保持明文
- `config.rs` 读写持有全局互斥（save_* 均为 load-modify-save）：新增保存函数须沿用 `lock_cfg()` + `*_unlocked` 模式，勿在持锁时调用加锁公共入口（std Mutex 不可重入）
- 断线自动重连在 `connection.rs` `spawn_connection`（1/2/4/…/30s 退避、10 次上限）；改连接收尾逻辑时保留取消令牌身份检查（`finish_connection`），否则旧任务会误清新连接状态
- 「发送测试弹幕」（`src/danmaku-preview.ts` + 弹幕页「弹幕样式」标签）走的是**真实事件通道**：本地 emit 与 Rust 同名的 `danmaku` / `gift` / `welcome` 事件（`core:default` 已含 `core:event:default` 的 `allow-emit`，所以不需要 Rust 命令、也不需要新权限），预览与真实弹幕共用同一套组件与 CSS——**不要另写一套预览渲染**，那必然与真实渲染漂移（徽章列宽 `role-slot`、换行续行对齐、描边），预览就会骗人。预览行带 `preview: true`：① 绕过弹幕显示过滤（否则房间设了「只显示舰长」时普通样例会被整条丢弃，用户以为按钮坏了）；② 每次推送前先 emit `preview-reset`，overlay 只删带 `isPreview` / `preview` 标记的行，**真弹幕 / 真礼物 / 真欢迎行一条不动**（预览 id 也带 `preview-` 前缀，不会撞）；设置页的「清除测试弹幕」按钮就是再发一次同一个 `preview-reset`。入口在弹幕页「弹幕样式」标签底部（与「显示舰长/房管标记 / 粉丝牌 / 荣耀等级」这几个开关同页，改样式时顺手就能验证）。预览**不**绕过「礼物区 / 欢迎信息」开关：关掉的部分不推并在界面说明。未连接直播间时 `currentRoomId` 为 0，正常判定会把所有粉丝牌判成「其它房」，故预览行自带 `is_room_medal` 标记来展示「本房牌」绿色配色。注意行是真画在弹幕窗上的：推流时会被 OBS 采集（按钮文案已写明）。
- 发送弹幕需登录态（`bili_jct` 做 CSRF 签名，游客无 bili_jct 会报错）；发送框窗口始终吸附 Overlay 下方（`window.rs` `sync_sender_docked`），**左端与面板背景左边缘同一条线**（6px padding + `--panel-inset` 5.15em，不再对齐正文列）、**宽度取面板宽度的 50%（上限 260px）**、高度随弹幕字号缩放（`window.rs` 里 `font_size * 3.1 + 6`，要同时容下输入行与出错时才出现的提示行）；**输入区字号 0.9em**（比弹幕小一档）＋紧内边距：210px 的框约 8 个中文字，调回 1em 就只剩 6 个、连 placeholder 都会截尾——宽度是「不能挡太多游戏画面」的硬约束，要更宽得改 `SENDER_WIDTH_RATIO` / `SENDER_MAX_WIDTH`。右端带「穿透」开关按钮（发送框是独立窗口，穿透开启后仍可点；但发送框可整条隐藏，此时关掉穿透的入口在主界面「弹幕窗」标签与托盘菜单）——改 Overlay 布局（容器 padding、`--role-indent` / `--panel-inset` / `--audience-indent`）时须同步 `sender_layout_metrics` 里的这几个系数。**左侧那条 `--panel-inset` 宽的留白是头衔列**（房管 / 舰长 / 粉丝牌画在面板底色左侧，底色左边缘就是这条带的右界，不是布局错位）：鼠标进入弹幕窗时由 `.title-band` 浮出淡底 + 虚线提示（平时全透明，避免 OBS 录到常驻线条），虚线同时是**缩放手柄** `.resize-handle-west`——系统的缩放热区只在窗口最外缘（实测只有 3px），离可见边缘差一个 `--panel-inset`，用户按直觉抓不到，故用 `getCurrentWindow().startResizeDragging('West')` 把它搬到可见边缘（capabilities 需要 `core:window:allow-start-resize-dragging`）。提示带与手柄的 `top: calc(6px + 1.4em)` 与 `.drag-strip` 高度绑定，改 `.drag-strip` 高度时三处一起改。弹幕窗与发送框的右键菜单在**入口脚本**里挂 `document` 级 `contextmenu` 拦截屏蔽（`src/context-menu.ts` 的 `disableDefaultContextMenu()`，两个入口各调一次），**不要退回 `@contextmenu.prevent`**：按住右键拖动、指针离开窗口后再松开时 `contextmenu` 事件的 target 会落到 `<html>`，挂在组件根节点上的修饰符收不到，WebView2 就会弹出宿主默认菜单（issue #37）
- 发送框可**整条隐藏**（`SenderState.visible`，广播 `sender-visible`；设置页走 `sender_get_config` / `sender_set_config`，托盘与快捷键走 `commands::toggle_sender`），配合**全局快捷键**用来避开游戏里的误点、需要时再唤出回复弹幕：快捷键走 `shortcut.rs`（`tauri-plugin-global-shortcut`，串形如 `Alt+KeyQ` = 修饰键 + UI Events 键码，前端录制要求 ≥1 个修饰键或 F1-F24），**默认不注册**——注册成功后该组合键被系统级占用，其它程序（包括游戏）都收不到，默认值等于替用户抢一个键。换键位走 `apply_hotkey(old, new)`：**先注册新的、成功后再注销旧的**，失败就保留原来能用的那个；别用 `unregister_all`，那会把将来可能新增的其它快捷键（roadmap 里的 TTS / 穿透）一起清掉。事件处理挂在 `Builder::with_handler` 且只认 `ShortcutState::Pressed`。`sender_set_config` 先试注册、失败即整体不生效；启动时注册失败也会写进 `hotkey_error`（设置页读 `sender_hotkey_error` 提示），避免「配置里写着某键位、实际没注册上」的假象。插件必须钉 `~2.3`：2.4 起要求 `tauri ^2.12`，会把 lock 里的 tauri 顶到 2.12，而 npm 侧 `@tauri-apps/api` 还是 2.11，Tauri CLI 会以「大小版本不一致」直接拒绝 `tauri build`。隐藏发送框会连「穿透」按钮一起隐藏，兜底入口是主界面「弹幕窗」标签 + 托盘菜单「显示/隐藏发送框」；唤出时 `window.rs::set_sender_visible` 会 `show + set_focus` 并广播 `sender-shown`，发送框前端据此把光标放进输入框。可见性**不在按键路径上落盘**：快捷键/托盘的 `commands::toggle_sender` 只改 `SenderState.visible` + 广播，写盘交给 lib.rs 里与 `flush_overlay_bounds` 同一个 2s 周期任务（`window::flush_sender_visible`，内存态与 `saved_visible` 不同才写）——写盘是整份 config 的 load-modify-save + DPAPI 加解密，而热键回调跑在**主线程**（插件的隐藏窗口建在主线程，`GlobalHotKeyManager::new()` 不另起线程也不轮询），每按一下写一次会让主线程跟着磁盘走；设置页走 `sender_set_config` 仍然同步落盘（那是显式保存动作，同时更新 `saved_visible`）。取舍与窗口位置同款：切完 2s 内直接退出应用，最后一次切换可能没落盘。
- 朗读与显示是**两套独立的 `DanmakuFilter`**（`danmaku_filter` / `tts.filter`）：显示筛选在前端 Overlay 做，朗读筛选在 Rust `tts::on_danmaku` 做，改任一侧别把两者耦合成一份配置
- `src/styles/settings.css` 是**全局样式**（无 scoped），类名是主窗口所有页面的共用契约：往里加规则前先确认不和已有页面的类名撞车（`StatusDashboard` 的 `.tip` 就因此改名 `.dashboard-tip`）；页面独有的差异项（如 `.select` 的 min-width）留在各自组件的 scoped 样式里覆盖
- **对比度（可读性）**：`style.css` 的 `--text-faint` 已按 WCAG AA 正文 4.5:1 校过（浅色 `#626775` 最差 4.86:1、深色 `#8b8e98` 最差 4.73:1，按**卡片底色** `#eceef2` / `#1d1e24` 算——那是最差背景）。改这两个值前先算一遍对比度，别调回 `#8a8f9c` / `#6a6d77`（分别只有 2.79:1 / 3.22:1）。`.tip` 这类整段要读的说明文字用 `--text-dim`（更高一档），`--text-faint` 只给角标 / 次要标签
- **弹幕窗断行**：弹幕行 / 礼物行 / 在线榜昵称统一 `overflow-wrap: anywhere`，**不要**退回 `word-break: break-all`——后者会把英文单词从中间切断（实测「请神doinb了说是」被切成「请神doi / nb了说是」）；中文本身允许任意字间断行，不需要 break-all
- 设置页的开关统一用 `components/ToggleSwitch.vue`（40×22 滑动开关，`role="switch"`）：**不要**再往设置行插槽里写 `<input type="checkbox" class="switch">`，`.switch` 的全局样式已从 `settings.css` 删掉。组件的事件顺序是「先 `update:modelValue`、再 `change`」，所以父组件写 `v-model="x" @change="apply()"` 与用原生 input 时一样、且 `apply()` 里读到的是新值；由 Rust 状态控制的几行（弹幕窗的显示/穿透/置顶/两个在线列开关、发送框显示）用 `:model-value="x"` + `@change="setX(!x)"`，**不要**用 v-model——IPC 失败时乐观更新会把本地状态带脏
- 设置页的「整行可点」（`SettingRow.vue` 的 `onRowClick` + `switchOf`）：只接管**没点在控件上**的点击，且只在「整行唯一控件就是这个开关」时才生效——行里还有别的控件时（弹幕过滤页的荣耀等级行，标签里嵌数字输入框）不做整行可点，否则点歪到输入框旁边会误切开关；「试听」「发送测试弹幕」「录制快捷键」这些行里有点一下就会真的执行操作的按钮，误触发的代价比不切换开关更大。**不要**把 `.setting-row` 改成 `<label>` 包住插槽：label 的整行点击会转给第一个可标注元素（往往正是那个按钮）。`DanmakuFilterPanel` 的四行是手写的（荣耀那一行套不进 `SettingRow`），同一套逻辑在那边再实现了一遍——改一处要改两处。**鼠标样式与悬停底色**都由 `settings.css` 里同一条 `:has()` 规则给出（`cursor: pointer` + `:hover { background: var(--hover) }`）；设置行用「左右 `padding` + 负 `margin`」铺满卡片内宽，改 `.setting-card` 的左右 padding 时要同步行的两个 `14px`。不可点的行**不要**加悬停底色——会诱导用户去点空白
- 直播间的房间号校验走 `useRoomConnection.ts` 的 `parseRoomId`（纯数字原样用 / 直播间链接取路径里的数字 / 兜底取最后一段 3 位以上数字），输入框是 `type="text"`：**不要**改回 `type="number"` + `Number(输入串)`，number 输入框会把粘贴进来的直播间链接吞成空串，用户只会看到「请输入有效的直播间 ID」。连接失败提示紧贴输入框行（原来在折叠区之后，离操作点上百像素）
- 发送框的回车发送监听的是 `keydown` + 组合态判断（`sender/SenderApp.vue` 的 `onEnter` + `composing` 标记）：中文输入法里回车是「选词上屏」，Chromium 的派发顺序是 keydown(isComposing=true) → compositionend → keyup(**isComposing 已是 false**)，所以 `@keyup.enter` 会把半成品拼音直接发出去（`v-model` 有组合态守卫，按键修饰符没有）。别退回按键修饰符
- 登录二维码**只在弹窗打开时生成、关闭弹窗即停轮询**（`LoginDialog.vue` 里 watch `loginDialogOpen`）：不要改回 `onMounted(startLogin)`——那等于每次启动应用都白拿一个二维码并轮询到过期，用户过几分钟真去扫时看到的是「二维码已过期」。已扫码待确认（`phase === 'confirmed'`）时遮罩点击与 Esc 都不关窗（关掉就停轮询、二维码作废），只留「关闭」按钮这条显式取消路径
- 窗口位置一律要**夹进显示器工作区**（都在 `window.rs`）：① `sync_sender_docked` 算完吸附点后走 `fit_docked_position`，落点顺序是「弹幕窗正下方 → 正上方 → 贴工作区下沿」——弹幕窗贴到屏幕底部时正下方就是屏幕外，而发送框不可拖动、也不在任务栏显示，用户既看不到它、也不知道要先把弹幕窗挪上来；左右同理，**越界时可见优先于对齐**（正常情况发送框左端仍与面板底色左边缘同一条线，只有弹幕窗自己都在屏幕外时才会偏离）。② 启动恢复位置前用 `clamp_overlay_bounds` 校验「窗口中心是否落在某块屏的工作区内」，越界就搬回主显示器中央——判据用中心点：中心还在屏幕上就说明用户看得见、也拖得动，半截露在屏幕外属于用户自己摆的位置，不替他改。③ 兜底入口是「弹幕窗」标签的「移回主屏中央」按钮（`overlay_reset_position`，尺寸只在比工作区还大时才收窄）。多屏且缩放不同时 `logical_work_area` 的换算只对窗口真正所在那块屏精确，算不准只会偏保守（误判成不可见 → 搬回主屏），不会把看得见的窗口搬到看不见的地方
- 弹幕样式滑块的「拖动实时预览」分成两条命令：拖动中走 `overlay_preview_style`（只改内存态 + 广播 `overlay-style`，**不落盘**），松手的 `change` 才走 `overlay_set_style`（落盘 + `sync_sender_docked`）。前端在 `DanmakuView.vue` 里做 80ms 节流——`overlay_set_style` 每次是整份 config 的 load-modify-save + DPAPI 加解密，拖动中每帧调一次会把主进程拖在磁盘上。**新增「实时预览」需求照这个分两段做**，别把持久化命令直接挂到 `@input`。预览**不**重排发送框（窗口 set_position/set_size 每帧会跟拖动抢帧），发送框尺寸在松手后才跟着字号变
- 弹幕窗的 `min_inner_size` 必须与「弹幕窗」标签宽高滑块的下限一致（240×120）：再小的话左右两条列（头衔列 5.15em + 在线列 6.15em）会把面板挤成负宽，拖到 120×40 只会得到一个不可用的窗口。右侧在线列在「已连接但首次数据未到」时显示「加载中…」占位（`OverlayApp.vue`），否则那是一条什么都没有的深色竖带，看起来像渲染坏了
- **界面主题（跟随系统 / 浅色 / 深色）**：状态与解析在 `src/composables/useTheme.ts`，入口在侧边栏底部（三段式按钮）。CSS 只认 `<html data-theme="dark">`（`style.css` 里 `:root[data-theme='dark']` 覆盖变量），**不要**再加回 `@media (prefers-color-scheme: dark)`——那只能在 Windows 设置里切，应用内切换会失效。主题**存 localStorage**（键 `bili-danmu:theme`）而不是 `config.json`：`public/theme-boot.js` 要在首帧前、任何 IPC/Vue 之前**同步**读到它，否则深色用户开机会闪一下浅色；CSP 是 `script-src 'self'`，所以引导脚本只能放 `public/` 走同源外链（`index.html` 里在 module 脚本之前引用），不能内联。主题只影响主窗口：overlay / sender 用各自的 CSS（颜色固定），不要给它们套主题
- 设置页的保存提示**全应用只有一个浮层**：状态在 `composables/useSaveTip.ts` 的模块作用域（单例），`<SaveTip>` 只在 `App.vue` 挂载一次；任何页面 / 子组件（弹幕窗、发送框这些标签页）直接调 `showSaved()` / `showError()` 即可。**不要**再在卡片里内联 `<p v-if="saved">`——它会把下方内容顶下去（页面抖动），也是「有的页面是浮层、有的内联、有的干脆没有」的来源：历史上弹幕窗标签页完全没有成功提示、发送框标签页是卡片底部内联文字（上游 v1.5.0 统一 `SaveTip` 时只改了三个页面级组件，两个 fork 自有子组件漏了）。新增任何会落盘的操作都要调 `showSaved()`，失败走 `showError()`——「保存了但界面没反馈」是 bug
- 设置页的零碎交互约定：① 敏感词 textarea **改完 0.6 秒自动保存**（`DanmakuFilterPanel` 里 debounce + 「解析后的词表与 draft 比较」防止回环），`保存敏感词` 按钮只是「立刻生效」；② 切换侧边栏页面时把 `.content` 的 `scrollTop` 归零（`App.vue` 的 watch——5 个页面共用同一个滚动容器）；③ 页内标签栏 `flex-wrap: wrap`，主窗缩到 520px 时 5 个标签放不下一行，换行比横向滚动好发现（Chromium 的横向滚动不吃竖向滚轮）；④ `style.css` 里统一了 `:focus-visible` 焦点环（输入框仍靠 border-color，各自写了 `outline: none`）；⑤「鼠标穿透 / 始终置顶」是运行态、不落盘，重启回默认——界面上已写明，别改成持久化而不改文案
- `DisplayDanmaku`（弹幕事件 + `isRoomMedal` 派生标记）定义在 `types/ipc.ts`，Overlay 列表与 `DanmakuRow` 共用
- Edge TTS 协议集中在 `tts/edge.rs`（音色表在 `edge/voices.rs`、SSML 在 `edge/ssml.rs`、DRM 令牌在 `edge/drm.rs`）：改协议用 `scripts/edge-tts-probe.ps1` 验证（`-DumpHeader` 看帧头、`-OutputFormat` 换格式），联网单测是 `cargo test --lib -- --ignored edge_synthesizes_mp3_live` / `voice_list_fetches_live`
- `Sec-MS-GEC` 令牌是 **v2**（`tts/edge/drm.rs`，`Sec-MS-GEC-Version: 2-…`）：微软 2026-10 起强制，v1（纯 SHA256 + `1-` 前缀）在部分地区已握手上报 403（`Invalid response status`），现象是本地能朗读、别人不能。v2 = `<16 hex CS64>-<64 hex SHA256>`，两套哈希的常数都被刻意改过，**不能**换成 `sha2` 标准实现；唯一可信的回归判据是官方向量单测 `cargo test --lib drm`（`v2_token_matches_captured_vector`）。另外 ticks 必须走浮点路径，别「优化」成整数运算——官方实测某些时间戳会因此错开、令牌失配。握手 403 时 `connect` 会按响应头 `Date` 校准本机时钟偏差（`drm.rs` 的 `CLOCK_SKEW_MS`，官方同款）再重试一次：偏差超过 5 分钟窗口时令牌怎么算都错，表现同样是「朗读全挂、弹幕正常」，本机时钟不准的用户靠这条自愈（联网回归测试 `edge_recovers_from_clock_skew_live`）
- `tts/text.rs` 的 `NAMED_CHARS`（`_`→下划线、`ω`→欧米伽等噪声字符表）是逐字实测得出的，加字符前先用探测脚本实测 Δ 字节数；实测数据在 `Task/findings.md`（该目录被 gitignore，仅本机可见）
- TTS 播放走 winmm MCI + 固定临时文件 `%TEMP%\bili-danmu-tts.mp3`，依赖队列串行（同一时刻只有一个 MCI 句柄）；`max_len` 只约束弹幕正文，不含身份前缀与用户名
- TTS 停播有两条互不相同的路径，别合并：①`epoch` 递增 = 作废**还没开播**的在途音频（积压时打断；正在念的那条念完，否则开了念用户名时只能听到半句名字）；②`worker.rs` 里 `player::play_mp3` 的 `stop_now` 闭包 = 每 100ms 查总开关，关掉朗读时立刻停。豁免只给 `manual`（试听）：它两道拦截都免疫；礼物 / 欢迎朗读以前靠 `force` 豁免，现在**只有 force 不豁免总开关**（挡的只是打断与挤占）。不要改回「从其他线程直接调 MCI stop」：它会与 open/play 交错，且阻塞弹幕回调
- 礼物朗读仍受合成串行约束：“插队”只改变**队列顺序**（`push_front`），当前这条合成/播放完才能轮到打赏，故插队后仍有最多「一条弹幕的合成 + 播放」延迟；想提前只能掐断正在念的音频，那会听到半句（见上一条）
- 单条合成有两层超时：`tts/worker.rs` 的 `SYNTH_TIMEOUT`（调用方包住整条，含建连）与 `edge.rs` 的 `RECV_TIMEOUT`（收流阶段），别只留一个
- Edge TTS **不接受连接复用**：同一条连接发第二轮 speech.config + ssml 必被 RST（10054，实测）。`synthesize` 每条新建连接，别改成连接池/长连接
- Edge TTS 建连走 `edge.rs` 自研的 `dial_tcp`（自己解析 DNS、**IPv4 优先**、全部失败才回退 IPv6），别换回 `tokio_tungstenite::connect_async`：后者按 DNS 返回顺序连（Windows 上 IPv6 通常排前）且不会因链路劣化换地址族，国内 IPv6 直连微软偶发被 RST，现象是弹幕一路正常而朗读整段全挂（收流阶段 10054）。合成失败日志末尾带「对端 IP」就是为了区分是哪条路径出的问题
- 合成失败必须走熔断退避（`tts/worker.rs`：连续 2 次 → 清空积压 + 1/2/4…封顶 60s）：失败是 ~0.1s 级响应、成功是 ~2s，若“失败立即重试下一条”，请求速率会瞬时飙升十倍，把服务端限流撞得更紧并自我维持（表现为持续 10054 + 20s 黑洞超时）。排查用 `cargo test --lib -- --ignored edge_burst_live --nocapture`
- 日志一律走 `log::info!/warn!/error!`，**禁止新增 `eprintln!` / `println!`**：release 是 `windows_subsystem = "windows"`，stderr 直接丢失，排障时拿不到任何现场（`#[cfg(test)]` 里的探测打印除外）。插件配置（落盘目录 / 5MB 轮转 / 保留 7 份 / 本地时区 / dev 终端）只在 `lib.rs` 注册处一处，细节见 `docs/技术说明.md` 的「统一日志」；日志里不得出现 Cookie 凭据
