mod operations;
use rusqlite::Connection;

pub use operations::create_pipeline_entry;
pub use operations::get_latest_entry;

pub struct PipelineEntry {
    pub id: Option<u32>,
    pub name: String,
    pub logs: String,
    pub failed: bool,
    pub date: String,
}

impl PipelineEntry {
    pub fn new(name: String, logs: String, failed: bool, date: String) -> PipelineEntry {
        PipelineEntry {
            id: None,
            name,
            logs,
            failed,
            date,
        }
    }
}

pub fn create_tables(allowed_repos: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open("/var/lib/cythe/cythe.db")?;
    for repo in allowed_repos {
        let fixed_name = repo.replace("/", "_");
        let stmt = format!(
            "CREATE TABLE IF NOT EXISTS \"{}\" (
            id INTEGER PRIMARY KEY,
            logs TEXT,
            failed INTEGER,
            date TEXT
        )",
            fixed_name
        );
        conn.execute(&stmt, [])?;
        println!("Created table: {}", fixed_name);
    }
    Ok(())
}
