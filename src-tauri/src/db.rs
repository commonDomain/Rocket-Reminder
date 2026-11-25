use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Local};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::model::{
    next_loop_occurrence, next_occurrence, LoopConfig, Reminder, ReminderStatus, RepeatType,
};

#[derive(Debug, Clone)]
pub struct DatabaseHealth {
    pub quick_check_ok: bool,
    pub quick_check_message: String,
    pub foreign_key_issues: usize,
    pub total: i64,
    pub pending: i64,
    pub running: i64,
    pub completed: i64,
    pub missed: i64,
    pub stopped: i64,
}

impl DatabaseHealth {
    pub fn is_ok(&self) -> bool {
        self.quick_check_ok && self.foreign_key_issues == 0
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS reminders (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  content TEXT NOT NULL,
  scheduled_at TEXT NOT NULL,
  repeat_type TEXT NOT NULL CHECK (repeat_type IN ('none', 'daily', 'weekly')),
  status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'completed', 'missed', 'stopped')),
  created_at TEXT NOT NULL,
  completed_at TEXT,
  source_reminder_id INTEGER UNIQUE REFERENCES reminders(id) ON DELETE SET NULL,
  loop_enabled INTEGER NOT NULL DEFAULT 0 CHECK (loop_enabled IN (0, 1)),
  loop_interval_minutes INTEGER,
  loop_start_time TEXT,
  loop_end_time TEXT,
  CHECK (
    (loop_enabled = 0 AND loop_interval_minutes IS NULL AND loop_start_time IS NULL AND loop_end_time IS NULL)
    OR
    (loop_enabled = 1 AND loop_interval_minutes > 0 AND loop_start_time IS NOT NULL AND loop_end_time IS NOT NULL)
  )
);
CREATE INDEX IF NOT EXISTS idx_reminders_pending_time
  ON reminders(status, scheduled_at);
";

const MIGRATE_REMINDERS_SCHEMA: &str = "
CREATE TABLE reminders_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  content TEXT NOT NULL,
  scheduled_at TEXT NOT NULL,
  repeat_type TEXT NOT NULL CHECK (repeat_type IN ('none', 'daily', 'weekly')),
  status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'completed', 'missed', 'stopped')),
  created_at TEXT NOT NULL,
  completed_at TEXT,
  source_reminder_id INTEGER UNIQUE REFERENCES reminders_new(id) ON DELETE SET NULL,
  loop_enabled INTEGER NOT NULL DEFAULT 0 CHECK (loop_enabled IN (0, 1)),
  loop_interval_minutes INTEGER,
  loop_start_time TEXT,
  loop_end_time TEXT,
  CHECK (
    (loop_enabled = 0 AND loop_interval_minutes IS NULL AND loop_start_time IS NULL AND loop_end_time IS NULL)
    OR
    (loop_enabled = 1 AND loop_interval_minutes > 0 AND loop_start_time IS NOT NULL AND loop_end_time IS NOT NULL)
  )
);
INSERT INTO reminders_new (
  id, content, scheduled_at, repeat_type, status, created_at, completed_at, source_reminder_id,
  loop_enabled, loop_interval_minutes, loop_start_time, loop_end_time
)
SELECT
  id, content, scheduled_at, repeat_type, status, created_at, completed_at, source_reminder_id,
  loop_enabled, loop_interval_minutes, loop_start_time, loop_end_time
FROM reminders;
DROP TABLE reminders;
ALTER TABLE reminders_new RENAME TO reminders;
CREATE INDEX idx_reminders_pending_time
  ON reminders(status, scheduled_at);
";

const MIGRATE_LEGACY_REMINDERS_SCHEMA: &str = "
CREATE TABLE reminders_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  content TEXT NOT NULL,
  scheduled_at TEXT NOT NULL,
  repeat_type TEXT NOT NULL CHECK (repeat_type IN ('none', 'daily', 'weekly')),
  status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'completed', 'missed', 'stopped')),
  created_at TEXT NOT NULL,
  completed_at TEXT,
  source_reminder_id INTEGER UNIQUE REFERENCES reminders_new(id) ON DELETE SET NULL,
  loop_enabled INTEGER NOT NULL DEFAULT 0 CHECK (loop_enabled IN (0, 1)),
  loop_interval_minutes INTEGER,
  loop_start_time TEXT,
  loop_end_time TEXT,
  CHECK (
    (loop_enabled = 0 AND loop_interval_minutes IS NULL AND loop_start_time IS NULL AND loop_end_time IS NULL)
    OR
    (loop_enabled = 1 AND loop_interval_minutes > 0 AND loop_start_time IS NOT NULL AND loop_end_time IS NOT NULL)
  )
);
INSERT INTO reminders_new (
  id, content, scheduled_at, repeat_type, status, created_at, completed_at, source_reminder_id
)
SELECT
  id, content, scheduled_at, repeat_type, status, created_at, completed_at, source_reminder_id
