# Spec: Daemon Mode

**ID:** 027-daemon-mode  
**Status:** Draft  
**Dependencies:** [026-cli-commands]

## Summary

Implement daemon mode that runs TaskDaemon as a background process with PID file management, signal handling, and proper service lifecycle. The daemon should be robust, handle system signals gracefully, and integrate well with system service managers.

## Acceptance Criteria

1. **Daemonization**
   - Fork to background process
   - PID file creation/cleanup
   - Working directory setup
   - File descriptor management

2. **Signal Handling**
   - SIGTERM for graceful shutdown
   - SIGHUP for configuration reload
   - SIGUSR1/2 for custom actions
   - Signal masking during critical sections

3. **Service Integration**
   - Systemd service file
   - Init.d script support
   - Health check endpoint
   - Status reporting

4. **Lifecycle Management**
   - Clean startup sequence
   - Graceful shutdown
   - Resource cleanup
   - State preservation

## Implementation Phases

### Phase 1: Basic Daemonization
- Process forking
- PID file handling
- Signal setup
- Logging redirect

### Phase 2: Signal Management
- Signal handlers
- Graceful shutdown
- Config reload
- Custom signals

### Phase 3: Service Integration
- Systemd support
- Init scripts
- Health checks
- Status interface

### Phase 4: Advanced Features
- Watchdog support
- Automatic restart
- Resource limits
- Monitoring hooks

## Technical Details

### Module Structure
```
src/daemon/
├── mod.rs
├── process.rs     # Daemonization logic
├── signals.rs     # Signal handling
├── pidfile.rs     # PID file management
├── service.rs     # Service integration
├── health.rs      # Health checks
└── lifecycle.rs   # Lifecycle management
```

### Daemonization Process
```rust
pub struct Daemon {
    config: DaemonConfig,
    pid_file: PidFile,
    signal_handler: SignalHandler,
    app: Arc<TaskDaemonApp>,
}

pub struct DaemonConfig {
    pub pid_file_path: PathBuf,
    pub work_dir: PathBuf,
    pub umask: u32,
    pub user: Option<String>,
    pub group: Option<String>,
    pub stdout_redirect: Option<PathBuf>,
    pub stderr_redirect: Option<PathBuf>,
}

impl Daemon {
    pub fn daemonize(&mut self) -> Result<(), Error> {
        // First fork
        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                // Parent process exits
                println!("TaskDaemon started with PID {}", child);
                std::process::exit(0);
            }
            Ok(ForkResult::Child) => {
                // Continue as child
            }
            Err(e) => return Err(Error::Fork(e)),
        }
        
        // Create new session
        setsid()?;
        
        // Second fork (prevents acquiring controlling terminal)
        match unsafe { fork() } {
            Ok(ForkResult::Parent { .. }) => {
                std::process::exit(0);
            }
            Ok(ForkResult::Child) => {
                // Continue as daemon
            }
            Err(e) => return Err(Error::Fork(e)),
        }
        
        // Set up daemon environment
        self.setup_daemon_environment()?;
        
        Ok(())
    }
    
    fn setup_daemon_environment(&self) -> Result<(), Error> {
        // Change working directory
        std::env::set_current_dir(&self.config.work_dir)?;
        
        // Set umask
        unsafe {
            libc::umask(self.config.umask as libc::mode_t);
        }
        
        // Close standard file descriptors
        self.close_standard_fds()?;
        
        // Redirect stdout/stderr if configured
        if let Some(stdout_path) = &self.config.stdout_redirect {
            self.redirect_fd(1, stdout_path)?;
        }
        
        if let Some(stderr_path) = &self.config.stderr_redirect {
            self.redirect_fd(2, stderr_path)?;
        }
        
        // Drop privileges if configured
        if let Some(user) = &self.config.user {
            self.drop_privileges(user, self.config.group.as_deref())?;
        }
        
        Ok(())
    }
}
```

