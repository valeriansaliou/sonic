// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::config::BackendConfig;
use crate::error::{RouterError, RouterResult};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendStatus {
    #[default]
    Active,
    Draining,
    Offline,
}

impl FromStr for BackendStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "draining" => Ok(Self::Draining),
            "offline" => Ok(Self::Offline),
            _ => Err("invalid_backend_status".to_owned()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Backend {
    pub id: String,
    pub address: String,
    #[serde(default, skip_serializing)]
    pub auth_password: String,
    pub status: BackendStatus,
    pub weight: u32,
    #[serde(default)]
    pub assigned_buckets: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementState {
    #[default]
    Stable,
    Copying,
    CatchingUp,
    Cutover,
    Draining,
}

impl PlacementState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Copying => "copying",
            Self::CatchingUp => "catching_up",
            Self::Cutover => "cutover",
            Self::Draining => "draining",
        }
    }
}

impl FromStr for PlacementState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "stable" => Ok(Self::Stable),
            "copying" => Ok(Self::Copying),
            "catching_up" => Ok(Self::CatchingUp),
            "cutover" => Ok(Self::Cutover),
            "draining" => Ok(Self::Draining),
            _ => Err("invalid_placement_state".to_owned()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Placement {
    pub collection: String,
    pub bucket: String,
    pub primary: String,
    pub target: Option<String>,
    pub previous: Option<String>,
    pub state: PlacementState,
    pub epoch: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DirectoryData {
    pub version: u64,
    pub backends: BTreeMap<String, Backend>,
    pub placements: BTreeMap<String, Placement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    pub primary: Backend,
    pub mirror: Option<Backend>,
    pub epoch: u64,
}

pub struct Directory {
    connection: Mutex<Connection>,
    topology_lock: Mutex<()>,
    route_backends: RwLock<BTreeMap<String, Backend>>,
    route_placements: RwLock<BTreeMap<String, Placement>>,
}

impl Directory {
    pub fn open(path: impl Into<PathBuf>, bootstrap: &[BackendConfig]) -> RouterResult<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path).map_err(sql_error)?;
        connection
            .busy_timeout(Duration::from_secs(10))
            .map_err(sql_error)?;

        connection
            .execute_batch(
                "
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = FULL;
                CREATE TABLE IF NOT EXISTS metadata (
                    key TEXT PRIMARY KEY,
                    value INTEGER NOT NULL
                );
                INSERT OR IGNORE INTO metadata (key, value) VALUES ('version', 0);
                CREATE TABLE IF NOT EXISTS placements (
                    collection TEXT NOT NULL,
                    bucket TEXT NOT NULL,
                    primary_backend TEXT NOT NULL,
                    target_backend TEXT,
                    previous_backend TEXT,
                    state TEXT NOT NULL,
                    epoch INTEGER NOT NULL,
                    PRIMARY KEY (collection, bucket)
                );
                CREATE INDEX IF NOT EXISTS placements_primary
                    ON placements(primary_backend);
                ",
            )
            .map_err(sql_error)?;

        let directory = Self {
            connection: Mutex::new(connection),
            topology_lock: Mutex::new(()),
            route_backends: RwLock::new(BTreeMap::new()),
            route_placements: RwLock::new(BTreeMap::new()),
        };

        directory.reload_route_cache()?;
        directory.replace_backends(bootstrap)?;

        Ok(directory)
    }

    pub fn snapshot(&self) -> RouterResult<DirectoryData> {
        let connection = self.lock()?;

        let version = connection
            .query_row(
                "SELECT value FROM metadata WHERE key = 'version'",
                [],
                |row| row.get(0),
            )
            .map_err(sql_error)?;
        let backends = self.backends()?;

        let mut placements = BTreeMap::new();
        let mut statement = connection
            .prepare(
                "SELECT collection, bucket, primary_backend, target_backend,
                        previous_backend, state, epoch
                 FROM placements ORDER BY collection, bucket",
            )
            .map_err(sql_error)?;

        for placement in statement
            .query_map([], placement_from_row)
            .map_err(sql_error)?
        {
            let placement = placement.map_err(sql_error)?;
            placements.insert(
                placement_key(&placement.collection, &placement.bucket),
                placement,
            );
        }

        Ok(DirectoryData {
            version,
            backends,
            placements,
        })
    }

    pub fn backends(&self) -> RouterResult<BTreeMap<String, Backend>> {
        Ok(self
            .route_backends
            .read()
            .map_err(|_| RouterError::code("route_cache_poisoned"))?
            .clone())
    }

    pub fn replace_backends(&self, configs: &[BackendConfig]) -> RouterResult<()> {
        let _topology_guard = self
            .topology_lock
            .lock()
            .map_err(|_| RouterError::code("topology_lock_poisoned"))?;

        let placements = self
            .route_placements
            .read()
            .map_err(|_| RouterError::code("route_cache_poisoned"))?;

        let mut assigned = BTreeMap::<String, u64>::new();

        for placement in placements.values() {
            *assigned.entry(placement.primary.clone()).or_default() += 1;
        }

        let mut backends = BTreeMap::new();

        for config in configs {
            if config.weight == 0 {
                return Err(RouterError::code("weight_must_be_positive"));
            }
            if backends.contains_key(&config.id) {
                return Err(RouterError::code("duplicate_backend_id"));
            }
            backends.insert(
                config.id.clone(),
                Backend {
                    id: config.id.clone(),
                    address: config.address.clone(),
                    auth_password: config.auth_password.clone(),
                    status: config.status,
                    weight: config.weight,
                    assigned_buckets: assigned.get(&config.id).copied().unwrap_or(0),
                },
            );
        }

        for placement in placements.values() {
            for id in [
                Some(&placement.primary),
                placement.target.as_ref(),
                placement.previous.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                if !backends.contains_key(id) {
                    return Err(RouterError::context("configured_backend_missing", id));
                }
            }
        }

        drop(placements);

        *self
            .route_backends
            .write()
            .map_err(|_| RouterError::code("route_cache_poisoned"))? = backends;

        Ok(())
    }

    pub fn route(
        &self,
        collection: &str,
        bucket: &str,
        create: bool,
        writing: bool,
    ) -> RouterResult<Route> {
        if create {
            self.assign(collection, bucket)?;
        }

        let placement = self
            .route_placements
            .read()
            .map_err(|_| RouterError::code("route_cache_poisoned"))?
            .get(&placement_key(collection, bucket))
            .cloned()
            .ok_or_else(|| RouterError::code("bucket_not_assigned"))?;

        let backends = self
            .route_backends
            .read()
            .map_err(|_| RouterError::code("route_cache_poisoned"))?;
        let primary = backends
            .get(&placement.primary)
            .cloned()
            .filter(|backend| backend.status != BackendStatus::Offline)
            .ok_or_else(|| RouterError::code("primary_backend_unavailable"))?;

        let mirror_id = if writing {
            match placement.state {
                PlacementState::Copying | PlacementState::CatchingUp => placement.target.as_ref(),
                PlacementState::Cutover | PlacementState::Draining => placement.previous.as_ref(),
                PlacementState::Stable => None,
            }
        } else {
            None
        };

        let mirror = match mirror_id {
            Some(id) => backends
                .get(id)
                .cloned()
                .filter(|backend| backend.status != BackendStatus::Offline),
            None => None,
        };

        Ok(Route {
            primary,
            mirror,
            epoch: placement.epoch,
        })
    }

    pub fn assign(&self, collection: &str, bucket: &str) -> RouterResult<Placement> {
        if let Some(placement) = self
            .route_placements
            .read()
            .map_err(|_| RouterError::code("route_cache_poisoned"))?
            .get(&placement_key(collection, bucket))
            .cloned()
        {
            return Ok(placement);
        }

        let _topology_guard = self
            .topology_lock
            .lock()
            .map_err(|_| RouterError::code("topology_lock_poisoned"))?;

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;

        if let Some(placement) = query_placement(&transaction, collection, bucket)? {
            self.cache_placement(placement.clone())?;
            return Ok(placement);
        }

        let backend = choose_backend(&self.backends()?, collection, bucket)?;

        transaction
            .execute(
                "INSERT INTO placements (
                    collection, bucket, primary_backend, state, epoch
                 ) VALUES (?1, ?2, ?3, 'stable', 1)",
                params![collection, bucket, backend],
            )
            .map_err(sql_error)?;
        bump_version(&transaction)?;
        transaction.commit().map_err(sql_error)?;

        let placement = Placement {
            collection: collection.to_owned(),
            bucket: bucket.to_owned(),
            primary: backend.clone(),
            target: None,
            previous: None,
            state: PlacementState::Stable,
            epoch: 1,
        };

        self.cache_placement(placement.clone())?;
        self.update_backend_counts(None, Some(&backend))?;

        Ok(placement)
    }

    pub fn placements_for_backend(&self, id: &str) -> RouterResult<Vec<Placement>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT collection, bucket, primary_backend, target_backend,
                        previous_backend, state, epoch
                 FROM placements WHERE primary_backend = ?1
                 ORDER BY collection, bucket",
            )
            .map_err(sql_error)?;
        statement
            .query_map([id], placement_from_row)
            .map_err(sql_error)?
            .map(|placement| placement.map_err(sql_error))
            .collect()
    }

    pub fn placement(&self, collection: &str, bucket: &str) -> RouterResult<Placement> {
        self.route_placements
            .read()
            .map_err(|_| RouterError::code("route_cache_poisoned"))?
            .get(&placement_key(collection, bucket))
            .cloned()
            .ok_or_else(|| RouterError::code("placement_not_found"))
    }

    pub fn start_migration(
        &self,
        collection: &str,
        bucket: &str,
        target: &str,
    ) -> RouterResult<Placement> {
        let _topology_guard = self
            .topology_lock
            .lock()
            .map_err(|_| RouterError::code("topology_lock_poisoned"))?;

        let target_backend = self
            .backends()?
            .get(target)
            .filter(|backend| backend.status == BackendStatus::Active)
            .cloned()
            .ok_or_else(|| RouterError::code("target_backend_not_active"))?;

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;

        let mut placement = query_placement(&transaction, collection, bucket)?
            .ok_or_else(|| RouterError::code("placement_not_found"))?;

        if placement.state != PlacementState::Stable {
            return Err(RouterError::code("migration_already_in_progress"));
        }
        if placement.primary == target_backend.id {
            return Err(RouterError::code("target_is_already_primary"));
        }

        placement.target = Some(target_backend.id);
        placement.state = PlacementState::Copying;
        placement.epoch += 1;

        save_placement(&transaction, &placement)?;
        bump_version(&transaction)?;
        transaction.commit().map_err(sql_error)?;

        self.cache_placement(placement.clone())?;

        Ok(placement)
    }

    pub fn mark_catching_up(&self, collection: &str, bucket: &str) -> RouterResult<Placement> {
        self.simple_transition(
            collection,
            bucket,
            PlacementState::Copying,
            PlacementState::CatchingUp,
        )
    }

    pub fn cutover(&self, collection: &str, bucket: &str) -> RouterResult<Placement> {
        let _topology_guard = self
            .topology_lock
            .lock()
            .map_err(|_| RouterError::code("topology_lock_poisoned"))?;

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;

        let mut placement =
            checked_placement(&transaction, collection, bucket, PlacementState::CatchingUp)?;
        let source = placement.primary.clone();
        let target = placement
            .target
            .take()
            .ok_or_else(|| RouterError::code("migration_target_missing"))?;

        placement.previous = Some(source);
        placement.primary = target.clone();
        placement.state = PlacementState::Cutover;
        placement.epoch += 1;

        save_placement(&transaction, &placement)?;
        bump_version(&transaction)?;
        transaction.commit().map_err(sql_error)?;

        self.cache_placement(placement.clone())?;
        self.update_backend_counts(placement.previous.as_deref(), Some(&target))?;

        Ok(placement)
    }

    pub fn mark_draining(&self, collection: &str, bucket: &str) -> RouterResult<Placement> {
        self.simple_transition(
            collection,
            bucket,
            PlacementState::Cutover,
            PlacementState::Draining,
        )
    }

    pub fn finish_migration(&self, collection: &str, bucket: &str) -> RouterResult<Placement> {
        let _topology_guard = self
            .topology_lock
            .lock()
            .map_err(|_| RouterError::code("topology_lock_poisoned"))?;

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;

        let mut placement =
            checked_placement(&transaction, collection, bucket, PlacementState::Draining)?;

        placement.previous = None;
        placement.state = PlacementState::Stable;
        placement.epoch += 1;

        save_placement(&transaction, &placement)?;
        bump_version(&transaction)?;
        transaction.commit().map_err(sql_error)?;

        self.cache_placement(placement.clone())?;

        Ok(placement)
    }

    pub fn rollback(&self, collection: &str, bucket: &str) -> RouterResult<Placement> {
        let _topology_guard = self
            .topology_lock
            .lock()
            .map_err(|_| RouterError::code("topology_lock_poisoned"))?;

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;

        let mut placement = query_placement(&transaction, collection, bucket)?
            .ok_or_else(|| RouterError::code("placement_not_found"))?;
        let mut count_change = None;

        match placement.state {
            PlacementState::Copying | PlacementState::CatchingUp => placement.target = None,
            PlacementState::Cutover | PlacementState::Draining => {
                let previous = placement
                    .previous
                    .take()
                    .ok_or_else(|| RouterError::code("previous_backend_missing"))?;
                let current = placement.primary.clone();
                placement.primary = previous.clone();
                count_change = Some((current, previous));
            }
            PlacementState::Stable => return Err(RouterError::code("migration_not_in_progress")),
        }

        placement.state = PlacementState::Stable;
        placement.epoch += 1;

        save_placement(&transaction, &placement)?;
        bump_version(&transaction)?;
        transaction.commit().map_err(sql_error)?;

        self.cache_placement(placement.clone())?;

        if let Some((current, previous)) = count_change {
            self.update_backend_counts(Some(&current), Some(&previous))?;
        }

        Ok(placement)
    }

    fn simple_transition(
        &self,
        collection: &str,
        bucket: &str,
        expected: PlacementState,
        next: PlacementState,
    ) -> RouterResult<Placement> {
        let _topology_guard = self
            .topology_lock
            .lock()
            .map_err(|_| RouterError::code("topology_lock_poisoned"))?;

        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;

        let mut placement = checked_placement(&transaction, collection, bucket, expected)?;

        placement.state = next;
        placement.epoch += 1;

        save_placement(&transaction, &placement)?;
        bump_version(&transaction)?;
        transaction.commit().map_err(sql_error)?;

        self.cache_placement(placement.clone())?;

        Ok(placement)
    }

    fn reload_route_cache(&self) -> RouterResult<()> {
        let snapshot = self.snapshot()?;
        *self
            .route_backends
            .write()
            .map_err(|_| RouterError::code("route_cache_poisoned"))? = snapshot.backends;
        *self
            .route_placements
            .write()
            .map_err(|_| RouterError::code("route_cache_poisoned"))? = snapshot.placements;
        Ok(())
    }

    fn cache_placement(&self, placement: Placement) -> RouterResult<()> {
        self.route_placements
            .write()
            .map_err(|_| RouterError::code("route_cache_poisoned"))?
            .insert(
                placement_key(&placement.collection, &placement.bucket),
                placement,
            );
        Ok(())
    }

    fn update_backend_counts(
        &self,
        source: Option<&str>,
        target: Option<&str>,
    ) -> RouterResult<()> {
        let mut backends = self
            .route_backends
            .write()
            .map_err(|_| RouterError::code("route_cache_poisoned"))?;

        if let Some(source) = source {
            let backend = backends
                .get_mut(source)
                .ok_or_else(|| RouterError::code("backend_not_found"))?;
            backend.assigned_buckets = backend
                .assigned_buckets
                .checked_sub(1)
                .ok_or_else(|| RouterError::code("backend_bucket_count_underflow"))?;
        }

        if let Some(target) = target {
            let backend = backends
                .get_mut(target)
                .ok_or_else(|| RouterError::code("backend_not_found"))?;
            backend.assigned_buckets += 1;
        }

        Ok(())
    }

    fn lock(&self) -> RouterResult<std::sync::MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| RouterError::code("directory_lock_poisoned"))
    }
}

