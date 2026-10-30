use std::{path::PathBuf, sync::Mutex};

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Clone, Debug, PartialEq)]
pub struct Subscription {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub auto_update: bool,
    pub update_interval: String,
    pub user_agent: String,
    pub raw_config: String,
    pub upload: String,
    pub download: String,
    pub total: String,
    pub expire_at: Option<i64>,
    pub reset_at: Option<i64>,
    pub last_updated: Option<String>,
    pub node_count: u32,
    pub usable_node_count: u32,
    pub selected: bool,
    pub selected_node_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoreInstall {
    pub engine: String,
    pub channel: String,
    pub version: String,
    pub executable_path: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoreRelease {
    pub version: String,
    pub asset_name: String,
    pub download_url: String,
    pub published_at: String,
}

/// Latest test data for a single node.
///
/// Latency and Google exit are written independently, so a fresh latency
/// result keeps the previous exit data until the exit probe reports again.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeTestResult {
    pub node_key: String,
    pub latency_ms: Option<u64>,
    /// The last latency test failed. Kept separately from the latency value so
    /// a failure survives an application restart and stays distinguishable
    /// from a node that has never been tested.
    pub latency_failed: bool,
    /// Share of the latency probes that timed out in the last test.
    pub packet_loss: Option<u8>,
    pub exit_ip: Option<String>,
    pub exit_country_code: Option<String>,
    pub exit_country: Option<String>,
    pub exit_region: Option<String>,
}

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    pub fn open() -> rusqlite::Result<Self> {
        let database_path = database_path();
        std::fs::create_dir_all(database_path.parent().expect("database path has parent"))
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        Self::from_connection(Connection::open(database_path)?)
    }

