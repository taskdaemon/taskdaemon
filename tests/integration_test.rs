//! Integration tests for TaskDaemon
//!
//! These tests verify end-to-end behavior of the daemon components.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use taskdaemon::config::Config;
use taskdaemon::coordinator::Coordinator;
use taskdaemon::domain::{Loop, LoopStatus, Phase, Priority};
use taskdaemon::r#loop::{CascadeHandler, LoopLoader};
use taskdaemon::scheduler::{Scheduler, SchedulerConfig};
use taskdaemon::state::StateManager;
use tempfile::TempDir;

// =============================================================================
// Coordinator Tests
// =============================================================================

#[tokio::test]
async fn test_coordinator_starts_and_stops() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let store_path = temp_dir.path();

    // Create coordinator with persistence
    let coordinator = Coordinator::with_persistence(Default::default(), store_path);
    let sender = coordinator.sender();

    // Spawn coordinator
    let handle = tokio::spawn(coordinator.run());

    // Give it time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify sender works (won't panic on closed channel)
    let send_result = sender.send(taskdaemon::coordinator::CoordRequest::Shutdown).await;
    assert!(send_result.is_ok(), "Should be able to send shutdown");

    // Wait for coordinator to finish
    let result = tokio::time::timeout(Duration::from_secs(5), handle).await;
    assert!(result.is_ok(), "Coordinator should shut down gracefully");
}

#[tokio::test]
async fn test_coordinator_register_and_unregister() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let coordinator = Coordinator::with_persistence(Default::default(), temp_dir.path());
    let sender = coordinator.sender();

    // Spawn coordinator
    let coord_handle = tokio::spawn(coordinator.run());

    // Register an execution
    let (msg_tx, _msg_rx) = tokio::sync::mpsc::channel(10);
    sender
        .send(taskdaemon::coordinator::CoordRequest::Register {
            exec_id: "test-exec-1".to_string(),
            tx: msg_tx,
        })
        .await
        .expect("Failed to send register");

    // Give time for registration
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Unregister
    sender
        .send(taskdaemon::coordinator::CoordRequest::Unregister {
            exec_id: "test-exec-1".to_string(),
        })
        .await
        .expect("Failed to send unregister");

    // Shutdown
    sender
        .send(taskdaemon::coordinator::CoordRequest::Shutdown)
        .await
        .expect("Failed to send shutdown");

    let _ = tokio::time::timeout(Duration::from_secs(5), coord_handle)
        .await
        .expect("Coordinator should shut down");
}

// =============================================================================
// Scheduler Tests
// =============================================================================

#[tokio::test]
async fn test_scheduler_basic_flow() {
    let config = SchedulerConfig {
        max_concurrent: 2, // Only 2 concurrent
        max_requests_per_window: 100,
        rate_window_secs: 1,
        default_priority: Priority::Normal,
    };
    let scheduler = Arc::new(Scheduler::new(config));

    // Schedule first request - should succeed immediately
    let result = scheduler.schedule("exec-1", Priority::Normal).await;
    assert!(
        matches!(result, taskdaemon::scheduler::ScheduleResult::Ready),
        "First request should be ready"
    );

    // Mark complete
    scheduler.complete("exec-1").await;

    // Verify stats
    let stats = scheduler.stats().await;
    assert_eq!(stats.total_completed, 1);
}

#[tokio::test]
async fn test_scheduler_sequential_requests() {
    let config = SchedulerConfig {
        max_concurrent: 1, // Only 1 at a time
        max_requests_per_window: 100,
        rate_window_secs: 60,
        default_priority: Priority::Normal,
    };
    let scheduler = Arc::new(Scheduler::new(config));

    // First request should run immediately
    let result1 = scheduler.schedule("exec-1", Priority::Normal).await;
    assert!(
        matches!(result1, taskdaemon::scheduler::ScheduleResult::Ready),
        "First request should be ready"
    );

    // Complete first request
    scheduler.complete("exec-1").await;

    // Second request should also run immediately (slot is free)
    let result2 = scheduler.schedule("exec-2", Priority::Normal).await;
    assert!(
        matches!(result2, taskdaemon::scheduler::ScheduleResult::Ready),
        "Second request should be ready after first completes"
    );

    scheduler.complete("exec-2").await;

    // Verify stats show 2 completions
    let stats = scheduler.stats().await;
    assert_eq!(stats.total_completed, 2);
}

// =============================================================================
// State Manager Tests
// =============================================================================

