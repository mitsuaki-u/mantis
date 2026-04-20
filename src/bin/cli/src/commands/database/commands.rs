use crate::config::Config;
use crate::error::Error;
use crate::infrastructure::database::Database;
use clap::Parser;
use log::info;
use std::io::{self, Write};

#[derive(Parser)]
pub enum DatabaseCommands {
    /// Reset the database - deletes all data and recreates with latest schema
    ///
    /// WARNING: This is destructive and will delete all trading history,
    /// positions, and market data. A confirmation prompt will be shown.
    ///
    /// Example: mantis database reset
    Reset,

    /// Display the current database schema
    ///
    /// Shows the structure of all tables, columns, and indexes.
    ///
    /// Examples:
    ///   mantis database schema
    ///   mantis database schema --output schema.sql
    Schema {
        /// Write schema to a file instead of displaying it
        #[arg(short, long)]
        output: Option<String>,
    },
}

/// Handle database management commands
pub async fn handle_database_command(
    command: DatabaseCommands,
    config: Config,
    _db: Database,
) -> Result<(), Error> {
    match command {
        DatabaseCommands::Reset => {
            println!(
                "WARNING: This will delete all data in the database '{}'.",
                config.database.dbname
            );
            print!("Are you sure you want to proceed? (yes/no): ");
            io::stdout().flush()?; // Make sure the prompt is shown before reading

            let mut confirmation = String::new();
            io::stdin().read_line(&mut confirmation)?;

            if confirmation.trim().eq_ignore_ascii_case("yes") {
                info!("Starting database reset...");
                Database::reset_database(&config).await?; // Use async reset
                info!("✅ Database reset complete.");
            } else {
                info!("Database reset cancelled.");
            }
        }
        DatabaseCommands::Schema { output } => {
            println!("Schema display not implemented for PostgreSQL.");
            println!("Use: psql -d mantis -c '\\d+' to view schema");
            if let Some(_path) = output {
                println!("To export schema: pg_dump --schema-only mantis > schema.sql");
            }
        }
    }
    Ok(())
}