fn query_placement(
    connection: &Connection,
    collection: &str,
    bucket: &str,
) -> RouterResult<Option<Placement>> {
    connection
        .query_row(
            "SELECT collection, bucket, primary_backend, target_backend,
                    previous_backend, state, epoch
             FROM placements WHERE collection = ?1 AND bucket = ?2",
            params![collection, bucket],
            placement_from_row,
        )
        .optional()
        .map_err(sql_error)
}

fn checked_placement(
    connection: &Connection,
    collection: &str,
    bucket: &str,
    expected: PlacementState,
) -> RouterResult<Placement> {
    let placement = query_placement(connection, collection, bucket)?
        .ok_or_else(|| RouterError::code("placement_not_found"))?;
    if placement.state != expected {
        return Err(RouterError::code("invalid_migration_state"));
    }
    Ok(placement)
}

fn choose_backend(
    backends: &BTreeMap<String, Backend>,
    collection: &str,
    bucket: &str,
) -> RouterResult<String> {
    let candidates = backends
        .values()
        .filter(|backend| backend.status == BackendStatus::Active)
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return Err(RouterError::code("no_active_backend"));
    }

    if candidates.len() == 1 {
        return Ok(candidates[0].id.clone());
    }

    let key = placement_key(collection, bucket);
    let first = stable_hash(key.as_bytes(), 0) as usize % candidates.len();
    let mut second = stable_hash(key.as_bytes(), 1) as usize % candidates.len();

    if first == second {
        second = (second + 1) % candidates.len();
    }

    let left = &candidates[first];
    let right = &candidates[second];
    let left_load = u128::from(left.assigned_buckets) * u128::from(right.weight);
    let right_load = u128::from(right.assigned_buckets) * u128::from(left.weight);

    if (left_load, &left.id) <= (right_load, &right.id) {
        Ok(left.id.clone())
    } else {
        Ok(right.id.clone())
    }
}

