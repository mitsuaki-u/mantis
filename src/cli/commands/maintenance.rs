use clap::{Args, Subcommand};
use crate::core::error::Error;
use crate::infra::db::Database;
use crate::config::Config;
use log::{info, error, warn, debug};
use colored::Colorize;

#[derive(Args, Debug)]
pub struct MaintenanceArgs {
    #[command(subcommand)]
    command: MaintenanceCommand,
}

#[derive(Subcommand, Debug)]
pub enum MaintenanceCommand {
    /// Perform maintenance on the database
    Db {
        /// Run VACUUM to reclaim space
        #[arg(long)]
        vacuum: bool,
        
        /// Run ANALYZE to update statistics
        #[arg(long)]
        analyze: bool,
        
        /// Run integrity check on the database
        #[arg(long)]
        check_integrity: bool,
        
        /// Run a full maintenance cycle (vacuum, analyze, integrity check)
        #[arg(long)]
        full: bool,
        
        /// Fix any detected issues with the database schema
        #[arg(long)]
        fix_schema: bool,
        
        /// Reset the database (DANGER: this will delete all data)
        #[arg(long)]
        reset: bool,
    },
}

pub async fn handle_maintenance_command(args: MaintenanceArgs, config: Config) -> Result<(), Error> {
    match args.command {
        MaintenanceCommand::Db { vacuum, analyze, check_integrity, full, fix_schema, reset } => {
            perform_db_maintenance(vacuum, analyze, check_integrity, full, fix_schema, reset, &config).await
        }
    }
}

/// Performs database maintenance operations
async fn perform_db_maintenance(
    vacuum: bool, 
    analyze: bool, 
    check_integrity: bool, 
    full: bool, 
    fix_schema: bool,
    reset: bool,
    config: &Config,
) -> Result<(), Error> {
    // Check if the reset flag is set, and handle it separately
    if reset {
        println!("⚠️ {} This will delete all your data!", "WARNING: DATABASE RESET REQUESTED.".bright_red());
        println!("All positions, trades, and settings will be lost.");
        
        // Confirm with the user
        println!("\nDo you want to continue? Type 'yes' to confirm:");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        
        if input.trim().to_lowercase() == "yes" {
            println!("{}", "Resetting database...".bright_red());
            if let Err(e) = Database::reset_database() {
                error!("Failed to reset database: {}", e);
                return Err(e);
            }
            println!("✅ Database has been reset and initialized with a fresh schema");
            return Ok(());
        } else {
            println!("Database reset cancelled.");
            return Ok(());
        }
    }
    
    let db_path = Config::db_path(config)?;
    
    println!("🛠️ {} on database at {}", "Performing maintenance".bright_green(), db_path.display());
    
    // Create database connection
    let db = Database::new_with_path(db_path, false)?;
    
    // Flag to track if we've performed any operations
    let mut performed_operations = false;
    
    // Ensure the database is initialized
    db.ensure_initialized()?;
    println!("✅ Database initialization check passed");
    
    // Check database file limits
    db.check_file_limits()?;
    
    // Perform schema validation if requested
    if fix_schema {
        println!("🔍 Validating and fixing database schema...");
        db.check_database_structure()?;
        performed_operations = true;
        println!("✅ Schema validation completed");
    }
    
    // Get a connection for other maintenance tasks
    let mut conn = db.get_connection()?;
    
    // Run VACUUM if requested or if full maintenance
    if vacuum || full {
        println!("🧹 Running VACUUM to defragment database and reclaim space...");
        conn.execute_batch("VACUUM")?;
        performed_operations = true;
        println!("✅ VACUUM completed");
    }
    
    // Run ANALYZE if requested or if full maintenance
    if analyze || full {
        println!("📊 Running ANALYZE to update optimization statistics...");
        conn.execute_batch("ANALYZE")?;
        performed_operations = true;
        println!("✅ ANALYZE completed");
    }
    
    // Run integrity check if requested or if full maintenance
    if check_integrity || full {
        println!("🔍 Running integrity check on database...");
        let integrity_check: String = conn.query_row(
            "PRAGMA integrity_check",
            [],
            |row| row.get(0)
        )?;
        
        if integrity_check == "ok" {
            println!("✅ Integrity check passed");
        } else {
            println!("❌ {} integrity issues found: {}", "WARNING:".bright_red(), integrity_check);
            
            // If there are issues, recommend additional steps
            println!("\nRecommended steps:");
            println!("1. Make a backup of your database");
            println!("2. Try exporting your data");
            println!("3. If possible, create a new database and import your data");
            println!("4. As a last resort, you can reset the database with --reset");
        }
        performed_operations = true;
    }
    
    // If no specific operations were requested, show help
    if !performed_operations {
        println!("\n{}", "No maintenance operations specified.".yellow());
        println!("Try running with --full for complete maintenance, or specify:");
        println!("  --vacuum: Defragment database and reclaim space");
        println!("  --analyze: Update database statistics for query optimization");
        println!("  --check-integrity: Verify database integrity");
        println!("  --fix-schema: Validate and fix database schema issues");
        println!("  --reset: Reset the database (DANGER: this will delete all data)");
    } else {
        println!("\n✅ Database maintenance completed successfully");
    }
    
    Ok(())
} 