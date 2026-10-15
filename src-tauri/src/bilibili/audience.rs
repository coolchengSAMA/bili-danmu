//! 「当前在线人数 + 实时在线榜」的数据源（弹幕窗右侧那一列）
//!
//! 接口 `xlive/general-interface/v1/rank/getOnlineGoldRank`：**匿名可调**（不需要登录态、
//! 不需要 WBI 签名），一次请求同时给出人数与榜单前 N 名，所以右侧列只需这一个请求。
//!
//! 语义实测（2026-10，探测脚本在 `Task/`）：
//! - `onlineNum` = 官方口径的「在线观众数量」；热度房会饱和成 `9999+`（`onlineNumText`）
//! - `OnlineRankItem` = **此刻在房间里 ∩ 今天有贡献值**的人，按当日贡献值排序；
//!   人一离开直播间 **≤20 秒**就掉榜（受控实验：小号进房上榜 → 关掉页面 → 20 秒内消失），
//!   所以这是「实时在线榜」，不是当日累计榜
//! - 榜单条数 = 房间里符合条件的人数（实测出现过 5 / 8 / 10 条），不是固定条数；
//!   潜水观众永远不在榜上（B 站隐私边界），界面上必须写清这一点
//! - 未开播房间同样有数据，调用方结合 `live_status` 理解
//! - `pageSize` 生效：右侧列只要前 [`RANK_SIZE`] 名，取小一点响应也小

/// 右侧列展示的榜单条数（前 5）
pub const RANK_SIZE: usize = 5;

/// 榜上一位观众
#[derive(Debug, Clone, serde::Serialize)]
pub struct AudienceUser {
    pub uid: i64,
    /// 昵称（B 站可能给空串，前端原样显示）
    pub name: String,
    /// 贡献值（当日累计）
    pub score: i64,
    /// 大航海：0 无 / 1 总督 / 2 提督 / 3 舰长
    pub guard_level: i64,
}

/// 一次拉取的快照
#[derive(Debug, Clone, serde::Serialize)]
pub struct AudienceSnapshot {
    /// 在线人数（数值）
    pub online_num: i64,
    /// 在线人数的展示文本（热度房 B 站直接给 `9999+`）
    pub online_text: String,
    /// 实时在线榜前 [`RANK_SIZE`] 名
    pub users: Vec<AudienceUser>,
}

