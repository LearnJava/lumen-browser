//! `PRAGMA user_version`-based schema migration helper shared by every SQLite store.
//!
//! Before this module each store duplicated `PRAGMA journal_mode`/`CREATE TABLE
//! IF NOT EXISTS` in its own `init()`, and the two stores that outgrew a single
//! `CREATE TABLE` (`profiles`, `bookmarks`) each invented their own ad-hoc
//! detection for "has this ALTER already run?" — one swallowing the "duplicate
//! column" error, the other reading `PRAGMA table_info` by hand. Neither records
//! a schema version anywhere, so there is no way to tell "not yet migrated" from
//! "migrated by a newer binary that added a column this one doesn't know about".
//!
//! [`run_migrations`] replaces both patterns: every store lists its migrations as
//! plain SQL keyed by the `user_version` they reach, and this function applies
//! only the ones above the file's current version, in order, inside one
//! transaction — and refuses to open a database whose `user_version` is already
//! past the last migration a given binary knows about.

use rusqlite::{ffi, Connection, Error as SqlError, Result as SqlResult};

/// One schema migration, identified by the `user_version` it upgrades the
/// database to.
pub struct Migration {
    /// Target `user_version` reached after this migration's `sql` runs.
    pub version: u32,
    /// SQL statement(s) executed via `execute_batch` to reach `version`.
    pub sql: &'static str,
}

/// Sets the WAL pragmas every store used to set individually. Call once right
/// after `Connection::open`/`open_in_memory`, before [`run_migrations`].
pub fn set_common_pragmas(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
}

/// Applies every migration in `migrations` whose `version` is greater than the
/// database's current `user_version`, ascending, inside a single transaction,
/// then stamps `user_version` to the highest version in `migrations`.
///
/// Returns `Err` without touching the database if `user_version` is already
/// past the highest version in `migrations` — that means a newer binary
/// already upgraded this file, and applying an older binary's migrations (or
/// silently doing nothing) would leave the schema in a state this build's
/// queries don't expect.
pub fn run_migrations(conn: &mut Connection, migrations: &[Migration]) -> SqlResult<()> {
    let current: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let latest = migrations.iter().map(|m| m.version).max().unwrap_or(0);
    if current > latest {
        return Err(SqlError::SqliteFailure(
            ffi::Error::new(ffi::SQLITE_SCHEMA),
            Some(format!(
                "database schema version {current} is newer than the highest version ({latest}) this build knows how to open"
            )),
        ));
    }
    let mut pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > current).collect();
    if pending.is_empty() {
        return Ok(());
    }
    pending.sort_by_key(|m| m.version);
    let tx = conn.transaction()?;
    for m in &pending {
        tx.execute_batch(m.sql)?;
    }
    tx.pragma_update(None, "user_version", latest)?;
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_migrations_in_order_and_stamps_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        set_common_pragmas(&conn).unwrap();
        let migrations = [
            Migration {
                version: 1,
                sql: "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT NOT NULL);",
            },
            Migration {
                version: 2,
                sql: "ALTER TABLE t ADD COLUMN age INTEGER DEFAULT NULL;",
            },
        ];
        run_migrations(&mut conn, &migrations).unwrap();
        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        conn.execute(
            "INSERT INTO t (id, name, age) VALUES (1, 'a', 30)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn is_idempotent_on_reopen() {
        let mut conn = Connection::open_in_memory().unwrap();
        let migrations = [Migration {
            version: 1,
            sql: "CREATE TABLE t (id INTEGER PRIMARY KEY);",
        }];
        run_migrations(&mut conn, &migrations).unwrap();
        // Second call must not re-run the CREATE TABLE (which would error).
        run_migrations(&mut conn, &migrations).unwrap();
    }

    #[test]
    fn applies_only_migrations_above_current_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY); PRAGMA user_version = 1;",
        )
        .unwrap();
        let migrations = [
            Migration {
                version: 1,
                sql: "CREATE TABLE t (id INTEGER PRIMARY KEY);",
            },
            Migration {
                version: 2,
                sql: "ALTER TABLE t ADD COLUMN name TEXT;",
            },
        ];
        // Version 1's CREATE TABLE must be skipped (table already exists);
        // only version 2's ALTER must run.
        run_migrations(&mut conn, &migrations).unwrap();
        conn.execute("INSERT INTO t (id, name) VALUES (1, 'a')", [])
            .unwrap();
    }

    #[test]
    fn refuses_to_open_database_from_the_future() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 5).unwrap();
        let migrations = [Migration {
            version: 2,
            sql: "CREATE TABLE t (id INTEGER PRIMARY KEY);",
        }];
        let err = run_migrations(&mut conn, &migrations).unwrap_err();
        assert!(matches!(err, SqlError::SqliteFailure(_, _)));
    }

    #[test]
    fn transaction_rolls_back_on_failure() {
        let mut conn = Connection::open_in_memory().unwrap();
        let migrations = [
            Migration {
                version: 1,
                sql: "CREATE TABLE t (id INTEGER PRIMARY KEY);",
            },
            Migration {
                version: 2,
                sql: "not valid sql;",
            },
        ];
        assert!(run_migrations(&mut conn, &migrations).is_err());
        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        // Neither migration's version stamp survives — the whole batch rolled back.
        assert_eq!(version, 0);
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='t'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap();
        assert!(!table_exists);
    }
}
