//! bili-danmu 核心入口（M2：B 站真实 WebSocket 弹幕连接 + 扫码登录）
//!
//! 连接流程：connect_room → 短号解析真实房间号 → 获取弹幕服务器配置(token)
//!   → WebSocket 认证(protover=3, 登录 UID) → 30s 心跳 → 实时接收弹幕 → emit 给前端
//! 登录：B 站 2025+ 要求登录态才推送弹幕，扫码登录后 cookie 持久化于本地配置
//!
//! 模块分工：状态见 `state.rs`，IPC 命令见 `commands.rs`，连接与重连见 `connection.rs`，
//! 窗口辅助见 `window.rs`；本文件只做组装（状态托管、日志插件、窗口创建、托盘、命令注册）。

mod bilibili;
mod commands;
mod config;
mod connection;
mod gift;
mod shortcut;
mod state;
mod tts;
mod update;
mod welcome;
mod window;

use state::{AppState, OverlayState, SenderState};
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use window::{capture_overlay_bounds, flush_overlay_bounds, flush_sender_visible, sync_sender_docked};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // 单实例：必须最先注册——第二个实例在插件初始化阶段就退出，
        // 不会走到 setup 去建托盘/Overlay/连接；同时唤出已有实例的主窗口
        // （点托盘关闭后主窗是隐藏态，需先 show 再聚焦）
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        // 统一日志：level / 落盘目录 / 轮转策略都在这里定，业务代码只调 log::* 宏
        //
        // 默认 max_file_size 只有 40KB 且 KeepOne（超限即丢旧文件），对「出事翻日志」
        // 毫无用处，故放大到 5MB 并保留最近 7 份（轮转出的旧文件会带上日期后缀）。
        // 时区必须显式切本地时间：默认 UTC，国内用户对着日志还原现场会差 8 小时。
        // release 是 windows_subsystem = "windows"，没有可用的 stdout，只挂 LogDir；
        // dev 额外挂 Stdout，保留 `pnpm tauri dev` 的终端输出。
        .plugin({
            let mut targets = vec![tauri_plugin_log::Target::new(
                tauri_plugin_log::TargetKind::LogDir { file_name: None },
            )];
            if cfg!(debug_assertions) {
                targets.push(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ));
            }
            tauri_plugin_log::Builder::new()
                .targets(targets)
                .level(log::LevelFilter::Info)
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(7))
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .build()
        })
        // 全局快捷键：发送框显示/隐藏。事件处理挂在这里（对该插件所有已注册快捷键生效），
        // 具体键位的注册/注销在 setup 与设置页里做，见 shortcut::apply_hotkey
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // 只认按下：按下与松开各触发一次的话等于没切
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        commands::toggle_sender(app);
                    }
                })
                .build(),
        )
        .manage(AppState {
            status: Mutex::new(state::RoomStatus::Disconnected),
            cancel: Mutex::new(None),
            auth: Mutex::new(None),
            danmaku_ticks: Mutex::new(VecDeque::new()),
        })
        .manage(OverlayState::default())
        .manage(SenderState::default())
        .manage(state::AudienceState::default())
        .manage(tts::TtsState::new(config::TtsConfig::default()))
        .manage(gift::GiftState::default())
        .manage(welcome::WelcomeState::default())
        .setup(|app| {
            // 加载持久化配置：登录态 → AppState.auth；样式/位置 → OverlayState
            let cfg = config::load_config(app.handle());
            if let Some(auth) = &cfg.auth {
                let st = app.state::<AppState>();
                *st.auth.lock().unwrap() = Some(auth.clone());
                log::info!("[auth] 已恢复登录态 uid={}", auth.uid);
            } else {
                log::info!("[auth] 未登录（游客态收不到弹幕，请扫码登录）");
            }
            {
                let ov = app.state::<OverlayState>();
                *ov.style.lock().unwrap() = cfg.overlay_style.clone();
                *ov.filter.lock().unwrap() = cfg.danmaku_filter.clone();
                *ov.gift.lock().unwrap() = cfg.gift.clone();
                *ov.welcome.lock().unwrap() = cfg.welcome.clone();
            }
            // TTS：恢复朗读配置并启动串行朗读 worker（与弹幕显示完全解耦）
            app.state::<tts::TtsState>()
                .set_config(cfg.tts.clone());
            app.state::<tts::TtsState>()
                .set_gift_config(cfg.gift_tts.clone());
            if cfg.gift_tts.enabled {
                log::info!("[tts] 已恢复礼物朗读开关，门槛={} 元", cfg.gift_tts.min_amount_yuan);
            }
            if cfg.tts.enabled {
                log::info!("[tts] 已恢复朗读开关，音色={}", cfg.tts.voice);
            }
            tts::spawn_worker(app.handle().clone());

            // Overlay：透明 / 无边框 / 置顶 / 可调整 / 跳过任务栏，恢复上次位置
            let mut win_builder = WebviewWindowBuilder::new(
                app,
                "overlay",
                WebviewUrl::App("overlay.html".into()),
            )
            .title("bili-danmu-streamer 弹幕窗")
            .inner_size(700.0, 400.0)
            .position(80.0, 80.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .resizable(true)
            .min_inner_size(120.0, 40.0)
            .skip_taskbar(true)
            .shadow(false);
            if let Some(b) = cfg.overlay_bounds {
                win_builder = win_builder.position(b.x, b.y).inner_size(b.w, b.h);
            }
            if let Ok(win) = win_builder.build() {
                let app2 = app.handle().clone();
                // 播种初始尺寸：窗口建好后先把真实 bounds 写进内存态（顺带广播一次 overlay-size），
                // 否则前端启动时读 overlay_get_size 会撞上「窗口还没建好」的竞态、滑块停在默认值
                capture_overlay_bounds(&app2);
                win.on_window_event(move |event| {
                    if matches!(
                        event,
                        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
                    ) {
                        capture_overlay_bounds(&app2);
                        // 发送框始终吸附：弹幕窗移动/缩放时跟随
                        sync_sender_docked(&app2);
                    }
                });
            } else {
                log::error!("[overlay] 创建失败");
            }
            // 周期落盘窗口位置与发送框可见性（拖动/缩放/快捷键切换中去重保存）
            // + 每 30 秒拉一次在线人数 / 实时在线榜（2s × 15；换房后立刻拉一次）
            let app3 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(2));
                let mut ticks: u64 = 0;
                loop {
                    interval.tick().await;
                    flush_overlay_bounds(&app3);
                    // 快捷键/托盘切换只改内存态，这里去重落盘（避免每按一下写整份 config）
                    flush_sender_visible(&app3);
                    ticks += 1;
                    if ticks % AUDIENCE_EVERY_TICKS == 0 || audience_room_changed(&app3) {
                        let app_aud = app3.clone();
                        tauri::async_runtime::spawn(async move {
                            poll_audience(&app_aud).await;
                        });
                    }
                }
            });

            // Sender：发送框始终吸附在弹幕窗下方（无边框 / 置顶 / 跳过任务栏）
            let sender_builder = WebviewWindowBuilder::new(
                app,
                "sender",
                WebviewUrl::App("sender.html".into()),
            )
            .title("bili-danmu-streamer 发送")
            .inner_size(320.0, 80.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .resizable(false)
            .skip_taskbar(true)
            // 隐藏态在创建时就带上：否则每次启动都 show→hide 一次，可能闪一帧
            .visible(cfg.sender.visible)
            .shadow(false);
            if sender_builder.build().is_err() {
                log::error!("[sender] 创建失败");
            }
            // 创建后立即对齐到弹幕窗下方（覆盖默认位置/尺寸）
            sync_sender_docked(app.handle());
            // 发送框：把配置写进内存态并注册全局快捷键（窗口可见性在创建时已按配置设好，
            // 不必再 show→hide 一次）。快捷键注册失败（被其它程序占用）只记日志，
            // 设置页会读 `sender_hotkey_error` 提示换键位。
            {
                let st = app.state::<SenderState>();
                *st.visible.lock().unwrap() = cfg.sender.visible;
                *st.hotkey.lock().unwrap() = cfg.sender.hotkey.clone();
                // 刚从配置读出来的值就是「已落盘值」，别让周期任务再写一遍
                *st.saved_visible.lock().unwrap() = cfg.sender.visible;
            }
            // 启动时没有旧键位要注销，故 old 传空串
            match shortcut::apply_hotkey(app.handle(), "", &cfg.sender.hotkey) {
                Ok(()) if !cfg.sender.hotkey.is_empty() => {
                    log::info!("[sender] 已注册显示/隐藏快捷键 {}", cfg.sender.hotkey);
                }
                Ok(()) => log::info!("[sender] 未设置快捷键（可在设置页录制）"),
                Err(e) => {
                    log::warn!("[sender] 快捷键{e}，可在设置页改一个");
                    // 让设置页也能看到：避免「配置里写着某键位、实际被占用没注册上」的假象
                    *app.state::<SenderState>().hotkey_error.lock().unwrap() = Some(e);
                }
            }

            // 主窗口 × → 最小化到托盘（不退出；由托盘菜单唤出/退出）
            if let Some(main_win) = app.get_webview_window("main") {
                let app2 = app.handle().clone();
                main_win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = app2.get_webview_window("main").map(|w| w.hide());
                    }
                });
            }

            // 系统托盘：左键单击显示设置窗口
            let show_item = MenuItem::with_id(app, "show", "显示设置", true, None::<&str>)
                .map_err(|e| log::error!("[tray] 创建菜单项失败: {e}"))
                .ok();
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
                .map_err(|e| log::error!("[tray] 创建菜单项失败: {e}"))
                .ok();
            // 发送框隐藏后，快捷键被游戏占用/被改坏时的兜底入口（托盘左键也能唤出主窗口）
            let sender_item =
                MenuItem::with_id(app, "sender", "显示/隐藏发送框", true, None::<&str>)
                    .map_err(|e| log::error!("[tray] 创建菜单项失败: {e}"))
                    .ok();
            let mut items: Vec<&dyn tauri::menu::IsMenuItem<_>> = Vec::new();
            if let Some(i) = &show_item {
                items.push(i);
            }
            if let Some(i) = &sender_item {
                items.push(i);
            }
            if let Some(i) = &quit_item {
                items.push(i);
            }
            if let Ok(menu) = Menu::with_items(app, &items) {
                let tray = TrayIconBuilder::with_id("main-tray")
                    .icon(app.default_window_icon().expect("default window icon").clone())
                    .tooltip("bili-danmu-streamer")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                        "sender" => commands::toggle_sender(app),
                        "quit" => {
                            // 先销毁窗口再退出：destroy 绕过主窗 ×→隐藏 拦截，
                            // 让 WebView2 在进程退出前释放 HWND，避免 1412 unregister 竞态
                            let wins: Vec<_> =
                                app.webview_windows().into_values().collect();
                            for w in wins {
                                let _ = w.destroy();
                            }
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                    })
                    .build(app)
                    .map_err(|e| log::error!("[tray] 创建托盘失败: {e}"))
                    .ok();
                let _ = tray; // 托盘由 tauri 管理，持有即保活
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            connection::connect_room,
            connection::disconnect_room,
            connection::get_connection_status,
            commands::get_login_info,
            commands::qr_generate,
            commands::qr_poll,
            commands::logout,
            commands::send_danmaku,
            commands::overlay_set_visible,
            commands::overlay_set_clickthrough,
            commands::overlay_get_clickthrough,
            commands::overlay_is_visible,
            commands::overlay_set_always_on_top,
            commands::sender_get_config,
            commands::sender_set_config,
            commands::sender_hotkey_error,
            commands::get_dashboard_status,
            commands::overlay_get_style,
            commands::overlay_set_style,
            commands::danmaku_get_filter,
            commands::danmaku_set_filter,
            commands::gift_get_config,
            commands::gift_set_config,
            commands::gift_tts_get_config,
            commands::gift_tts_set_config,
            commands::welcome_get_config,
            commands::welcome_set_config,
            commands::tts_get_config,
            commands::tts_set_config,
            commands::tts_test_speak,
            commands::tts_list_voices,
            commands::tts_refresh_voices,
            commands::get_recent_rooms,
            commands::my_room_info,
            commands::list_following_live,
            commands::audience_get,
            commands::overlay_set_audience,
            commands::overlay_get_size,
            commands::overlay_set_size,
            update::check_update,
            update::open_url,
            commands::open_log_dir,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // 退出前落盘 Overlay 窗口位置
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            flush_overlay_bounds(handle);
        }
    });
}

