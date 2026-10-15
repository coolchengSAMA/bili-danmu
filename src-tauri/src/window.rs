//! Overlay / Sender 窗口辅助：取窗口句柄、吸附布局、位置落盘
//!
//! 窗口的创建与事件绑定在 `lib.rs` 的 setup 里，这里只放被多处复用的操作。

use crate::config;
use crate::state::{OverlayState, SenderState};
use tauri::{AppHandle, Emitter, Manager};

/// 获取 Overlay 窗口实例（不存在返回 None）
pub(crate) fn overlay_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("overlay")
}

/// 获取 Sender 窗口实例（不存在返回 None）
pub(crate) fn sender_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("sender")
}

/// 读取窗口当前矩形 → 更新内存态；尺寸变化时广播给主界面「弹幕窗」标签的宽高滑块
///
/// 统一换算成**逻辑像素**再存：窗口 API（outer_position / inner_size）返回物理像素，
/// 高 DPI 下差一个 scale_factor 的倍数；而恢复窗口用的 position/inner_size(构建器)
/// 与设置页的 px 都是逻辑像素，不换算就会出现「存的比恢复的大」。
pub(crate) fn capture_overlay_bounds(app: &AppHandle) {
    let Some(win) = overlay_window(app) else { return };
    let Ok(pos) = win.outer_position() else { return };
    let Ok(size) = win.inner_size() else { return };
    let scale = win.scale_factor().unwrap_or(1.0);
    let pos = pos.to_logical::<f64>(scale);
    let size = size.to_logical::<f64>(scale);
    let b = config::WindowBounds {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    };
    let st = app.state::<OverlayState>();
    let size_changed = {
        let mut bounds = st.bounds.lock().unwrap();
        let changed = bounds.map(|p| (p.w, p.h) != (b.w, b.h)).unwrap_or(true);
        *bounds = Some(b);
        changed
    };
    // 只在尺寸变化时广播：拖动窗口（只改位置）会每帧触发窗口事件，全发会把 IPC 刷满。
    // 取整后发出，与 overlay_get_size 同一口径（滑块的取值是整数）。
    if size_changed {
        let _ = app.emit(
            "overlay-size",
            serde_json::json!({ "width": size.width.round(), "height": size.height.round() }),
        );
    }
}

/// 内存态位置与已保存不同则写盘（去重）
pub(crate) fn flush_overlay_bounds(app: &AppHandle) {
    let st = app.state::<OverlayState>();
    let current = *st.bounds.lock().unwrap();
    if current.is_none() {
        return;
    }
    let saved = *st.saved_bounds.lock().unwrap();
    if current != saved {
        let _ = config::save_overlay_bounds(app, &current.unwrap());
        *st.saved_bounds.lock().unwrap() = current;
    }
}

/// 发送框始终吸附：左端与弹幕区面板背景左边缘齐平，宽度取面板宽度的 50%（上限 260px），高度随字号
///
/// 吸附点算完还要**夹进弹幕窗所在显示器的工作区**（见 `fit_docked_position`）：
/// 弹幕窗被拖到屏幕底部时正下方就是屏幕外，而发送框不可拖动、也不在任务栏显示，
/// 用户既看不到它、也不知道要先把弹幕窗挪上来。
pub(crate) fn sync_sender_docked(app: &AppHandle) {
    let (Some(ov), Some(sender)) = (overlay_window(app), sender_window(app)) else {
        return;
    };
    let Ok(pos) = ov.outer_position() else { return };
    let Ok(size) = ov.outer_size() else { return };
    let scale = ov.scale_factor().unwrap_or(1.0);
    let (indent, w, h) = sender_layout_metrics(app, scale, size.width);
    let (x, y) = fit_docked_position(
        // 用弹幕窗所在显示器的工作区：多屏时发送框跟着弹幕窗走
        ov.current_monitor().ok().flatten().map(|m| *m.work_area()),
        pos.x,
        pos.y,
        size.height,
        indent as i32,
        w,
        h,
    );
    let _ = sender.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(x, y)));
    let _ = sender.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(w, h)));
}

