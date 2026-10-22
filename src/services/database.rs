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

    fn from_connection(connection: Connection) -> rusqlite::Result<Self> {
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
        ] {
            if let Err(error) = connection.execute(migration, []) {
                if !error.to_string().contains("duplicate column name") {
                    return Err(error);
                }
            }
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    #[cfg(test)]
    fn in_memory() -> rusqlite::Result<Self> {
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
        let mut statement = connection.prepare(
            "SELECT id, name, url, auto_update, update_interval, user_agent, raw_config,
                    upload, download, total, expire_at, reset_at, last_updated,
                          node_count, usable_node_count,
                          selected_node_key,
                          CASE WHEN selected_subscription.subscription_id = subscriptions.id THEN 1 ELSE 0 END
                      FROM subscriptions LEFT JOIN selected_subscription ON selected_subscription.singleton = 1
                      ORDER BY created_at DESC, subscriptions.id DESC",
        )?;
        let rows = statement.query_map([], |row| {
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
        self.connection
            .lock()
            .expect("database mutex poisoned")
            .execute(
                "UPDATE subscriptions SET selected_node_key = ?1 WHERE id = ?2",
                params![node_key, subscription_id],
            )?;
        Ok(())
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
        self.connection
            .lock()
            .expect("database mutex poisoned")
            .execute(
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
                 url = ?2, auto_update = ?3, update_interval = ?4, user_agent = ?5
                 WHERE id = ?6",
                params![name, url, auto_update, update_interval, user_agent, id],
            )?;
        Ok(())
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

    pub fn remove_subscription(&self, id: i64) -> rusqlite::Result<()> {
        self.connection
            .lock()
            .expect("database mutex poisoned")
            .execute("DELETE FROM subscriptions WHERE id = ?1", [id])?;
        Ok(())
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
}

pub fn data_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cloak-dx")
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

        database.remove_subscription(subscriptions[0].id).unwrap();
        assert!(database.subscriptions().unwrap().is_empty());
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
}
