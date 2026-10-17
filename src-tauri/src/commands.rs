//! IPC 命令：前端调用的全部入口
//!
//! 连接相关命令（connect_room / disconnect_room / get_connection_status）在 `connection.rs`。
//! 这里只做「读内存态 / 写内存态 + 广播事件 + 落盘」的薄封装，业务逻辑不落在此处。

use crate::bilibili;
use crate::config;
use crate::shortcut;
use crate::state::{prune_ticks, AppState, OverlayState, RoomStatus, SenderState};
use crate::tts;
use crate::window::{overlay_window, reset_overlay_position, set_sender_visible, sync_sender_docked};
use serde_json::json;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

#[tauri::command]
pub(crate) fn ping() -> String {
    "pong".into()
}

/// 显示 / 隐藏 Overlay（返回新可见状态）
///
/// 可见性写内存态并广播 `overlay-visible`：设置页与发送框都可能改它，
/// 且前端启动时就会读一次（此时窗口未必已创建），一律以内存态为准
#[tauri::command]
pub(crate) fn overlay_set_visible(
    app: AppHandle,
    state: State<'_, OverlayState>,
    visible: bool,
) -> Result<bool, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    if visible {
        win.show().map_err(|e| format!("显示失败: {e}"))?;
    } else {
        win.hide().map_err(|e| format!("隐藏失败: {e}"))?;
    }
    *state.visible.lock().unwrap() = visible;
    let _ = app.emit("overlay-visible", visible);
    Ok(visible)
}

/// 开关鼠标穿透（核心桌面功能：开启后鼠标事件穿透 Overlay 落到下层窗口）
#[tauri::command]
pub(crate) fn overlay_set_clickthrough(
    app: AppHandle,
    state: State<'_, OverlayState>,
    enabled: bool,
) -> Result<bool, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    win.set_ignore_cursor_events(enabled)
        .map_err(|e| format!("设置穿透失败: {e}"))?;
    *state.clickthrough.lock().unwrap() = enabled;
    // 广播：穿透开关有两个入口（主界面「弹幕窗」标签、发送框右端按钮），另一侧要跟着更新
    let _ = app.emit("overlay-clickthrough", enabled);
    Ok(enabled)
}

/// 查询当前穿透状态
#[tauri::command]
pub(crate) fn overlay_get_clickthrough(state: State<'_, OverlayState>) -> bool {
    *state.clickthrough.lock().unwrap()
}

/// 查询 Overlay 是否可见（读内存态，不查窗口：窗口未必已创建，见 `OverlayState::visible`）
#[tauri::command]
pub(crate) fn overlay_is_visible(state: State<'_, OverlayState>) -> bool {
    *state.visible.lock().unwrap()
}

/// 读取发送框配置（可见性 + 已注册的快捷键）
#[tauri::command]
pub(crate) fn sender_get_config(state: State<'_, SenderState>) -> config::SenderConfig {
    config::SenderConfig {
        visible: *state.visible.lock().unwrap(),
        hotkey: state.hotkey.lock().unwrap().clone(),
    }
}

/// 设置发送框配置（设置页用）
///
/// 顺序有讲究：**先试注册快捷键**，失败就直接返回错误、可见性与配置都不改——
/// 否则会出现「设置页显示新键位、实际没注册上」的假象。注册成功后才动可见性并落盘。
#[tauri::command]
pub(crate) fn sender_set_config(
    app: AppHandle,
    state: State<'_, SenderState>,
    sender: config::SenderConfig,
) -> Result<config::SenderConfig, String> {
    let old_hotkey = state.hotkey.lock().unwrap().clone();
    if sender.hotkey != old_hotkey {
        shortcut::apply_hotkey(&app, &old_hotkey, &sender.hotkey)?;
        *state.hotkey.lock().unwrap() = sender.hotkey.clone();
        *state.hotkey_error.lock().unwrap() = None;
    } else if !sender.hotkey.is_empty() && state.hotkey_error.lock().unwrap().is_some() {
        // 上次没注册上（例如启动时被别的程序占着）：借这次保存再试一次，
        // 用户可能已经关掉那个程序了。失败也不拦保存，只更新 hotkey_error，
        // 由前端在保存后回读 `sender_hotkey_error` 决定提示「已保存」还是「键位没注册上」。
        match shortcut::apply_hotkey(&app, "", &sender.hotkey) {
            Ok(()) => *state.hotkey_error.lock().unwrap() = None,
            Err(e) => *state.hotkey_error.lock().unwrap() = Some(e),
        }
    }
    apply_sender_visible(&app, sender.visible)?;
    config::save_sender_config(&app, &sender)?;
    // 设置页是显式保存动作：同步落盘后同步「已落盘值」，免得周期任务再写一遍
    *state.saved_visible.lock().unwrap() = sender.visible;
    Ok(sender)
}