/// 算出吸附后的发送框位置（物理像素），并把结果夹进工作区
///
/// 落点优先级：弹幕窗正下方 → 正上方（下方放不下）→ 贴工作区下沿
/// （上下都放不下，即弹幕窗本身比工作区还高时：宁可压住弹幕窗也要能用）。
/// 左右同理，但**越界时可见优先于对齐**：正常情况发送框左端仍与面板底色左边缘同一条线，
/// 只有弹幕窗自己都跑到屏幕外时才会偏离。取不到显示器信息（`None`）就按原样吸附，
/// 与改动前行为一致。
fn fit_docked_position(
    work: Option<tauri::PhysicalRect<i32, u32>>,
    overlay_x: i32,
    overlay_y: i32,
    overlay_h: u32,
    indent: i32,
    w: u32,
    h: u32,
) -> (i32, i32) {
    let x = overlay_x + indent;
    let y = overlay_y + overlay_h as i32;
    let Some(work) = work else { return (x, y) };
    let (iw, ih) = (w as i32, h as i32);
    let left = work.position.x;
    let top = work.position.y;
    let right = work.position.x + work.size.width as i32;
    let bottom = work.position.y + work.size.height as i32;
    let want_y = if y + ih <= bottom {
        y
    } else if overlay_y - ih >= top {
        overlay_y - ih
    } else {
        bottom - ih
    };
    (
        x.clamp(left, (right - iw).max(left)),
        want_y.clamp(top, (bottom - ih).max(top)),
    )
}

/// 显示 / 隐藏发送框
///
/// 显示时先同步吸附位置再 show：隐藏期间弹幕窗可能被拖动或缩放过，直接 show 会出现在旧位置。
/// 显示后**尽力**聚焦窗口并广播 `sender-shown`，发送框前端收到后把光标放进输入框。
/// 注意这是尽力而为：Windows 的前台窗口切换限制（游戏占前台时）会让 `set_focus` 只闪任务栏，
/// 此时输入框虽已聚焦，用户仍可能需要点一下窗口才能打字，故失败只记日志、不当作错误返回。
pub(crate) fn set_sender_visible(app: &AppHandle, visible: bool) -> Result<bool, String> {
    let win = sender_window(app).ok_or("发送框窗口未创建")?;
    if visible {
        sync_sender_docked(app);
        win.show().map_err(|e| format!("显示发送框失败: {e}"))?;
        if let Err(e) = win.set_focus() {
            log::warn!("[sender] 唤出后置焦失败（可能被前台锁挡住，用户需手点一下）: {e}");
        }
        let _ = app.emit("sender-shown", ());
    } else {
        win.hide().map_err(|e| format!("隐藏发送框失败: {e}"))?;
    }
    Ok(visible)
}

/// 发送框可见性：内存态与已落盘值不同则写盘（去重）
///
/// 快捷键 / 托盘切换只改内存态（见 `commands::toggle_sender`），落盘交给这个函数，
/// 由 lib.rs 里与 `flush_overlay_bounds` 同一个 2s 周期任务调用：
///  - 连续按十下：中途写盘与否都不重要，停手后最多 2s 落一次，且来回切回原值就完全不写；
///  - 代价是「切完立刻退出应用」时最后一次切换可能没落盘（与窗口位置同款取舍，
///    这两个状态都不是关键数据）；
///  - 保存失败保持「未落盘」，下个 tick 自动重试。
pub(crate) fn flush_sender_visible(app: &AppHandle) {
    let st = app.state::<SenderState>();
    let visible = *st.visible.lock().unwrap();
    if visible == *st.saved_visible.lock().unwrap() {
        return;
    }
    let hotkey = st.hotkey.lock().unwrap().clone();
    match config::save_sender_config(app, &config::SenderConfig { visible, hotkey }) {
        Ok(()) => *st.saved_visible.lock().unwrap() = visible,
        Err(e) => log::warn!("[sender] 保存可见性失败（下个周期重试）: {e}"),
    }
}