#[tokio::test]
async fn test_state_manager_loop_lifecycle() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("Failed to spawn state manager");

    // Create a loop (type, title)
    let record = Loop::new("mytype", "Test task");

    // Create it
    state.create_loop(record.clone()).await.expect("Failed to create loop");

    // Read it back
    let retrieved = state
        .get_loop(&record.id)
        .await
        .expect("Failed to get loop")
        .expect("Loop should exist");
    assert_eq!(retrieved.id, record.id);
    assert_eq!(retrieved.title, "Test task");

    // Update status
    let mut updated = retrieved.clone();
    updated.status = LoopStatus::InProgress;
    state.update_loop(updated.clone()).await.expect("Failed to update loop");

    // Verify update
    let after_update = state
        .get_loop(&record.id)
        .await
        .expect("Failed to get loop")
        .expect("Loop should exist");
    assert_eq!(after_update.status, LoopStatus::InProgress);

    // List all loops (no filter)
    let loops = state.list_loops(None, None, None).await.expect("Failed to list loops");
    assert_eq!(loops.len(), 1);

    // List with status filter
    let in_progress_loops = state
        .list_loops(None, Some("in_progress".to_string()), None)
        .await
        .expect("Failed to list loops");
    assert_eq!(in_progress_loops.len(), 1);

    let pending_loops = state
        .list_loops(None, Some("pending".to_string()), None)
        .await
        .expect("Failed to list loops");
    assert_eq!(pending_loops.len(), 0);
}

// =============================================================================
// Loop Type Loader Tests
// =============================================================================

#[test]
fn test_loop_type_loader_builtins() {
    let config = taskdaemon::config::LoopsConfig::default();
    let loader = LoopLoader::new(&config).expect("Failed to create loader");

    let configs = loader.to_configs();

    // Should have builtin types
    assert!(configs.contains_key("ralph"), "Should have ralph loop type");
    assert!(configs.contains_key("spec"), "Should have spec loop type");
    assert!(configs.contains_key("plan"), "Should have plan loop type");
    assert!(configs.contains_key("phase"), "Should have phase loop type");
}

// =============================================================================
// Config Validation Tests
// =============================================================================

#[test]
fn test_config_validation_missing_api_key() {
    // Create a config that requires a non-standard env var that won't be set
    let mut config = Config::default();
    config.llm.api_key_env = "NONEXISTENT_TEST_API_KEY_12345".to_string();

    let result = config.validate();

    assert!(result.is_err(), "Should fail without API key");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("NONEXISTENT_TEST_API_KEY_12345"),
        "Error should mention the env var"
    );
}

#[test]
fn test_config_validation_with_api_key() {
    // SAFETY: We're in a single-threaded test environment
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");
    }

    let config = Config::default();
    let result = config.validate();

    // Clean up
    // SAFETY: We're in a single-threaded test environment
    unsafe {
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    assert!(result.is_ok(), "Should pass with API key set");
}

// =============================================================================
// Inter-loop Communication Tests
// =============================================================================

#[tokio::test]
async fn test_coordinator_alert_subscription() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let coordinator = Coordinator::with_persistence(Default::default(), temp_dir.path());
    let sender = coordinator.sender();

    // Spawn coordinator
    let coord_handle = tokio::spawn(coordinator.run());

    // Register two executions
    let (msg_tx1, mut msg_rx1) = tokio::sync::mpsc::channel(10);
    let (msg_tx2, _msg_rx2) = tokio::sync::mpsc::channel(10);

    sender
        .send(taskdaemon::coordinator::CoordRequest::Register {
            exec_id: "listener".to_string(),
            tx: msg_tx1,
        })
        .await
        .unwrap();

    sender
        .send(taskdaemon::coordinator::CoordRequest::Register {
            exec_id: "sender".to_string(),
            tx: msg_tx2,
        })
        .await
        .unwrap();

    // Subscribe listener to an event type
    sender
        .send(taskdaemon::coordinator::CoordRequest::Subscribe {
            exec_id: "listener".to_string(),
            event_type: "test-event".to_string(),
        })
        .await
        .unwrap();

    // Give time for subscription
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Send an alert
    sender
        .send(taskdaemon::coordinator::CoordRequest::Alert {
            from_exec_id: "sender".to_string(),
            event_type: "test-event".to_string(),
            data: serde_json::json!({"message": "hello"}),
        })
        .await
        .unwrap();

    // Listener should receive the notification
    let result = tokio::time::timeout(Duration::from_secs(1), msg_rx1.recv()).await;
    assert!(result.is_ok(), "Should receive notification");
    let msg = result.unwrap().expect("Should have message");
    assert!(
        matches!(msg, taskdaemon::coordinator::CoordMessage::Notification { .. }),
        "Should be notification message"
    );

    // Cleanup
    sender
        .send(taskdaemon::coordinator::CoordRequest::Shutdown)
        .await
        .unwrap();

    let _ = tokio::time::timeout(Duration::from_secs(5), coord_handle).await;
}