/// 查询上次快捷键注册失败的原因（启动时被占用也走这里；无错返回 None）
#[tauri::command]
pub(crate) fn sender_hotkey_error(state: State<'_, SenderState>) -> Option<String> {
    state.hotkey_error.lock().unwrap().clone()
}

/// 写内存态 + 广播 + 真正动窗口（命令、快捷键、托盘三处共用）
fn apply_sender_visible(app: &AppHandle, visible: bool) -> Result<bool, String> {
    let state = app.state::<SenderState>();
    if *state.visible.lock().unwrap() == visible {
        return Ok(visible);
    }
    let v = set_sender_visible(app, visible)?;
    *state.visible.lock().unwrap() = v;
    let _ = app.emit("sender-visible", v);
    Ok(v)
}

/// 翻转发送框可见性（全局快捷键与托盘菜单共用）
///
/// 这里不能用 `State<'_, SenderState>`：快捷键回调与托盘事件只拿得到 `AppHandle`。
/// **不在这里落盘**：热键回调跑在主线程（插件的隐藏窗口建在主线程），而写盘是整份 config 的
/// load-modify-save + DPAPI 加解密——每按一下写一次会让主线程跟着磁盘走。可见性先只改内存态，
/// 由周期任务 `window::flush_sender_visible` 去重后落盘（连续按十下只在停手后写一次）。
pub(crate) fn toggle_sender(app: &AppHandle) {
    let visible = !*app.state::<SenderState>().visible.lock().unwrap();
    if let Err(e) = apply_sender_visible(app, visible) {
        log::warn!("[sender] 切换可见性失败: {e}");
    }
}

/// 开关始终置顶
#[tauri::command]
pub(crate) fn overlay_set_always_on_top(
    app: AppHandle,
    state: State<'_, OverlayState>,
    enabled: bool,
) -> Result<bool, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    win.set_always_on_top(enabled)
        .map_err(|e| format!("设置置顶失败: {e}"))?;
    *state.always_on_top.lock().unwrap() = enabled;
    // 广播：置顶开关有两个入口（「弹幕窗」标签、直播间页快捷开关），另一侧要跟着更新
    let _ = app.emit("overlay-always-on-top", enabled);
    Ok(enabled)
}

/// 仪表盘状态快照（主界面直播间页底部只读展示，前端按秒轮询）
#[tauri::command]
pub(crate) fn get_dashboard_status(app: AppHandle) -> serde_json::Value {
    let overlay = {
        let st = app.state::<OverlayState>();
        json!({
            "visible": *st.visible.lock().unwrap(),
            "clickthrough": *st.clickthrough.lock().unwrap(),
            "always_on_top": *st.always_on_top.lock().unwrap(),
        })
    };

    let tts_state = app.state::<tts::TtsState>();
    let tts = tts_state.config();
    let gift_tts = tts_state.gift_config();
    let welcome = app.state::<OverlayState>().welcome.lock().unwrap().clone();

    let now = Instant::now();
    let danmaku_rate = {
        let st = app.state::<AppState>();
        let mut ticks = st.danmaku_ticks.lock().unwrap();
        prune_ticks(&mut ticks, now);
        ticks.len()
    };

    json!({
        "overlay": overlay,
        "tts": {
            "master_enabled": tts.master_enabled,
            "enabled": tts.enabled,
            "filter": tts.filter,
        },
        "gift_tts": { "enabled": gift_tts.enabled },
        "welcome": { "tts_guard": welcome.tts_guard },
        "danmaku_rate": danmaku_rate,
    })
}

/// 读取当前 Overlay 弹幕样式
#[tauri::command]
pub(crate) fn overlay_get_style(state: State<'_, OverlayState>) -> config::OverlayStyle {
    state.style.lock().unwrap().clone()
}

