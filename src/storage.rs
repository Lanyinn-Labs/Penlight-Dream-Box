//! MongoDB persistence: monthly/event top snapshots, border cutoffs, player
//! profiles, and master info, mirroring GarupaSpeedTracker's storage semantics
//! with per-day buckets, per-server+uid player upserts, per-tier cutoff series.

use std::collections::{BTreeMap, HashSet};

use bson::doc;
use futures::TryStreamExt;
use mongodb::options::IndexOptions;
use mongodb::{Client, Collection, Database, IndexModel};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::debug;

use crate::error::{AppError, AppResult};
use crate::upstream::{EventInfo, MonthlyRankingInfo, RankingUser};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Valid monthly-ranking border tiers per GarupaSpeedTracker contract.
pub const MONTHLY_BORDER_TIERS: &[i64] = &[
    20, 30, 40, 50, 100, 200, 300, 500, 1000, 2000, 3000, 4000, 5000,
];

/// Valid event-wide border tiers per GarupaSpeedTracker contract.
pub const EVENT_BORDER_TIERS: &[i64] = &[
    20, 30, 40, 50, 100, 200, 300, 500, 1000, 1500, 2000, 3000, 4000, 5000, 10000, 20000, 30000,
    40000, 50000, 100000,
];

/// Valid per-song border tiers per GarupaSpeedTracker contract.
pub const MUSIC_BORDER_TIERS: &[i64] = &[
    20, 30, 40, 50, 100, 200, 300, 500, 1000, 2000, 5000, 10000, 20000, 50000, 100000,
];

/// Number of servers exposed by the API contract with 0 meaning JP. Only JP is served.
pub const SERVER_COUNT: i64 = 1;

pub fn is_monthly_border_tier(tier: i64) -> bool {
    MONTHLY_BORDER_TIERS.contains(&tier)
}

pub fn is_event_border_tier(tier: i64) -> bool {
    EVENT_BORDER_TIERS.contains(&tier)
}

pub fn is_music_border_tier(tier: i64) -> bool {
    MUSIC_BORDER_TIERS.contains(&tier)
}

// ---------------------------------------------------------------------------
// Collection names
// ---------------------------------------------------------------------------

const C_MONTHLY_INFO: &str = "monthly_ranking_info";
const C_MONTHLY_TOP: &str = "monthly_ranking_top_points";
const C_MONTHLY_BORDER: &str = "monthly_ranking_border_points";
const C_PLAYERS: &str = "monthly_ranking_players";
const C_EVENT_TOP: &str = "event_top_points";
const C_EVENT_BORDER: &str = "event_border_points";
const C_EVENTS: &str = "events";

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

/// A single timestamped point in a top snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopPoint {
    pub time: i64,
    pub uid: i64,
    pub value: i64,
}

/// A single timestamped border cutoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CutoffPoint {
    pub time: i64,
    pub ep: i64,
}

/// Player profile stored per server and uid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerDoc {
    pub server: i64,
    pub uid: i64,
    pub name: String,
    pub introduction: String,
    pub rank: i64,
    pub sid: i64,
    pub strained: i64,
    pub degrees: Vec<i64>,
    pub updated_at: i64,
}

impl PlayerDoc {
    fn from_user(server: i64, u: &RankingUser, ts: i64) -> Self {
        Self {
            server,
            uid: u.uid,
            name: u.name.clone(),
            introduction: u.introduction.clone(),
            rank: u.rank,
            sid: u.sid,
            strained: u.strained,
            degrees: u.degrees.clone(),
            updated_at: ts,
        }
    }
}