fn save_placement(connection: &Connection, placement: &Placement) -> RouterResult<()> {
    connection
        .execute(
            "UPDATE placements
             SET primary_backend = ?3, target_backend = ?4, previous_backend = ?5,
                 state = ?6, epoch = ?7
             WHERE collection = ?1 AND bucket = ?2",
            params![
                placement.collection,
                placement.bucket,
                placement.primary,
                placement.target,
                placement.previous,
                placement.state.as_str(),
                placement.epoch
            ],
        )
        .map_err(sql_error)?;
    Ok(())
}

fn bump_version(connection: &Connection) -> RouterResult<()> {
    connection
        .execute(
            "UPDATE metadata SET value = value + 1 WHERE key = 'version'",
            [],
        )
        .map_err(sql_error)?;
    Ok(())
}

fn placement_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Placement> {
    let state: String = row.get(5)?;
    Ok(Placement {
        collection: row.get(0)?,
        bucket: row.get(1)?,
        primary: row.get(2)?,
        target: row.get(3)?,
        previous: row.get(4)?,
        state: state.parse().map_err(|error| conversion_error(5, error))?,
        epoch: row.get(6)?,
    })
}

fn conversion_error(column: usize, error: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
    )
}

fn stable_hash(value: &[u8], salt: u64) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64 ^ salt;
    for byte in value {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn placement_key(collection: &str, bucket: &str) -> String {
    format!("{}:{collection}{bucket}", collection.len())
}

fn sql_error(error: rusqlite::Error) -> RouterError {
    error.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend(id: &str, port: u16) -> BackendConfig {
        BackendConfig {
            id: id.to_owned(),
            address: format!("[::1]:{port}"),
            auth_password: String::new(),
            status: BackendStatus::Active,
            weight: 1,
        }
    }

    #[test]
    fn assignment_is_persistent_and_balanced() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = Directory::open(
            temporary.path().join("directory.db"),
            &[backend("a", 1491), backend("b", 1492)],
        )
        .unwrap();
        for index in 0..10_000 {
            directory
                .assign("messages", &format!("bucket:{index}"))
                .unwrap();
        }
        let snapshot = directory.snapshot().unwrap();
        let difference = snapshot.backends["a"]
            .assigned_buckets
            .abs_diff(snapshot.backends["b"].assigned_buckets);
        assert!(difference <= 2);
        assert_eq!(
            directory.assign("messages", "bucket:42").unwrap(),
            directory.assign("messages", "bucket:42").unwrap()
        );
    }

    #[test]
    fn migration_routes_and_rolls_back() {
        let temporary = tempfile::tempdir().unwrap();
        let source = backend("a", 1491);
        let target = backend("b", 1492);
        let directory = Directory::open(
            temporary.path().join("directory.db"),
            std::slice::from_ref(&source),
        )
        .unwrap();
        directory.assign("messages", "bucket").unwrap();
        directory.replace_backends(&[source, target]).unwrap();
        directory
            .start_migration("messages", "bucket", "b")
            .unwrap();
        let route = directory.route("messages", "bucket", false, true).unwrap();
        assert_eq!(route.primary.id, "a");
        assert_eq!(route.mirror.unwrap().id, "b");
        directory.mark_catching_up("messages", "bucket").unwrap();
        directory.cutover("messages", "bucket").unwrap();
        let route = directory.route("messages", "bucket", false, true).unwrap();
        assert_eq!(route.primary.id, "b");
        assert_eq!(route.mirror.unwrap().id, "a");
        directory.rollback("messages", "bucket").unwrap();
        assert_eq!(
            directory
                .route("messages", "bucket", false, false)
                .unwrap()
                .primary
                .id,
            "a"
        );
    }

    #[test]
    fn draining_backend_receives_no_new_buckets() {
        let temporary = tempfile::tempdir().unwrap();
        let mut draining = backend("a", 1491);
        draining.status = BackendStatus::Draining;
        let directory = Directory::open(
            temporary.path().join("directory.db"),
            &[draining, backend("b", 1492)],
        )
        .unwrap();
        assert_eq!(directory.assign("messages", "bucket").unwrap().primary, "b");
    }

    #[test]
    fn reloads_server_config_without_reassigning_buckets() {
        let temporary = tempfile::tempdir().unwrap();
        let original = backend("a", 1491);
        let directory = Directory::open(
            temporary.path().join("directory.db"),
            std::slice::from_ref(&original),
        )
        .unwrap();
        directory.assign("messages", "bucket").unwrap();
        let mut updated = original;
        updated.address = "sonic-0.sonic:1491".to_owned();
        directory.replace_backends(&[updated]).unwrap();
        let route = directory.route("messages", "bucket", false, false).unwrap();
        assert_eq!(route.primary.id, "a");
        assert_eq!(route.primary.address, "sonic-0.sonic:1491");
    }

    #[test]
    fn rejects_removing_a_server_with_assigned_buckets() {
        let temporary = tempfile::tempdir().unwrap();
        let directory =
            Directory::open(temporary.path().join("directory.db"), &[backend("a", 1491)]).unwrap();
        directory.assign("messages", "bucket").unwrap();
        assert_eq!(
            directory.replace_backends(&[]).unwrap_err().to_string(),
            "configured_backend_missing:a"
        );
    }

    #[test]
    fn offline_backend_does_not_fail_over_implicitly() {
        let temporary = tempfile::tempdir().unwrap();
        let active = backend("a", 1491);
        let directory = Directory::open(
            temporary.path().join("directory.db"),
            std::slice::from_ref(&active),
        )
        .unwrap();
        directory.assign("messages", "bucket").unwrap();
        let mut offline = active;
        offline.status = BackendStatus::Offline;
        directory.replace_backends(&[offline]).unwrap();
        assert_eq!(
            directory
                .route("messages", "bucket", false, false)
                .unwrap_err()
                .to_string(),
            "primary_backend_unavailable"
        );
    }

    #[test]
    fn reopens_persistent_directory_without_reassigning() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("directory.db");
        let servers = [backend("a", 1491), backend("b", 1492)];
        let primary = {
            let directory = Directory::open(&path, &servers).unwrap();
            directory.assign("messages", "bucket").unwrap().primary
        };
        let reopened = Directory::open(&path, &servers).unwrap();
        assert_eq!(
            reopened.assign("messages", "bucket").unwrap().primary,
            primary
        );
    }

    #[test]
    fn reopens_directory_during_migration_without_losing_routes() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("directory.db");
        let servers = [backend("a", 1491), backend("b", 1492)];
        let (primary, target) = {
            let directory = Directory::open(&path, &servers).unwrap();
            let primary = directory.assign("messages", "bucket").unwrap().primary;
            let target = if primary == "a" { "b" } else { "a" };
            directory
                .start_migration("messages", "bucket", target)
                .unwrap();
            (primary, target.to_owned())
        };
        let reopened = Directory::open(&path, &servers).unwrap();
        let placement = reopened.placement("messages", "bucket").unwrap();
        let route = reopened.route("messages", "bucket", false, true).unwrap();
        assert_eq!(placement.state, PlacementState::Copying);
        assert_eq!(route.primary.id, primary);
        assert_eq!(route.mirror.unwrap().id, target);
    }
}
