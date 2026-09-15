use crate::telemetry::{Snapshot, now, summarize};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct Database(
    Arc<Mutex<Connection>>,
    Arc<Mutex<BTreeMap<String, Value>>>,
    usize,
);
#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub backlog_threshold: u64,
    pub refresh_seconds: u64,
    pub retention_days: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            backlog_threshold: 10000,
            refresh_seconds: 5,
            retention_days: 7,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=1_000_000_000).contains(&self.backlog_threshold)
            || !(2..=300).contains(&self.refresh_seconds)
            || !(1..=90).contains(&self.retention_days)
        {
            return Err(
                "Threshold: 1-1,000,000,000; refresh: 2-300 seconds; retention: 1-90 days.".into(),
            );
        }
        Ok(())
    }
}
impl Database {
    pub fn open(dir: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let budget_mb: usize = std::env::var("NATSUI_HISTORY_MAX_MB")
            .unwrap_or("128".into())
            .parse()?;
        if !(16..=4096).contains(&budget_mb) {
            return Err("NATSUI_HISTORY_MAX_MB must be between 16 and 4096".into());
        }
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(std::path::Path::new(dir).join("natsui.sqlite3"))?;
        let version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > 3 {
            return Err("Database schema is newer than this application".into());
        }
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS samples (id INTEGER PRIMARY KEY, scope TEXT NOT NULL, demo INTEGER NOT NULL, at INTEGER NOT NULL, status TEXT NOT NULL, body TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS samples_scope_time ON samples(scope,demo,at);
            CREATE TABLE IF NOT EXISTS activity (id INTEGER PRIMARY KEY, scope TEXT NOT NULL, demo INTEGER NOT NULL, at INTEGER NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS activity_scope_time ON activity(scope,demo,at);
            CREATE TABLE IF NOT EXISTS incidents (id INTEGER PRIMARY KEY, scope TEXT NOT NULL, demo INTEGER NOT NULL, at INTEGER NOT NULL, body TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS incidents_scope_time ON incidents(scope,demo,at);
            CREATE TABLE IF NOT EXISTS profiles (scope TEXT NOT NULL, demo INTEGER NOT NULL, binding TEXT NOT NULL, PRIMARY KEY(scope,demo));
            PRAGMA user_version=3;")?;
        for table in ["samples", "incidents"] {
            let present:bool=conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name='body_bytes')"),[],|r|r.get(0))?;
            if !present {
                conn.execute_batch(&format!("BEGIN; ALTER TABLE {table} ADD COLUMN body_bytes INTEGER NOT NULL DEFAULT 0; UPDATE {table} SET body_bytes=length(CAST(body AS BLOB)); COMMIT;"))?;
            }
        }
        conn.execute_batch("CREATE INDEX IF NOT EXISTS samples_backlog_cover ON samples(scope,demo,at,id,status,json_extract(body,'$.largest'),json_extract(body,'$.interval_seconds'));")?;
        conn.execute(
            "INSERT OR IGNORE INTO settings VALUES (1,?1)",
            [serde_json::to_string(&Settings::default())?],
        )?;
        Ok(Self(
            Arc::new(Mutex::new(conn)),
            Arc::new(Mutex::new(BTreeMap::new())),
            budget_mb * 1024 * 1024,
        ))
    }
    async fn run<T: Send + 'static>(
        &self,
        operation: &'static str,
        action: impl FnOnce(&mut Connection) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let db = self.0.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut connection = db.lock().map_err(|_| "Database lock unavailable")?;
            action(&mut connection)
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|result| result);
        if let Ok(mut health) = self.1.lock() {
            let row = health.entry(operation.into()).or_insert(json!({}));
            row["status"] = json!(if result.is_ok() { "ok" } else { "failed" });
            row[if result.is_ok() {
                "last_success"
            } else {
                "last_failure"
            }] = json!(now());
        }
        result
    }
    pub fn health(&self) -> Value {
        match self.1.lock() {
            Ok(operations) => {
                json!({"status":if operations.values().any(|v|v["status"]=="failed") {"degraded"}else{"ok"},"operations":*operations,"history_budget_bytes":self.2,"incident_budget_bytes":16*1024*1024})
            }
            Err(_) => json!({"status":"unavailable"}),
        }
    }
    pub async fn bind_profile(
        &self,
        scope: &str,
        demo: bool,
        binding: &str,
        adopt_legacy: bool,
    ) -> Result<(), String> {
        let (scope, binding) = (scope.to_owned(), binding.to_owned());
        self.run("profile", move |db| {
            let tx=db.transaction().map_err(|e|e.to_string())?;
            let existing:Option<String>=tx.query_row("SELECT binding FROM profiles WHERE scope=?1 AND demo=?2",params![scope,demo],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
            if let Some(existing)=existing {
                if existing!=binding {return Err("Profile belongs to a different connection identity. Choose a new NATSUI_PROFILE or a separate NATSUI_DATA_DIR.".into());}
            } else {
                let legacy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM samples WHERE scope=?1 AND demo=?2 UNION ALL SELECT 1 FROM incidents WHERE scope=?1 AND demo=?2 UNION ALL SELECT 1 FROM activity WHERE scope=?1 AND demo=?2)",params![scope,demo],|r|r.get(0)).map_err(|e|e.to_string())?;
                if legacy && !adopt_legacy {return Err("Existing history has no connection binding. Use a new profile, or set NATSUI_ADOPT_LEGACY_PROFILE=1 only after verifying that the existing history belongs to this connection.".into());}
                tx.execute("INSERT INTO profiles(scope,demo,binding) VALUES (?1,?2,?3)",params![scope,demo,binding]).map_err(|e|e.to_string())?;
            }
            tx.commit().map_err(|e|e.to_string())
        }).await
    }
    pub async fn settings(&self) -> Result<Settings, String> {
        self.run("settings", |db| {
            let body: String = db
                .query_row("SELECT body FROM settings WHERE id=1", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            serde_json::from_str::<Settings>(&body)
                .map_err(|e| e.to_string())
                .and_then(|settings| {
                    settings.validate()?;
                    Ok(settings)
                })
        })
        .await
    }
    pub async fn save_settings(
        &self,
        settings: Settings,
        scope: &str,
        demo: bool,
    ) -> Result<(), String> {
        let scope = scope.to_owned();
        self.run("save_settings", move |db| {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            let body = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
            tx.execute("UPDATE settings SET body=?1 WHERE id=1", [&body])
                .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO activity(scope,demo,at,kind,detail) VALUES (?1,?2,?3,'settings',?4)",
                params![
                    scope,
                    demo,
                    now(),
                    format!("Local operator changed dashboard settings: {body}")
                ],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())
        })
        .await
    }
    #[cfg(test)]
    pub async fn sample(&self, snapshot: &Snapshot) -> Result<(), String> {
        self.sample_resources(snapshot, &json!({"status":"not_configured","nodes":[]}))
            .await
    }
    pub async fn sample_resources(
        &self,
        snapshot: &Snapshot,
        monitoring: &Value,
    ) -> Result<(), String> {
        let settings = self.settings().await?;
        let mut summary = summarize(snapshot, settings.backlog_threshold);
        summary["interval_seconds"] = json!(settings.refresh_seconds);
        summary["resources"] = crate::resources::project(snapshot, monitoring);
        // Compact summaries retain operational trends without retaining payloads
        // or serializing every stream and consumer configuration on each tick.
        let body = serde_json::to_string(&summary).map_err(|e| e.to_string())?;
        let snapshot = snapshot.clone();
        let budget = self.2;
        if body.len() > budget {
            return Err("A history sample exceeds the configured history budget".into());
        }
        self.run("sample_resources", move |db| {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO samples(scope,demo,at,status,body,body_bytes) VALUES (?1,?2,?3,?4,?5,length(CAST(?5 AS BLOB)))",
                params![
                    snapshot.scope,
                    snapshot.demo,
                    snapshot.observed_at,
                    snapshot.status,
                    body
                ],
            )
            .map_err(|e| e.to_string())?;
            prune_bytes(&tx,"samples",budget)?;
            let cutoff = now().saturating_sub(settings.retention_days * 86400);
            tx.execute("DELETE FROM samples WHERE at < ?1", [cutoff])
                .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM activity WHERE at < ?1", [cutoff])
                .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM incidents WHERE at < ?1", [cutoff])
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())
        })
        .await
    }
    pub async fn incidents(
        &self,
        scope: &str,
        demo: bool,
        events: Vec<Value>,
    ) -> Result<(), String> {
        let scope = scope.to_owned();
        self.run("incidents", move |db| {
            let tx = db.transaction().map_err(|e| e.to_string())?;
            for e in events {
                tx.execute(
                    "INSERT INTO incidents(scope,demo,at,body,body_bytes) VALUES (?1,?2,?3,?4,length(CAST(?4 AS BLOB)))",
                    params![
                        scope,
                        demo,
                        e["at"].as_u64().unwrap_or(now()),
                        e.to_string()
                    ],
                )
                .map_err(|e| e.to_string())?;
            }
            prune_bytes(&tx,"incidents",16*1024*1024)?;
            tx.commit().map_err(|e| e.to_string())
        })
        .await
    }
    pub async fn incident_history(
        &self,
        scope: &str,
        demo: bool,
        window: Option<(u64, u64)>,
    ) -> Result<Value, String> {
        let scope = scope.to_owned();
        self.run("incident_history", move|db|{let mut st=db.prepare("SELECT id,body FROM incidents WHERE scope=?1 AND demo=?2 AND (?3 IS NULL OR at>=?3) AND (?4 IS NULL OR at<=?4) AND json_extract(body,'$.detail') NOT IN ('io.nats.jetstream.advisory.v1.api_audit','io.nats.jetstream.advisory.v1.nak') ORDER BY at DESC,id DESC LIMIT 500").map_err(|e|e.to_string())?;
        let rows=st.query_map(params![scope,demo,window.map(|v|v.0),window.map(|v|v.1)],|r|Ok((r.get::<_,u64>(0)?,r.get::<_,String>(1)?))).map_err(|e|e.to_string())?;let mut result=vec![];for row in rows {let(id,body)=row.map_err(|e|e.to_string())?;let mut e:Value=serde_json::from_str(&body).map_err(|e|e.to_string())?;e["id_local"]=id.into();result.push(e);}Ok(json!(result))}).await
    }
    pub async fn event(
        &self,
        scope: &str,
        demo: bool,
        kind: &str,
        detail: &str,
    ) -> Result<(), String> {
        let (scope, kind, detail) = (scope.to_owned(), kind.to_owned(), detail.to_owned());
        self.run("event", move |db| {
            db.execute("DELETE FROM activity WHERE id IN (SELECT id FROM activity ORDER BY at DESC,id DESC LIMIT -1 OFFSET 9999)",[]).map_err(|e|e.to_string())?;
            db.execute(
                "INSERT INTO activity(scope,demo,at,kind,detail) VALUES (?1,?2,?3,?4,?5)",
                params![scope, demo, now(), kind, detail],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
        .await
    }
    pub async fn history(&self, scope: &str, demo: bool) -> Result<Value, String> {
        let scope = scope.to_owned();
        self.run("history", move|db| {
            let mut statement=db.prepare("SELECT at,status,body FROM samples WHERE scope=?1 AND demo=?2 ORDER BY at DESC,id DESC LIMIT 240").map_err(|e|e.to_string())?;
            let rows=statement.query_map(params![scope,demo],|r|Ok((r.get::<_,u64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(|e|e.to_string())?;
            let mut items=vec![];let mut bytes=0;
            for row in rows { let (at,status,body)=row.map_err(|e|e.to_string())?;bytes+=body.len();if bytes>16_000_000 {break;} items.push(json!({"at":at,"status":status,"summary":serde_json::from_str::<Value>(&body).map_err(|e|e.to_string())?})); }
            items.reverse();Ok(json!(items))
        }).await
    }
    pub async fn history_window(
        &self,
        scope: &str,
        demo: bool,
        from: u64,
        to: u64,
    ) -> Result<Value, String> {
        if from >= to || to - from > 21600 {
            return Err("Choose an increasing time window of at most six hours".into());
        }
        let scope = scope.to_owned();
        self.run("history_window", move|db|{
            let total:u64=db.query_row("SELECT COUNT(*) FROM samples WHERE scope=?1 AND demo=?2 AND at>=?3 AND at<=?4",params![scope,demo,from,to],|r|r.get(0)).map_err(|e|e.to_string())?;
            let mut st=db.prepare("SELECT at,status,body FROM (SELECT id,at,status,body FROM samples WHERE scope=?1 AND demo=?2 AND at>=?3 AND at<=?4 ORDER BY at DESC,id DESC LIMIT 240) ORDER BY at,id").map_err(|e|e.to_string())?;
            let rows=st.query_map(params![scope,demo,from,to],|r|Ok((r.get::<_,u64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(|e|e.to_string())?;
            let mut items=vec![];let mut bytes=0;
            for row in rows {let(at,status,body)=row.map_err(|e|e.to_string())?;bytes+=body.len();if bytes>16_000_000{return Err("Historical response exceeds 16 MB. Choose a narrower time range.".into());}items.push(json!({"at":at,"status":status,"summary":serde_json::from_str::<Value>(&body).map_err(|e|e.to_string())?}));}
            Ok(json!({"from":from,"to":to,"total":total,"truncated":total>240,"samples":items}))
        }).await
    }
    pub async fn backlog_history(
        &self,
        scope: &str,
        demo: bool,
        from: u64,
        to: u64,
    ) -> Result<Value, String> {
        if from >= to || to - from > 21600 {
            return Err("Choose an increasing time window of at most six hours".into());
        }
        let scope = scope.to_owned();
        self.run("backlog_history", move |db| {
            // Project only backlog evidence. Resource inventories can dwarf the chart data.
            let mut statement = db.prepare("SELECT at,status,json_extract(body,'$.largest'),json_extract(body,'$.interval_seconds') FROM samples INDEXED BY samples_backlog_cover WHERE scope=?1 AND demo=?2 AND at>=?3 AND at<=?4 ORDER BY at,id LIMIT 20001").map_err(|e| e.to_string())?;
            let rows = statement.query_map(params![scope,demo,from,to], |row| Ok((row.get::<_,u64>(0)?,row.get::<_,String>(1)?,row.get::<_,Option<String>>(2)?,row.get::<_,Option<u64>>(3)?))).map_err(|e| e.to_string())?;
            let mut samples = Vec::new();
            for row in rows {
                if samples.len() == 20000 { return Err("Backlog source limit reached. Choose a narrower window.".into()); }
                let (at,status,largest,cadence) = row.map_err(|e| e.to_string())?;
                let largest = largest.map(|body| serde_json::from_str::<Value>(&body)).transpose().map_err(|e| e.to_string())?;
                samples.push(json!({"at":at,"status":status,"summary":{"largest":largest},"interval_seconds":cadence}));
            }
            Ok(crate::backlog::aggregate(samples, from, to))
        }).await
    }
    pub async fn activity(&self, scope: &str, demo: bool) -> Result<Value, String> {
        let scope = scope.to_owned();
        self.run("activity", move|db| {
            let mut statement=db.prepare("SELECT at,kind,detail FROM activity WHERE scope=?1 AND demo=?2 ORDER BY at DESC,id DESC LIMIT 100").map_err(|e|e.to_string())?;
            let rows=statement.query_map(params![scope,demo],|r|Ok(json!({"at":r.get::<_,u64>(0)?,"kind":r.get::<_,String>(1)?,"detail":r.get::<_,String>(2)?}))).map_err(|e|e.to_string())?;
            rows.collect::<Result<Vec<_>,_>>().map(|v|json!(v)).map_err(|e|e.to_string())
        }).await
    }
}

// Byte counters avoid scanning payload text during rolling budget enforcement.
fn prune_bytes(db: &Connection, table: &str, budget: usize) -> Result<(), String> {
    db.execute(&format!("DELETE FROM {table} WHERE id IN (SELECT id FROM (SELECT id,SUM(body_bytes) OVER (ORDER BY at DESC,id DESC) AS used FROM {table}) WHERE used>?1)"),[budget]).map(|_|()).map_err(|e|e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn history_budget_preserves_newest_samples() {
        let dir =
            std::env::temp_dir().join(format!("natsui-budget-{}-{}", std::process::id(), now()));
        let mut db = Database::open(dir.to_str().unwrap()).unwrap();
        let mut sample = crate::simulation::snapshot("budget", 0);
        db.sample(&sample).await.unwrap();
        let size: usize =
            db.0.lock()
                .unwrap()
                .query_row("SELECT body_bytes FROM samples", [], |r| r.get(0))
                .unwrap();
        db.2 = size * 2;
        for _ in 1..10 {
            sample.observed_at += 1;
            db.sample(&sample).await.unwrap();
        }
        let history = db.history("budget", true).await.unwrap();
        assert_eq!(history.as_array().unwrap().len(), 2);
        assert_eq!(
            history.as_array().unwrap().last().unwrap()["at"],
            sample.observed_at
        );
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn profile_binding_and_storage_failures_are_visible() {
        let dir =
            std::env::temp_dir().join(format!("natsui-guard-{}-{}", std::process::id(), now()));
        let db = Database::open(dir.to_str().unwrap()).unwrap();
        db.sample(&crate::simulation::snapshot("legacy", 0))
            .await
            .unwrap();
        assert!(
            db.bind_profile("legacy", true, "identity-a", false)
                .await
                .is_err()
        );
        db.bind_profile("legacy", true, "identity-a", true)
            .await
            .unwrap();
        assert!(
            db.bind_profile("legacy", true, "identity-b", true)
                .await
                .is_err()
        );
        db.bind_profile("legacy", false, "identity-b", false)
            .await
            .unwrap();
        db.bind_profile("legacy", true, "identity-a", false)
            .await
            .unwrap();
        db.0.lock()
            .unwrap()
            .execute_batch("PRAGMA query_only=ON")
            .unwrap();
        assert!(
            db.sample(&crate::simulation::snapshot("legacy", 0))
                .await
                .is_err()
        );
        assert_eq!(db.health()["status"], "degraded");
        db.settings().await.unwrap();
        assert_eq!(
            db.health()["status"],
            "degraded",
            "Successful reads must not hide failed history writes"
        );
        db.0.lock()
            .unwrap()
            .execute_batch("PRAGMA query_only=OFF")
            .unwrap();
        db.sample(&crate::simulation::snapshot("legacy", 0))
            .await
            .unwrap();
        assert_eq!(db.health()["status"], "ok");
        assert!(db.health()["operations"]["sample_resources"]["last_failure"].is_u64());
        drop(db);
        let db = Database::open(dir.to_str().unwrap()).unwrap();
        assert!(
            db.bind_profile("legacy", true, "identity-b", false)
                .await
                .is_err()
        );
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn backlog_projection_covers_range_without_resource_inventories() {
        let dir =
            std::env::temp_dir().join(format!("natsui-backlog-{}-{}", std::process::id(), now()));
        let db = Database::open(dir.to_str().unwrap()).unwrap();
        db.run("fixture", |db| {
            for at in 0..600 {
                let body=json!({"interval_seconds":1,"largest":{"pending":if at==123 {9999} else {at},"name":"worker"},"resources":{"payload":"must not leave SQLite"}}).to_string();
                db.execute("INSERT INTO samples(scope,demo,at,status,body,body_bytes) VALUES('chart',1,?1,'complete',?2,?3)",params![at,body,body.len()]).unwrap();
            }
            let mut stmt=db.prepare("EXPLAIN SELECT at,status,json_extract(body,'$.largest'),json_extract(body,'$.interval_seconds') FROM samples INDEXED BY samples_backlog_cover WHERE scope='chart' AND demo=1 AND at>=0 AND at<=599 ORDER BY at,id LIMIT 20001").unwrap();
            let opcodes=stmt.query_map([],|row|Ok((row.get::<_,String>(1)?,row.get::<_,i32>(2)?,row.get::<_,i32>(3)?))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
            assert!(!opcodes.iter().any(|(op,_,_)|op=="Function"));
            assert!(!opcodes.iter().any(|(op,cursor,_)|op=="Column"&&*cursor==0));
            Ok(())
        }).await.unwrap();
        let result = db.backlog_history("chart", true, 0, 599).await.unwrap();
        assert_eq!(result["total"], 600);
        assert_eq!(
            result["samples"].as_array().unwrap().first().unwrap()["at"],
            0
        );
        assert_eq!(
            result["samples"].as_array().unwrap().last().unwrap()["at"],
            599
        );
        assert!(!result.to_string().contains("must not leave"));
        assert_eq!(
            db.backlog_history("chart", false, 0, 599).await.unwrap()["total"],
            0
        );
        assert_eq!(
            db.backlog_history("other", true, 0, 599).await.unwrap()["total"],
            0
        );
        drop(db);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn historical_windows_and_incidents_survive_migration() {
        let dir =
            std::env::temp_dir().join(format!("natsui-window-{}-{}", std::process::id(), now()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = Connection::open(dir.join("natsui.sqlite3")).unwrap();
        old.execute_batch("PRAGMA user_version=1;CREATE TABLE samples(id INTEGER PRIMARY KEY,scope TEXT NOT NULL,demo INTEGER NOT NULL,at INTEGER NOT NULL,status TEXT NOT NULL,body TEXT NOT NULL);").unwrap();
        old.execute(
            "INSERT INTO samples(scope,demo,at,status,body) VALUES (?1,1,?2,'complete',?3)",
            params!["window-test", now() - 30, "{\"existing\":true}"],
        )
        .unwrap();
        drop(old);
        let db = Database::open(dir.to_str().unwrap()).unwrap();
        let mut s = crate::simulation::snapshot("window-test", 0);
        let at = now();
        s.observed_at = at - 20;
        db.sample(&s).await.unwrap();
        s.observed_at = at - 10;
        db.sample(&s).await.unwrap();
        db.incidents(
            "window-test",
            true,
            vec![
                json!({"at":at-20,"kind":"configuration","detail":"before window"}),
                json!({"at":at-10,"kind":"backlog-high","detail":"inside window"}),
            ],
        )
        .await
        .unwrap();
        drop(db);
        let db = Database::open(dir.to_str().unwrap()).unwrap();
        let window = db
            .history_window("window-test", true, at - 15, at)
            .await
            .unwrap();
        assert_eq!(window["samples"].as_array().unwrap().len(), 1);
        assert_eq!(window["total"], 1);
        assert_eq!(
            db.history_window("window-test", true, at - 40, at)
                .await
                .unwrap()["total"],
            3
        );
        let events = db
            .incident_history("window-test", true, Some((at - 15, at)))
            .await
            .unwrap();
        assert_eq!(events.as_array().unwrap().len(), 1);
        assert_eq!(events[0]["detail"], "inside window");
        assert!(
            db.incident_history("window-test", false, None)
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            db.history_window("window-test", true, at, at - 1)
                .await
                .is_err()
        );
        drop(db);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[tokio::test]
    async fn sqlite_survives_reopen_and_separates_demo() {
        let dir =
            std::env::temp_dir().join(format!("natsui-test-{}-{}", std::process::id(), now()));
        let path = dir.to_str().unwrap();
        let db = Database::open(path).unwrap();
        db.save_settings(
            Settings {
                backlog_threshold: 42,
                ..Settings::default()
            },
            "test",
            true,
        )
        .await
        .unwrap();
        db.sample(&crate::telemetry::demo("test")).await.unwrap();
        drop(db);
        let db = Database::open(path).unwrap();
        assert_eq!(db.settings().await.unwrap().backlog_threshold, 42);
        assert_eq!(
            db.history("test", true)
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            db.history("test", false)
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        drop(db);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn settings_reject_unbounded_collection() {
        assert!(
            Settings {
                refresh_seconds: 0,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
    }
}