/// Monthly ranking info document. Per-server fields are stored as 5-element
/// arrays with index 0 for JP and null elsewhere to match GarupaSpeedTracker's shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyInfoDoc {
    pub monthly_ranking_id: i64,
    pub monthly_ranking_name: Vec<Option<String>>,
    pub asset_bundle_name: String,
    pub bgm_file_name: String,
    pub start_at: Vec<Option<i64>>,
    pub end_at: Vec<Option<i64>>,
    pub enable_flag: Vec<Option<bool>>,
    pub public_start_at: Vec<Option<i64>>,
    pub public_end_at: Vec<Option<i64>>,
    pub distribution_start_at: Vec<Option<i64>>,
    pub distribution_end_at: Vec<Option<i64>>,
    pub aggregate_end_at: Vec<Option<i64>>,
    pub reception_end_at: Vec<Option<i64>>,
    #[serde(default)]
    pub rewards: Vec<Value>,
    #[serde(default)]
    pub grades: Vec<Value>,
}

/// Event document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventDoc {
    pub event_id: i64,
    pub event_type: String,
    pub event_name: String,
    pub asset_bundle_name: String,
    pub start_at: i64,
    pub end_at: i64,
    pub enable_flg: bool,
    pub public_start_at: i64,
    pub public_end_at: i64,
    pub distribution_start_at: i64,
    pub distribution_end_at: i64,
    pub bgm_asset_bundle_name: String,
    pub bgm_file_name: String,
    pub aggregate_end_at: i64,
    pub event_exchanges_end_at: i64,
    pub reception_end_at: i64,
    pub previous_event_id: i64,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// MongoDB-backed storage. Cheap to clone.
#[derive(Clone)]
pub struct Storage {
    monthly_info: Collection<MonthlyInfoDoc>,
    monthly_top: Collection<mongodb::bson::Document>,
    monthly_border: Collection<mongodb::bson::Document>,
    players: Collection<PlayerDoc>,
    event_top: Collection<mongodb::bson::Document>,
    event_border: Collection<mongodb::bson::Document>,
    events: Collection<EventDoc>,
}

/// Wraps a single element of a server array into the 5-slot shape.
fn server_array<T: Clone>(value: T) -> Vec<Option<T>> {
    let mut arr: Vec<Option<T>> = vec![None; 5];
    arr[0] = Some(value);
    arr
}

impl Storage {
    pub async fn connect(uri: &str, db_name: &str) -> AppResult<Self> {
        let client = Client::with_uri_str(uri)
            .await
            .map_err(|e| AppError::internal(format!("MongoDB connect failed: {e}")))?;
        let db: Database = client.database(db_name);
        let storage = Self {
            monthly_info: db.collection(C_MONTHLY_INFO),
            monthly_top: db.collection(C_MONTHLY_TOP),
            monthly_border: db.collection(C_MONTHLY_BORDER),
            players: db.collection(C_PLAYERS),
            event_top: db.collection(C_EVENT_TOP),
            event_border: db.collection(C_EVENT_BORDER),
            events: db.collection(C_EVENTS),
        };
        storage.init_indexes().await?;
        Ok(storage)
    }

