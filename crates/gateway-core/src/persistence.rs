use crate::{domain::*, ports::Persistence};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct LocalEventStore {
    connection: Connection,
    pub scope: String,
}
impl LocalEventStore {
    pub fn open(path: &Path) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS commands(id TEXT PRIMARY KEY, phase TEXT NOT NULL, simulation INTEGER NOT NULL, scope TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS outbox(id TEXT PRIMARY KEY, command_id TEXT NOT NULL, payload TEXT NOT NULL, scope TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS journal(id TEXT PRIMARY KEY, payload TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
        Ok(Self {
            connection,
            scope: String::new(),
        })
    }
    pub fn settings(&self) -> Result<Settings> {
        let text: Option<String> = self
            .connection
            .query_row("SELECT value FROM metadata WHERE key='settings'", [], |r| {
                r.get(0)
            })
            .optional()?;
        match text {
            Some(s) => serde_json::from_str(&s).map_err(|_| Error::Config("corrupt settings")),
            None => Ok(Settings::default()),
        }
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        self.put(
            "settings",
            &serde_json::to_string(settings).map_err(|_| Error::Config("settings"))?,
        )
    }
    pub fn put(&self, key: &str, value: &str) -> Result<()> {
        self.connection.execute("INSERT INTO metadata VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,value])?;
        Ok(())
    }
    pub fn inspection(&self) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM metadata WHERE key='inspection'",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .as_deref()
            == Some("true"))
    }
    pub fn history(&self) -> Result<Vec<CommandExecutionResult>> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM journal ORDER BY rowid DESC LIMIT 100")?;
        let mut events = vec![];
        for row in statement.query_map([], |r| r.get::<_, String>(0))? {
            events.push(serde_json::from_str(&row?).map_err(|_| Error::Config("corrupt journal"))?);
        }
        Ok(events)
    }
    pub fn outbox(&mut self, scope: &str) -> Result<Vec<CommandExecutionResult>> {
        let mut statement = self
            .connection
            .prepare("SELECT payload FROM outbox WHERE scope=?1 ORDER BY rowid LIMIT 32")?;
        let mut events = Vec::new();
        for row in statement.query_map([scope], |r| r.get::<_, String>(0))? {
            events.push(serde_json::from_str(&row?).map_err(|_| Error::Config("corrupt outbox"))?);
        }
        Ok(events)
    }
    pub fn delivered(&mut self, event_id: uuid::Uuid) -> Result<()> {
        self.connection
            .execute("DELETE FROM outbox WHERE id=?1", [event_id.to_string()])?;
        Ok(())
    }
}
impl Persistence for LocalEventStore {
    fn receive(&mut self, id: CommandId, simulation: bool) -> Result<bool> {
        let count: u64 = self
            .connection
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))?;
        if count >= 10000 {
            return Err(Error::Safety("outbox full, intake stopped"));
        }
        Ok(self.connection.execute(
            "INSERT OR IGNORE INTO commands VALUES(?1,'RECEIVED',?2,?3)",
            params![id.to_string(), simulation, self.scope],
        )? == 1)
    }
    fn record(&mut self, event: &CommandExecutionResult, simulation: bool) -> Result<()> {
        let payload = serde_json::to_string(event).map_err(|_| Error::Config("event encoding"))?;
        let transaction = self.connection.transaction()?;
        let scope: String = transaction.query_row(
            "SELECT scope FROM commands WHERE id=?1",
            [event.command_id.to_string()],
            |r| r.get(0),
        )?;
        transaction.execute(
            "UPDATE commands SET phase=?2 WHERE id=?1",
            params![event.command_id.to_string(), event.kind],
        )?;
        transaction.execute(
            "INSERT INTO journal VALUES(?1,?2)",
            params![event.event_id.to_string(), payload],
        )?;
        transaction.execute("DELETE FROM journal WHERE rowid NOT IN (SELECT rowid FROM journal ORDER BY rowid DESC LIMIT 2000)",[])?;
        if !simulation {
            transaction.execute(
                "INSERT INTO outbox VALUES(?1,?2,?3,?4)",
                params![
                    event.event_id.to_string(),
                    event.command_id.to_string(),
                    payload,
                    scope
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
    fn recover(&mut self) -> Result<Vec<CommandExecutionResult>> {
        let ids = {
            let mut statement = self.connection.prepare("SELECT id,simulation FROM commands WHERE phase NOT IN ('ARRIVED','FAULT','NEEDS_INSPECTION','REJECTED')")?;
            let rows =
                statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut recovered = vec![];
        for (id, simulation) in ids {
            let id = uuid::Uuid::parse_str(&id).map_err(|_| Error::Config("corrupt command id"))?;
            let event = CommandExecutionResult::new(id, "NEEDS_INSPECTION");
            self.put("inspection", "true")?;
            self.record(&event, simulation)?;
            recovered.push(event);
        }
        Ok(recovered)
    }
}
