//! 弹幕朗读：Edge TTS 引擎 + 流水线播放队列
//!
//! 与弹幕显示完全解耦（prd.md §25）：显示过滤在前端 Overlay 做，朗读过滤在 `text.rs` 做，
//! 二者用各自独立的 `DanmakuFilter` 实例，互不影响。
//!
//! 数据流：弹幕事件 → 开关/筛选判定 → 文案清洗 → 有界队列 → 合成 →（通道）→ 串行播放。
//! 合成与播放两个任务见 `worker.rs`；音色与协议见 `edge/`；文案清洗见 `text.rs`。

pub mod edge;
mod clean;
mod gift;
mod player;
mod text;
mod worker;

pub use worker::spawn_worker;

use crate::bilibili::event::{Backing, Danmaku, Welcome, WelcomeKind};
use crate::config::{GiftTtsConfig, TtsConfig};
use crate::state::OverlayState;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;
use clean::clean_for_speech;
use text::{build_backing_text, build_text, build_welcome_text, is_emote, matches_filter};

/// 打断冷却：距上次打断不足这么久就不再作废在途音频
/// （否则高频房间合成刚起步就被反复作废，白忙一场）
const INTERRUPT_COOLDOWN: Duration = Duration::from_millis(1800);

/// 队列项
///
/// 两个布尔位各管一件事，别再混用：
/// - `force`：**压力下别丢**——不被新弹幕挤掉（见 `push` 的淘汰策略）、不被「积压打断」作废、
///   熔断清积压时保留。礼物朗读 / 欢迎朗读 / 试听都带它。
/// - `manual`：**用户当场点的试听**，是唯一能绕过朗读总开关的项；其它朗读在总开关关掉时
///   一律不出声（见 `set_config` 与 `worker.rs` 的三处判定）。
struct QueueItem {
    text: String,
    force: bool,
    manual: bool,
}

/// 朗读队列与配置（Tauri 管理状态）
pub struct TtsState {
    config: Mutex<TtsConfig>,
    /// 礼物朗读配置（独立于弹幕朗读开关）
    gift_config: Mutex<GiftTtsConfig>,
    /// 待合成队列（满时丢最旧）
    queue: Mutex<VecDeque<QueueItem>>,
    /// 礼物连击聚合（静默满窗口才朗读，见 `gift.rs`）
    gift: Mutex<gift::GiftAggregator>,
    /// 唤醒合成端
    notify: Notify,
    /// 打断代次：递增即作废所有在途音频（含已合成未播与正在合成的那条）
    epoch: AtomicU64,
    /// 是否正在出声（决定打断是否有意义）
    playing: AtomicBool,
    /// 上次打断时刻（冷却用）
    last_interrupt: Mutex<Option<Instant>>,
    /// 可选音色 (ID, 显示名)：初值为内置中文音色，拉取成功后换成微软完整列表
    voices: Mutex<Vec<(String, String)>>,
}

impl TtsState {
    pub fn new(config: TtsConfig) -> Self {
        Self {
            config: Mutex::new(config),
            gift_config: Mutex::new(GiftTtsConfig::default()),
            queue: Mutex::new(VecDeque::new()),
            gift: Mutex::new(gift::GiftAggregator::default()),
            notify: Notify::new(),
            epoch: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            last_interrupt: Mutex::new(None),
            voices: Mutex::new(edge::builtin_voices()),
        }
    }

    pub fn config(&self) -> TtsConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn gift_config(&self) -> GiftTtsConfig {
        self.gift_config.lock().unwrap().clone()
    }

    /// 更新礼物朗读配置：关掉时丢弃还没念出去的连击分组
    pub fn set_gift_config(&self, config: GiftTtsConfig) {
        if !config.enabled {
            self.gift.lock().unwrap().clear();
        }
        *self.gift_config.lock().unwrap() = config;
    }

    /// 全部朗读总开关（弹幕 / 礼物 / 欢迎一起管；试听除外）
    pub fn master_enabled(&self) -> bool {
        self.config.lock().unwrap().master_enabled
    }