    async fn init_indexes(&self) -> AppResult<()> {
        macro_rules! create_unique_index {
            ($coll:expr, $keys:expr) => {
                $coll
                    .create_index(
                        IndexModel::builder()
                            .keys($keys)
                            .options(IndexOptions::builder().unique(true).build())
                            .build(),
                    )
                    .await
                    .map_err(|e| AppError::internal(format!("index creation failed: {e}")))?;
            };
        }

        create_unique_index!(self.monthly_info, doc! { "monthlyRankingId": 1 });
        create_unique_index!(
            self.monthly_top,
            doc! { "server": 1, "monthlyId": 1, "bucket": 1 }
        );
        create_unique_index!(
            self.monthly_border,
            doc! { "server": 1, "monthlyId": 1, "tier": 1 }
        );
        create_unique_index!(self.players, doc! { "server": 1, "uid": 1 });
        create_unique_index!(
            self.event_top,
            doc! { "server": 1, "eventId": 1, "mid": 1, "bucket": 1 }
        );
        create_unique_index!(
            self.event_border,
            doc! { "server": 1, "eventId": 1, "mid": 1, "tier": 1 }
        );
        create_unique_index!(self.events, doc! { "eventId": 1 });
        debug!("storage indexes ready");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Master lists
    // -----------------------------------------------------------------------

    /// Upserts monthly ranking info from the Dream-API master list.
    pub async fn upsert_monthly_infos(&self, infos: &[MonthlyRankingInfo]) -> AppResult<usize> {
        for info in infos {
            let doc = MonthlyInfoDoc {
                monthly_ranking_id: info.monthly_ranking_id,
                monthly_ranking_name: server_array(info.monthly_ranking_name.clone()),
                asset_bundle_name: info.asset_bundle_name.clone(),
                bgm_file_name: info.bgm_file_name.clone(),
                start_at: server_array(info.start_at),
                end_at: server_array(info.end_at),
                enable_flag: server_array(info.enable_flg),
                public_start_at: server_array(info.public_start_at),
                public_end_at: server_array(info.public_end_at),
                distribution_start_at: server_array(info.distribution_start_at),
                distribution_end_at: server_array(info.distribution_end_at),
                aggregate_end_at: server_array(info.aggregate_end_at),
                reception_end_at: server_array(info.reception_end_at),
                rewards: info.rewards.clone(),
                grades: info.grades.clone(),
            };
            self.monthly_info
                .replace_one(doc! { "monthlyRankingId": info.monthly_ranking_id }, doc)
                .upsert(true)
                .await
                .map_err(|e| AppError::internal(format!("monthly info upsert failed: {e}")))?;
        }
        Ok(infos.len())
    }

    /// All monthly infos as raw documents, for the API.
    pub async fn all_monthly_infos(&self) -> AppResult<BTreeMap<String, Value>> {
        let mut out = BTreeMap::new();
        let cursor = self
            .monthly_info
            .find(doc! {})
            .await
            .map_err(|e| AppError::internal(format!("monthly info query failed: {e}")))?;
        let docs = cursor
            .try_collect::<Vec<MonthlyInfoDoc>>()
            .await
            .map_err(|e| AppError::internal(format!("monthly info read failed: {e}")))?;
        for d in docs {
            let v = serde_json::to_value(&d)
                .map_err(|e| AppError::internal(format!("monthly info serialize failed: {e}")))?;
            out.insert(d.monthly_ranking_id.to_string(), v);
        }
        Ok(out)
    }

    /// Id of the currently active monthly ranking, if any.
    pub async fn active_monthly_id(&self, now: i64) -> AppResult<Option<i64>> {
        Ok(self
            .active_monthlies(now, 0)
            .await?
            .into_iter()
            .map(|(id, _)| id)
            .max())
    }

    /// Active monthly rankings: started, and not ended or within
    /// `post_end_window_secs` after end. Returns monthly_id and end_at pairs.
    pub async fn active_monthlies(
        &self,
        now: i64,
        post_end_window_secs: i64,
    ) -> AppResult<Vec<(i64, i64)>> {
        let docs = self
            .monthly_info
            .find(doc! {})
            .await
            .map_err(|e| AppError::internal(format!("monthly info query failed: {e}")))?
            .try_collect::<Vec<MonthlyInfoDoc>>()
            .await
            .map_err(|e| AppError::internal(format!("monthly info read failed: {e}")))?;

        Ok(docs
            .iter()
            .filter(|d| {
                let start = d.start_at.first().and_then(|v| *v).unwrap_or(0);
                let end = d.end_at.first().and_then(|v| *v).unwrap_or(0);
                start > 0 && start <= now && (end == 0 || end + post_end_window_secs * 1000 >= now)
            })
            .map(|d| {
                (
                    d.monthly_ranking_id,
                    d.end_at.first().and_then(|v| *v).unwrap_or(0),
                )
            })
            .collect())
    }

    /// Upserts events from the Dream-API event master list.
    pub async fn upsert_events(&self, events: &[EventInfo]) -> AppResult<usize> {
        for e in events {
            let doc = EventDoc {
                event_id: e.event_id,
                event_type: e.event_type.clone(),
                event_name: e.event_name.clone(),
                asset_bundle_name: e.asset_bundle_name.clone(),
                start_at: e.start_at,
                end_at: e.end_at,
                enable_flg: e.enable_flg,
                public_start_at: e.public_start_at,
                public_end_at: e.public_end_at,
                distribution_start_at: e.distribution_start_at,
                distribution_end_at: e.distribution_end_at,
                bgm_asset_bundle_name: e.bgm_asset_bundle_name.clone(),
                bgm_file_name: e.bgm_file_name.clone(),
                aggregate_end_at: e.aggregate_end_at,
                event_exchanges_end_at: e.event_exchanges_end_at,
                reception_end_at: e.reception_end_at,
                previous_event_id: e.previous_event_id,
            };
            self.events
                .replace_one(doc! { "eventId": e.event_id }, doc)
                .upsert(true)
                .await
                .map_err(|e| AppError::internal(format!("event upsert failed: {e}")))?;
        }
        Ok(events.len())
    }

    /// All events keyed by string id.
    pub async fn all_events(&self) -> AppResult<BTreeMap<String, Value>> {
        let mut out = BTreeMap::new();
        let cursor = self
            .events
            .find(doc! {})
            .await
            .map_err(|e| AppError::internal(format!("events query failed: {e}")))?;
        let docs = cursor
            .try_collect::<Vec<EventDoc>>()
            .await
            .map_err(|e| AppError::internal(format!("events read failed: {e}")))?;
        for d in docs {
            let v = serde_json::to_value(&d)
                .map_err(|e| AppError::internal(format!("event serialize failed: {e}")))?;
            out.insert(d.event_id.to_string(), v);
        }
        Ok(out)
    }

    /// Active events: started and not ended or within `post_end_window_secs`
    /// after end. Returns event_id and end_at pairs.
    pub async fn active_events(
        &self,
        now: i64,
        post_end_window_secs: i64,
    ) -> AppResult<Vec<(i64, i64)>> {
        let docs = self
            .events
            .find(doc! {})
            .await
            .map_err(|e| AppError::internal(format!("events query failed: {e}")))?
            .try_collect::<Vec<EventDoc>>()
            .await
            .map_err(|e| AppError::internal(format!("events read failed: {e}")))?;

        Ok(docs
            .iter()
            .filter(|d| {
                d.start_at > 0
                    && d.start_at <= now
                    && (d.end_at == 0 || d.end_at + post_end_window_secs * 1000 >= now)
            })
            .map(|d| (d.event_id, d.end_at))
            .collect())
    }

    // -----------------------------------------------------------------------
    // Monthly ranking snapshots
    // -----------------------------------------------------------------------

    /// Appends a top snapshot for `server` and `monthly_id`, and upserts
    /// player profiles.
    pub async fn append_monthly_top(
        &self,
        server: i64,
        monthly_id: i64,
        ts: i64,
        users: &[RankingUser],
    ) -> AppResult<usize> {
        let bucket = utc_bucket(ts);
        let filter = doc! { "server": server, "monthlyId": monthly_id, "bucket": bucket };
        let points: Vec<TopPoint> = users
            .iter()
            .map(|u| TopPoint {
                time: ts,
                uid: u.uid,
                value: u.point,
            })
            .collect();
        let points_bson = serde_to_bson(&points)?;
        let n = points.len();

        let wrote = self
            .upsert_time_series(
                &self.monthly_top,
                filter,
                "points",
                ts,
                points_bson,
                "monthly top",
            )
            .await?;
        if !wrote {
            return Ok(0);
        }

        self.upsert_players(server, users, ts).await?;
        Ok(n)
    }

    /// Full monthly top history for `server` and `monthly_id`: points sorted by
    /// time plus the latest player profiles of the involved users.
    pub async fn monthly_top(
        &self,
        server: i64,
        monthly_id: i64,
        since: i64,
    ) -> AppResult<(Vec<TopPoint>, Vec<PlayerDoc>)> {
        let filter = if since > 0 {
            doc! { "server": server, "monthlyId": monthly_id, "bucket": { "$gte": utc_bucket(since) } }
        } else {
            doc! { "server": server, "monthlyId": monthly_id }
        };
        self.query_top_points(&self.monthly_top, filter, server, since, "monthly top")
            .await
    }

    /// Appends border cutoffs for `server` and `monthly_id` from border users.
    pub async fn append_monthly_borders(
        &self,
        server: i64,
        monthly_id: i64,
        ts: i64,
        border_users: &[RankingUser],
    ) -> AppResult<usize> {
        let mut by_tier: BTreeMap<i64, Vec<CutoffPoint>> = BTreeMap::new();
        for u in border_users {
            if is_monthly_border_tier(u.tier) {
                by_tier.entry(u.tier).or_default().push(CutoffPoint {
                    time: ts,
                    ep: u.point,
                });
            }
        }
        let mut written = 0;
        for (tier, cutoffs) in by_tier {
            if self
                .upsert_cutoffs(
                    &self.monthly_border,
                    doc! { "server": server, "monthlyId": monthly_id, "tier": tier },
                    ts,
                    &cutoffs,
                )
                .await?
            {
                written += cutoffs.len();
            }
        }
        Ok(written)
    }

    /// Border cutoff history for `server`, `monthly_id`, and `tier`.
    pub async fn monthly_border(
        &self,
        server: i64,
        monthly_id: i64,
        tier: i64,
    ) -> AppResult<Vec<CutoffPoint>> {
        self.query_cutoffs(
            &self.monthly_border,
            doc! { "server": server, "monthlyId": monthly_id, "tier": tier },
        )
        .await
    }

    // -----------------------------------------------------------------------
    // Event snapshots
    // -----------------------------------------------------------------------

    /// Appends a top snapshot for `server`, `event_id` and `mid` where `mid = 0`
    /// means the event-wide ranking, and upserts player profiles.
    pub async fn append_event_top(
        &self,
        server: i64,
        event_id: i64,
        mid: i64,
        ts: i64,
        users: &[RankingUser],
    ) -> AppResult<usize> {
        let bucket = utc_bucket(ts);
        let filter = doc! { "server": server, "eventId": event_id, "mid": mid, "bucket": bucket };
        let points: Vec<TopPoint> = users
            .iter()
            .map(|u| TopPoint {
                time: ts,
                uid: u.uid,
                value: u.point,
            })
            .collect();
        let points_bson = serde_to_bson(&points)?;
        let n = points.len();

        let wrote = self
            .upsert_time_series(
                &self.event_top,
                filter,
                "points",
                ts,
                points_bson,
                "event top",
            )
            .await?;
        if !wrote {
            return Ok(0);
        }

        self.upsert_players(server, users, ts).await?;
        Ok(n)
    }

    /// Full event top history for `server`, `event_id`, and `mid`.
    pub async fn event_top(
        &self,
        server: i64,
        event_id: i64,
        mid: i64,
        since: i64,
    ) -> AppResult<(Vec<TopPoint>, Vec<PlayerDoc>)> {
        let filter = if since > 0 {
            doc! { "server": server, "eventId": event_id, "mid": mid, "bucket": { "$gte": utc_bucket(since) } }
        } else {
            doc! { "server": server, "eventId": event_id, "mid": mid }
        };
        self.query_top_points(&self.event_top, filter, server, since, "event top")
            .await
    }

    /// Returns whether any top-point snapshot exists for `(server, event_id, mid)`.
    pub async fn has_event_top(&self, server: i64, event_id: i64, mid: i64) -> AppResult<bool> {
        let doc = self
            .event_top
            .find_one(doc! {
                "server": server,
                "eventId": event_id,
                "mid": mid,
                "points.0": { "$exists": true },
            })
            .await
            .map_err(|e| AppError::internal(format!("event top existence check failed: {e}")))?;
        Ok(doc.is_some())
    }

    /// Appends border cutoffs for `server`, `event_id`, and `mid`.
    pub async fn append_event_borders(
        &self,
        server: i64,
        event_id: i64,
        mid: i64,
        ts: i64,
        border_users: &[RankingUser],
        music: bool,
    ) -> AppResult<usize> {
        let mut by_tier: BTreeMap<i64, Vec<CutoffPoint>> = BTreeMap::new();
        for u in border_users {
            let valid = if music {
                is_music_border_tier(u.tier)
            } else {
                is_event_border_tier(u.tier)
            };
            if valid {
                by_tier.entry(u.tier).or_default().push(CutoffPoint {
                    time: ts,
                    ep: u.point,
                });
            }
        }
        let mut written = 0;
        for (tier, cutoffs) in by_tier {
            if self
                .upsert_cutoffs(
                    &self.event_border,
                    doc! { "server": server, "eventId": event_id, "mid": mid, "tier": tier },
                    ts,
                    &cutoffs,
                )
                .await?
            {
                written += cutoffs.len();
            }
        }
        Ok(written)
    }

    /// Border cutoff history for `server`, `event_id`, `mid`, and `tier`.
    pub async fn event_border(
        &self,
        server: i64,
        event_id: i64,
        mid: i64,
        tier: i64,
    ) -> AppResult<Vec<CutoffPoint>> {
        self.query_cutoffs(
            &self.event_border,
            doc! { "server": server, "eventId": event_id, "mid": mid, "tier": tier },
        )
        .await
    }

    // -----------------------------------------------------------------------
    // Shared time-series and player helpers
    // -----------------------------------------------------------------------

    async fn upsert_players(&self, server: i64, users: &[RankingUser], ts: i64) -> AppResult<()> {
        for u in users {
            let p = PlayerDoc::from_user(server, u, ts);
            self.players
                .replace_one(doc! { "server": server, "uid": u.uid }, p)
                .upsert(true)
                .await
                .map_err(|e| AppError::internal(format!("player upsert failed: {e}")))?;
        }
        Ok(())
    }

    async fn query_top_points(
        &self,
        coll: &Collection<mongodb::bson::Document>,
        filter: mongodb::bson::Document,
        server: i64,
        since: i64,
        label: &str,
    ) -> AppResult<(Vec<TopPoint>, Vec<PlayerDoc>)> {
        let cursor = coll
            .find(filter)
            .await
            .map_err(|e| AppError::internal(format!("{label} query failed: {e}")))?;
        let docs = cursor
            .try_collect::<Vec<mongodb::bson::Document>>()
            .await
            .map_err(|e| AppError::internal(format!("{label} read failed: {e}")))?;

        let (points, uids) = extract_top_points(&docs, since);
        let players = if uids.is_empty() {
            Vec::new()
        } else {
            let cursor = self
                .players
                .find(doc! { "server": server, "uid": { "$in": &uids } })
                .await
                .map_err(|e| AppError::internal(format!("players query failed: {e}")))?;
            cursor
                .try_collect::<Vec<PlayerDoc>>()
                .await
                .map_err(|e| AppError::internal(format!("players read failed: {e}")))?
        };

        Ok((points, players))
    }

    /// Idempotent time-series upsert: inserts when missing, appends when `ts`
    /// is newer than `updatedAt`, replaces only entries with `time == ts` when
    /// `ts == updatedAt` (preserving earlier snapshots), and skips when stale.
    async fn upsert_time_series(
        &self,
        coll: &Collection<mongodb::bson::Document>,
        filter: mongodb::bson::Document,
        array_field: &str,
        ts: i64,
        items_bson: mongodb::bson::Bson,
        label: &str,
    ) -> AppResult<bool> {
        let existing = coll
            .find_one(filter.clone())
            .await
            .map_err(|e| AppError::internal(format!("{label} query failed: {e}")))?;
        let existing_updated_at = existing
            .as_ref()
            .map(|doc| doc.get_i64("updatedAt").unwrap_or(0));

        match decide_snapshot_action(existing_updated_at, ts) {
            SnapshotAction::Insert => {
                let doc = build_insert_doc(&filter, array_field, ts, items_bson);
                coll.insert_one(doc)
                    .await
                    .map_err(|e| AppError::internal(format!("{label} insert failed: {e}")))?;
                Ok(true)
            }
            SnapshotAction::Append => {
                let update = build_append_update(array_field, ts, items_bson);
                coll.update_one(filter, update)
                    .await
                    .map_err(|e| AppError::internal(format!("{label} update failed: {e}")))?;
                Ok(true)
            }
            SnapshotAction::ReplaceAtTs => {
                // MongoDB rejects $pull and $push on the same array field in a
                // single update document, so pull `time == ts` before pushing.
                let pull = build_pull_at_ts_update(array_field, ts);
                coll.update_one(filter.clone(), pull)
                    .await
                    .map_err(|e| AppError::internal(format!("{label} pull failed: {e}")))?;
                let push = build_append_update(array_field, ts, items_bson);
                coll.update_one(filter, push)
                    .await
                    .map_err(|e| AppError::internal(format!("{label} update failed: {e}")))?;
                Ok(true)
            }
            SnapshotAction::Skip => Ok(false),
        }
    }

    /// Idempotent cutoff series upsert: appends when `ts` is newer, replaces
    /// only entries at `ts` when `ts` equals `updatedAt`, skips when older.
    async fn upsert_cutoffs(
        &self,
        coll: &Collection<mongodb::bson::Document>,
        filter: mongodb::bson::Document,
        ts: i64,
        cutoffs: &[CutoffPoint],
    ) -> AppResult<bool> {
        let cutoffs_bson = serde_to_bson(cutoffs)?;
        self.upsert_time_series(coll, filter, "cutoffs", ts, cutoffs_bson, "cutoff")
            .await
    }

    async fn query_cutoffs(
        &self,
        coll: &Collection<mongodb::bson::Document>,
        filter: mongodb::bson::Document,
    ) -> AppResult<Vec<CutoffPoint>> {
        let doc = coll
            .find_one(filter)
            .await
            .map_err(|e| AppError::internal(format!("cutoff query failed: {e}")))?;
        let mut cutoffs: Vec<CutoffPoint> = Vec::new();
        if let Some(doc) = doc {
            if let Ok(arr) = doc.get_array("cutoffs") {
                for item in arr {
                    if let Ok(c) = bson::from_bson::<CutoffPoint>(item.clone()) {
                        cutoffs.push(c);
                    }
                }
            }
        }
        cutoffs.sort_by_key(|c| c.time);
        Ok(cutoffs)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotAction {
    Insert,
    Append,
    ReplaceAtTs,
    Skip,
}

fn decide_snapshot_action(existing_updated_at: Option<i64>, ts: i64) -> SnapshotAction {
    match existing_updated_at {
        None => SnapshotAction::Insert,
        Some(last) if last < ts => SnapshotAction::Append,
        Some(last) if last == ts => SnapshotAction::ReplaceAtTs,
        Some(_) => SnapshotAction::Skip,
    }
}

fn build_insert_doc(
    filter: &mongodb::bson::Document,
    array_field: &str,
    ts: i64,
    items_bson: mongodb::bson::Bson,
) -> mongodb::bson::Document {
    let mut doc = doc! { array_field: items_bson, "updatedAt": ts };
    for (k, v) in filter.iter() {
        doc.insert(k.clone(), v.clone());
    }
    doc
}

fn build_pull_at_ts_update(array_field: &str, ts: i64) -> mongodb::bson::Document {
    doc! { "$pull": { array_field: { "time": ts } } }
}

fn build_append_update(
    array_field: &str,
    ts: i64,
    items_bson: mongodb::bson::Bson,
) -> mongodb::bson::Document {
    doc! {
        "$set": { "updatedAt": ts },
        "$push": { array_field: { "$each": items_bson } },
    }
}

fn extract_top_points(docs: &[mongodb::bson::Document], since: i64) -> (Vec<TopPoint>, Vec<i64>) {
    let mut points: Vec<TopPoint> = Vec::new();
    let mut uids: Vec<i64> = Vec::new();
    let mut seen_uids: HashSet<i64> = HashSet::new();
    for doc in docs {
        if let Ok(arr) = doc.get_array("points") {
            for item in arr {
                if let Ok(p) = bson::from_bson::<TopPoint>(item.clone()) {
                    if since > 0 && p.time < since {
                        continue;
                    }
                    if seen_uids.insert(p.uid) {
                        uids.push(p.uid);
                    }
                    points.push(p);
                }
            }
        }
    }
    points.sort_by_key(|p| p.time);
    (points, uids)
}

/// Per-UTC-day bucket index: `ts / 86_400_000`.
///
/// GarupaSpeedTracker buckets by 8 days, which is sized for a 5-minute poll
/// interval. At the default 1-minute interval a top-100 snapshot set grows to
/// ~52 MB per 8-day bucket — beyond MongoDB's 16 MB document limit — so we
/// bucket per day instead.
/// Buckets are internal sharding keys; API output is unaffected.
fn utc_bucket(ts: i64) -> i64 {
    ts.div_euclid(86_400_000)
}

fn serde_to_bson<T: serde::Serialize + ?Sized>(value: &T) -> AppResult<mongodb::bson::Bson> {
    bson::to_bson(value).map_err(|e| AppError::internal(format!("bson serialize failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decides_snapshot_action_for_missing_newer_equal_and_stale_timestamps() {
        assert_eq!(decide_snapshot_action(None, 1_000), SnapshotAction::Insert);
        assert_eq!(
            decide_snapshot_action(Some(900), 1_000),
            SnapshotAction::Append
        );
        assert_eq!(
            decide_snapshot_action(Some(1_000), 1_000),
            SnapshotAction::ReplaceAtTs
        );
        assert_eq!(
            decide_snapshot_action(Some(1_100), 1_000),
            SnapshotAction::Skip
        );
    }

    #[test]
    fn pull_and_append_updates_preserve_earlier_entries_and_avoid_operator_conflict() {
        let pull = build_pull_at_ts_update("cutoffs", 2_000);
        assert_eq!(pull, doc! { "$pull": { "cutoffs": { "time": 2_000_i64 } } });
        assert!(!pull.contains_key("$push"));
        assert!(!pull.contains_key("$set"));

        let items = serde_to_bson(&[CutoffPoint {
            time: 2_000,
            ep: 123_456,
        }])
        .unwrap();
        let append = build_append_update("cutoffs", 2_000, items.clone());
        assert_eq!(
            append,
            doc! {
                "$set": { "updatedAt": 2_000_i64 },
                "$push": { "cutoffs": { "$each": items } },
            }
        );
        assert!(!append.contains_key("$pull"));
    }

    #[test]
    fn insert_doc_copies_filter_keys_and_sets_initial_series() {
        let filter = doc! { "server": 0_i64, "eventId": 42_i64, "mid": 0_i64, "tier": 1000_i64 };
        let items = serde_to_bson(&[CutoffPoint {
            time: 5_000,
            ep: 9_999,
        }])
        .unwrap();
        let inserted = build_insert_doc(&filter, "cutoffs", 5_000, items.clone());
        assert_eq!(inserted.get_i64("server").unwrap(), 0);
        assert_eq!(inserted.get_i64("eventId").unwrap(), 42);
        assert_eq!(inserted.get_i64("mid").unwrap(), 0);
        assert_eq!(inserted.get_i64("tier").unwrap(), 1000);
        assert_eq!(inserted.get_i64("updatedAt").unwrap(), 5_000);
        assert_eq!(inserted.get("cutoffs"), Some(&items));
    }

    #[test]
    fn extract_top_points_filters_since_sorts_and_deduplicates_uids() {
        let docs = vec![
            doc! {
                "points": [
                    { "time": 200_i64, "uid": 2_i64, "value": 20_i64 },
                    { "time": 100_i64, "uid": 1_i64, "value": 10_i64 },
                ]
            },
            doc! {
                "points": [
                    { "time": 150_i64, "uid": 1_i64, "value": 15_i64 },
                    { "time": 150_i64, "uid": 3_i64, "value": 30_i64 },
                ]
            },
        ];

        let (points, uids) = extract_top_points(&docs, 150);
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].time, 150);
        assert_eq!(points[1].time, 150);
        assert_eq!(points[2].time, 200);
        assert_eq!(uids, vec![2, 1, 3]);
    }
}