/// 在线人数 / 实时在线榜的拉取节奏：2 秒 tick × 15 = 30 秒
const AUDIENCE_EVERY_TICKS: u64 = 15;
/// 失败后最短重试间隔（秒）：接口偶发风控时不要把 2 秒 tick 变成每 2 秒一次的请求
const AUDIENCE_RETRY_MIN_SECS: u64 = 5;

/// 在线人数 / 实时在线榜的拉取周期（秒）
const AUDIENCE_POLL_SECS: u64 = 30;

/// 是否需要立刻拉一次（连接上/换房后不等 30 秒）
///
/// 判据是「当前连接房间」与「上次尝试的房间」不同；失败时限速重试，
/// 否则接口一直失败就会变成每 2 秒打一次。
fn audience_room_changed(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    let room = {
        let st = app.state::<AppState>();
        // 先把状态值拷出来（RoomStatus 是 Copy），否则 match 会借到已释放的 MutexGuard
        let status = *st.status.lock().unwrap();
        match status {
            state::RoomStatus::Connected { room_id } => room_id,
            state::RoomStatus::Connecting { .. } | state::RoomStatus::Disconnected => return false,
        }
    };
    let aud = app.state::<state::AudienceState>();
    let now = std::time::Instant::now();
    // 距上次成功不足一个周期就不拉（正常节奏由 ticks 计数负责）
    let last_ok = *aud.last_ok.lock().unwrap();
    if let Some(ok) = last_ok {
        if now.duration_since(ok).as_secs() < AUDIENCE_POLL_SECS {
            return false;
        }
    }
    // 同样是先把值拷出来：match 的临时 MutexGuard 会活到函数末尾，届时 aud 已释放
    let last_try = *aud.last_try.lock().unwrap();
    match last_try {
        Some((r, t)) if r == room => now.duration_since(t).as_secs() >= AUDIENCE_RETRY_MIN_SECS,
        _ => true,
    }
}