    /// 这一项现在该不该出声：入队 / 合成后 / 播放中三处共用同一个判据
    ///
    /// - 试听（`manual`）永远读；
    /// - 礼物朗读 / 欢迎朗读（`force`）只看**总开关**；
    /// - 普通弹幕还要看「弹幕朗读」开关（`enabled`）——关掉它只停弹幕，礼物照念。
    pub fn allowed(&self, force: bool, manual: bool) -> bool {
        if manual {
            return true;
        }
        let cfg = self.config.lock().unwrap();
        cfg.master_enabled && (force || cfg.enabled)
    }

    /// 当前音色列表（内置兜底 + 已拉取的完整列表）
    pub fn voices(&self) -> Vec<(String, String)> {
        self.voices.lock().unwrap().clone()
    }

    pub fn set_voices(&self, voices: Vec<(String, String)>) {
        *self.voices.lock().unwrap() = voices;
    }

    /// 更新配置：按关掉的开关清队列（在播的那条由播放端的 `stop_now` 验闸门后自行停下）
    ///
    /// 两档策略别合并：
    /// - **总开关**关掉 → 弹幕 / 礼物 / 欢迎一起清，只留用户当场点的试听（全部静音）；
    /// - 只关**弹幕朗读** → 只清待朗读弹幕，礼物 / 欢迎的排队项保留（上游语义：关弹幕朗读不影响打赏朗读）。
    pub fn set_config(&self, config: TtsConfig) {
        let master_off = !config.master_enabled;
        let danmaku_off = !config.enabled;
        *self.config.lock().unwrap() = config;
        if master_off {
            self.queue.lock().unwrap().retain(|item| item.manual);
            self.epoch.fetch_add(1, Ordering::Relaxed);
        } else if danmaku_off {
            // `force` 覆盖礼物 / 欢迎 / 试听（试听也带 force），它们不受弹幕朗读开关影响
            self.queue.lock().unwrap().retain(|item| item.force);
            // 作废在途的弹幕音频；force 项在 worker 里不参与 epoch 作废，照常念完
            self.epoch.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 入队；队列满时丢弃最旧的待朗读项（高速直播间不堆积）
    fn push(&self, text: String, max_queue: u32, force: bool, manual: bool) {
        let mut queue = self.queue.lock().unwrap();
        while queue.len() >= max_queue.max(1) as usize {
            // 丢最旧的**弹幕**：队首可能就是插队的礼物，直接 pop_front 会把它顶掉
            // （高频直播间每秒几十条弹幕，上舰/醒目留言这种只来一次的大额插队后往往活不到开播）
            match queue.iter().position(|item| !item.force) {
                Some(i) => {
                    queue.remove(i);
                }
                // 队列里全是插队项：只能丢最旧的那个
                None => {
                    queue.pop_front();
                }
            }
        }
        queue.push_back(QueueItem { text, force, manual });
        drop(queue);
        self.notify.notify_one();
    }

    /// 插队入队（礼物朗读）：放到队首，排在所有待朗读弹幕之前。
    /// 队列满时丢最旧的：待朗读的弹幕比打赏更可等，打赏不该被挤出队列。
    fn push_front(&self, text: String, max_queue: u32) {
        let mut queue = self.queue.lock().unwrap();
        while queue.len() >= max_queue.max(1) as usize {
            queue.pop_front();
        }
        queue.push_front(QueueItem { text, force: true, manual: false });
        drop(queue);
        self.notify.notify_one();
    }

    /// 取一条待朗读项，队列空则挂起等待
    async fn pop(&self) -> QueueItem {
        loop {
            if let Some(item) = self.queue.lock().unwrap().pop_front() {
                return item;
            }
            // Notify 会保留一次许可，push 与 pop 竞态下不会丢唤醒
            self.notify.notified().await;
        }
    }

    /// 积压时打断：已在出声且队列里还有更新的弹幕，就作废在途音频
    /// （正在合成的 + 已合成未播的），让更新的弹幕尽快接上。
    /// **不**掐正在念的那条：音频整条合成，掐断只会听到半句（开了念用户名时连正文都没开始），
    /// 故延迟上限是一条朗读时长。
    fn interrupt_if_backlogged(&self, enabled: bool) {
        if !enabled || !self.playing.load(Ordering::Relaxed) {
            return;
        }
        if self.queue.lock().unwrap().is_empty() {
            return;
        }
        {
            let mut last = self.last_interrupt.lock().unwrap();
            if last.is_some_and(|t| t.elapsed() < INTERRUPT_COOLDOWN) {
                return;
            }
            *last = Some(Instant::now());
        }
        // 递增代次：正在合成、已合成未播的那条会因代次不符被丢弃（在播的那条念完）
        self.epoch.fetch_add(1, Ordering::Relaxed);
    }
}

/// 弹幕到达钩子：判定通过则清洗入队（非阻塞，在弹幕事件回调里调用）
pub fn on_danmaku(app: &AppHandle, d: &Danmaku) {
    let state = app.state::<TtsState>();
    let config = state.config.lock().unwrap();
    let master = config.master_enabled;
    let enabled = config.enabled;
    let pass = matches_filter(d, &config.filter);
    // 弹幕要过两道闸：总开关 + 弹幕朗读开关（后者是「关弹幕、留礼物朗读」的那个）
    if !master || !enabled || !pass {
        return;
    }
    let text = build_text(d, &config);
    let max_queue = config.max_queue;
    let interrupt = config.interrupt_on_backlog;
    // 逐条弹幕日志走 debug：日志级别是 Info，平时不落盘；排查「表情弹幕还在被念」时
    // 把 lib.rs 的 level 临时改成 Debug 即可看到识别与朗读判定
    if is_emote(d) {
        log::debug!(
            "[emote] 含表情弹幕「{}」read_emote={} → {}朗读",
            d.content,
            config.read_emote,
            if text.is_empty() { "不" } else { "" }
        );
    }
    drop(config);
    if text.is_empty() {
        return;
    }
    state.push(text, max_queue, false, false);
    state.interrupt_if_backlogged(interrupt);
}

/// 欢迎事件到达钩子：只念舰长进场
///
/// 进房 / 关注 / 点赞的量级是弹幕的 1.5~10 倍，念出来会把队列一直占着、把弹幕挤没，
/// 所以只念舰长以上（`GuardEnter` 就是舰长 / 提督 / 总督进场）。
/// 排到**队尾**（不插队）但 `force = true`（压力下别丢）；欢迎信息自己的开关是第二道闸，
/// 第一道是**朗读总开关**（关掉后欢迎朗读也不出声）。
pub fn on_welcome(app: &AppHandle, w: &Welcome) {
    if w.kind != WelcomeKind::GuardEnter {
        return;
    }
    // 总开关优先：关掉后欢迎朗读也不出声（与弹幕朗读开关互不影响，见 `allowed`）
    let state = app.state::<TtsState>();
    if !state.master_enabled() {
        return;
    }
    if !app.state::<OverlayState>().welcome.lock().unwrap().tts_guard {
        return;
    }
    let text = build_welcome_text(w);
    if text.is_empty() {
        return;
    }
    state.push(text, state.config().max_queue, true, false);
}

/// 试听：直接把一段文本送进朗读队列（设置页调音色/语速用）
///
/// 带 `manual = true`：它是**唯一**不受朗读总开关限制的朗读。总开关关掉时用户仍可能在
/// 设置页点「试听」调音色，这时要能听到声音；其它自动朗读一律由总开关管住。
pub fn speak_test(app: &AppHandle, text: &str) {
    let text = clean_for_speech(text);
    if text.is_empty() {
        return;
    }
    app.state::<TtsState>().push(text, 1, true, true);
}

/// 打赏到达钩子（非阻塞，在弹幕事件回调里调用）
///
/// 连击类先攒着（静默满一个连击窗口才念，否则连送 20 个就念 20 遍），
/// 醒目留言与上舰不连击，立刻插队朗读。
pub fn on_backing(app: &AppHandle, b: &Backing) {
    let state = app.state::<TtsState>();
    // 关闭礼物朗读时也照常聚合：开关与门槛在 `speak_backing` 里判，
    // 到期没念出去的分组会自行丢弃（不会积压）
    let immediate = state
        .gift
        .lock()
        .unwrap()
        .on_backing(b.clone(), Instant::now());
    if let Some(row) = immediate {
        speak_backing(app, &row);
    }
}

/// 把一条打赏插队送进朗读队列（开关开启且过了朗读门槛才念）
fn speak_backing(app: &AppHandle, b: &Backing) {
    let state = app.state::<TtsState>();
    // 总开关优先：关掉后礼物朗读也不出声（与弹幕朗读开关互不影响，见 `allowed`）
    if !state.master_enabled() {
        return;
    }
    let cfg = state.gift_config();
    if !cfg.enabled || b.amount_fen < gift::threshold_fen(&cfg) {
        return;
    }
    let text = build_backing_text(b);
    if text.is_empty() {
        return;
    }
    state.push_front(text, state.config().max_queue);
}

/// 礼物朗读定时器：常驻任务，每「连击窗口」醒一次，把静默满窗口的分组念出去
///
/// 不给每个分组各起一个延时任务：连击 100 次会留下 99 个空转任务，还有
/// 「定时器退出」与「事件到达」的竞态；常驻轮询每轮只加一次锁，代价可忽略。
pub(super) fn spawn_gift_timer(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            // 1 秒一跳（不是按窗口整跳）：否则汇总最多晚一个窗口才能念——
            // 礼物在 t=1 到达、t=5 醒来时静默才 4 秒，得再等一整轮
            tokio::time::sleep(Duration::from_secs(1)).await;
            let window = combo_window(&app);
            let due = {
                let state = app.state::<TtsState>();
                let cfg = state.gift_config();
                let mut agg = state.gift.lock().unwrap();
                agg.take_due(Instant::now(), window, &cfg)
            };
            for row in due {
                speak_backing(&app, &row);
            }
        }
    });
}