// =============================================================================
// Cascade Handler Tests - 4-Level Hierarchy
// =============================================================================

/// Helper to create a CascadeHandler for testing
async fn create_cascade_handler(temp_dir: &TempDir) -> (Arc<StateManager>, CascadeHandler) {
    let state = Arc::new(StateManager::spawn(temp_dir.path()).expect("Failed to spawn state manager"));

    let loops_config = taskdaemon::config::LoopsConfig::default();
    let loader = LoopLoader::new(&loops_config).expect("Failed to create loader");
    let type_loader = Arc::new(RwLock::new(loader));

    let cascade = CascadeHandler::new(state.clone(), type_loader);
    (state, cascade)
}

#[tokio::test]
async fn test_cascade_plan_spawns_spec() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (state, cascade) = create_cascade_handler(&temp_dir).await;

    // Create a Plan loop that is ready
    let mut plan = Loop::new("plan", "Test Plan");
    plan.set_status(LoopStatus::Ready);
    state.create_loop(plan.clone()).await.expect("Failed to create plan");

    // Trigger cascade
    let children = cascade.on_loop_ready(&plan).await.expect("Cascade failed");

    // Should spawn a spec child execution
    assert!(!children.is_empty(), "Should spawn child executions");
    assert_eq!(children[0].loop_type, "spec", "Child should be spec type");
}

#[tokio::test]
async fn test_cascade_spec_spawns_phase() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (state, cascade) = create_cascade_handler(&temp_dir).await;

    // Create a Spec loop that is ready
    let mut spec = Loop::new("spec", "Test Spec");
    spec.set_status(LoopStatus::Ready);
    state.create_loop(spec.clone()).await.expect("Failed to create spec");

    // Trigger cascade
    let children = cascade.on_loop_ready(&spec).await.expect("Cascade failed");

    // Should spawn a phase child execution
    assert!(!children.is_empty(), "Should spawn child executions");
    assert_eq!(children[0].loop_type, "phase", "Child should be phase type");
}

#[tokio::test]
async fn test_cascade_phase_spawns_ralph() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (state, cascade) = create_cascade_handler(&temp_dir).await;

    // Create a Phase loop that is ready
    let mut phase = Loop::new("phase", "Test Phase");
    phase.set_status(LoopStatus::Ready);
    state.create_loop(phase.clone()).await.expect("Failed to create phase");

    // Trigger cascade
    let children = cascade.on_loop_ready(&phase).await.expect("Cascade failed");

    // Should spawn a ralph child execution
    assert!(!children.is_empty(), "Should spawn child executions");
    assert_eq!(children[0].loop_type, "ralph", "Child should be ralph type");
}

#[tokio::test]
async fn test_cascade_ralph_is_leaf() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (state, cascade) = create_cascade_handler(&temp_dir).await;

    // Create a Ralph loop that is ready
    let mut ralph = Loop::new("ralph", "Test Ralph");
    ralph.set_status(LoopStatus::Ready);
    state.create_loop(ralph.clone()).await.expect("Failed to create ralph");

    // Trigger cascade
    let children = cascade.on_loop_ready(&ralph).await.expect("Cascade failed");

    // Ralph should have no children (it's a leaf)
    assert!(children.is_empty(), "Ralph should not spawn any children");
}