/// 拉一次「在线人数 + 实时在线榜」：成功后写内存态并广播 `audience-update`
///
/// 失败只记日志（下轮重试）：这是弹幕窗角落的一个数字，不值得弹提示、更不能刷屏。
async fn poll_audience(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let room_id = {
        let st = app.state::<AppState>();
        let status = *st.status.lock().unwrap();
        match status {
            state::RoomStatus::Connected { room_id } => room_id,
            _ => return,
        }
    };
    {
        let aud = app.state::<state::AudienceState>();
        *aud.last_try.lock().unwrap() = Some((room_id, std::time::Instant::now()));
    }

    let client = bilibili::api::http_client();
    // 主播 uid：按房间缓存（榜单接口要 ruid，room_init 只在换房时调一次）
    let cached_uid = {
        let aud = app.state::<state::AudienceState>();
        let anchor = *aud.anchor.lock().unwrap();
        match anchor {
            Some((r, u)) if r == room_id => Some(u),
            _ => None,
        }
    };
    let anchor_uid = match cached_uid {
        Some(u) => u,
        None => match bilibili::audience::fetch_anchor_uid(&client, room_id).await {
            Ok(u) => {
                let aud = app.state::<state::AudienceState>();
                *aud.anchor.lock().unwrap() = Some((room_id, u));
                u
            }
            Err(e) => {
                log::warn!("[audience] 解析主播 uid 失败（下轮重试）: {e}");
                return;
            }
        },
    };

    match bilibili::audience::fetch_audience(&client, room_id, anchor_uid).await {
        Ok(snapshot) => {
            {
                let aud = app.state::<state::AudienceState>();
                *aud.snapshot.lock().unwrap() = Some(snapshot.clone());
                *aud.last_ok.lock().unwrap() = Some(std::time::Instant::now());
            }
            let _ = app.emit("audience-update", &snapshot);
        }
        Err(e) => log::warn!("[audience] 拉取在线人数/在线榜失败（下轮重试）: {e}"),
    }
}