FROM reminders;
DROP TABLE reminders;
ALTER TABLE reminders_new RENAME TO reminders;
CREATE INDEX idx_reminders_pending_time
  ON reminders(status, scheduled_at);
";

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("数据库操作失败：{0}")]
    Database(#[from] rusqlite::Error),
    #[error("日期时间格式错误：{0}")]
    DateTime(#[from] chrono::ParseError),
    #[error("数据库中的提醒数据无效：{0}")]
    InvalidData(String),
    #[error("无法访问提醒数据库，请稍后重试")]
    DatabaseLock,
}

pub struct ReminderRepository {
    conn: Mutex<Connection>,
}

impl ReminderRepository {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        Self::from_connection(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self, AppError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    pub fn insert_pending(
        &self,
        content: &str,
        scheduled_at: DateTime<Local>,
        repeat_type: RepeatType,
        loop_config: Option<LoopConfig>,
    ) -> Result<Reminder, AppError> {
        let conn = self.connection()?;
        let created_at = Local::now();
        let status = if loop_config.is_some() {
            ReminderStatus::Running
        } else {
            ReminderStatus::Pending
        };
        conn.execute(
            "INSERT INTO reminders (
                content, scheduled_at, repeat_type, status, created_at,
                loop_enabled, loop_interval_minutes, loop_start_time, loop_end_time
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                content,
                scheduled_at.to_rfc3339(),
                repeat_type.as_str(),
                status.as_str(),
                created_at.to_rfc3339(),
                loop_config.is_some(),
                loop_config.as_ref().map(|config| config.interval_minutes),
                loop_config.as_ref().map(|config| &config.start_time),
                loop_config.as_ref().map(|config| &config.end_time),
            ],
        )?;
        load_reminder(&conn, conn.last_insert_rowid())?
            .ok_or_else(|| AppError::InvalidData("新增提醒后无法读取已保存的数据".to_string()))
    }

    pub fn list(&self, include_history: bool) -> Result<Vec<Reminder>, AppError> {
        let conn = self.connection()?;
        let sql = if include_history {
            "SELECT id, content, scheduled_at, repeat_type, status, created_at,
                    completed_at, source_reminder_id, loop_enabled,
                    loop_interval_minutes, loop_start_time, loop_end_time
             FROM reminders"
        } else {
            "SELECT id, content, scheduled_at, repeat_type, status, created_at,
                    completed_at, source_reminder_id, loop_enabled,
                    loop_interval_minutes, loop_start_time, loop_end_time
             FROM reminders
             WHERE status IN ('pending', 'running', 'stopped')"
        };
        let mut statement = conn.prepare(sql)?;
        let rows = statement.query_map([], StoredReminder::from_row)?;
        let mut reminders = rows
            .map(|row| row.map_err(AppError::from).and_then(Reminder::try_from))
            .collect::<Result<Vec<_>, _>>()?;
        reminders.sort_by_key(|reminder| reminder.scheduled_at);
        Ok(reminders)
    }

    pub fn delete(&self, id: i64) -> Result<(), AppError> {
        self.connection()?
            .execute("DELETE FROM reminders WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn update_pending(
        &self,
        id: i64,
        content: &str,
        scheduled_at: DateTime<Local>,
        repeat_type: RepeatType,
        loop_config: Option<LoopConfig>,
    ) -> Result<Option<Reminder>, AppError> {
        let conn = self.connection()?;
        let Some(existing) = load_reminder(&conn, id)? else {
            return Ok(None);
        };
        if !matches!(
            existing.status,
            ReminderStatus::Pending | ReminderStatus::Running | ReminderStatus::Stopped
        ) {
            return Ok(None);
        }
        let status = match (existing.status, loop_config.is_some()) {
            (ReminderStatus::Stopped, true) => ReminderStatus::Stopped,
            (_, true) => ReminderStatus::Running,
            (_, false) => ReminderStatus::Pending,
        };
        conn.execute(
            "UPDATE reminders
             SET content = ?1, scheduled_at = ?2, repeat_type = ?3, status = ?4,
                 completed_at = NULL, loop_enabled = ?5, loop_interval_minutes = ?6,
                 loop_start_time = ?7, loop_end_time = ?8
             WHERE id = ?9",
            params![
                content,
                scheduled_at.to_rfc3339(),
                repeat_type.as_str(),
                status.as_str(),
                loop_config.is_some(),
                loop_config.as_ref().map(|config| config.interval_minutes),
                loop_config.as_ref().map(|config| &config.start_time),
                loop_config.as_ref().map(|config| &config.end_time),
                id,
            ],
        )?;
        load_reminder(&conn, id)
    }

    pub fn pending(&self) -> Result<Vec<Reminder>, AppError> {
        Ok(self
            .list(false)?
            .into_iter()
            .filter(|reminder| reminder.status != ReminderStatus::Stopped)
            .collect())
    }

    pub fn get(&self, id: i64) -> Result<Option<Reminder>, AppError> {
        load_reminder(&*self.connection()?, id)
    }

    pub fn mark_completed(
        &self,
        id: i64,
        at: DateTime<Local>,
    ) -> Result<Option<Reminder>, AppError> {
        self.transition(id, at, ReminderStatus::Completed)
    }

    pub fn mark_missed(&self, id: i64, at: DateTime<Local>) -> Result<Option<Reminder>, AppError> {
        self.finish_occurrence(id, at, ReminderStatus::Missed)
    }

    pub fn finish_animation(
        &self,
        id: i64,
        at: DateTime<Local>,
    ) -> Result<Option<Reminder>, AppError> {
        self.finish_occurrence(id, at, ReminderStatus::Completed)
    }

    pub fn stop_loop(&self, id: i64, at: DateTime<Local>) -> Result<bool, AppError> {
        Ok(self.connection()?.execute(
            "UPDATE reminders
             SET status = 'stopped', completed_at = ?1
             WHERE id = ?2 AND loop_enabled = 1 AND status = 'running'",
            params![at.to_rfc3339(), id],
        )? > 0)
    }

    pub fn resume_loop(&self, id: i64, now: DateTime<Local>) -> Result<bool, AppError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        let Some(reminder) = load_reminder(&tx, id)? else {
            tx.commit()?;
            return Ok(false);
        };
        if !reminder.loop_enabled || reminder.status != ReminderStatus::Stopped {
            tx.commit()?;
            return Ok(false);
        }
        let Some(scheduled_at) = next_loop_occurrence(&reminder, now) else {
            tx.commit()?;
            return Ok(false);
        };
        let changed = tx.execute(
            "UPDATE reminders
             SET status = 'running', scheduled_at = ?1, completed_at = NULL
             WHERE id = ?2 AND status = 'stopped'",
            params![scheduled_at.to_rfc3339(), id],
        )? > 0;
        tx.commit()?;
        Ok(changed)
    }

    pub fn health_check(&self) -> Result<DatabaseHealth, AppError> {
        let conn = self.connection()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let mut quick_check = conn.prepare("PRAGMA quick_check")?;
        let messages = quick_check
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let quick_check_ok = messages.iter().all(|message| message == "ok");
        let quick_check_message = if messages.is_empty() {
            "没有返回结果".to_string()
        } else {
            messages.join("; ")
        };
        drop(quick_check);

        let mut foreign_key_check = conn.prepare("PRAGMA foreign_key_check(reminders)")?;
        let foreign_key_issues = foreign_key_check
            .query_map([], |_| Ok(()))?
            .collect::<Result<Vec<_>, _>>()?
            .len();
        drop(foreign_key_check);

        let counts = conn.query_row(
            "SELECT
                COUNT(*),
                COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'missed' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN status = 'stopped' THEN 1 ELSE 0 END), 0)
             FROM reminders",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )?;

        Ok(DatabaseHealth {
            quick_check_ok,
            quick_check_message,
            foreign_key_issues,
            total: counts.0,
            pending: counts.1,
            running: counts.2,
            completed: counts.3,
            missed: counts.4,
            stopped: counts.5,
        })
    }

    pub fn repair_lightly(&self) -> Result<(), AppError> {
        self.connection()?.execute_batch(
            "
            PRAGMA optimize;
            REINDEX;
            VACUUM;
            ",
        )?;
        Ok(())
    }

    fn from_connection(mut conn: Connection) -> Result<Self, AppError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        if reminders_table_exists(&conn)? {
            if !has_loop_columns(&conn)? {
                migrate_reminders_schema(&mut conn, MIGRATE_LEGACY_REMINDERS_SCHEMA)?;
            } else if !source_reminder_delete_sets_null(&conn)? || !status_supports_running(&conn)?
            {
                migrate_reminders_schema(&mut conn, MIGRATE_REMINDERS_SCHEMA)?;
            }
        } else {
            conn.execute_batch(SCHEMA)?;
        }
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        self.conn.lock().map_err(|_| AppError::DatabaseLock)
    }

    fn transition(
        &self,
        id: i64,
        at: DateTime<Local>,
        status: ReminderStatus,
    ) -> Result<Option<Reminder>, AppError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "UPDATE reminders
             SET status = ?1, completed_at = ?2
             WHERE id = ?3 AND status = 'pending'",
            params![status.as_str(), at.to_rfc3339(), id],
        )?;

        if changed == 0 {
            tx.commit()?;
            return Ok(None);
        }

        let reminder = load_reminder(&tx, id)?
            .ok_or_else(|| AppError::InvalidData(format!("状态更新后找不到提醒 {id}")))?;
        insert_next_occurrence(&tx, &reminder, at)?;
        tx.commit()?;
        Ok(Some(reminder))
    }

    fn finish_occurrence(
        &self,
        id: i64,
        at: DateTime<Local>,
        normal_status: ReminderStatus,
    ) -> Result<Option<Reminder>, AppError> {
        let reminder = self.get(id)?;
        if reminder
            .as_ref()
            .is_some_and(|reminder| reminder.loop_enabled)
        {
            self.advance_loop(id, at)
        } else {
            self.transition(id, at, normal_status)
        }
    }

    fn advance_loop(&self, id: i64, at: DateTime<Local>) -> Result<Option<Reminder>, AppError> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;
        let Some(reminder) = load_reminder(&tx, id)? else {
            tx.commit()?;
            return Ok(None);
        };
        if reminder.status != ReminderStatus::Running {
            tx.commit()?;
            return Ok(None);
        }
        if let Some(next) = next_loop_occurrence(&reminder, at) {
            tx.execute(
                "UPDATE reminders SET scheduled_at = ?1 WHERE id = ?2 AND status = 'running'",
                params![next.to_rfc3339(), id],
            )?;
        } else {
            tx.execute(
                "UPDATE reminders
                 SET status = 'completed', completed_at = ?1
                 WHERE id = ?2 AND status = 'running'",
                params![at.to_rfc3339(), id],
            )?;
        }
        let updated = load_reminder(&tx, id)?;
        tx.commit()?;
        Ok(updated)
    }
}