/// 更新 Overlay 弹幕样式：广播给 Overlay 窗口并持久化
#[tauri::command]
pub(crate) fn overlay_set_style(
    app: AppHandle,
    state: State<'_, OverlayState>,
    style: config::OverlayStyle,
) -> Result<(), String> {
    *state.style.lock().unwrap() = style.clone();
    let _ = app.emit("overlay-style", &style);
    config::save_overlay_style(&app, &style)?;
    // 弹幕字号变化影响发送框缩进与高度，同步重对齐
    sync_sender_docked(&app);
    Ok(())
}

/// 预览弹幕样式：只更新内存态并广播，**不落盘**
///
/// 设置页的滑块在拖动中会以 ~12 次/秒调用（`DanmakuView` 里 80ms 节流），
/// 而 `overlay_set_style` 每次都要整份 config 的 load-modify-save + DPAPI 加解密，
/// 拖动时每帧写盘会把主进程拖在磁盘上。所以：**拖动中走这条、松手时走 `overlay_set_style`**。
///
/// 故意不在这里重排发送框：`sync_sender_docked` 内部是 set_position + set_size 两次窗口操作，
/// 拖动中每帧做一次会和滑块抢帧；松手时的 `overlay_set_style` 会补上（发送框尺寸晚一步可见）。
#[tauri::command]
pub(crate) fn overlay_preview_style(
    app: AppHandle,
    state: State<'_, OverlayState>,
    style: config::OverlayStyle,
) {
    *state.style.lock().unwrap() = style.clone();
    let _ = app.emit("overlay-style", &style);
}

/// 读取「在线人数 + 实时在线榜」的最近一次快照
///
/// 弹幕窗挂载时读一次（此前已经拉过就立刻有数），之后靠 `audience-update` 事件更新。
#[tauri::command]
pub(crate) fn audience_get(
    state: State<'_, crate::state::AudienceState>,
) -> Option<bilibili::audience::AudienceSnapshot> {
    state.snapshot.lock().unwrap().clone()
}

/// 切换弹幕窗右侧在线列的显示（「在看 N 人」与「实时在线榜」两个开关，互相独立）
///
/// 走整份 style 的 load-modify-save，而不是让设置页提交整个 style 对象：设置页与其它页面
/// 各持一份副本时，后保存的会把前一个页面的改动一起覆盖掉。
#[tauri::command]
pub(crate) fn overlay_set_audience(
    app: AppHandle,
    state: State<'_, OverlayState>,
    show_online: bool,
    show_rank: bool,
) -> Result<(), String> {
    let mut style = state.style.lock().unwrap().clone();
    style.show_online = show_online;
    style.show_rank = show_rank;
    *state.style.lock().unwrap() = style.clone();
    let _ = app.emit("overlay-style", &style);
    config::save_overlay_style(&app, &style)?;
    // 右侧列出现/消失会改变面板宽度，发送框要跟着重新对齐
    sync_sender_docked(&app);
    Ok(())
}

/// 读取当前弹幕过滤配置
#[tauri::command]
pub(crate) fn danmaku_get_filter(state: State<'_, OverlayState>) -> config::DanmakuFilter {
    state.filter.lock().unwrap().clone()
}

/// 更新弹幕过滤配置：广播给 Overlay 窗口（实时生效）并持久化
#[tauri::command]
pub(crate) fn danmaku_set_filter(
    app: AppHandle,
    state: State<'_, OverlayState>,
    filter: config::DanmakuFilter,
) -> Result<(), String> {
    *state.filter.lock().unwrap() = filter.clone();
    let _ = app.emit("danmaku-filter", &filter);
    config::save_danmaku_filter(&app, &filter)
}

/// 读取当前礼物列表配置
#[tauri::command]
pub(crate) fn gift_get_config(state: State<'_, OverlayState>) -> config::GiftConfig {
    state.gift.lock().unwrap().clone()
}

/// 更新礼物列表配置：持久化（门槛与连击合并由 Rust 侧判定，广播供设置页回显）
#[tauri::command]
pub(crate) fn gift_set_config(
    app: AppHandle,
    state: State<'_, OverlayState>,
    gift: config::GiftConfig,
) -> Result<(), String> {
    *state.gift.lock().unwrap() = gift.clone();
    let _ = app.emit("gift-config", &gift);
    config::save_gift_config(&app, &gift)
}

/// 读取礼物朗读配置
#[tauri::command]
pub(crate) fn gift_tts_get_config(state: State<'_, tts::TtsState>) -> config::GiftTtsConfig {
    state.gift_config()
}

