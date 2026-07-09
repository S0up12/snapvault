use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Serialize)]
pub struct BuildProfileSummary {
    pub profile_found: bool,
}

/// Builds a single consolidated profile snapshot (account identity, friend
/// counts, engagement stats, bitmoji usage) from the export's `account.json`,
/// `user_profile.json`, `bitmoji.json`, `friends.json`, and `ranking.json`,
/// and upserts it into the singleton `profile_snapshots` row (id = 1).
///
/// The reference web app's profile view also surfaces location history,
/// Snapchat+/subscriptions, login security, and call/support history - all
/// deferred here since each needs its own JSON file's parsing/normalization
/// logic and none were asked for; this covers identity + the metrics that
/// are meaningfully "profile stats" without that extra surface area.
pub(super) fn build_profile_snapshot_blocking(
    conn: &Connection,
    job_dir: &Path,
    emit: &dyn Fn(usize, usize, String),
) -> Result<BuildProfileSummary, String> {
    emit(0, 1, "Parsing profile metadata".to_string());

    let part_dirs = super::find_part_dirs(job_dir)?;

    let account = read_json(&part_dirs, "account.json")?;
    let user_profile = read_json(&part_dirs, "user_profile.json")?;
    let bitmoji = read_json(&part_dirs, "bitmoji.json")?;
    let friends = read_json(&part_dirs, "friends.json")?;
    let ranking = read_json(&part_dirs, "ranking.json")?;

    if account.is_none() && user_profile.is_none() {
        emit(1, 1, "No profile metadata found in this export".to_string());
        return Ok(BuildProfileSummary { profile_found: false });
    }

    let snapshot = build_snapshot_json(
        account.as_ref(),
        user_profile.as_ref(),
        bitmoji.as_ref(),
        friends.as_ref(),
        ranking.as_ref(),
    );
    let snapshot_text = serde_json::to_string(&snapshot).map_err(|err| err.to_string())?;

    conn.execute(
        "INSERT INTO profile_snapshots (id, generated_at, snapshot)
         VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?1)
         ON CONFLICT(id) DO UPDATE SET generated_at = excluded.generated_at, snapshot = excluded.snapshot",
        [&snapshot_text],
    )
    .map_err(|err| format!("failed to save profile snapshot: {err}"))?;

    emit(1, 1, "Profile metadata imported".to_string());
    Ok(BuildProfileSummary { profile_found: true })
}

