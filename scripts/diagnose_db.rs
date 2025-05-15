use std::path::PathBuf;
use rusqlite::Connection;
use std::time::{Duration, Instant};
use std::thread;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("SQLite Database Diagnostics Tool");
    println!("--------------------------------");
    
    // Determine the path to the database file
    let app_dirs = directories::ProjectDirs::from("com", "honeybadger", "honeybadger")
        .ok_or_else(|| "Could not get application data directory".to_string())?;
    
    let data_dir = app_dirs.data_dir();
    let db_path = data_dir.join("trading_history.db");
    
    println!("Database path: {:?}", db_path);
    
    // Run basic diagnostics
    if !db_path.exists() {
        println!("Database file does not exist!");
        return Ok(());
    }
    
    println!("Database size: {} bytes", std::fs::metadata(&db_path)?.len());
    
    // Check for WAL and SHM files
    let wal_path = db_path.with_extension("db-wal");
    let shm_path = db_path.with_extension("db-shm");
    
    println!("WAL file exists: {}", wal_path.exists());
    if wal_path.exists() {
        println!("WAL file size: {} bytes", std::fs::metadata(&wal_path)?.len());
    }
    
    println!("SHM file exists: {}", shm_path.exists());
    if shm_path.exists() {
        println!("SHM file size: {} bytes", std::fs::metadata(&shm_path)?.len());
    }
    
    // Check database integrity
    check_integrity(&db_path)?;
    
    // Check database settings
    check_settings(&db_path)?;
    
    // Analyze database tables
    analyze_tables(&db_path)?;
    
    // Check if tables have proper indexes
    check_indexes(&db_path)?;
    
    // Check if database is locked
    test_concurrent_access(&db_path)?;
    
    println!("\nDiagnostics complete");
    println!("------------------");
    
    Ok(())
}

fn check_integrity(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nChecking database integrity...");
    
    let conn = Connection::open(db_path)?;
    
    // Run integrity check
    let integrity_check: String = conn.pragma_query_value(None, "integrity_check", |row| row.get(0))?;
    
    if integrity_check == "ok" {
        println!("✅ Integrity check passed");
    } else {
        println!("❌ Integrity check failed: {}", integrity_check);
    }
    
    Ok(())
}

fn check_settings(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nChecking database settings...");
    
    let conn = Connection::open(db_path)?;
    
    // Check journal mode
    let journal_mode: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    println!("Journal mode: {}", journal_mode);
    if journal_mode != "wal" {
        println!("⚠️  Journal mode is not WAL. This may lead to locking issues.");
    }
    
    // Check synchronous mode
    let synchronous: i64 = conn.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    println!("Synchronous mode: {}", synchronous);
    if synchronous > 1 {
        println!("⚠️  Synchronous mode is high. This may cause performance issues.");
    }
    
    // Check busy timeout
    let busy_timeout: i64 = conn.pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
    println!("Busy timeout: {}ms", busy_timeout);
    if busy_timeout < 5000 {
        println!("⚠️  Busy timeout is low. This may cause database locked errors.");
    }
    
    // Check cache size
    let cache_size: i64 = conn.pragma_query_value(None, "cache_size", |row| row.get(0))?;
    println!("Cache size: {}", cache_size);
    if cache_size > -1000 && cache_size < 0 {
        println!("⚠️  Cache size may be too small for your workload.");
    }
    
    // Check temp store
    let temp_store: i64 = conn.pragma_query_value(None, "temp_store", |row| row.get(0))?;
    println!("Temp store: {}", temp_store);
    if temp_store != 2 {
        println!("⚠️  Temp store is not set to MEMORY. This may cause performance issues.");
    }
    
    // Check if foreign keys are enabled
    let foreign_keys: i64 = conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    println!("Foreign keys: {}", foreign_keys);
    if foreign_keys != 1 {
        println!("⚠️  Foreign keys are not enabled. This may lead to data integrity issues.");
    }
    
    Ok(())
}