/// 更新礼物朗读配置：内存态实时生效并持久化（开关/门槛独立于弹幕朗读与礼物区）
#[tauri::command]
pub(crate) fn gift_tts_set_config(
    app: AppHandle,
    state: State<'_, tts::TtsState>,
    gift_tts: config::GiftTtsConfig,
) -> Result<(), String> {
    state.set_gift_config(gift_tts.clone());
    config::save_gift_tts_config(&app, &gift_tts)
}

/// 读取欢迎信息配置
#[tauri::command]
pub(crate) fn welcome_get_config(state: State<'_, OverlayState>) -> config::WelcomeConfig {
    state.welcome.lock().unwrap().clone()
}

/// 更新欢迎信息配置：持久化并广播
///
/// 欢迎信息在 Rust 侧完成去重与限速后才广播，前端只负责把收到的行渲染出来；
/// 关掉开关时 Rust 直接不广播，前端不需要再判一遍。
#[tauri::command]
pub(crate) fn welcome_set_config(
    app: AppHandle,
    state: State<'_, OverlayState>,
    welcome: config::WelcomeConfig,
) -> Result<(), String> {
    *state.welcome.lock().unwrap() = welcome.clone();
    let _ = app.emit("welcome-config", &welcome);
    config::save_welcome_config(&app, &welcome)
}

/// 查询 Overlay 当前尺寸（逻辑像素，与 overlay_set_size 同一口径）
///
/// 读内存态 `bounds`：窗口建好后由 `capture_overlay_bounds` 播种并随移动 / 缩放持续更新，
/// 直接查窗口 API 会有与 `overlay_is_visible` 相同的启动竞态（前端启动时窗口未必创建完）
#[tauri::command]
pub(crate) fn overlay_get_size(state: State<'_, OverlayState>) -> Result<serde_json::Value, String> {
    let b = state.bounds.lock().unwrap();
    match *b {
        Some(b) => Ok(json!({ "width": b.w.round(), "height": b.h.round() })),
        None => Err("弹幕窗尺寸尚未就绪".into()),
    }
}

/// 调整 Overlay 宽高（设置页滑块调用；尺寸变化自动被窗口事件捕获落盘）
#[tauri::command]
pub(crate) fn overlay_set_size(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    win.set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| format!("调整尺寸失败: {e}"))
}

/// 把弹幕窗移回主显示器中央
///
/// 换显示器 / 改分辨率 / 拔掉副屏之后，弹幕窗可能落在不存在的坐标上：看不见也拖不回来，
/// 而设置页只能改宽高，没有位置入口。尺寸只在比工作区还大时才收窄。
#[tauri::command]
pub(crate) fn overlay_reset_position(app: AppHandle) -> Result<(), String> {
    reset_overlay_position(&app)
}

/// 查询登录态
#[tauri::command]
pub(crate) fn get_login_info(state: State<'_, AppState>) -> serde_json::Value {
    let auth = state.auth.lock().unwrap();
    json!({
        "loggedIn": auth.is_some(),
        "uid": auth.as_ref().map(|a| a.uid).unwrap_or(0),
        "uname": auth.as_ref().and_then(|a| a.uname.clone()),
    })
}

/// 生成 B 站登录二维码
#[tauri::command]
pub(crate) async fn qr_generate() -> Result<bilibili::login::QrData, String> {
    let client = bilibili::api::http_client();
    bilibili::login::qr_generate(&client).await
}

/// 轮询扫码结果；成功后持久化登录态并返回用户 UID
#[tauri::command]
pub(crate) async fn qr_poll(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<bilibili::login::PollResult, String> {
    let client = bilibili::api::http_client();
    match bilibili::login::qr_poll(&client, &key).await? {
        bilibili::login::PollResult::Success { cookies } => {
            let uid = bilibili::login::cookie_value(&cookies, "DedeUserID")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            if uid == 0 {
                return Ok(bilibili::login::PollResult::Error {
                    message: "登录响应缺少 DedeUserID".into(),
                });
            }
            // 顺带拉取昵称（失败不影响登录，仅显示用）
            let uname = bilibili::login::fetch_uname(&client, &cookies).await.ok();
            let auth = config::AuthInfo {
                uid,
                cookies: cookies.clone(),
                uname,
            };
            config::save_auth(&app, &auth)?;
            *state.auth.lock().unwrap() = Some(auth);
            // 原样回传登录 Cookie 串（前端仅消费 status 字段，不需要关心内容）——
            // 不复用 cookies 字段传 uid，保持与 login::PollResult 的字段语义一致
            Ok(bilibili::login::PollResult::Success { cookies })
        }
        other => Ok(other),
    }
}

/// 退出登录（清除本地登录态）
#[tauri::command]
pub(crate) fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    *state.auth.lock().unwrap() = None;
    config::clear_auth(&app)
}