/// 弹幕窗容器 padding（px）：对应 OverlayApp.vue 的 .overlay-root padding
const OVERLAY_PADDING: f64 = 6.0;
/// 正文列起点（em）：role-slot 4.8em + 0.7em 间距，对应 OverlayApp.vue 的 --role-indent
const ROLE_INDENT_EM: f64 = 5.5;
/// 面板背景相对正文列再左让的宽度（em）：对应 OverlayApp.vue 的 --panel-inset
const PANEL_INSET_EM: f64 = 0.35;
/// 发送框宽度占面板宽度的比例
const SENDER_WIDTH_RATIO: f64 = 0.5;
/// 发送框宽度上限（逻辑 px）：面板很宽时不再跟着变宽，避免过长挡住游戏操作区。
/// 按逻辑 px 定义、使用时乘 scale 换算成物理 px，高 DPI 下显示宽度与 100% 缩放一致
const SENDER_MAX_WIDTH: f64 = 260.0;

/// 弹幕窗右侧在线列的宽度（em）：与 OverlayApp.vue 的 `--audience-indent` 必须一致
const AUDIENCE_INDENT_EM: f64 = 6.5;

/// 发送框相对弹幕窗的左缩进 / 宽度 / 高度（物理 px），随弹幕字号缩放。
/// 缩进 = 容器 padding + 面板左边缘（--panel-inset = --role-indent − 0.35em），
/// 发送框相对弹幕窗的左缩进 / 宽度 / 高度（物理 px），随弹幕字号缩放。
/// 缩进 = 容器 padding + 面板左边缘（--panel-inset = --role-indent − 0.35em），
/// 落在面板背景左边缘那条线上；宽度 = 面板宽度（弹幕窗宽 − 左缩进 − 右侧 padding
/// − 右侧在线列留白）的 50%，且不超过 SENDER_MAX_WIDTH（逻辑 px）。
/// 改 OverlayApp.vue 的 --role-indent / --panel-inset / --audience-indent / 容器 padding
/// 时要同步改这里。
///
/// 高度要同时容下输入行与出错时才出现的提示行（提示行是 v-if，不能按它不显示时的高度算）。
/// 宽度是硬约束（不能被太多游戏画面），输入区靠 SenderApp.vue 里的字号 0.9em + 紧内边距
/// 才放得下约 8 个中文字；想再宽只能调 SENDER_WIDTH_RATIO / SENDER_MAX_WIDTH。
fn sender_layout_metrics(app: &AppHandle, scale: f64, overlay_width: u32) -> (u32, u32, u32) {
    // 先把 State 绑到变量上：直接 `app.state::<..>().style.lock()` 会让临时 State 提前释放
    let overlay = app.state::<OverlayState>();
    let style = overlay.style.lock().unwrap();
    let font_size = style.font_size;
    // 右侧在线列（在看人数 / 实时在线榜）显示时，面板右边缘还要再让出一条留白
    let audience_inset = if style.show_online || style.show_rank {
        (AUDIENCE_INDENT_EM - PANEL_INSET_EM) * font_size * scale
    } else {
        0.0
    };
    // 面板宽度用未取整的浮点算：缩进先取整会把那半个像素的误差带进宽度
    let left = (OVERLAY_PADDING + (ROLE_INDENT_EM - PANEL_INSET_EM) * font_size) * scale;
    let panel_width = overlay_width as f64 - left - audience_inset - OVERLAY_PADDING * scale;
    let indent = left.round() as u32;
    let w = ((panel_width * SENDER_WIDTH_RATIO)
        .min(SENDER_MAX_WIDTH * scale)
        .round() as u32)
        .max(1);
    let h = ((font_size * 3.1 + 6.0) * scale).round() as u32;
    (indent, w, h)
}

