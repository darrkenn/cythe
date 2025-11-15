use rusqlite::Connection;

use crate::database::PipelineEntry;

pub fn create_pipeline_entry(
    pipeline_entry: PipelineEntry,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open("/var/lib/cythe/cythe.db")?;
    let fixed_name = pipeline_entry.name.replace("/", "_");

    let stmt = format!(
        "INSERT INTO \"{}\" (logs,failed,date) VALUES (?1, ?2, ?3)",
        fixed_name
    );
    conn.execute(
        &stmt,
        (
            pipeline_entry.logs,
            pipeline_entry.failed,
            pipeline_entry.date,
        ),
    )?;

    Ok(())
}

pub fn get_latest_entry(
    repo_full_name: String,
) -> Result<PipelineEntry, Box<dyn std::error::Error>> {
    let conn = Connection::open("/var/lib/cythe/cythe.db")?;
    let fixed_name = repo_full_name.replace("/", "_");
    let stmt = format!(
        "SELECT id, logs, failed, date FROM \"{}\" ORDER BY id DESC LIMIT 1",
        fixed_name
    );

    let pipeline_entry = conn.query_one(&stmt, [], |row| {
        Ok(PipelineEntry {
            id: row.get(0)?,
            name: repo_full_name.to_string(),
            logs: row.get(1)?,
            failed: row.get(2)?,
            date: row.get(3)?,
        })
    })?;
    Ok(pipeline_entry)
}