/// 连击合并窗口：沿用礼物渲染侧的配置（同一个直播间里「连击」应是同一口径）
fn combo_window(app: &AppHandle) -> Duration {
    let secs = app
        .state::<OverlayState>()
        .gift
        .lock()
        .unwrap()
        .combo_window_secs;
    Duration::from_secs(secs.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_drops_oldest_when_full() {
        let state = TtsState::new(TtsConfig::default());
        for i in 0..5 {
            state.push(format!("第{i}条"), 3, false, false);
        }
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.len(), 3);
        assert_eq!(queue.front().unwrap().text, "第2条");
    }

    #[test]
    fn interrupt_only_when_playing_with_backlog_and_outside_cooldown() {
        let state = TtsState::new(TtsConfig::default());
        state.push("新弹幕".into(), 5, false, false);

        // 没在出声 → 不打断
        state.interrupt_if_backlogged(true);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 0);
        // 开关关闭 → 不打断
        state.playing.store(true, Ordering::Relaxed);
        state.interrupt_if_backlogged(false);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 0);

        // 在出声 + 有积压 + 开关开启 → 打断一次
        state.interrupt_if_backlogged(true);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 1);
        // 冷却期内不重复打断（否则会一直被打断，听不到完整句子）
        state.interrupt_if_backlogged(true);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn no_interrupt_when_queue_empty() {
        let state = TtsState::new(TtsConfig::default());
        state.playing.store(true, Ordering::Relaxed);
        state.interrupt_if_backlogged(true);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn 插队的礼物排在待朗读弹幕之前() {
        let state = TtsState::new(TtsConfig::default());
        state.push("弹幕一".into(), 5, false, false);
        state.push("弹幕二".into(), 5, false, false);
        state.push_front("感谢老板A送的5个辣条".into(), 5);
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.front().unwrap().text, "感谢老板A送的5个辣条");
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn 插队的礼物不会被队列上限挤掉() {
        let state = TtsState::new(TtsConfig::default());
        for i in 0..5 {
            state.push(format!("弹幕{i}"), 2, false, false);
        }
        // 队列已满（上限 2）：插队时丢的是最旧的弹幕，礼物自己一定在队首
        state.push_front("感谢老板A送的5个辣条".into(), 2);
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.front().unwrap().text, "感谢老板A送的5个辣条");
    }

    #[test]
    fn 高频弹幕不会把插队的礼物顶掉() {
        // 队列上限 2：插队的是上舰（只来一次），后面弹幕连续涌入
        let state = TtsState::new(TtsConfig::default());
        state.push_front("感谢木茂上舰舰长".into(), 2);
        for i in 0..5 {
            state.push(format!("弹幕{i}"), 2, false, false);
        }
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.front().unwrap().text, "感谢木茂上舰舰长");
        assert_eq!(queue.back().unwrap().text, "弹幕4", "丢的应是最旧的弹幕");
    }

    #[test]
    fn 关闭总开关时只保留手动试听() {
        let state = TtsState::new(TtsConfig::default());
        state.push("弹幕一".into(), 5, false, false);
        state.push_front("感谢老板A送的5个辣条".into(), 5);
        state.push("试听文本".into(), 1, true, true);
        state.set_config(TtsConfig {
            master_enabled: false,
            ..TtsConfig::default()
        });
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.len(), 1, "总开关关掉 = 全部静音，只该留下手动试听");
        assert_eq!(queue.front().unwrap().text, "试听文本");
        assert!(queue.front().unwrap().manual);
    }

    #[test]
    fn 关闭弹幕朗读时保留礼物与试听() {
        let state = TtsState::new(TtsConfig::default());
        state.push("弹幕一".into(), 5, false, false);
        state.push_front("感谢老板A送的5个辣条".into(), 5);
        // 队列上限给足：试听真实的入队上限是 1，那会先把礼物挤掉，测不出「保留礼物」这件事
        state.push("试听文本".into(), 5, true, true);
        state.set_config(TtsConfig {
            enabled: false, // 总开关仍开，只关弹幕朗读
            ..TtsConfig::default()
        });
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.len(), 2, "只该丢掉待朗读的弹幕");
        assert_eq!(queue[0].text, "感谢老板A送的5个辣条");
        assert_eq!(queue[1].text, "试听文本");
    }

    #[test]
    fn 朗读闸门按总开关与弹幕开关判定() {
        let state = TtsState::new(TtsConfig {
            enabled: false,
            ..TtsConfig::default()
        });
        assert!(!state.allowed(false, false), "弹幕朗读关 → 弹幕不读");
        assert!(state.allowed(true, false), "弹幕朗读关不影响礼物 / 欢迎");
        assert!(state.allowed(true, true), "试听永远读");

        state.set_config(TtsConfig {
            master_enabled: false,
            enabled: true,
            ..TtsConfig::default()
        });
        assert!(!state.allowed(false, false), "总开关关 → 弹幕不读");
        assert!(!state.allowed(true, false), "总开关关 → 礼物 / 欢迎也不读");
        assert!(state.allowed(true, true), "总开关关也不影响试听");
    }

    #[test]
    fn 关闭礼物朗读时丢弃未念的分组() {
        let state = TtsState::new(TtsConfig::default());
        let b = crate::bilibili::event::Backing {
            id: "1".into(),
            kind: crate::bilibili::event::BackingKind::Gift,
            uid: 1,
            username: "老板A".into(),
            gift_name: "辣条".into(),
            gift_id: 100,
            num: 1,
            amount_fen: 100,
            timestamp: 0,
            message: None,
            guard_level: None,
            wealth_level: None,
        };
        state
            .gift
            .lock()
            .unwrap()
            .on_backing(b, Instant::now());
        state.set_gift_config(GiftTtsConfig {
            enabled: false,
            min_amount_yuan: 0.0,
        });
        let due = state.gift.lock().unwrap().take_due(
            Instant::now() + Duration::from_secs(10),
            Duration::from_secs(1),
            &GiftTtsConfig::default(),
        );
        assert!(due.is_empty());
    }
}