/// 发送弹幕到当前连接的直播间（需登录态）
#[tauri::command]
pub(crate) async fn send_danmaku(state: State<'_, AppState>, msg: String) -> Result<(), String> {
    let msg = msg.trim();
    if msg.is_empty() {
        return Err("弹幕内容不能为空".into());
    }
    if msg.chars().count() > 100 {
        return Err("弹幕最长 100 字".into());
    }

    // 登录态（含 bili_jct）——锁作用域内克隆，避免持锁 await
    let cookies = {
        let auth = state.auth.lock().unwrap();
        match auth.as_ref() {
            Some(a) => a.cookies.clone(),
            None => return Err("请先扫码登录后再发送弹幕".into()),
        }
    };

    // 当前连接的真实房间号（未连接时无发送目标）
    let room_id = {
        let st = state.status.lock().unwrap();
        match *st {
            RoomStatus::Connected { room_id } => room_id,
            _ => return Err("请先连接直播间后再发送弹幕".into()),
        }
    };

    let client = bilibili::api::http_client();
    bilibili::send::send_danmaku(&client, &cookies, room_id, msg).await
}

/// 查询「我的直播间」（streamer 分支主界面优先展示；只需登录态，与当前是否已连接房间无关）
///
/// 接口匿名可调，故**不传登录 Cookie**（少一处凭据外发）：只用登录态里的 uid。
#[tauri::command]
pub(crate) async fn my_room_info(
    state: State<'_, AppState>,
) -> Result<Option<bilibili::live::MyRoomInfo>, String> {
    let uid = {
        let auth = state.auth.lock().unwrap();
        match auth.as_ref() {
            Some(a) => a.uid,
            None => return Err("请先扫码登录后查看我的直播间".into()),
        }
    };

    let client = bilibili::api::http_client();
    bilibili::live::fetch_my_room(&client, uid).await
}

/// 查询「已关注且正在开播」的主播（主界面手动刷新；只需登录态，与当前是否已连接房间无关）
#[tauri::command]
pub(crate) async fn list_following_live(
    state: State<'_, AppState>,
) -> Result<Vec<bilibili::live::FollowingLive>, String> {
    // 关注列表接口未登录返回 -101；锁作用域内克隆，避免持锁 await
    let (cookies, uid) = {
        let auth = state.auth.lock().unwrap();
        match auth.as_ref() {
            Some(a) => (a.cookies.clone(), a.uid),
            None => return Err("请先扫码登录后查看关注在播".into()),
        }
    };

    let client = bilibili::api::http_client();
    let followings = bilibili::live::fetch_followings(&client, &cookies, uid).await?;
    let uids: Vec<i64> = followings.iter().map(|(u, _)| *u).collect();
    let live = bilibili::live::fetch_live_rooms(&client, &uids).await?;
    // 昵称取自关注列表（批量状态接口也带 uname，但没必要再解析一份）
    Ok(followings
        .into_iter()
        .filter_map(|(u, uname)| {
            live.get(&u).map(|&room_id| bilibili::live::FollowingLive {
                uid: u,
                uname,
                room_id,
            })
        })
        .collect())
}

/// 查询最近连接的直播间（主界面面包屑，已按最近在前排序）
#[tauri::command]
pub(crate) fn get_recent_rooms(app: AppHandle) -> Vec<config::RecentRoom> {
    config::load_config(&app).recent_rooms
}

/// 读取 TTS 朗读配置
#[tauri::command]
pub(crate) fn tts_get_config(state: State<'_, tts::TtsState>) -> config::TtsConfig {
    state.config()
}

/// 更新 TTS 朗读配置：内存态实时生效（下一段朗读即生效）并持久化
#[tauri::command]
pub(crate) fn tts_set_config(
    app: AppHandle,
    state: State<'_, tts::TtsState>,
    tts: config::TtsConfig,
) -> Result<(), String> {
    state.set_config(tts.clone());
    config::save_tts_config(&app, &tts)?;
    emit_tts_switches(&app, &tts);
    Ok(())
}