fn reminders_table_exists(conn: &Connection) -> Result<bool, AppError> {
    Ok(conn.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table' AND name = 'reminders'
         )",
        [],
        |row| row.get(0),
    )?)
}

fn source_reminder_delete_sets_null(conn: &Connection) -> Result<bool, AppError> {
    let mut statement = conn.prepare("PRAGMA foreign_key_list(reminders)")?;
    let foreign_keys = statement.query_map([], |row| {
        Ok((row.get::<_, String>(3)?, row.get::<_, String>(6)?))
    })?;
    for foreign_key in foreign_keys {
        let (from, on_delete) = foreign_key?;
        if from == "source_reminder_id" && on_delete.eq_ignore_ascii_case("SET NULL") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_loop_columns(conn: &Connection) -> Result<bool, AppError> {
    let mut statement = conn.prepare("PRAGMA table_info(reminders)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == "loop_enabled" {
            return Ok(true);
        }
    }
    Ok(false)
}

fn status_supports_running(conn: &Connection) -> Result<bool, AppError> {
    let schema = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'reminders'",
        [],
        |row| row.get::<_, String>(0),
    )?;
    Ok(schema.contains("'running'") && schema.contains("'stopped'"))
}

fn migrate_reminders_schema(conn: &mut Connection, schema: &str) -> Result<(), AppError> {
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let result = migrate_reminders_schema_without_foreign_keys(conn, schema);
    let enable_result = conn.pragma_update(None, "foreign_keys", "ON");
    result?;
    enable_result?;
    Ok(())
}