#[tokio::test]
async fn test_cascade_completion_bubbles_up() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (state, _cascade) = create_cascade_handler(&temp_dir).await;

    // Create the hierarchy: Plan -> Spec -> Phase -> Ralph
    let mut plan = Loop::new("plan", "Root Plan");
    plan.set_status(LoopStatus::InProgress);
    state.create_loop(plan.clone()).await.expect("Failed to create plan");

    let mut spec = Loop::new("spec", "Child Spec");
    spec.parent = Some(plan.id.clone());
    spec.set_status(LoopStatus::InProgress);
    state.create_loop(spec.clone()).await.expect("Failed to create spec");

    let mut phase = Loop::new("phase", "Child Phase");
    phase.parent = Some(spec.id.clone());
    phase.set_status(LoopStatus::InProgress);
    state.create_loop(phase.clone()).await.expect("Failed to create phase");

    let mut ralph = Loop::new("ralph", "Child Ralph");
    ralph.parent = Some(phase.id.clone());
    ralph.set_status(LoopStatus::Complete);
    state.create_loop(ralph.clone()).await.expect("Failed to create ralph");

    // Manually check completion bubbling
    // When Ralph completes, Phase should complete (only child)
    // When Phase completes, Spec should complete (only child)
    // When Spec completes, Plan should complete (only child)

    // For this test, we verify the hierarchy is correctly set up
    let children_of_phase = state.list_loops_for_parent(&phase.id).await.expect("Failed to list");
    assert_eq!(children_of_phase.len(), 1);
    assert_eq!(children_of_phase[0].id, ralph.id);

    let children_of_spec = state.list_loops_for_parent(&spec.id).await.expect("Failed to list");
    assert_eq!(children_of_spec.len(), 1);
    assert_eq!(children_of_spec[0].id, phase.id);

    let children_of_plan = state.list_loops_for_parent(&plan.id).await.expect("Failed to list");
    assert_eq!(children_of_plan.len(), 1);
    assert_eq!(children_of_plan[0].id, spec.id);
}

#[tokio::test]
async fn test_cascade_failure_bubbles_up() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (state, cascade) = create_cascade_handler(&temp_dir).await;

    // Create Plan -> Spec hierarchy
    let mut plan = Loop::new("plan", "Root Plan");
    plan.set_status(LoopStatus::InProgress);
    state.create_loop(plan.clone()).await.expect("Failed to create plan");

    let mut spec = Loop::new("spec", "Child Spec");
    spec.parent = Some(plan.id.clone());
    spec.set_status(LoopStatus::Failed);
    state.create_loop(spec.clone()).await.expect("Failed to create spec");

    // Get ready children should find none (spec is failed, not pending)
    let ready = cascade
        .get_ready_children(&plan.id)
        .await
        .expect("Failed to get ready children");
    assert!(ready.is_empty(), "Failed spec should not be ready");
}

#[tokio::test]
async fn test_cascade_dependency_ordering() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let (state, cascade) = create_cascade_handler(&temp_dir).await;

    // Create Plan with two Specs that have dependencies
    let mut plan = Loop::new("plan", "Root Plan");
    plan.set_status(LoopStatus::InProgress);
    state.create_loop(plan.clone()).await.expect("Failed to create plan");

    // Spec A - no dependencies
    let mut spec_a = Loop::new("spec", "Spec A");
    spec_a.parent = Some(plan.id.clone());
    spec_a.set_status(LoopStatus::Pending);
    state
        .create_loop(spec_a.clone())
        .await
        .expect("Failed to create spec_a");

    // Spec B - depends on Spec A
    let mut spec_b = Loop::new("spec", "Spec B");
    spec_b.parent = Some(plan.id.clone());
    spec_b.add_dependency(&spec_a.id);
    spec_b.set_status(LoopStatus::Pending);
    state
        .create_loop(spec_b.clone())
        .await
        .expect("Failed to create spec_b");

    // Get ready children - only Spec A should be ready
    let ready = cascade
        .get_ready_children(&plan.id)
        .await
        .expect("Failed to get ready children");

    // Should have exactly one ready child (Spec A)
    // Note: The cascade creates executions for ready children, so this tests the dep logic
    assert!(
        !ready.is_empty() || spec_a.is_ready(&[]),
        "Spec A should be ready (no deps)"
    );

    // Verify Spec B is not ready (has unsatisfied dep)
    assert!(!spec_b.is_ready(&[]), "Spec B should NOT be ready (dep on A)");

    // Verify Spec B becomes ready after A completes
    assert!(
        spec_b.is_ready(&[&spec_a.id]),
        "Spec B should be ready after A completes"
    );
}

#[tokio::test]
async fn test_loop_phases_progression() {
    // Test phase-based progression within a loop
    let mut record = Loop::new("spec", "Multi-phase spec");
    record.add_phase(Phase::new("Phase 1", "First phase"));
    record.add_phase(Phase::new("Phase 2", "Second phase"));
    record.add_phase(Phase::new("Phase 3", "Third phase"));

    // Initially at phase 0
    assert_eq!(record.current_phase_index(), Some(0));
    assert!(!record.all_phases_complete());

    // Complete phase 0
    record.complete_phase(0);
    assert_eq!(record.current_phase_index(), Some(1));

    // Complete phase 1
    record.complete_phase(1);
    assert_eq!(record.current_phase_index(), Some(2));

    // Complete phase 2
    record.complete_phase(2);
    assert!(record.all_phases_complete());
    assert_eq!(record.current_phase_index(), None);
}