// ── 显示器工作区与「窗口跑到屏幕外」的兜底 ──────────────────────────────
//
// 记忆下来的窗口位置可能是已经不存在的显示器（拔掉副屏、改分辨率、笔记本外接转内置），
// 弹幕窗会出现在屏幕外：看不见、拖不回来，设置页只能改尺寸。所以两处要处理：
//   ① 启动恢复位置前校验一次中心点（`clamp_overlay_bounds`）；
//   ② 界面上给一个显式入口（`reset_overlay_position` → 「移回主屏中央」按钮）。
// 判定与夹取都写成纯函数，方便单测（带 AppHandle 的部分只负责取显示器信息）。

/// 逻辑像素矩形（窗口位置 / 尺寸；配置里存的也是逻辑像素）
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// 显示器工作区 → 逻辑像素（按该显示器自己的缩放比换算）
///
/// 多屏且缩放不同时，这个换算只对「窗口真正所在的那块屏」精确；其它屏算不准也只是
/// 让判据偏保守（误判成不可见 → 搬回主屏中央），不会把看得见的窗口搬到看不见的地方。
fn logical_work_area(mon: &tauri::Monitor) -> Rect {
    let scale = mon.scale_factor();
    let wa = mon.work_area();
    Rect {
        x: wa.position.x as f64 / scale,
        y: wa.position.y as f64 / scale,
        w: wa.size.width as f64 / scale,
        h: wa.size.height as f64 / scale,
    }
}

/// 所有可见显示器的工作区（逻辑像素）
fn logical_work_areas(app: &AppHandle) -> Vec<Rect> {
    app.available_monitors()
        .map(|mons| mons.iter().map(logical_work_area).collect())
        .unwrap_or_default()
}

/// 主显示器工作区（逻辑像素）；取不到主显示器就退回第一块屏
fn primary_logical_work_area(app: &AppHandle) -> Option<Rect> {
    if let Ok(Some(mon)) = app.primary_monitor() {
        return Some(logical_work_area(&mon));
    }
    app.available_monitors().ok()?.first().map(logical_work_area)
}

/// 主显示器工作区（物理像素）
fn primary_work_area(app: &AppHandle) -> Option<tauri::PhysicalRect<i32, u32>> {
    let mon = match app.primary_monitor() {
        Ok(Some(mon)) => mon,
        _ => app.available_monitors().ok()?.into_iter().next()?,
    };
    Some(*mon.work_area())
}

/// 窗口中心是否落在某块屏的工作区里
///
/// 判据用中心点而不是「有没有交集」：只要中心还在屏幕上，用户就看得见、也能拖得动它，
/// 这时不做任何改动（半截露在屏幕外属于用户自己摆的位置，替他改反而更烦）。
fn center_visible(bounds: Rect, work_areas: &[Rect]) -> bool {
    let (cx, cy) = bounds.center();
    work_areas.iter().any(|wa| wa.contains(cx, cy))
}

/// 把窗口放进工作区中央；窗口比工作区还大时按工作区收窄
/// （收窄只发生在「已经跑到屏幕外」的兜底路径上，正常位置不会被改尺寸）
fn centered_in(work: Rect, w: f64, h: f64) -> Rect {
    let w = w.min(work.w);
    let h = h.min(work.h);
    Rect {
        x: work.x + (work.w - w) / 2.0,
        y: work.y + (work.h - h) / 2.0,
        w,
        h,
    }
}

/// 尺寸夹进工作区（至少 1px，避免出现 0 宽窗口）
fn fit_in_work_area(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    (w.min(max_w).max(1), h.min(max_h).max(1))
}