fn read_json(part_dirs: &[PathBuf], filename: &str) -> Result<Option<Value>, String> {
    for part_dir in part_dirs {
        let path = part_dir.join("json").join(filename);
        if path.is_file() {
            let contents = fs::read_to_string(&path)
                .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
            let value: Value = serde_json::from_str(&contents)
                .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn build_snapshot_json(
    account: Option<&Value>,
    user_profile: Option<&Value>,
    bitmoji: Option<&Value>,
    friends: Option<&Value>,
    ranking: Option<&Value>,
) -> Value {
    let basic = account.and_then(|a| a.get("Basic Information"));
    let app_profile = user_profile.and_then(|u| u.get("App Profile"));
    let engagement_list = user_profile.and_then(|u| u.get("Engagement")).and_then(|v| v.as_array());
    let engagement = |event: &str| -> Option<i64> {
        engagement_list?
            .iter()
            .find(|e| e.get("Event").and_then(|v| v.as_str()) == Some(event))
            .and_then(|e| e.get("Occurrences"))
            .and_then(|v| v.as_i64())
    };

    let bitmoji_basic = bitmoji.and_then(|b| b.get("Basic Information"));
    let bitmoji_analytics = bitmoji.and_then(|b| b.get("Analytics"));

    let friends_list = friends.and_then(|f| f.get("Friends")).and_then(|v| v.as_array());
    let top_friends: Vec<Value> = friends_list.map(|l| l.iter().take(6).cloned().collect()).unwrap_or_default();
    let count_of = |key: &str| -> usize {
        friends.and_then(|f| f.get(key)).and_then(|v| v.as_array()).map(Vec::len).unwrap_or(0)
    };

    let ranking_stats = ranking.and_then(|r| r.get("Statistics"));
    let snapscore = ranking_stats
        .and_then(|s| s.get("Snapscore"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| v as i64);
    let total_friends = ranking_stats
        .and_then(|s| s.get("Your Total Friends"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok());

    serde_json::json!({
        "account": {
            "username": basic.and_then(|b| b.get("Username")),
            "display_name": basic.and_then(|b| b.get("Name")),
            "created_at": basic.and_then(|b| b.get("Creation Date")),
            "country": basic
                .and_then(|b| b.get("Country"))
                .or_else(|| app_profile.and_then(|a| a.get("Country"))),
            "registration_ip": basic.and_then(|b| b.get("Registration IP")),
            "in_app_language": app_profile.and_then(|a| a.get("In-app Language")),
        },
        "friends": {
            "friends_count": count_of("Friends"),
            "blocked_count": count_of("Blocked Users"),
            "deleted_count": count_of("Deleted Friends"),
            "top_friends": top_friends,
        },
        "ranking": {
            "snapscore": snapscore,
            "total_friends": total_friends,
        },
        "engagement": {
            "application_opens": engagement("Application Opens"),
            "story_views": engagement("Story Views"),
            "snap_views": engagement("Snap Views"),
            "chats_sent": engagement("Chats Sent"),
            "chats_viewed": engagement("Chats Viewed"),
            "direct_snaps_created": engagement("Direct Snaps Created"),
        },
        "bitmoji": {
            "avatar_gender": bitmoji_analytics.and_then(|a| a.get("Avatar Gender")),
            "app_open_count": bitmoji_analytics.and_then(|a| a.get("App Open Count")),
            "outfit_save_count": bitmoji_analytics.and_then(|a| a.get("Outfit Save Count")),
            "share_count": bitmoji_analytics.and_then(|a| a.get("Share Count")),
            "account_created_at": bitmoji_basic.and_then(|b| b.get("Account Creation Date")),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;
    use uuid::Uuid;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("snapvault-profile-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn builds_and_upserts_snapshot_from_real_shaped_json() {
        let job_dir = tempdir();
        let json_dir = job_dir.join("part-000").join("json");
        fs::create_dir_all(&json_dir).unwrap();

        fs::write(
            json_dir.join("account.json"),
            serde_json::to_string(&serde_json::json!({
                "Basic Information": {
                    "Username": "sammykastanja",
                    "Name": "Sammy",
                    "Creation Date": "2017-02-02 11:16:14 UTC",
                    "Country": "NL",
                    "Registration IP": "1.2.3.4"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            json_dir.join("user_profile.json"),
            serde_json::to_string(&serde_json::json!({
                "App Profile": { "Country": "NL", "In-app Language": "" },
                "Engagement": [
                    { "Event": "Application Opens", "Occurrences": 514 },
                    { "Event": "Story Views", "Occurrences": 155 }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            json_dir.join("friends.json"),
            serde_json::to_string(&serde_json::json!({
                "Friends": [{ "Username": "nick", "Display Name": "Nick" }],
                "Blocked Users": [],
                "Deleted Friends": []
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            json_dir.join("ranking.json"),
            serde_json::to_string(&serde_json::json!({
                "Statistics": { "Snapscore": "104457.0", "Your Total Friends": "75" }
            }))
            .unwrap(),
        )
        .unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        let summary = build_profile_snapshot_blocking(&conn, &job_dir, &|_, _, _| {}).unwrap();
        assert!(summary.profile_found);

        let snapshot_text: String = conn
            .query_row("SELECT snapshot FROM profile_snapshots WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        let snapshot: Value = serde_json::from_str(&snapshot_text).unwrap();
        assert_eq!(snapshot["account"]["username"], "sammykastanja");
        assert_eq!(snapshot["friends"]["friends_count"], 1);
        assert_eq!(snapshot["ranking"]["snapscore"], 104457);
        assert_eq!(snapshot["engagement"]["application_opens"], 514);

        // Re-running (e.g. a second import) must update the same row, not duplicate it.
        build_profile_snapshot_blocking(&conn, &job_dir, &|_, _, _| {}).unwrap();
        let row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM profile_snapshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(row_count, 1);
    }

    #[test]
    fn reports_not_found_when_no_profile_json_present() {
        let job_dir = tempdir();
        fs::create_dir_all(job_dir.join("part-000").join("json")).unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        let summary = build_profile_snapshot_blocking(&conn, &job_dir, &|_, _, _| {}).unwrap();
        assert!(!summary.profile_found);
    }
}