### PID File Management
```rust
pub struct PidFile {
    path: PathBuf,
    pid: Option<Pid>,
    locked: bool,
}

impl PidFile {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            pid: None,
            locked: false,
        }
    }
    
    pub fn acquire(&mut self) -> Result<(), Error> {
        // Check if PID file exists
        if self.path.exists() {
            // Read existing PID
            let existing_pid = self.read_pid()?;
            
            // Check if process is still running
            if let Some(pid) = existing_pid {
                if self.is_process_running(pid)? {
                    return Err(Error::AlreadyRunning(pid));
                }
            }
            
            // Stale PID file, remove it
            fs::remove_file(&self.path)?;
        }
        
        // Create PID file with exclusive lock
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)?;
        
        // Lock the file
        file.try_lock_exclusive()?;
        
        // Write our PID
        let pid = std::process::id();
        writeln!(&file, "{}", pid)?;
        file.sync_all()?;
        
        self.pid = Some(Pid::from_raw(pid as i32));
        self.locked = true;
        
        Ok(())
    }
    
    pub fn release(&mut self) -> Result<(), Error> {
        if self.locked && self.path.exists() {
            fs::remove_file(&self.path)?;
            self.locked = false;
        }
        Ok(())
    }
    
    fn is_process_running(&self, pid: Pid) -> Result<bool, Error> {
        // Send signal 0 to check if process exists
        match kill(pid, None) {
            Ok(()) => Ok(true),
            Err(nix::Error::ESRCH) => Ok(false),
            Err(nix::Error::EPERM) => Ok(true), // Process exists but we can't signal it
            Err(e) => Err(e.into()),
        }
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
```

### Signal Handling
```rust
pub struct SignalHandler {
    shutdown_tx: broadcast::Sender<()>,
    reload_tx: broadcast::Sender<()>,
    status_tx: broadcast::Sender<()>,
}

impl SignalHandler {
    pub async fn setup() -> Result<Self, Error> {
        let (shutdown_tx, _) = broadcast::channel(1);
        let (reload_tx, _) = broadcast::channel(1);
        let (status_tx, _) = broadcast::channel(1);
        
        let handler = Self {
            shutdown_tx: shutdown_tx.clone(),
            reload_tx: reload_tx.clone(),
            status_tx: status_tx.clone(),
        };
        
        // Set up signal handlers
        let mut sigterm = signal(SignalKind::terminate())?;
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sighup = signal(SignalKind::hangup())?;
        let mut sigusr1 = signal(SignalKind::user_defined1())?;
        
        // Spawn signal handling task
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM, initiating graceful shutdown");
                        let _ = shutdown_tx.send(());
                    }
                    _ = sigint.recv() => {
                        info!("Received SIGINT, initiating graceful shutdown");
                        let _ = shutdown_tx.send(());
                    }
                    _ = sighup.recv() => {
                        info!("Received SIGHUP, reloading configuration");
                        let _ = reload_tx.send(());
                    }
                    _ = sigusr1.recv() => {
                        info!("Received SIGUSR1, dumping status");
                        let _ = status_tx.send(());
                    }
                }
            }
        });
        
        Ok(handler)
    }
    
    pub fn shutdown_signal(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }
    
    pub fn reload_signal(&self) -> broadcast::Receiver<()> {
        self.reload_tx.subscribe()
    }
}
```

### Service Lifecycle
```rust
pub struct ServiceLifecycle {
    app: Arc<TaskDaemonApp>,
    signal_handler: SignalHandler,
    health_server: HealthServer,
}

impl ServiceLifecycle {
    pub async fn run(&mut self) -> Result<(), Error> {
        // Start health check server
        let health_handle = self.health_server.start();
        
        // Get signal receivers
        let mut shutdown = self.signal_handler.shutdown_signal();
        let mut reload = self.signal_handler.reload_signal();
        let mut status = self.signal_handler.status_signal();
        
        // Main service loop
        loop {
            tokio::select! {
                // Run main application
                result = self.app.run() => {
                    match result {
                        Ok(()) => {
                            info!("Application completed normally");
                            break;
                        }
                        Err(e) => {
                            error!("Application error: {}", e);
                            return Err(e);
                        }
                    }
                }
                
                // Handle shutdown signal
                _ = shutdown.recv() => {
                    info!("Shutting down gracefully...");
                    self.graceful_shutdown().await?;
                    break;
                }
                
                // Handle reload signal
                _ = reload.recv() => {
                    info!("Reloading configuration...");
                    if let Err(e) = self.reload_config().await {
                        error!("Failed to reload config: {}", e);
                    }
                }
                
                // Handle status signal
                _ = status.recv() => {
                    self.dump_status().await;
                }
            }
        }
        
        // Stop health server
        health_handle.abort();
        
        Ok(())
    }
    
    async fn graceful_shutdown(&self) -> Result<(), Error> {
        // Set grace period
        let grace_period = Duration::from_secs(30);
        let deadline = Instant::now() + grace_period;
        
        // Stop accepting new work
        self.app.stop_accepting_work().await;
        
        // Wait for active work to complete
        while self.app.has_active_work().await {
            if Instant::now() > deadline {
                warn!("Grace period exceeded, forcing shutdown");
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
        
        // Save state
        self.app.save_state().await?;
        
        // Shutdown app
        self.app.shutdown().await?;
        
        Ok(())
    }
}
```