/// 广播朗读开关的当前值：直播间页快捷开关与「朗读设置」页要互相同步
fn emit_tts_switches(app: &AppHandle, tts: &config::TtsConfig) {
    let _ = app.emit(
        "tts-switches",
        json!({ "master_enabled": tts.master_enabled, "enabled": tts.enabled }),
    );
}

/// 只改一个开关并落盘：`state.set_config` 会按新状态清队列 / 作废在途音频
///
/// 做成单字段命令而不是让前端提交整份 `TtsConfig`：直播间页快捷开关与「朗读设置」页
/// 可能同时改配置（一个切开关、一个调音色），回传整份对象会把对方的改动覆盖掉。
fn save_switches(
    app: &AppHandle,
    state: &tts::TtsState,
    config: config::TtsConfig,
) -> Result<(), String> {
    state.set_config(config.clone());
    config::save_tts_config(app, &config)?;
    emit_tts_switches(app, &config);
    Ok(())
}

/// 全部朗读总开关（弹幕 / 礼物 / 欢迎一起管；设置页「试听」除外）
#[tauri::command]
pub(crate) fn tts_set_master_enabled(
    app: AppHandle,
    state: State<'_, tts::TtsState>,
    enabled: bool,
) -> Result<bool, String> {
    let mut config = state.config();
    config.master_enabled = enabled;
    save_switches(&app, &state, config)?;
    Ok(enabled)
}

/// 弹幕朗读开关（只影响弹幕；礼物 / 欢迎朗读不受它影响，但都受总开关管）
#[tauri::command]
pub(crate) fn tts_set_danmaku_enabled(
    app: AppHandle,
    state: State<'_, tts::TtsState>,
    enabled: bool,
) -> Result<bool, String> {
    let mut config = state.config();
    config.enabled = enabled;
    save_switches(&app, &state, config)?;
    Ok(enabled)
}

/// 礼物朗读开关（与弹幕朗读互不影响；总开关关着时同样不念）
#[tauri::command]
pub(crate) fn gift_tts_set_enabled(
    app: AppHandle,
    state: State<'_, tts::TtsState>,
    enabled: bool,
) -> Result<bool, String> {
    let mut cfg = state.gift_config();
    cfg.enabled = enabled;
    state.set_gift_config(cfg.clone());
    config::save_gift_tts_config(&app, &cfg)?;
    let _ = app.emit("gift-tts-enabled", enabled);
    Ok(enabled)
}

/// 欢迎信息朗读开关（只念舰长进场；与弹幕朗读互不影响，但都受总开关管）
#[tauri::command]
pub(crate) fn welcome_tts_set_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let state = app.state::<OverlayState>();
    let mut welcome = state.welcome.lock().unwrap().clone();
    welcome.tts_guard = enabled;
    *state.welcome.lock().unwrap() = welcome.clone();
    config::save_welcome_config(&app, &welcome)?;
    let _ = app.emit("welcome-tts-enabled", enabled);
    Ok(enabled)
}

/// 试听当前音色/语速（不受总开关限制）
#[tauri::command]
pub(crate) fn tts_test_speak(app: AppHandle, text: Option<String>) {
    let text = text.unwrap_or_else(|| "试听效果，欢迎来到直播间".into());
    tts::speak_test(&app, &text);
}

/// 当前音色列表（内置中文音色兜底，拉取成功后为微软完整列表）
#[tauri::command]
pub(crate) fn tts_list_voices(state: State<'_, tts::TtsState>) -> Vec<serde_json::Value> {
    voice_options(&state.voices())
}

/// 从微软拉取完整音色列表（当前 322 个）并缓存到内存
#[tauri::command]
pub(crate) async fn tts_refresh_voices(app: AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let voices = tts::edge::fetch_voices().await?;
    let state = app.state::<tts::TtsState>();
    state.set_voices(voices.clone());
    Ok(voice_options(&voices))
}

fn voice_options(voices: &[(String, String)]) -> Vec<serde_json::Value> {
    voices
        .iter()
        .map(|(id, label)| json!({ "id": id, "label": label }))
        .collect()
}

/// 打开日志目录（关于页「日志」按钮；排障时让用户能直接拿到日志文件）
///
/// 目录可能还不存在（本次运行还没写过日志、或用户手动清过），先建出来再开：
/// ShellExecuteW 对不存在的路径会返回 SE_ERR_* 而打不开。
#[tauri::command]
pub(crate) fn open_log_dir(app: AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("取日志目录失败: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建日志目录失败: {e}"))?;
    crate::update::open_in_shell(&dir.to_string_lossy())
}
