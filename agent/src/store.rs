use crate::workspace::Document;
use anyhow::Result;
use rusqlite::{Connection, params};
use serde::{Serialize, de::DeserializeOwned};
use std::path::Path;

pub struct Store {
    conn: std::sync::Mutex<Connection>,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = crate::storage::open(path)?.0;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }
    pub fn save<T: Serialize>(&self, id: &str, state: &T) -> Result<()> {
        self.conn.lock().unwrap().execute("INSERT INTO sessions VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET updated=excluded.updated,payload=excluded.payload", params![id,chrono::Utc::now().to_rfc3339(),serde_json::to_string(state)?])?;
        Ok(())
    }
    pub fn load<T: DeserializeOwned>(&self, id: &str) -> Result<T> {
        let text: String = self.conn.lock().unwrap().query_row(
            "SELECT payload FROM sessions WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&text)?)
    }
    pub fn sessions(&self) -> Result<Vec<(String, String)>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .prepare("SELECT id,updated FROM sessions ORDER BY updated DESC")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub fn begin_call(&self, session: &str, id: &str, name: &str, args: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("INSERT INTO ledger(session,call_id,name,arguments,status) VALUES (?1,?2,?3,?4,'started')",params![session,id,name,args])?;
        Ok(())
    }
    pub fn finish_call(&self, session: &str, id: &str, status: &str, result: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE ledger SET status=?1,result=?2 WHERE session=?3 AND call_id=?4",
            params![status, result, session, id],
        )?;
        Ok(())
    }
    pub fn call_result(&self, session: &str, id: &str) -> Result<Option<(String, Option<String>)>> {
        use rusqlite::OptionalExtension;
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT status,result FROM ledger WHERE session=?1 AND call_id=?2",
                params![session, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }
    pub fn reindex(&mut self, docs: &[Document]) -> Result<()> {
        let mut connection = self.conn.lock().unwrap();
        let tx = connection.transaction()?;
        tx.execute("DELETE FROM documents", [])?;
        for d in docs {
            tx.execute(
                "INSERT INTO documents VALUES (?1,?2,?3,?4,?5)",
                params![d.path, d.title, d.kind, d.body, d.hash],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn search(&self, query: &str) -> Result<Vec<String>> {
        // instr preserves Chinese and code identifiers without a whitespace tokenizer.
        Ok(self.conn.lock().unwrap().prepare("SELECT path FROM documents WHERE instr(lower(title || char(10) || body || char(10) || path),lower(?1))>0 ORDER BY path LIMIT 100")?.query_map([query],|r|r.get(0))?.collect::<std::result::Result<Vec<_>,_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_call_is_durable_and_not_reinsertable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime.db");
        {
            let s = Store::open(&path).unwrap();
            s.begin_call("s", "c", "external", "{}").unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(
            s.call_result("s", "c").unwrap(),
            Some(("started".into(), None))
        );
        assert!(s.begin_call("s", "c", "external", "{}").is_err());
    }
}
