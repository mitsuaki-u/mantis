use crate::infra::db::Database;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Connection statistics and tracking
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    /// Number of active connections
    pub active_connections: usize,
    /// Number of connection acquisitions
    pub connection_requests: usize,
    /// Number of connection failures
    pub connection_failures: usize,
    /// Number of locked database errors
    pub locked_errors: usize,
    /// Average connection acquisition time
    pub avg_acquisition_time_ms: f64,
    /// The timestamp of the last reported lock
    pub last_locked_time: Option<Instant>,
    /// The traceback when the last lock occurred
    pub last_locked_traceback: Option<String>,
}

/// For tracking and monitoring database connections
pub struct ConnectionMonitor {
    /// The database being monitored
    db: Arc<Database>,
    /// Connection statistics
    stats: Arc<Mutex<ConnectionStats>>,
    /// Active connections and their acquisition times
    active_connections: Arc<Mutex<HashMap<String, Instant>>>,
    /// Flag to enable or disable monitoring
    enabled: bool,
}

impl ConnectionMonitor {
    /// Create a new connection monitor
    pub fn new(db: Arc<Database>, enabled: bool) -> Self {
        Self {
            db,
            stats: Arc::new(Mutex::new(ConnectionStats {
                active_connections: 0,
                connection_requests: 0,
                connection_failures: 0,
                locked_errors: 0,
                avg_acquisition_time_ms: 0.0,
                last_locked_time: None,
                last_locked_traceback: None,
            })),
            active_connections: Arc::new(Mutex::new(HashMap::new())),
            enabled,
        }
    }

    /// Enable monitoring
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable monitoring
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Record a connection acquisition
    pub fn record_connection_acquisition(&self, connection_id: &str, start_time: Instant) {
        if !self.enabled {
            return;
        }

        let acquisition_time = start_time.elapsed();

        // Update statistics
        let mut stats = self.stats.lock().unwrap();
        stats.connection_requests += 1;
        stats.active_connections += 1;

        // Update average acquisition time
        let new_avg = (stats.avg_acquisition_time_ms * (stats.connection_requests - 1) as f64
            + acquisition_time.as_millis() as f64)
            / stats.connection_requests as f64;
        stats.avg_acquisition_time_ms = new_avg;

        // Log slow acquisitions
        if acquisition_time > Duration::from_millis(100) {
            warn!(
                "Slow connection acquisition: {:?} for connection {}",
                acquisition_time, connection_id
            );
        }

        // Track the active connection
        let mut connections = self.active_connections.lock().unwrap();
        connections.insert(connection_id.to_string(), Instant::now());

        // Log connection stats periodically
        if stats.connection_requests % 100 == 0 {
            info!(
                "Connection stats: {} active, {} total requests, {:.2}ms avg acquisition time",
                stats.active_connections, stats.connection_requests, stats.avg_acquisition_time_ms
            );

            // Check for long-held connections
            for (id, time) in connections.iter() {
                let age = time.elapsed();
                if age > Duration::from_secs(10) {
                    warn!("Connection {} has been held for {:?}", id, age);
                }
            }
        }
    }

    /// Record a connection release
    pub fn record_connection_release(&self, connection_id: &str) {
        if !self.enabled {
            return;
        }

        // Update statistics
        let mut stats = self.stats.lock().unwrap();
        stats.active_connections = stats.active_connections.saturating_sub(1);

        // Remove from active connections
        let mut connections = self.active_connections.lock().unwrap();
        if let Some(time) = connections.remove(connection_id) {
            let held_duration = time.elapsed();

            // Log long-held connections
            if held_duration > Duration::from_secs(5) {
                warn!(
                    "Connection {} was held for {:?}",
                    connection_id, held_duration
                );
            }
        }
    }

    /// Record a database locked error
    pub fn record_locked_error(&self, traceback: &str) {
        if !self.enabled {
            return;
        }

        let mut stats = self.stats.lock().unwrap();
        stats.locked_errors += 1;
        stats.last_locked_time = Some(Instant::now());
        stats.last_locked_traceback = Some(traceback.to_string());

        // Log active connections at time of lock
        let connections = self.active_connections.lock().unwrap();
        error!(
            "Database locked error occurred! Active connections: {}",
            connections.len()
        );

        for (id, time) in connections.iter() {
            let age = time.elapsed();
            error!("Connection {} has been held for {:?}", id, age);
        }

        // Log the traceback
        error!("Locked error traceback: {}", traceback);
    }

    /// Get the current connection statistics
    pub fn get_stats(&self) -> ConnectionStats {
        let stats = self.stats.lock().unwrap();
        stats.clone()
    }

    /// Print a connection report
    pub fn print_report(&self) {
        if !self.enabled {
            info!("Connection monitoring disabled");
            return;
        }

        let stats = self.stats.lock().unwrap();
        let connections = self.active_connections.lock().unwrap();

        info!("=== Database Connection Report ===");
        info!("Active connections: {}", stats.active_connections);
        info!("Total connection requests: {}", stats.connection_requests);
        info!("Connection failures: {}", stats.connection_failures);
        info!("Database locked errors: {}", stats.locked_errors);
        info!(
            "Average acquisition time: {:.2}ms",
            stats.avg_acquisition_time_ms
        );

        if let Some(time) = stats.last_locked_time {
            info!("Last locked error: {:?} ago", time.elapsed());

            if let Some(ref traceback) = stats.last_locked_traceback {
                info!("Last locked traceback: {}", traceback);
            }
        }

        info!("=== Active Connections ===");
        for (id, time) in connections.iter() {
            info!("Connection {}: held for {:?}", id, time.elapsed());
        }

        info!("=== End Connection Report ===");
    }

    /// Run diagnostics to check for issues
    pub fn run_diagnostics(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let stats = self.stats.lock().unwrap();
        let connections = self.active_connections.lock().unwrap();

        // Check for locked errors
        if stats.locked_errors > 0 {
            issues.push(format!(
                "Database has experienced {} locked errors",
                stats.locked_errors
            ));
        }

        // Check for slow connection acquisition
        if stats.avg_acquisition_time_ms > 50.0 {
            issues.push(format!(
                "Slow connection acquisition (avg {:.2}ms)",
                stats.avg_acquisition_time_ms
            ));
        }

        // Check for too many active connections
        if stats.active_connections > 3 {
            issues.push(format!(
                "High number of active connections: {}",
                stats.active_connections
            ));
        }

        // Check for long-held connections
        for (id, time) in connections.iter() {
            let age = time.elapsed();
            if age > Duration::from_secs(30) {
                issues.push(format!("Connection {} held for too long: {:?}", id, age));
            }
        }

        issues
    }

    /// Check if the database connection is healthy
    async fn is_healthy(&self) -> bool {
        match self.db.check_pool_health().await {
            (true, _) => true,
            (false, message) => {
                error!("Database connection unhealthy: {}", message);
                false
            }
        }
    }
}

/// Initialize the connection monitor with the database
pub fn initialize_connection_monitor(db: Arc<Database>) -> Arc<ConnectionMonitor> {
    let monitor = ConnectionMonitor::new(db, true);
    Arc::new(monitor)
}