    fn from_connection(mut connection: Connection) -> rusqlite::Result<Self> {
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS subscriptions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                raw_config TEXT NOT NULL DEFAULT '',
                xray_config TEXT NOT NULL DEFAULT '',
                sing_box_config TEXT NOT NULL DEFAULT '',
                auto_update INTEGER NOT NULL DEFAULT 1,
                update_interval TEXT NOT NULL DEFAULT '12h',
                user_agent TEXT NOT NULL DEFAULT '',
                upload TEXT NOT NULL DEFAULT '0 B',
                download TEXT NOT NULL DEFAULT '0 B',
                total TEXT NOT NULL DEFAULT '0 B',
                expire_at INTEGER,
                reset_at INTEGER,
                last_updated TEXT,
                node_count INTEGER NOT NULL DEFAULT 0,
                usable_node_count INTEGER NOT NULL DEFAULT 0,
                selected_node_key TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );
             CREATE TABLE IF NOT EXISTS core_installs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                engine TEXT NOT NULL CHECK(engine IN ('xray', 'sing-box')),
                channel TEXT NOT NULL CHECK(channel IN ('main', 'preview')),
                version TEXT NOT NULL,
                executable_path TEXT NOT NULL,
                installed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(engine, channel, version)
             );
                 CREATE TABLE IF NOT EXISTS selected_subscription (
                     singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                     subscription_id INTEGER NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE
                 );
             CREATE TABLE IF NOT EXISTS core_selection (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                install_id INTEGER NOT NULL REFERENCES core_installs(id)
                 );
                 CREATE TABLE IF NOT EXISTS core_release_cache (
                     engine TEXT NOT NULL,
                     channel TEXT NOT NULL,
                     version TEXT NOT NULL,
                     asset_name TEXT NOT NULL,
                     download_url TEXT NOT NULL,
                     published_at TEXT NOT NULL,
                     cached_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                     PRIMARY KEY (engine, channel, version)
                 );
                 CREATE TABLE IF NOT EXISTS node_test_results (
                     subscription_id INTEGER NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
                     engine TEXT NOT NULL DEFAULT 'xray',
                     node_key TEXT NOT NULL,
                     latency_ms INTEGER,
                     latency_failed INTEGER NOT NULL DEFAULT 0,
                     packet_loss INTEGER,
                     exit_ip TEXT,
                     exit_country_code TEXT,
                     exit_country TEXT,
                     exit_region TEXT,
                     updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                     PRIMARY KEY (subscription_id, engine, node_key)
                 );
                 CREATE TABLE IF NOT EXISTS app_settings (
                     key TEXT PRIMARY KEY,
                     value TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS engine_node_selection (
                     subscription_id INTEGER NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
                     engine TEXT NOT NULL,
                     node_key TEXT NOT NULL,
                     PRIMARY KEY (subscription_id, engine)
                 );",
        )?;
        for migration in [
            "ALTER TABLE subscriptions ADD COLUMN upload TEXT NOT NULL DEFAULT '0 B'",
            "ALTER TABLE subscriptions ADD COLUMN download TEXT NOT NULL DEFAULT '0 B'",
            "ALTER TABLE subscriptions ADD COLUMN total TEXT NOT NULL DEFAULT '0 B'",
            "ALTER TABLE subscriptions ADD COLUMN expire_at INTEGER",
            "ALTER TABLE subscriptions ADD COLUMN reset_at INTEGER",
            "ALTER TABLE subscriptions ADD COLUMN last_updated TEXT",
            "ALTER TABLE subscriptions ADD COLUMN node_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE subscriptions ADD COLUMN usable_node_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE subscriptions ADD COLUMN selected_node_key TEXT",
            "ALTER TABLE node_test_results ADD COLUMN latency_failed INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE node_test_results ADD COLUMN packet_loss INTEGER",
        ] {
            if let Err(error) = connection.execute(migration, []) {
                if !error.to_string().contains("duplicate column name") {
                    return Err(error);
                }
            }
        }
        // Results recorded by an older build predate the engine column and can
        // only have come from Xray, which was the only engine it could start.
        migrate_node_test_results_engine(&mut connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> rusqlite::Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn add_subscription(
        &self,
        name: &str,
        url: &str,
        auto_update: bool,
        update_interval: &str,
        user_agent: &str,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO subscriptions (name, url, raw_config, auto_update, update_interval, user_agent)
             VALUES (?1, ?2, '', ?3, ?4, ?5)",
            params![name, url, auto_update, update_interval, user_agent],
        )?;
        let subscription_id = transaction.last_insert_rowid();
        transaction.execute(
            "INSERT OR IGNORE INTO selected_subscription (singleton, subscription_id)
             VALUES (1, ?1)",
            [subscription_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn subscriptions(&self) -> rusqlite::Result<Vec<Subscription>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let engine = query_selected_engine(&connection)?.unwrap_or_default();
        let mut statement = connection.prepare(
            "SELECT id, name, url, auto_update, update_interval, user_agent, raw_config,
                    upload, download, total, expire_at, reset_at, last_updated,
                          node_count, usable_node_count,
                          COALESCE(engine_node_selection.node_key, subscriptions.selected_node_key),
                          CASE WHEN selected_subscription.subscription_id = subscriptions.id THEN 1 ELSE 0 END
                      FROM subscriptions
                      LEFT JOIN selected_subscription ON selected_subscription.singleton = 1
                      LEFT JOIN engine_node_selection
                          ON engine_node_selection.subscription_id = subscriptions.id
                         AND engine_node_selection.engine = ?1
                      ORDER BY created_at DESC, subscriptions.id DESC",
        )?;
        let rows = statement.query_map(params![engine], |row| {
            Ok(Subscription {
                id: row.get(0)?,
                name: row.get(1)?,
                url: row.get(2)?,
                auto_update: row.get(3)?,
                update_interval: row.get(4)?,
                user_agent: row.get(5)?,
                raw_config: row.get(6)?,
                upload: row.get(7)?,
                download: row.get(8)?,
                total: row.get(9)?,
                expire_at: row.get(10)?,
                reset_at: row.get(11)?,
                last_updated: row.get(12)?,
                node_count: row.get(13)?,
                usable_node_count: row.get(14)?,
                selected_node_key: row.get(15)?,
                selected: row.get(16)?,
            })
        })?;
        rows.collect()
    }

    pub fn selected_subscription(&self) -> rusqlite::Result<Option<Subscription>> {
        Ok(self
            .subscriptions()?
            .into_iter()
            .find(|subscription| subscription.selected))
    }

    /// Engine of the core picked in Settings, or `None` when no core has been
    /// installed yet.
    pub fn selected_engine(&self) -> rusqlite::Result<Option<String>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        query_selected_engine(&connection)
    }

    pub fn select_subscription(&self, id: i64) -> rusqlite::Result<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        connection.execute(
            "INSERT INTO selected_subscription (singleton, subscription_id)
             SELECT 1, id FROM subscriptions WHERE id = ?1
             ON CONFLICT(singleton) DO UPDATE SET subscription_id = excluded.subscription_id",
            [id],
        )?;
        Ok(())
    }

    pub fn select_subscription_node(
        &self,
        subscription_id: i64,
        node_key: &str,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let engine = query_selected_engine(&connection)?;
        let transaction = connection.transaction()?;
        if let Some(engine) = engine {
            record_engine_node_selection(&transaction, subscription_id, &engine, node_key)?;
        }
        transaction.execute(
            "UPDATE subscriptions SET selected_node_key = ?1 WHERE id = ?2",
            params![node_key, subscription_id],
        )?;
        transaction.commit()
    }

    /// Stores a node under one engine without consulting the current core
    /// selection.
    ///
    /// Used when the user switches cores: the node the old engine was serving
    /// is saved before the new engine becomes active, so switching back can
    /// restore it. Each engine keeps its own row, and a node key is only a
    /// preference (the Proxies page falls back to the first visible node when
    /// a subscription refresh removed it).
    pub fn select_subscription_node_for_engine(
        &self,
        subscription_id: i64,
        engine: &str,
        node_key: &str,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        record_engine_node_selection(&transaction, subscription_id, engine, node_key)?;
        transaction.execute(
            "UPDATE subscriptions SET selected_node_key = ?1 WHERE id = ?2",
            params![node_key, subscription_id],
        )?;
        transaction.commit()
    }

    pub fn update_subscription(
        &self,
        id: i64,
        name: &str,
        url: &str,
        auto_update: bool,
        update_interval: &str,
        user_agent: &str,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let previous_url: Option<String> = connection
            .query_row("SELECT url FROM subscriptions WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE subscriptions SET name = ?1,
             raw_config = CASE WHEN url <> ?2 THEN '' ELSE raw_config END,
             upload = CASE WHEN url <> ?2 THEN '0 B' ELSE upload END,
             download = CASE WHEN url <> ?2 THEN '0 B' ELSE download END,
             total = CASE WHEN url <> ?2 THEN '0 B' ELSE total END,
             expire_at = CASE WHEN url <> ?2 THEN NULL ELSE expire_at END,
             reset_at = CASE WHEN url <> ?2 THEN NULL ELSE reset_at END,
             last_updated = CASE WHEN url <> ?2 THEN NULL ELSE last_updated END,
             node_count = CASE WHEN url <> ?2 THEN 0 ELSE node_count END,
             usable_node_count = CASE WHEN url <> ?2 THEN 0 ELSE usable_node_count END,
             selected_node_key = CASE WHEN url <> ?2 THEN NULL ELSE selected_node_key END,
             url = ?2, auto_update = ?3, update_interval = ?4, user_agent = ?5
             WHERE id = ?6",
            params![name, url, auto_update, update_interval, user_agent, id],
        )?;
        // Node keys belong to the old subscription content; a new URL makes
        // every per-engine selection meaningless.
        if previous_url.as_deref() != Some(url) {
            transaction.execute(
                "DELETE FROM engine_node_selection WHERE subscription_id = ?1",
                [id],
            )?;
        }
        transaction.commit()
    }

    pub fn update_subscription_refresh(
        &self,
        id: i64,
        raw_config: &str,
        upload: &str,
        download: &str,
        total: &str,
        expire_at: Option<i64>,
        reset_at: Option<i64>,
        node_count: u32,
        usable_node_count: u32,
    ) -> rusqlite::Result<()> {
        self.connection
            .lock()
            .expect("database mutex poisoned")
            .execute(
                "UPDATE subscriptions SET raw_config = ?1, upload = ?2, download = ?3,
                 total = ?4, expire_at = ?5, reset_at = ?6, node_count = ?7,
                 usable_node_count = ?8, last_updated = CURRENT_TIMESTAMP WHERE id = ?9",
                params![
                    raw_config,
                    upload,
                    download,
                    total,
                    expire_at,
                    reset_at,
                    node_count,
                    usable_node_count,
                    id
                ],
            )?;
        Ok(())
    }

    /// Removes a subscription and everything stored on its behalf: the fetched
    /// configuration (and therefore every node parsed from it), the cached node
    /// test results and the selection pointing at it.
    ///
    /// The child tables declare `ON DELETE CASCADE`, but deleting explicitly
    /// keeps databases created before that constraint from retaining orphaned
    /// rows, and lets a single transaction cover every table.
    pub fn remove_subscription(&self, id: i64) -> rusqlite::Result<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM node_test_results WHERE subscription_id = ?1",
            [id],
        )?;
        transaction.execute(
            "DELETE FROM selected_subscription WHERE subscription_id = ?1",
            [id],
        )?;
        transaction.execute(
            "DELETE FROM engine_node_selection WHERE subscription_id = ?1",
            [id],
        )?;
        transaction.execute("DELETE FROM subscriptions WHERE id = ?1", [id])?;
        transaction.commit()
    }

    pub fn record_core_install(&self, install: &CoreInstall) -> rusqlite::Result<()> {
        self.connection
            .lock()
            .expect("database mutex poisoned")
            .execute(
                "INSERT INTO core_installs (engine, channel, version, executable_path)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(engine, channel, version) DO UPDATE SET executable_path = excluded.executable_path",
                params![install.engine, install.channel, install.version, install.executable_path],
            )?;
        Ok(())
    }

    pub fn core_installs(&self) -> rusqlite::Result<Vec<CoreInstall>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT i.engine, i.channel, i.version, i.executable_path,
                    CASE WHEN s.install_id = i.id THEN 1 ELSE 0 END
             FROM core_installs i LEFT JOIN core_selection s ON s.install_id = i.id
             ORDER BY i.engine, i.channel, i.installed_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(CoreInstall {
                engine: row.get(0)?,
                channel: row.get(1)?,
                version: row.get(2)?,
                executable_path: row.get(3)?,
                selected: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn select_core(&self, engine: &str, channel: &str, version: &str) -> rusqlite::Result<()> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let install_id: Option<i64> = connection
            .query_row(
                "SELECT id FROM core_installs WHERE engine = ?1 AND channel = ?2 AND version = ?3",
                params![engine, channel, version],
                |row| row.get(0),
            )
            .optional()?;
        let install_id = install_id.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
        connection.execute(
            "INSERT INTO core_selection (singleton, install_id) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET install_id = excluded.install_id",
            [install_id],
        )?;
        Ok(())
    }

    pub fn remove_core_install(
        &self,
        engine: &str,
        channel: &str,
        version: &str,
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        let install_id: Option<i64> = transaction
            .query_row(
                "SELECT id FROM core_installs WHERE engine = ?1 AND channel = ?2 AND version = ?3",
                params![engine, channel, version],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(install_id) = install_id {
            transaction.execute(
                "DELETE FROM core_selection WHERE install_id = ?1",
                [install_id],
            )?;
            transaction.execute("DELETE FROM core_installs WHERE id = ?1", [install_id])?;
        }
        transaction.commit()
    }

    pub fn cached_core_releases(
        &self,
        engine: &str,
        channel: &str,
    ) -> rusqlite::Result<Vec<CoreRelease>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT version, asset_name, download_url, published_at
             FROM core_release_cache WHERE engine = ?1 AND channel = ?2
             ORDER BY cached_at DESC, rowid ASC",
        )?;
        let rows = statement.query_map(params![engine, channel], |row| {
            Ok(CoreRelease {
                version: row.get(0)?,
                asset_name: row.get(1)?,
                download_url: row.get(2)?,
                published_at: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn replace_core_release_cache(
        &self,
        engine: &str,
        channel: &str,
        releases: &[CoreRelease],
    ) -> rusqlite::Result<()> {
        let mut connection = self.connection.lock().expect("database mutex poisoned");
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM core_release_cache WHERE engine = ?1 AND channel = ?2",
            params![engine, channel],
        )?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO core_release_cache
                 (engine, channel, version, asset_name, download_url, published_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for release in releases {
                statement.execute(params![
                    engine,
                    channel,
                    release.version,
                    release.asset_name,
                    release.download_url,
                    release.published_at,
                ])?;
            }
        }
        transaction.commit()
    }

    /// Loads the newest latency and Google exit values a subscription recorded
    /// while running on `engine`.
    pub fn node_test_results(
        &self,
        subscription_id: i64,
        engine: &str,
    ) -> rusqlite::Result<Vec<NodeTestResult>> {
        let connection = self.connection.lock().expect("database mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT node_key, latency_ms, latency_failed, packet_loss, exit_ip, exit_country_code,
                    exit_country, exit_region
             FROM node_test_results WHERE subscription_id = ?1 AND engine = ?2 ORDER BY node_key",
        )?;
        let rows = statement.query_map(params![subscription_id, engine], |row| {
            let latency_ms = row.get::<_, Option<i64>>(1)?.and_then(|value| {
                // Latency is a positive duration; ignore corrupt negative rows.
                u64::try_from(value).ok()
            });
            Ok(NodeTestResult {
                node_key: row.get(0)?,
                latency_ms,
                latency_failed: row.get::<_, i64>(2)? != 0,
                packet_loss: row
                    .get::<_, Option<i64>>(3)?
                    .and_then(|value| u8::try_from(value).ok()),
                exit_ip: row.get(4)?,
                exit_country_code: row.get(5)?,
                exit_country: row.get(6)?,
                exit_region: row.get(7)?,
            })
        })?;
        rows.collect()
    }

    /// Replaces the stored latency for a node on one engine. Only the latest
    /// test is kept, and passing `None` marks the last test as failed.
    pub fn record_node_latency(
        &self,
        subscription_id: i64,
        engine: &str,
        node_key: &str,
        latency_ms: Option<u64>,
        packet_loss: Option<u8>,
    ) -> rusqlite::Result<()> {
        let latency_ms = latency_ms.and_then(|value| i64::try_from(value).ok());
        let packet_loss = packet_loss.map(i64::from);
        self.connection
            .lock()
            .expect("database mutex poisoned")
            .execute(
                "INSERT INTO node_test_results
                     (subscription_id, engine, node_key, latency_ms, latency_failed, packet_loss)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(subscription_id, engine, node_key) DO UPDATE SET
                     latency_ms = excluded.latency_ms,
                     latency_failed = excluded.latency_failed,
                     packet_loss = excluded.packet_loss,
                     updated_at = CURRENT_TIMESTAMP",
                params![
                    subscription_id,
                    engine,
                    node_key,
                    latency_ms,
                    i64::from(latency_ms.is_none()),
                    packet_loss
                ],
            )?;
        Ok(())
    }

    /// Replaces the stored Google exit for a node on one engine without
    /// touching its latency.
    pub fn record_node_google_exit(
        &self,
        subscription_id: i64,
        engine: &str,
        node_key: &str,
        exit_ip: &str,
        country_code: Option<&str>,
        country: Option<&str>,
        region: Option<&str>,
    ) -> rusqlite::Result<()> {
        self.connection
            .lock()
            .expect("database mutex poisoned")
            .execute(
                "INSERT INTO node_test_results
                     (subscription_id, engine, node_key, exit_ip, exit_country_code, exit_country, exit_region)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(subscription_id, engine, node_key) DO UPDATE SET
                     exit_ip = excluded.exit_ip,
                     exit_country_code = excluded.exit_country_code,
                     exit_country = excluded.exit_country,
                     exit_region = excluded.exit_region,
                     updated_at = CURRENT_TIMESTAMP",
                params![
                    subscription_id,
                    engine,
                    node_key,
                    exit_ip,
                    country_code,
                    country,
                    region
                ],
            )?;
        Ok(())
    }

    /// Reads a persisted application preference.
    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.connection
            .lock()
            .expect("database mutex poisoned")
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()
    }

    /// Stores an application preference, replacing any previous value.
    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.connection
            .lock()
            .expect("database mutex poisoned")
            .execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        Ok(())
    }
}

/// Rebuilds `node_test_results` so the engine is part of its primary key.
///
/// SQLite cannot change a primary key in place, and an older table has no
/// engine column at all. Every row it holds was produced by Xray because that
/// was the only engine earlier builds could start, so the migration attributes
/// them to `xray` instead of discarding the history.
fn migrate_node_test_results_engine(connection: &mut Connection) -> rusqlite::Result<()> {
    if table_has_column(connection, "node_test_results", "engine")? {
        return Ok(());
    }
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE node_test_results_with_engine (
             subscription_id INTEGER NOT NULL REFERENCES subscriptions(id) ON DELETE CASCADE,
             engine TEXT NOT NULL DEFAULT 'xray',
             node_key TEXT NOT NULL,
             latency_ms INTEGER,
             latency_failed INTEGER NOT NULL DEFAULT 0,
             packet_loss INTEGER,
             exit_ip TEXT,
             exit_country_code TEXT,
             exit_country TEXT,
             exit_region TEXT,
             updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
             PRIMARY KEY (subscription_id, engine, node_key)
         );
         INSERT INTO node_test_results_with_engine
             (subscription_id, engine, node_key, latency_ms, latency_failed, packet_loss,
              exit_ip, exit_country_code, exit_country, exit_region, updated_at)
         SELECT subscription_id, 'xray', node_key, latency_ms, latency_failed, packet_loss,
                exit_ip, exit_country_code, exit_country, exit_region, updated_at
         FROM node_test_results;
         DROP TABLE node_test_results;
         ALTER TABLE node_test_results_with_engine RENAME TO node_test_results;",
    )?;
    transaction.commit()
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name.eq_ignore_ascii_case(column) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn data_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cloak-dx")
}