### Health Checks
```rust
pub struct HealthServer {
    port: u16,
    app: Arc<TaskDaemonApp>,
}

impl HealthServer {
    pub fn start(&self) -> JoinHandle<()> {
        let app = self.app.clone();
        let port = self.port;
        
        tokio::spawn(async move {
            let health_route = warp::path("health")
                .map(move || {
                    let health = app.health_status();
                    match health.overall {
                        HealthStatus::Healthy => {
                            warp::reply::with_status(
                                warp::reply::json(&health),
                                StatusCode::OK,
                            )
                        }
                        HealthStatus::Degraded => {
                            warp::reply::with_status(
                                warp::reply::json(&health),
                                StatusCode::OK,
                            )
                        }
                        HealthStatus::Unhealthy => {
                            warp::reply::with_status(
                                warp::reply::json(&health),
                                StatusCode::SERVICE_UNAVAILABLE,
                            )
                        }
                    }
                });
            
            let ready_route = warp::path("ready")
                .map(move || {
                    if app.is_ready() {
                        StatusCode::OK
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                });
            
            let routes = health_route.or(ready_route);
            
            warp::serve(routes)
                .run(([127, 0, 0, 1], port))
                .await;
        })
    }
}
```

### Systemd Service File
```ini
[Unit]
Description=TaskDaemon - AI-powered task execution daemon
Documentation=https://github.com/example/taskdaemon
After=network.target

[Service]
Type=forking
ExecStart=/usr/bin/taskdaemon start
ExecReload=/bin/kill -HUP $MAINPID
ExecStop=/usr/bin/taskdaemon stop
PIDFile=/var/run/taskdaemon.pid
Restart=on-failure
RestartSec=5
User=taskdaemon
Group=taskdaemon

# Security settings
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/taskdaemon /var/log/taskdaemon

# Resource limits
LimitNOFILE=65536
LimitNPROC=4096
MemoryLimit=2G
CPUQuota=200%

# Environment
Environment="RUST_LOG=info"
Environment="TASKDAEMON_CONFIG=/etc/taskdaemon/config.yml"

[Install]
WantedBy=multi-user.target
```

### Init.d Script
```bash
#!/bin/bash
### BEGIN INIT INFO
# Provides:          taskdaemon
# Required-Start:    $remote_fs $syslog $network
# Required-Stop:     $remote_fs $syslog $network
# Default-Start:     2 3 4 5
# Default-Stop:      0 1 6
# Short-Description: TaskDaemon service
# Description:       AI-powered task execution daemon
### END INIT INFO

NAME=taskdaemon
DAEMON=/usr/bin/taskdaemon
PIDFILE=/var/run/taskdaemon.pid
LOGFILE=/var/log/taskdaemon/daemon.log

. /lib/lsb/init-functions

case "$1" in
    start)
        log_daemon_msg "Starting $NAME"
        start-stop-daemon --start --quiet --pidfile $PIDFILE \
            --exec $DAEMON -- start --pid-file $PIDFILE \
            >> $LOGFILE 2>&1
        log_end_msg $?
        ;;
    stop)
        log_daemon_msg "Stopping $NAME"
        $DAEMON stop --pid-file $PIDFILE
        log_end_msg $?
        ;;
    restart)
        $0 stop
        sleep 2
        $0 start
        ;;
    reload)
        log_daemon_msg "Reloading $NAME"
        kill -HUP $(cat $PIDFILE)
        log_end_msg $?
        ;;
    status)
        $DAEMON status
        ;;
    *)
        echo "Usage: $0 {start|stop|restart|reload|status}"
        exit 1
        ;;
esac

exit 0
```

## Notes

- Ensure proper cleanup in all error paths to avoid orphaned PID files
- Implement comprehensive logging before daemonizing
- Consider using systemd socket activation for zero-downtime updates
- Test signal handling thoroughly, especially during critical operations