fn analyze_tables(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nAnalyzing database tables...");
    
    let conn = Connection::open(db_path)?;
    
    // Get a list of all tables
    let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
    let table_names: Vec<String> = stmt.query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?;
    
    println!("Found {} tables", table_names.len());
    
    for table_name in &table_names {
        println!("\nTable: {}", table_name);
        
        // Get row count
        let row_count: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {}", table_name),
            [],
            |row| row.get(0)
        )?;
        
        println!("Row count: {}", row_count);
        
        // Check if table has proper indexes for large tables
        if row_count > 1000 {
            let mut index_stmt = conn.prepare(&format!(
                "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='{}'",
                table_name
            ))?;
            
            let index_count = index_stmt.query_map([], |_| Ok(()))?.count();
            
            if index_count == 0 {
                println!("⚠️  Table has {} rows but no indexes. This may cause performance issues.", row_count);
            } else {
                println!("Indexes: {}", index_count);
            }
        }
    }
    
    Ok(())
}

fn check_indexes(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nChecking database indexes...");
    
    let conn = Connection::open(db_path)?;
    
    // Get a list of all indexes
    let mut stmt = conn.prepare("SELECT name, tbl_name FROM sqlite_master WHERE type='index'")?;
    let indexes: Vec<(String, String)> = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
    
    println!("Found {} indexes", indexes.len());
    
    for (index_name, table_name) in &indexes {
        // Check if index is used (this is an approximation)
        let index_info: Result<String, _> = conn.query_row(
            &format!("EXPLAIN QUERY PLAN SELECT * FROM {} WHERE rowid=1", table_name),
            [],
            |row| row.get(0)
        );
        
        match index_info {
            Ok(info) => {
                if info.contains(&format!("USING INDEX {}", index_name)) {
                    println!("✅ Index {} on table {} is used", index_name, table_name);
                } else {
                    println!("⚠️  Index {} on table {} may not be used effectively", index_name, table_name);
                }
            },
            Err(_) => {
                println!("⚠️  Could not analyze index {} on table {}", index_name, table_name);
            }
        }
    }
    
    Ok(())
}

fn test_concurrent_access(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("\nTesting concurrent database access...");
    
    let db_path = db_path.clone();
    let errors = Arc::new(Mutex::new(Vec::new()));
    let stop_flag = Arc::new(AtomicBool::new(false));
    
    // Create multiple threads to test concurrent access
    let mut handles = vec![];
    
    for i in 0..5 {
        let thread_db_path = db_path.clone();
        let thread_errors = errors.clone();
        let thread_stop_flag = stop_flag.clone();
        
        let handle = thread::spawn(move || {
            let conn = match Connection::open(&thread_db_path) {
                Ok(conn) => conn,
                Err(e) => {
                    let mut errors = thread_errors.lock().unwrap();
                    errors.push(format!("Thread {} failed to open connection: {}", i, e));
                    return;
                }
            };
            
            // Set a busy timeout to avoid immediate locks
            if let Err(e) = conn.busy_timeout(Duration::from_secs(1)) {
                let mut errors = thread_errors.lock().unwrap();
                errors.push(format!("Thread {} failed to set busy timeout: {}", i, e));
                return;
            }
            
            // Run simple queries in a loop
            let mut iteration = 0;
            while !thread_stop_flag.load(Ordering::Relaxed) && iteration < 10 {
                match conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |_| Ok(())) {
                    Ok(_) => {},
                    Err(e) => {
                        let mut errors = thread_errors.lock().unwrap();
                        errors.push(format!("Thread {} query failed on iteration {}: {}", i, iteration, e));
                    }
                }
                
                thread::sleep(Duration::from_millis(50));
                iteration += 1;
            }
        });
        
        handles.push(handle);
    }
    
    // Let threads run for a short time
    thread::sleep(Duration::from_secs(2));
    
    // Signal threads to stop
    stop_flag.store(true, Ordering::Relaxed);
    
    // Wait for all threads to finish
    for handle in handles {
        let _ = handle.join();
    }
    
    // Check for errors
    let error_list = errors.lock().unwrap();
    if error_list.is_empty() {
        println!("✅ Concurrent access test passed with no errors");
    } else {
        println!("❌ Concurrent access test failed with {} errors:", error_list.len());
        for error in error_list.iter() {
            println!("  - {}", error);
        }
    }
    
    Ok(())
} 