fn query_selected_engine(connection: &Connection) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT i.engine FROM core_selection s
             JOIN core_installs i ON i.id = s.install_id",
            [],
            |row| row.get(0),
        )
        .optional()
}

fn record_engine_node_selection(
    connection: &Connection,
    subscription_id: i64,
    engine: &str,
    node_key: &str,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO engine_node_selection (subscription_id, engine, node_key)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(subscription_id, engine) DO UPDATE SET node_key = excluded.node_key",
        params![subscription_id, engine, node_key],
    )?;
    Ok(())
}

pub fn database_path() -> PathBuf {
    data_directory().join("cloak-dx.sqlite3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscriptions_are_persisted_and_removable() {
        let database = Database::in_memory().unwrap();
        database
            .add_subscription(
                "Example",
                "https://example.com/subscription",
                true,
                "12h",
                "cloak-dx-test",
            )
            .unwrap();

        let subscriptions = database.subscriptions().unwrap();
        assert_eq!(subscriptions.len(), 1);
        assert_eq!(subscriptions[0].name, "Example");
        assert_eq!(subscriptions[0].url, "https://example.com/subscription");
        assert_eq!(subscriptions[0].update_interval, "12h");
        let subscription_id = subscriptions[0].id;
        assert!(subscriptions[0].selected);

        database
            .record_node_latency(
                subscription_id,
                "xray",
                "vless|node-a|host-a",
                Some(120),
                Some(0),
            )
            .unwrap();
        database
            .record_node_google_exit(
                subscription_id,
                "xray",
                "vless|node-a|host-a",
                "203.0.113.7",
                Some("US"),
                None,
                None,
            )
            .unwrap();
        assert_eq!(
            database
                .node_test_results(subscription_id, "xray")
                .unwrap()
                .len(),
            1
        );

        database.remove_subscription(subscription_id).unwrap();
        assert!(database.subscriptions().unwrap().is_empty());
        // Deleting a subscription also erases its node test data and the
        // selection row instead of leaving them behind.
        assert!(database
            .node_test_results(subscription_id, "xray")
            .unwrap()
            .is_empty());
        assert!(database.selected_subscription().unwrap().is_none());
    }

    #[test]
    fn first_subscription_is_selected_and_selection_can_change() {
        let database = Database::in_memory().unwrap();
        database
            .add_subscription("First", "https://example.com/first", true, "12 hr", "")
            .unwrap();
        let first_id = database.subscriptions().unwrap()[0].id;
        assert_eq!(
            database.selected_subscription().unwrap().unwrap().id,
            first_id
        );

        database
            .add_subscription("Second", "https://example.com/second", true, "12 hr", "")
            .unwrap();
        let subscriptions = database.subscriptions().unwrap();
        let second_id = subscriptions
            .iter()
            .find(|item| item.name == "Second")
            .unwrap()
            .id;
        assert!(
            subscriptions
                .iter()
                .find(|item| item.id == first_id)
                .unwrap()
                .selected
        );
        database.select_subscription(second_id).unwrap();
        assert_eq!(
            database.selected_subscription().unwrap().unwrap().id,
            second_id
        );
    }

    #[test]
    fn selected_node_is_persisted_per_subscription() {
        let database = Database::in_memory().unwrap();
        database
            .add_subscription("One", "https://example.com/one", true, "12 hr", "")
            .unwrap();
        database
            .add_subscription("Two", "https://example.com/two", true, "12 hr", "")
            .unwrap();
        let subscriptions = database.subscriptions().unwrap();
        let first_id = subscriptions
            .iter()
            .find(|item| item.name == "One")
            .unwrap()
            .id;
        let second_id = subscriptions
            .iter()
            .find(|item| item.name == "Two")
            .unwrap()
            .id;

        database
            .select_subscription_node(first_id, "vless|node-a|host-a")
            .unwrap();
        database
            .select_subscription_node(second_id, "trojan|node-b|host-b")
            .unwrap();

        let subscriptions = database.subscriptions().unwrap();
        assert_eq!(
            subscriptions
                .iter()
                .find(|item| item.id == first_id)
                .unwrap()
                .selected_node_key
                .as_deref(),
            Some("vless|node-a|host-a")
        );
        assert_eq!(
            subscriptions
                .iter()
                .find(|item| item.id == second_id)
                .unwrap()
                .selected_node_key
                .as_deref(),
            Some("trojan|node-b|host-b")
        );
    }

    #[test]
    fn each_engine_remembers_the_node_it_was_running() {
        let database = Database::in_memory().unwrap();
        let xray = CoreInstall {
            engine: "xray".to_string(),
            channel: "main".to_string(),
            version: "v25.1.1".to_string(),
            executable_path: "/tmp/xray/xray".to_string(),
            selected: false,
        };
        let sing_box = CoreInstall {
            engine: "sing-box".to_string(),
            channel: "main".to_string(),
            version: "1.14.3".to_string(),
            executable_path: "/tmp/sing-box/sing-box".to_string(),
            selected: false,
        };
        database.record_core_install(&xray).unwrap();
        database.record_core_install(&sing_box).unwrap();
        database
            .select_core(&xray.engine, &xray.channel, &xray.version)
            .unwrap();
        database
            .add_subscription("Nodes", "https://example.com/nodes", true, "12 hr", "")
            .unwrap();
        let subscription_id = database.subscriptions().unwrap()[0].id;

        let selected_node = |database: &Database| {
            database.subscriptions().unwrap()[0]
                .selected_node_key
                .clone()
        };

        database
            .select_subscription_node(subscription_id, "shared-node")
            .unwrap();
        assert_eq!(selected_node(&database).as_deref(), Some("shared-node"));

        // Switching saves what the old engine was serving and hands the new
        // engine the last node until it has a preference of its own.
        database
            .select_subscription_node_for_engine(subscription_id, "xray", "shared-node")
            .unwrap();
        database
            .select_core(&sing_box.engine, &sing_box.channel, &sing_box.version)
            .unwrap();
        assert_eq!(selected_node(&database).as_deref(), Some("shared-node"));
        database
            .select_subscription_node(subscription_id, "sing-box-only-node")
            .unwrap();
        assert_eq!(
            selected_node(&database).as_deref(),
            Some("sing-box-only-node")
        );

        // Switching back restores the node Xray was serving, and switching
        // again restores sing-box's own node.
        database
            .select_subscription_node_for_engine(subscription_id, "sing-box", "sing-box-only-node")
            .unwrap();
        database
            .select_core(&xray.engine, &xray.channel, &xray.version)
            .unwrap();
        assert_eq!(selected_node(&database).as_deref(), Some("shared-node"));
        database
            .select_subscription_node_for_engine(subscription_id, "xray", "shared-node")
            .unwrap();
        database
            .select_core(&sing_box.engine, &sing_box.channel, &sing_box.version)
            .unwrap();
        assert_eq!(
            selected_node(&database).as_deref(),
            Some("sing-box-only-node")
        );
    }

    #[test]
    fn changing_the_url_forgets_every_engine_selection() {
        let database = Database::in_memory().unwrap();
        database
            .add_subscription("Nodes", "https://example.com/old", true, "12 hr", "")
            .unwrap();
        let subscription_id = database.subscriptions().unwrap()[0].id;
        database
            .select_subscription_node_for_engine(subscription_id, "xray", "node-a")
            .unwrap();
        database
            .select_subscription_node_for_engine(subscription_id, "sing-box", "node-b")
            .unwrap();

        database
            .update_subscription(
                subscription_id,
                "Nodes",
                "https://example.com/new",
                true,
                "12 hr",
                "",
            )
            .unwrap();

        let subscription = database.subscriptions().unwrap().remove(0);
        assert_eq!(subscription.selected_node_key, None);
    }

    #[test]
    fn subscription_refresh_data_survives_metadata_edits_and_clears_on_url_change() {
        let database = Database::in_memory().unwrap();
        database
            .add_subscription("Old name", "https://example.com/old", true, "12 hr", "")
            .unwrap();
        let subscription = database.subscriptions().unwrap().remove(0);
        database
            .update_subscription_refresh(
                subscription.id,
                "raw subscription response",
                "1 GB",
                "2 GB",
                "10 GB",
                Some(1_800_000_000),
                Some(1_700_000_000),
                7,
                7,
            )
            .unwrap();

        database
            .update_subscription(
                subscription.id,
                "New name",
                &subscription.url,
                true,
                "24 hr",
                "",
            )
            .unwrap();
        let updated = database.subscriptions().unwrap().remove(0);
        assert_eq!(updated.name, "New name");
        assert_eq!(updated.raw_config, "raw subscription response");
        assert_eq!(updated.upload, "1 GB");
        assert_eq!(updated.node_count, 7);
        assert_eq!(updated.usable_node_count, 7);

        database
            .update_subscription(
                subscription.id,
                "New name",
                "https://example.com/new",
                true,
                "24 hr",
                "",
            )
            .unwrap();
        let changed_url = database.subscriptions().unwrap().remove(0);
        assert!(changed_url.raw_config.is_empty());
        assert_eq!(changed_url.upload, "0 B");
        assert_eq!(changed_url.expire_at, None);
        assert_eq!(changed_url.node_count, 0);
        assert_eq!(changed_url.usable_node_count, 0);
    }

    #[test]
    fn installed_core_selection_is_persisted() {
        let database = Database::in_memory().unwrap();
        let core = CoreInstall {
            engine: "sing-box".to_string(),
            channel: "preview".to_string(),
            version: "1.11.0-rc.1".to_string(),
            executable_path: "/tmp/sing-box".to_string(),
            selected: false,
        };
        database.record_core_install(&core).unwrap();
        database
            .select_core(&core.engine, &core.channel, &core.version)
            .unwrap();

        let installed = database.core_installs().unwrap();
        assert_eq!(installed.len(), 1);
        assert!(installed[0].selected);
        assert_eq!(installed[0].executable_path, "/tmp/sing-box");
    }

    #[test]
    fn removing_core_install_clears_its_selection() {
        let database = Database::in_memory().unwrap();
        let xray = CoreInstall {
            engine: "xray".to_string(),
            channel: "main".to_string(),
            version: "v25.1.1".to_string(),
            executable_path: "/tmp/xray/xray".to_string(),
            selected: false,
        };
        let sing_box = CoreInstall {
            engine: "sing-box".to_string(),
            channel: "main".to_string(),
            version: "1.14.3".to_string(),
            executable_path: "/tmp/sing-box/sing-box".to_string(),
            selected: false,
        };
        database.record_core_install(&xray).unwrap();
        database.record_core_install(&sing_box).unwrap();
        database
            .select_core(&xray.engine, &xray.channel, &xray.version)
            .unwrap();

        database
            .remove_core_install(&xray.engine, &xray.channel, &xray.version)
            .unwrap();

        let installed = database.core_installs().unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].engine, "sing-box");
        assert!(!installed[0].selected);
        database
            .select_core(&sing_box.engine, &sing_box.channel, &sing_box.version)
            .unwrap();
        assert!(database.core_installs().unwrap()[0].selected);
    }

    #[test]
    fn node_results_are_kept_separately_per_engine() {
        let database = Database::in_memory().unwrap();
        database
            .add_subscription("Nodes", "https://example.com/nodes", true, "12h", "")
            .unwrap();
        let subscription_id = database.subscriptions().unwrap()[0].id;
        database
            .record_node_latency(subscription_id, "xray", "node-a", Some(120), Some(0))
            .unwrap();
        database
            .record_node_google_exit(
                subscription_id,
                "xray",
                "node-a",
                "203.0.113.7",
                Some("US"),
                None,
                None,
            )
            .unwrap();
        database
            .record_node_latency(subscription_id, "sing-box", "node-a", Some(45), Some(0))
            .unwrap();

        let xray = database.node_test_results(subscription_id, "xray").unwrap();
        assert_eq!(xray.len(), 1);
        assert_eq!(xray[0].latency_ms, Some(120));
        assert_eq!(xray[0].exit_ip.as_deref(), Some("203.0.113.7"));

        // The same node keeps a different result under the other engine; the
        // Xray values are not overwritten or reused.
        let sing_box = database
            .node_test_results(subscription_id, "sing-box")
            .unwrap();
        assert_eq!(sing_box.len(), 1);
        assert_eq!(sing_box[0].latency_ms, Some(45));
        assert_eq!(sing_box[0].exit_ip, None);
    }

    #[test]
    fn results_from_before_the_engine_column_are_attributed_to_xray() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE subscriptions (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     name TEXT NOT NULL,
                     url TEXT NOT NULL UNIQUE,
                     raw_config TEXT NOT NULL DEFAULT '',
                     xray_config TEXT NOT NULL DEFAULT '',
                     sing_box_config TEXT NOT NULL DEFAULT '',
                     auto_update INTEGER NOT NULL DEFAULT 1,
                     update_interval TEXT NOT NULL DEFAULT '12h',
                     user_agent TEXT NOT NULL DEFAULT '',
                     upload TEXT NOT NULL DEFAULT '0 B',
                     download TEXT NOT NULL DEFAULT '0 B',
                     total TEXT NOT NULL DEFAULT '0 B',
                     expire_at INTEGER,
                     reset_at INTEGER,
                     last_updated TEXT,
                     node_count INTEGER NOT NULL DEFAULT 0,
                     usable_node_count INTEGER NOT NULL DEFAULT 0,
                     selected_node_key TEXT,
                     created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 INSERT INTO subscriptions (id, name, url)
                 VALUES (1, 'Legacy', 'https://example.com/legacy');
                 CREATE TABLE node_test_results (
                     subscription_id INTEGER NOT NULL,
                     node_key TEXT NOT NULL,
                     latency_ms INTEGER,
                     latency_failed INTEGER NOT NULL DEFAULT 0,
                     packet_loss INTEGER,
                     exit_ip TEXT,
                     exit_country_code TEXT,
                     exit_country TEXT,
                     exit_region TEXT,
                     updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                     PRIMARY KEY (subscription_id, node_key)
                 );
                 INSERT INTO node_test_results (subscription_id, node_key, latency_ms)
                 VALUES (1, 'node-a', 42);",
            )
            .unwrap();

        let database = Database::from_connection(connection).unwrap();
        let xray = database.node_test_results(1, "xray").unwrap();
        assert_eq!(xray.len(), 1);
        assert_eq!(xray[0].latency_ms, Some(42));
        assert!(database
            .node_test_results(1, "sing-box")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn selected_engine_names_the_core_picked_in_settings() {
        let database = Database::in_memory().unwrap();
        assert_eq!(database.selected_engine().unwrap(), None);

        let xray = CoreInstall {
            engine: "xray".to_string(),
            channel: "main".to_string(),
            version: "v25.1.1".to_string(),
            executable_path: "/tmp/xray/xray".to_string(),
            selected: false,
        };
        let sing_box = CoreInstall {
            engine: "sing-box".to_string(),
            channel: "main".to_string(),
            version: "1.14.3".to_string(),
            executable_path: "/tmp/sing-box/sing-box".to_string(),
            selected: false,
        };
        database.record_core_install(&xray).unwrap();
        database.record_core_install(&sing_box).unwrap();
        database
            .select_core(&xray.engine, &xray.channel, &xray.version)
            .unwrap();
        assert_eq!(database.selected_engine().unwrap().as_deref(), Some("xray"));
        database
            .select_core(&sing_box.engine, &sing_box.channel, &sing_box.version)
            .unwrap();
        assert_eq!(
            database.selected_engine().unwrap().as_deref(),
            Some("sing-box")
        );
    }

    #[test]
    fn persistent_database_path_is_absolute() {
        assert!(database_path().is_absolute());
    }

    #[test]
    fn release_versions_are_persisted_per_engine_and_channel() {
        let database = Database::in_memory().unwrap();
        let releases = vec![CoreRelease {
            version: "1.14.3".to_string(),
            asset_name: "sing-box-1.14.3-linux-amd64.tar.gz".to_string(),
            download_url: "https://example.com/sing-box.tar.gz".to_string(),
            published_at: "2026-10-22".to_string(),
        }];
        database
            .replace_core_release_cache("sing-box", "main", &releases)
            .unwrap();

        assert_eq!(
            database.cached_core_releases("sing-box", "main").unwrap(),
            releases
        );
        assert!(database
            .cached_core_releases("xray", "main")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn node_results_keep_only_the_latest_values() {
        let database = Database::in_memory().unwrap();
        database
            .add_subscription("Nodes", "https://example.com/nodes", true, "12h", "")
            .unwrap();
        let subscription_id = database.subscriptions().unwrap()[0].id;

        database
            .record_node_latency(subscription_id, "xray", "node-a", Some(120), Some(0))
            .unwrap();
        database
            .record_node_google_exit(
                subscription_id,
                "xray",
                "node-a",
                "203.0.113.7",
                Some("US"),
                Some("United States"),
                Some("California"),
            )
            .unwrap();

        // A newer latency replaces the old one without touching the exit data.
        database
            .record_node_latency(subscription_id, "xray", "node-a", Some(85), Some(25))
            .unwrap();
        let results = database.node_test_results(subscription_id, "xray").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].latency_ms, Some(85));
        assert!(!results[0].latency_failed);
        assert_eq!(results[0].packet_loss, Some(25));
        assert_eq!(results[0].exit_ip.as_deref(), Some("203.0.113.7"));
        assert_eq!(results[0].exit_country_code.as_deref(), Some("US"));

        // A newer exit replaces the old one without touching the latency.
        database
            .record_node_google_exit(
                subscription_id,
                "xray",
                "node-a",
                "198.51.100.9",
                Some("JP"),
                Some("Japan"),
                Some("Tokyo"),
            )
            .unwrap();
        let results = database.node_test_results(subscription_id, "xray").unwrap();
        assert_eq!(results[0].latency_ms, Some(85));
        assert_eq!(results[0].exit_ip.as_deref(), Some("198.51.100.9"));
        assert_eq!(results[0].exit_country_code.as_deref(), Some("JP"));
        assert_eq!(results[0].exit_region.as_deref(), Some("Tokyo"));
    }

    #[test]
    fn application_settings_are_persisted_and_updated() {
        let database = Database::in_memory().unwrap();
        assert_eq!(database.get_setting("hide_timeout").unwrap(), None);

        database.set_setting("hide_timeout", "1").unwrap();
        assert_eq!(
            database.get_setting("hide_timeout").unwrap().as_deref(),
            Some("1")
        );

        database.set_setting("hide_timeout", "0").unwrap();
        assert_eq!(
            database.get_setting("hide_timeout").unwrap().as_deref(),
            Some("0")
        );
    }

    #[test]
    fn failed_latency_clears_the_stored_latency_but_keeps_the_exit() {
        let database = Database::in_memory().unwrap();
        database
            .add_subscription("Nodes", "https://example.com/nodes", true, "12h", "")
            .unwrap();
        let subscription_id = database.subscriptions().unwrap()[0].id;
        database
            .record_node_latency(subscription_id, "xray", "node-a", Some(42), Some(0))
            .unwrap();
        database
            .record_node_google_exit(
                subscription_id,
                "xray",
                "node-a",
                "203.0.113.7",
                Some("US"),
                None,
                None,
            )
            .unwrap();

        database
            .record_node_latency(subscription_id, "xray", "node-a", None, None)
            .unwrap();

        let results = database.node_test_results(subscription_id, "xray").unwrap();
        assert_eq!(results[0].latency_ms, None);
        assert!(results[0].latency_failed);
        assert_eq!(results[0].packet_loss, None);
        assert_eq!(results[0].exit_ip.as_deref(), Some("203.0.113.7"));

        // A later success clears the persisted failure marker.
        database
            .record_node_latency(subscription_id, "xray", "node-a", Some(30), Some(0))
            .unwrap();
        let results = database.node_test_results(subscription_id, "xray").unwrap();
        assert_eq!(results[0].latency_ms, Some(30));
        assert!(!results[0].latency_failed);
        assert_eq!(results[0].packet_loss, Some(0));
    }
}