fn migrate_reminders_schema_without_foreign_keys(
    conn: &mut Connection,
    schema: &str,
) -> Result<(), AppError> {
    let tx = conn.transaction()?;
    let previous_sequence = tx
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'reminders'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    tx.execute_batch(schema)?;

    let mut foreign_key_check = tx.prepare("PRAGMA foreign_key_check(reminders)")?;
    if foreign_key_check.exists([])? {
        return Err(AppError::InvalidData(
            "升级提醒数据库时发现无效的重复提醒关联，原数据未被修改".to_string(),
        ));
    }
    drop(foreign_key_check);

    let maximum_id = tx.query_row("SELECT COALESCE(MAX(id), 0) FROM reminders", [], |row| {
        row.get::<_, i64>(0)
    })?;
    let sequence = previous_sequence.max(maximum_id);
    tx.execute(
        "DELETE FROM sqlite_sequence WHERE name IN ('reminders', 'reminders_new')",
        [],
    )?;
    if sequence > 0 {
        tx.execute(
            "INSERT INTO sqlite_sequence(name, seq) VALUES ('reminders', ?1)",
            [sequence],
        )?;
    }

    tx.commit()?;
    Ok(())
}

fn insert_next_occurrence(
    tx: &Transaction<'_>,
    reminder: &Reminder,
    created_at: DateTime<Local>,
) -> Result<(), AppError> {
    let Some(scheduled_at) = next_occurrence(reminder.scheduled_at, reminder.repeat_type) else {
        return Ok(());
    };
    tx.execute(
        "INSERT OR IGNORE INTO reminders (
            content, scheduled_at, repeat_type, status, created_at, source_reminder_id
         ) VALUES (?1, ?2, ?3, 'pending', ?4, ?5)",
        params![
            reminder.content,
            scheduled_at.to_rfc3339(),
            reminder.repeat_type.as_str(),
            created_at.to_rfc3339(),
            reminder.id
        ],
    )?;
    Ok(())
}