/// 拉取「在线人数 + 实时在线榜」（`anchor_uid` 由 [`fetch_anchor_uid`] 取，调用方按房间缓存）
pub async fn fetch_audience(
    client: &reqwest::Client,
    room_id: u32,
    anchor_uid: i64,
) -> Result<AudienceSnapshot, String> {
    let resp = client
        .get("https://api.live.bilibili.com/xlive/general-interface/v1/rank/getOnlineGoldRank")
        .query(&[
            ("ruid", anchor_uid.to_string()),
            ("roomId", room_id.to_string()),
            ("page", "1".to_string()),
            ("pageSize", RANK_SIZE.to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("请求在线榜失败: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("在线榜响应解析失败: {e}"))?;
    parse_audience(&resp)
}

/// 按房间号取主播 uid：榜单接口要 `ruid`，`room_init` 匿名可调
///
/// 换房才需要调一次（调用方缓存），所以这次额外请求的代价可以忽略。
pub async fn fetch_anchor_uid(client: &reqwest::Client, room_id: u32) -> Result<i64, String> {
    let resp = client
        .get("https://api.live.bilibili.com/room/v1/Room/room_init")
        .query(&[("id", room_id.to_string())])
        .send()
        .await
        .map_err(|e| format!("请求房间信息失败: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("房间信息响应解析失败: {e}"))?;

    let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "获取房间信息失败(code={code}): {}",
            resp.get("message").and_then(|m| m.as_str()).unwrap_or("")
        ));
    }
    resp.pointer("/data/uid")
        .and_then(|v| v.as_i64())
        .filter(|uid| *uid > 0)
        .ok_or_else(|| "房间信息缺少 uid".to_string())
}

/// 解析榜单响应（抽出来便于单测，不依赖网络）
fn parse_audience(resp: &serde_json::Value) -> Result<AudienceSnapshot, String> {
    let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "获取在线榜失败(code={code}): {}",
            resp.get("message").and_then(|m| m.as_str()).unwrap_or("")
        ));
    }

    let data = resp.get("data").filter(|d| !d.is_null());
    let online_num = data
        .and_then(|d| d.get("onlineNum"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    // 热度房的 onlineNumText 就是 "9999+"，直接拿来展示；缺失时退回数值
    let online_text = data
        .and_then(|d| d.get("onlineNumText"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| online_num.to_string());

    let users = data
        .and_then(|d| d.get("OnlineRankItem"))
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .take(RANK_SIZE)
                .filter_map(|it| {
                    // uid 无效的条目直接跳过：name 可能是空的，但 uid 一定得有
                    let uid = it.get("uid").and_then(|v| v.as_i64())?;
                    if uid <= 0 {
                        return None;
                    }
                    Some(AudienceUser {
                        uid,
                        name: it
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                        score: it.get("score").and_then(|v| v.as_i64()).unwrap_or(0),
                        guard_level: it.get("guard_level").and_then(|v| v.as_i64()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(AudienceSnapshot {
        online_num,
        online_text,
        users,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 在线榜解析人数与榜单() {
        let v = json!({
            "code": 0,
            "data": {
                "onlineNum": 1234,
                "onlineNumText": "1234",
                "OnlineRankItem": [
                    { "uid": 1001, "name": " 舰长A ", "score": 1380, "guard_level": 3 },
                    { "uid": 1002, "name": "观众B", "score": 20, "guard_level": 0 }
                ]
            }
        });
        let snap = parse_audience(&v).unwrap();
        assert_eq!(snap.online_num, 1234);
        assert_eq!(snap.online_text, "1234");
        assert_eq!(snap.users.len(), 2);
        assert_eq!(snap.users[0].name, "舰长A"); // 前后空白裁掉
        assert_eq!(snap.users[0].guard_level, 3);
        assert_eq!(snap.users[1].score, 20);
    }

    #[test]
    fn 在线榜热度房直接用官方文本() {
        let v = json!({
            "code": 0,
            "data": { "onlineNum": 9999, "onlineNumText": "9999+", "OnlineRankItem": [] }
        });
        let snap = parse_audience(&v).unwrap();
        assert_eq!(snap.online_text, "9999+");
        assert!(snap.users.is_empty());
    }

    #[test]
    fn 在线榜缺字段时退回安全默认值() {
        // 缺 onlineNumText 用数值；缺 OnlineRankItem 当空榜；缺 score/guard_level 记 0
        let v = json!({
            "code": 0,
            "data": { "onlineNum": 7, "OnlineRankItem": [{ "uid": 9, "name": "甲" }] }
        });
        let snap = parse_audience(&v).unwrap();
        assert_eq!(snap.online_text, "7");
        assert_eq!(snap.users[0].score, 0);
        assert_eq!(snap.users[0].guard_level, 0);
    }

    #[test]
    fn 在线榜最多取前五名() {
        let items: Vec<serde_json::Value> = (1..=8)
            .map(|i| json!({ "uid": i, "name": format!("观众{i}"), "score": 100 - i }))
            .collect();
        let v = json!({ "code": 0, "data": { "onlineNum": 10, "OnlineRankItem": items } });
        let snap = parse_audience(&v).unwrap();
        assert_eq!(snap.users.len(), RANK_SIZE);
        assert_eq!(snap.users[0].name, "观众1");
    }

    #[test]
    fn 在线榜跳过uid无效的条目() {
        let v = json!({
            "code": 0,
            "data": {
                "onlineNum": 1,
                "OnlineRankItem": [
                    { "uid": 0, "name": "坏数据" },
                    { "name": "没有 uid" },
                    { "uid": 5, "name": "好数据" }
                ]
            }
        });
        let snap = parse_audience(&v).unwrap();
        assert_eq!(snap.users.len(), 1);
        assert_eq!(snap.users[0].name, "好数据");
    }

    #[test]
    fn 在线榜非零返回码报错() {
        let e = parse_audience(&json!({ "code": -352, "message": "风控" })).unwrap_err();
        assert!(e.contains("-352") && e.contains("风控"));
    }
}