/// 启动恢复位置前的校验：窗口中心不在任何屏幕里就搬回主显示器中央
///
/// 返回值直接喂给窗口构建器（逻辑像素）。屏幕列表都取不到时原样返回，不冒险改动。
pub(crate) fn clamp_overlay_bounds(
    app: &AppHandle,
    bounds: config::WindowBounds,
) -> config::WindowBounds {
    let work_areas = logical_work_areas(app);
    let rect = Rect {
        x: bounds.x,
        y: bounds.y,
        w: bounds.w,
        h: bounds.h,
    };
    if work_areas.is_empty() || center_visible(rect, &work_areas) {
        return bounds;
    }
    let Some(primary) = primary_logical_work_area(app) else {
        return bounds;
    };
    log::warn!(
        "[overlay] 记忆的位置 ({:.0}, {:.0}) 不在任何屏幕的工作区内，搬回主显示器中央",
        bounds.x,
        bounds.y
    );
    let fixed = centered_in(primary, bounds.w, bounds.h);
    config::WindowBounds {
        x: fixed.x,
        y: fixed.y,
        w: fixed.w,
        h: fixed.h,
    }
}

/// 把弹幕窗搬回主显示器中央（「弹幕窗」标签的「移回主屏中央」按钮）
///
/// 用物理像素一次算好位置与尺寸：尺寸保持当前值，只有「比工作区还大」时才收窄，
/// 保证搬完之后整块都在屏幕内。位置与尺寸的变更会照常触发窗口事件，
/// bounds 捕获 / 落盘 / 发送框重新吸附都走既有那条路径，这里不重复做。
pub(crate) fn reset_overlay_position(app: &AppHandle) -> Result<(), String> {
    let win = overlay_window(app).ok_or("Overlay 窗口未创建")?;
    let work = primary_work_area(app).ok_or("没有识别到可用的显示器")?;
    let size = win
        .outer_size()
        .map_err(|e| format!("读取弹幕窗尺寸失败: {e}"))?;
    let (w, h) = fit_in_work_area(size.width, size.height, work.size.width, work.size.height);
    let x = work.position.x + (work.size.width.saturating_sub(w) / 2) as i32;
    let y = work.position.y + (work.size.height.saturating_sub(h) / 2) as i32;
    win.set_size(tauri::PhysicalSize::new(w, h))
        .map_err(|e| format!("调整弹幕窗尺寸失败: {e}"))?;
    win.set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| format!("移动弹幕窗失败: {e}"))?;
    log::info!("[overlay] 已把弹幕窗移回主屏中央：{w}×{h} @ ({x}, {y})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    fn physical_work(x: i32, y: i32, w: u32, h: u32) -> tauri::PhysicalRect<i32, u32> {
        tauri::PhysicalRect {
            position: tauri::PhysicalPosition::new(x, y),
            size: tauri::PhysicalSize::new(w, h),
        }
    }

    #[test]
    fn 中心在屏幕工作区内才算可见() {
        let screens = [rect(0.0, 0.0, 1920.0, 1040.0)];
        // 整块在正中间
        assert!(center_visible(rect(500.0, 400.0, 700.0, 400.0), &screens));
        // 半截露在外面，但中心还在屏幕里 → 不动它
        assert!(center_visible(rect(-300.0, 800.0, 700.0, 400.0), &screens));
        // 跑到屏幕右边外面
        assert!(!center_visible(rect(1900.0, 100.0, 700.0, 400.0), &screens));
        // 跑到屏幕下方（笔记本外接屏拔掉后常见的坐标）
        assert!(!center_visible(rect(100.0, 1600.0, 700.0, 400.0), &screens));
    }

    #[test]
    fn 副屏也算可见() {
        let screens = [rect(0.0, 0.0, 1920.0, 1040.0), rect(1920.0, 0.0, 2560.0, 1440.0)];
        assert!(center_visible(rect(2200.0, 300.0, 700.0, 400.0), &screens));
        // 放在第二块屏负方向（屏幕在左边的情况）
        let left_screens = [rect(0.0, 0.0, 1920.0, 1040.0), rect(-2560.0, 0.0, 2560.0, 1440.0)];
        assert!(center_visible(rect(-2000.0, 300.0, 700.0, 400.0), &left_screens));
    }

    #[test]
    fn 搬回中央时保留尺寸() {
        let work = rect(0.0, 0.0, 1920.0, 1040.0);
        assert_eq!(
            centered_in(work, 700.0, 400.0),
            rect(610.0, 320.0, 700.0, 400.0)
        );
    }

    #[test]
    fn 窗口比工作区还大时按工作区收窄() {
        let work = rect(0.0, 0.0, 1280.0, 720.0);
        assert_eq!(centered_in(work, 1600.0, 900.0), rect(0.0, 0.0, 1280.0, 720.0));
    }

    #[test]
    fn 工作区有偏移时中央位置跟着偏移() {
        // 右侧副屏：工作区从 x=1920 开始
        let work = rect(1920.0, 0.0, 1600.0, 860.0);
        assert_eq!(centered_in(work, 800.0, 400.0), rect(2320.0, 230.0, 800.0, 400.0));
    }

    #[test]
    fn 尺寸夹取不会算出零宽窗口() {
        assert_eq!(fit_in_work_area(3000, 2000, 1920, 1040), (1920, 1040));
        assert_eq!(fit_in_work_area(700, 400, 1920, 1040), (700, 400));
        assert_eq!(fit_in_work_area(0, 0, 1920, 1040), (1, 1));
    }

    #[test]
    fn 发送框正常情况下就在弹幕窗下方() {
        let work = physical_work(0, 0, 1920, 1040);
        // 弹幕窗 700×400 @ (80, 80)，缩进 88，发送框 260×76
        assert_eq!(
            fit_docked_position(Some(work), 80, 80, 400, 88, 260, 76),
            (168, 480)
        );
    }

    #[test]
    fn 弹幕窗贴屏幕底部时发送框翻到上方() {
        let work = physical_work(0, 0, 1920, 1040);
        // 弹幕窗底边 1040 = 工作区下沿，正下方放不下 → 翻到弹幕窗正上方
        assert_eq!(
            fit_docked_position(Some(work), 80, 900, 140, 88, 260, 76),
            (168, 824)
        );
    }

    #[test]
    fn 上下都放不下时贴工作区下沿() {
        let work = physical_work(0, 0, 1920, 1040);
        // 弹幕窗比工作区还高：上方也放不下 → 贴下沿，宁可压住弹幕窗也要能用
        assert_eq!(
            fit_docked_position(Some(work), 0, 0, 1200, 88, 260, 76),
            (88, 964)
        );
    }

    #[test]
    fn 弹幕窗跑到屏幕外时发送框夹回工作区内() {
        let work = physical_work(0, 0, 1920, 1040);
        // 弹幕窗在屏幕右边外面：发送框的 x 被夹回工作区右侧内部，y 仍按下方算
        assert_eq!(
            fit_docked_position(Some(work), 2000, 80, 400, 88, 260, 76),
            (1660, 480)
        );
        // 弹幕窗在屏幕左边外面（负坐标）
        assert_eq!(
            fit_docked_position(Some(work), -500, 80, 400, 88, 260, 76),
            (0, 480)
        );
    }

    #[test]
    fn 工作区在副屏时按副屏夹取() {
        let work = physical_work(1920, 0, 1600, 860);
        assert_eq!(
            fit_docked_position(Some(work), 2000, 100, 400, 88, 260, 76),
            (2088, 500)
        );
        // 副屏下沿是 860：放不下就翻到弹幕窗上方
        assert_eq!(
            fit_docked_position(Some(work), 2000, 760, 100, 88, 260, 76),
            (2088, 684)
        );
    }

    #[test]
    fn 取不到显示器信息时按原样吸附() {
        assert_eq!(
            fit_docked_position(None, 80, 80, 400, 88, 260, 76),
            (168, 480)
        );
    }
}
