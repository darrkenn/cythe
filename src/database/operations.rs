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