fn load_reminder(conn: &Connection, id: i64) -> Result<Option<Reminder>, AppError> {
    let stored = conn
        .query_row(
            "SELECT id, content, scheduled_at, repeat_type, status, created_at,
                    completed_at, source_reminder_id, loop_enabled,
                    loop_interval_minutes, loop_start_time, loop_end_time
             FROM reminders
             WHERE id = ?1",
            [id],
            StoredReminder::from_row,
        )
        .optional()?;
    stored.map(Reminder::try_from).transpose()
}

struct StoredReminder {
    id: i64,
    content: String,
    scheduled_at: String,
    repeat_type: String,
    status: String,
    created_at: String,
    completed_at: Option<String>,
    source_reminder_id: Option<i64>,
    loop_enabled: bool,
    loop_interval_minutes: Option<i64>,
    loop_start_time: Option<String>,
    loop_end_time: Option<String>,
}

impl StoredReminder {
    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            content: row.get(1)?,
            scheduled_at: row.get(2)?,
            repeat_type: row.get(3)?,
            status: row.get(4)?,
            created_at: row.get(5)?,
            completed_at: row.get(6)?,
            source_reminder_id: row.get(7)?,
            loop_enabled: row.get(8)?,
            loop_interval_minutes: row.get(9)?,
            loop_start_time: row.get(10)?,
            loop_end_time: row.get(11)?,
        })
    }
}

impl TryFrom<StoredReminder> for Reminder {
    type Error = AppError;

    fn try_from(stored: StoredReminder) -> Result<Self, Self::Error> {
        Ok(Self {
            id: stored.id,
            content: stored.content,
            scheduled_at: parse_local_datetime(&stored.scheduled_at)?,
            repeat_type: RepeatType::parse(&stored.repeat_type)?,
            status: ReminderStatus::parse(&stored.status)?,
            created_at: parse_local_datetime(&stored.created_at)?,
            completed_at: stored
                .completed_at
                .as_deref()
                .map(parse_local_datetime)
                .transpose()?,
            source_reminder_id: stored.source_reminder_id,
            loop_enabled: stored.loop_enabled,
            loop_interval_minutes: stored.loop_interval_minutes,
            loop_start_time: stored.loop_start_time,
            loop_end_time: stored.loop_end_time,
        })
    }
}

fn parse_local_datetime(value: &str) -> Result<DateTime<Local>, AppError> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Local))
}

impl RepeatType {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
        }
    }

    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "none" => Ok(Self::None),
            "daily" => Ok(Self::Daily),
            "weekly" => Ok(Self::Weekly),
            _ => Err(AppError::InvalidData(format!("未知重复类型：{value}"))),
        }
    }
}

impl ReminderStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Missed => "missed",
            Self::Stopped => "stopped",
        }
    }

    fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "missed" => Ok(Self::Missed),
            "stopped" => Ok(Self::Stopped),
            _ => Err(AppError::InvalidData(format!("未知提醒状态：{value}"))),
        }
    }
}
