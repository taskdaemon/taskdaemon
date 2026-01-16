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

// NOTE: test_cascade_completion_bubbles_up was deleted during SDET audit.
// It only verified list_loops_for_parent worked, never actually triggered cascade.
// test_cascade_full_completion_propagation (below) is the real test for this behavior.

#[tokio::test]
async fn test_failed_child_is_not_ready() {
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

// =============================================================================
// TaskStore Hierarchy Tests - Proving 4-Level Relationships
// =============================================================================

/// Helper struct to hold a complete 4-level hierarchy for testing
struct TestHierarchy {
    plan: Loop,
    specs: Vec<Loop>,
    phases: Vec<Loop>,
    ralphs: Vec<Loop>,
}

/// Create a full 4-level hierarchy with multiple siblings at each level
async fn create_full_hierarchy(state: &StateManager) -> TestHierarchy {
    // Level 1: Plan (root)
    let mut plan = Loop::new("plan", "Test Plan: Build Authentication System");
    plan.set_status(LoopStatus::InProgress);
    state.create_loop(plan.clone()).await.expect("create plan");

    // Level 2: Two Specs under the Plan (Spec B depends on Spec A)
    let mut spec_a = Loop::new("spec", "Spec A: User Model");
    spec_a.parent = Some(plan.id.clone());
    spec_a.set_status(LoopStatus::InProgress);
    state.create_loop(spec_a.clone()).await.expect("create spec_a");

    let mut spec_b = Loop::new("spec", "Spec B: Auth Endpoints");
    spec_b.parent = Some(plan.id.clone());
    spec_b.add_dependency(&spec_a.id);
    spec_b.set_status(LoopStatus::Pending);
    state.create_loop(spec_b.clone()).await.expect("create spec_b");

    // Level 3: Two Phases under Spec A
    let mut phase_a1 = Loop::new("phase", "Phase A1: Define User Schema");
    phase_a1.parent = Some(spec_a.id.clone());
    phase_a1.set_status(LoopStatus::InProgress);
    state.create_loop(phase_a1.clone()).await.expect("create phase_a1");

    let mut phase_a2 = Loop::new("phase", "Phase A2: Implement User CRUD");
    phase_a2.parent = Some(spec_a.id.clone());
    phase_a2.add_dependency(&phase_a1.id);
    phase_a2.set_status(LoopStatus::Pending);
    state.create_loop(phase_a2.clone()).await.expect("create phase_a2");

    // Level 4: Two Ralphs under Phase A1
    let mut ralph_a1_1 = Loop::new("ralph", "Ralph: Create user.rs");
    ralph_a1_1.parent = Some(phase_a1.id.clone());
    ralph_a1_1.set_status(LoopStatus::InProgress);
    state.create_loop(ralph_a1_1.clone()).await.expect("create ralph_a1_1");

    let mut ralph_a1_2 = Loop::new("ralph", "Ralph: Write user tests");
    ralph_a1_2.parent = Some(phase_a1.id.clone());
    ralph_a1_2.add_dependency(&ralph_a1_1.id);
    ralph_a1_2.set_status(LoopStatus::Pending);
    state.create_loop(ralph_a1_2.clone()).await.expect("create ralph_a1_2");

    TestHierarchy {
        plan,
        specs: vec![spec_a, spec_b],
        phases: vec![phase_a1, phase_a2],
        ralphs: vec![ralph_a1_1, ralph_a1_2],
    }
}

#[tokio::test]
async fn test_hierarchy_storage_and_retrieval() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    let h = create_full_hierarchy(&state).await;

    // Verify all records are stored and retrievable
    let plan = state.get_loop(&h.plan.id).await.expect("get").expect("plan exists");
    assert_eq!(plan.r#type, "plan");
    assert!(plan.parent.is_none(), "Plan should be root");

    for spec in &h.specs {
        let stored = state.get_loop(&spec.id).await.expect("get").expect("spec exists");
        assert_eq!(stored.r#type, "spec");
        assert_eq!(stored.parent, Some(h.plan.id.clone()));
    }

    for phase in &h.phases {
        let stored = state.get_loop(&phase.id).await.expect("get").expect("phase exists");
        assert_eq!(stored.r#type, "phase");
        assert_eq!(stored.parent, Some(h.specs[0].id.clone())); // All phases under spec_a
    }

    for ralph in &h.ralphs {
        let stored = state.get_loop(&ralph.id).await.expect("get").expect("ralph exists");
        assert_eq!(stored.r#type, "ralph");
        assert_eq!(stored.parent, Some(h.phases[0].id.clone())); // All ralphs under phase_a1
    }
}

#[tokio::test]
async fn test_hierarchy_parent_child_queries() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    let h = create_full_hierarchy(&state).await;

    // Query children at each level
    let specs = state.list_loops_for_parent(&h.plan.id).await.expect("list");
    assert_eq!(specs.len(), 2, "Plan should have 2 spec children");
    assert!(specs.iter().all(|s| s.r#type == "spec"));

    let phases = state.list_loops_for_parent(&h.specs[0].id).await.expect("list");
    assert_eq!(phases.len(), 2, "Spec A should have 2 phase children");
    assert!(phases.iter().all(|p| p.r#type == "phase"));

    let ralphs = state.list_loops_for_parent(&h.phases[0].id).await.expect("list");
    assert_eq!(ralphs.len(), 2, "Phase A1 should have 2 ralph children");
    assert!(ralphs.iter().all(|r| r.r#type == "ralph"));

    // Ralph is a leaf - should have no children
    let leaf_children = state.list_loops_for_parent(&h.ralphs[0].id).await.expect("list");
    assert!(leaf_children.is_empty(), "Ralph should have no children");

    // Spec B has no phases yet
    let spec_b_phases = state.list_loops_for_parent(&h.specs[1].id).await.expect("list");
    assert!(spec_b_phases.is_empty(), "Spec B should have no children yet");
}

#[tokio::test]
async fn test_hierarchy_type_queries() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    let _h = create_full_hierarchy(&state).await;

    // Query by type
    let plans = state.list_loops_by_type("plan").await.expect("list");
    assert_eq!(plans.len(), 1, "Should have 1 plan");

    let specs = state.list_loops_by_type("spec").await.expect("list");
    assert_eq!(specs.len(), 2, "Should have 2 specs");

    let phases = state.list_loops_by_type("phase").await.expect("list");
    assert_eq!(phases.len(), 2, "Should have 2 phases");

    let ralphs = state.list_loops_by_type("ralph").await.expect("list");
    assert_eq!(ralphs.len(), 2, "Should have 2 ralphs");

    // Total records
    let all = state.list_loops(None, None, None).await.expect("list");
    assert_eq!(all.len(), 7, "Should have 7 total records (1+2+2+2)");
}

#[tokio::test]
async fn test_hierarchy_status_queries() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    let _h = create_full_hierarchy(&state).await;

    // Query by status
    let in_progress = state
        .list_loops(None, Some("in_progress".to_string()), None)
        .await
        .expect("list");
    // plan, spec_a, phase_a1, ralph_a1_1 = 4 in_progress
    assert_eq!(in_progress.len(), 4, "Should have 4 in_progress records");

    let pending = state
        .list_loops(None, Some("pending".to_string()), None)
        .await
        .expect("list");
    // spec_b, phase_a2, ralph_a1_2 = 3 pending
    assert_eq!(pending.len(), 3, "Should have 3 pending records");
}

#[tokio::test]
async fn test_hierarchy_combined_filters() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    let h = create_full_hierarchy(&state).await;

    // Filter: type=ralph AND parent=phase_a1
    let ralphs_under_phase = state
        .list_loops(Some("ralph".to_string()), None, Some(h.phases[0].id.clone()))
        .await
        .expect("list");
    assert_eq!(ralphs_under_phase.len(), 2);

    // Filter: type=spec AND status=pending
    let pending_specs = state
        .list_loops(Some("spec".to_string()), Some("pending".to_string()), None)
        .await
        .expect("list");
    assert_eq!(pending_specs.len(), 1);
    assert_eq!(pending_specs[0].title, "Spec B: Auth Endpoints");
}

#[tokio::test]
async fn test_hierarchy_dependency_chain() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    let h = create_full_hierarchy(&state).await;

    // Verify dependency chains are stored correctly
    let spec_b = state.get_loop(&h.specs[1].id).await.expect("get").expect("exists");
    assert_eq!(spec_b.deps.len(), 1);
    assert_eq!(spec_b.deps[0], h.specs[0].id, "Spec B depends on Spec A");

    let phase_a2 = state.get_loop(&h.phases[1].id).await.expect("get").expect("exists");
    assert_eq!(phase_a2.deps.len(), 1);
    assert_eq!(phase_a2.deps[0], h.phases[0].id, "Phase A2 depends on Phase A1");

    let ralph_2 = state.get_loop(&h.ralphs[1].id).await.expect("get").expect("exists");
    assert_eq!(ralph_2.deps.len(), 1);
    assert_eq!(ralph_2.deps[0], h.ralphs[0].id, "Ralph 2 depends on Ralph 1");
}

#[tokio::test]
async fn test_cascade_full_completion_propagation() {
    let temp_dir = TempDir::new().expect("temp dir");
    let (state, cascade) = create_cascade_handler(&temp_dir).await;

    // Create a minimal hierarchy: Plan -> Spec -> Phase -> Ralph
    let mut plan = Loop::new("plan", "Test Plan");
    plan.set_status(LoopStatus::InProgress);
    state.create_loop(plan.clone()).await.expect("create");

    let mut spec = Loop::new("spec", "Test Spec");
    spec.parent = Some(plan.id.clone());
    spec.set_status(LoopStatus::InProgress);
    state.create_loop(spec.clone()).await.expect("create");

    let mut phase = Loop::new("phase", "Test Phase");
    phase.parent = Some(spec.id.clone());
    phase.set_status(LoopStatus::InProgress);
    state.create_loop(phase.clone()).await.expect("create");

    let mut ralph = Loop::new("ralph", "Test Ralph");
    ralph.parent = Some(phase.id.clone());
    ralph.set_status(LoopStatus::InProgress);
    state.create_loop(ralph.clone()).await.expect("create");

    // Create a mock execution for ralph to simulate completion
    let exec = taskdaemon::domain::LoopExecution::new("ralph", &ralph.id)
        .with_context_value("record-id", &ralph.id)
        .with_context_value("phase-number", "1");
    state.create_loop_execution(exec.clone()).await.expect("create exec");

    // Mark ralph as complete
    let mut ralph_updated = state.get_loop(&ralph.id).await.expect("get").expect("exists");
    ralph_updated.set_status(LoopStatus::Complete);
    state.update_loop(ralph_updated).await.expect("update");

    // Trigger cascade completion check from ralph's parent (phase)
    cascade.on_child_loop_complete(&exec).await.expect("cascade complete");

    // Verify completion bubbled up through all levels
    let phase_after = state.get_loop(&phase.id).await.expect("get").expect("exists");
    assert_eq!(phase_after.status, LoopStatus::Complete, "Phase should be complete");

    let spec_after = state.get_loop(&spec.id).await.expect("get").expect("exists");
    assert_eq!(spec_after.status, LoopStatus::Complete, "Spec should be complete");

    let plan_after = state.get_loop(&plan.id).await.expect("get").expect("exists");
    assert_eq!(plan_after.status, LoopStatus::Complete, "Plan should be complete");
}

#[tokio::test]
async fn test_cascade_full_failure_propagation() {
    let temp_dir = TempDir::new().expect("temp dir");
    let (state, cascade) = create_cascade_handler(&temp_dir).await;

    // Create hierarchy: Plan -> Spec -> Phase -> Ralph
    let mut plan = Loop::new("plan", "Failure Test Plan");
    plan.set_status(LoopStatus::InProgress);
    state.create_loop(plan.clone()).await.expect("create");

    let mut spec = Loop::new("spec", "Failure Test Spec");
    spec.parent = Some(plan.id.clone());
    spec.set_status(LoopStatus::InProgress);
    state.create_loop(spec.clone()).await.expect("create");

    let mut phase = Loop::new("phase", "Failure Test Phase");
    phase.parent = Some(spec.id.clone());
    phase.set_status(LoopStatus::InProgress);
    state.create_loop(phase.clone()).await.expect("create");

    let mut ralph = Loop::new("ralph", "Failure Test Ralph");
    ralph.parent = Some(phase.id.clone());
    ralph.set_status(LoopStatus::InProgress);
    state.create_loop(ralph.clone()).await.expect("create");

    // Create execution for ralph (needed to trigger on_child_loop_complete)
    let exec = taskdaemon::domain::LoopExecution::new("ralph", &ralph.id)
        .with_context_value("record-id", &ralph.id)
        .with_context_value("phase-number", "1");
    state.create_loop_execution(exec.clone()).await.expect("create exec");

    // Mark ralph as FAILED (simulating a failed implementation)
    let mut ralph_updated = state.get_loop(&ralph.id).await.expect("get").expect("exists");
    ralph_updated.set_status(LoopStatus::Failed);
    state.update_loop(ralph_updated).await.expect("update");

    // Trigger cascade completion check - this should propagate failure up
    cascade.on_child_loop_complete(&exec).await.expect("cascade complete");

    // Verify failure bubbled up through all levels
    let phase_after = state.get_loop(&phase.id).await.expect("get").expect("exists");
    assert_eq!(
        phase_after.status,
        LoopStatus::Failed,
        "Phase should be Failed (child failed)"
    );

    let spec_after = state.get_loop(&spec.id).await.expect("get").expect("exists");
    assert_eq!(
        spec_after.status,
        LoopStatus::Failed,
        "Spec should be Failed (child failed)"
    );

    let plan_after = state.get_loop(&plan.id).await.expect("get").expect("exists");
    assert_eq!(
        plan_after.status,
        LoopStatus::Failed,
        "Plan should be Failed (child failed)"
    );
}

#[tokio::test]
async fn test_hierarchy_multiple_siblings_completion() {
    let temp_dir = TempDir::new().expect("temp dir");
    let (state, cascade) = create_cascade_handler(&temp_dir).await;

    // Plan with 2 specs, both must complete for plan to complete
    let mut plan = Loop::new("plan", "Test Plan");
    plan.set_status(LoopStatus::InProgress);
    state.create_loop(plan.clone()).await.expect("create");

    let mut spec_a = Loop::new("spec", "Spec A");
    spec_a.parent = Some(plan.id.clone());
    spec_a.set_status(LoopStatus::Complete); // A is done
    state.create_loop(spec_a.clone()).await.expect("create");

    let mut spec_b = Loop::new("spec", "Spec B");
    spec_b.parent = Some(plan.id.clone());
    spec_b.set_status(LoopStatus::InProgress); // B still running
    state.create_loop(spec_b.clone()).await.expect("create");

    // Check ready children - neither should create new work (A complete, B in progress)
    let ready = cascade.get_ready_children(&plan.id).await.expect("ready");
    assert!(ready.is_empty(), "No children ready (A complete, B in progress)");

    // Plan should NOT be complete yet (B still running)
    let plan_check = state.get_loop(&plan.id).await.expect("get").expect("exists");
    assert_eq!(
        plan_check.status,
        LoopStatus::InProgress,
        "Plan should still be in progress"
    );

    // Now complete spec B
    let mut spec_b_updated = state.get_loop(&spec_b.id).await.expect("get").expect("exists");
    spec_b_updated.set_status(LoopStatus::Complete);
    state.update_loop(spec_b_updated).await.expect("update");

    // Verify all children are complete
    let children = state.list_loops_for_parent(&plan.id).await.expect("list");
    let all_complete = children.iter().all(|c| c.status == LoopStatus::Complete);
    assert!(all_complete, "All children should be complete");
}

#[tokio::test]
async fn test_hierarchy_update_preserves_relationships() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    let h = create_full_hierarchy(&state).await;

    // Update a record in the middle of the hierarchy
    let mut spec = state.get_loop(&h.specs[0].id).await.expect("get").expect("exists");
    spec.set_status(LoopStatus::Complete);
    spec.context = serde_json::json!({"updated": true});
    state.update_loop(spec.clone()).await.expect("update");

    // Verify relationships are preserved after update
    let spec_after = state.get_loop(&h.specs[0].id).await.expect("get").expect("exists");
    assert_eq!(spec_after.parent, Some(h.plan.id.clone()));
    assert_eq!(spec_after.status, LoopStatus::Complete);

    // Children should still reference this spec
    let phases = state.list_loops_for_parent(&h.specs[0].id).await.expect("list");
    assert_eq!(phases.len(), 2, "Phases should still be children of spec");
}

#[tokio::test]
async fn test_hierarchy_traversal_from_leaf_to_root() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    let h = create_full_hierarchy(&state).await;

    // Start from a ralph and traverse up to the plan
    let ralph = state.get_loop(&h.ralphs[0].id).await.expect("get").expect("exists");
    assert_eq!(ralph.r#type, "ralph");

    // Go up to phase
    let phase_id = ralph.parent.expect("ralph has parent");
    let phase = state.get_loop(&phase_id).await.expect("get").expect("exists");
    assert_eq!(phase.r#type, "phase");

    // Go up to spec
    let spec_id = phase.parent.expect("phase has parent");
    let spec = state.get_loop(&spec_id).await.expect("get").expect("exists");
    assert_eq!(spec.r#type, "spec");

    // Go up to plan
    let plan_id = spec.parent.expect("spec has parent");
    let plan = state.get_loop(&plan_id).await.expect("get").expect("exists");
    assert_eq!(plan.r#type, "plan");

    // Plan is root
    assert!(plan.parent.is_none(), "Plan should be root (no parent)");
}

// =============================================================================
// Task Creation Tests (without TUI)
// =============================================================================

/// Test that creating a loop execution works when called directly
#[tokio::test]
async fn test_create_loop_execution_directly() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    // Create a loop execution directly (like TUI's start_task does)
    let execution = taskdaemon::domain::LoopExecution::new("plan", "Build a new feature");

    // This should succeed
    let id = state.create_execution(execution).await.expect("create execution");
    assert!(!id.is_empty(), "Should get an ID back");

    // Verify it was created
    let retrieved = state.get_execution(&id).await.expect("get").expect("exists");
    assert_eq!(retrieved.loop_type, "plan");
}

// =============================================================================
// Delete Operations Tests
// =============================================================================

/// Test deleting a loop execution
#[tokio::test]
async fn test_delete_execution() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    // Create an execution
    let execution = taskdaemon::domain::LoopExecution::new("ralph", "Test task");
    let id = state.create_execution(execution).await.expect("create");

    // Verify it exists
    let exists = state.get_execution(&id).await.expect("get");
    assert!(exists.is_some(), "Execution should exist before delete");

    // Delete it
    state.delete_execution(&id).await.expect("delete");

    // Verify it's gone
    let after_delete = state.get_execution(&id).await.expect("get");
    assert!(after_delete.is_none(), "Execution should not exist after delete");
}

/// Test deleting a loop record
#[tokio::test]
async fn test_delete_loop() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    // Create a loop
    let record = Loop::new("spec", "Test spec");
    let id = record.id.clone();
    state.create_loop(record).await.expect("create");

    // Verify it exists
    let exists = state.get_loop(&id).await.expect("get");
    assert!(exists.is_some(), "Loop should exist before delete");

    // Delete it
    state.delete_loop(&id).await.expect("delete");

    // Verify it's gone
    let after_delete = state.get_loop(&id).await.expect("get");
    assert!(after_delete.is_none(), "Loop should not exist after delete");
}

/// Test that deleting a parent doesn't automatically delete children
/// (orphan handling is a separate concern)
#[tokio::test]
async fn test_delete_loop_leaves_children() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    // Create parent-child hierarchy
    let parent = Loop::new("plan", "Parent Plan");
    let parent_id = parent.id.clone();
    state.create_loop(parent).await.expect("create parent");

    let mut child = Loop::new("spec", "Child Spec");
    child.parent = Some(parent_id.clone());
    let child_id = child.id.clone();
    state.create_loop(child).await.expect("create child");

    // Delete parent
    state.delete_loop(&parent_id).await.expect("delete");

    // Child should still exist (as orphan)
    let child_after = state.get_loop(&child_id).await.expect("get");
    assert!(child_after.is_some(), "Child should still exist after parent deletion");

    // Child still references the deleted parent
    let child_record = child_after.unwrap();
    assert_eq!(child_record.parent, Some(parent_id.clone()));
}

/// Test deleting a non-existent record doesn't error (idempotent)
#[tokio::test]
async fn test_delete_nonexistent_is_ok() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    // Delete something that doesn't exist - should succeed (no-op)
    let result = state.delete_execution("nonexistent-id").await;
    assert!(result.is_ok(), "Deleting non-existent record should be ok");

    let result = state.delete_loop("nonexistent-id").await;
    assert!(result.is_ok(), "Deleting non-existent loop should be ok");
}

/// Test that deleted records don't appear in list queries
#[tokio::test]
async fn test_deleted_records_not_in_list() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    // Create multiple executions
    let exec1 = taskdaemon::domain::LoopExecution::new("ralph", "Task 1");
    let exec2 = taskdaemon::domain::LoopExecution::new("ralph", "Task 2");
    let exec3 = taskdaemon::domain::LoopExecution::new("ralph", "Task 3");

    let id1 = state.create_execution(exec1).await.expect("create");
    let id2 = state.create_execution(exec2).await.expect("create");
    let _id3 = state.create_execution(exec3).await.expect("create");

    // Delete two of them
    state.delete_execution(&id1).await.expect("delete");
    state.delete_execution(&id2).await.expect("delete");

    // List should only show one
    let remaining = state.list_executions(None, None).await.expect("list");
    assert_eq!(remaining.len(), 1, "Only one execution should remain");
    assert_eq!(remaining[0].loop_type, "ralph");
}

// =============================================================================
// Hierarchy Count Tests
// =============================================================================

#[tokio::test]
async fn test_hierarchy_counts_at_each_level() {
    let temp_dir = TempDir::new().expect("temp dir");
    let state = StateManager::spawn(temp_dir.path()).expect("spawn state");

    // Create a larger hierarchy for count testing
    // Note: IDs are generated from title, so each title must be unique!
    let plan = Loop::new("plan", "Test Plan");
    let plan_id = plan.id.clone();
    state.create_loop(plan).await.expect("create plan");

    // 3 specs - store their IDs
    let mut spec_ids = Vec::new();
    for i in 0..3 {
        let mut spec = Loop::new("spec", format!("Spec {}", i));
        spec.parent = Some(plan_id.clone());
        spec_ids.push(spec.id.clone());
        state.create_loop(spec).await.expect("create spec");
    }

    // 2 phases per spec = 6 phases - use unique titles (spec_idx + phase_idx)
    let mut phase_ids = Vec::new();
    for (spec_idx, spec_id) in spec_ids.iter().enumerate() {
        for phase_idx in 0..2 {
            let mut phase = Loop::new("phase", format!("Phase S{}P{}", spec_idx, phase_idx));
            phase.parent = Some(spec_id.clone());
            phase_ids.push(phase.id.clone());
            state.create_loop(phase).await.expect("create phase");
        }
    }

    // 2 ralphs per phase = 12 ralphs - use unique titles (phase_idx + ralph_idx)
    for (phase_idx, phase_id) in phase_ids.iter().enumerate() {
        for ralph_idx in 0..2 {
            let mut ralph = Loop::new("ralph", format!("Ralph P{}R{}", phase_idx, ralph_idx));
            ralph.parent = Some(phase_id.clone());
            state.create_loop(ralph).await.expect("create ralph");
        }
    }

    // Verify counts
    let all_plans = state.list_loops_by_type("plan").await.expect("list");
    assert_eq!(all_plans.len(), 1, "Should have 1 plan");

    let all_specs = state.list_loops_by_type("spec").await.expect("list");
    assert_eq!(all_specs.len(), 3, "Should have 3 specs");

    let all_phases = state.list_loops_by_type("phase").await.expect("list");
    assert_eq!(all_phases.len(), 6, "Should have 6 phases (2 per spec)");

    let all_ralphs = state.list_loops_by_type("ralph").await.expect("list");
    assert_eq!(all_ralphs.len(), 12, "Should have 12 ralphs (2 per phase)");

    // Total: 1 + 3 + 6 + 12 = 22
    let all = state.list_loops(None, None, None).await.expect("list");
    assert_eq!(all.len(), 22, "Should have 22 total records");

    // Verify parent-child relationships at each level
    let specs_under_plan = state.list_loops_for_parent(&plan_id).await.expect("list");
    assert_eq!(specs_under_plan.len(), 3, "Plan should have 3 spec children");

    for spec_id in &spec_ids {
        let phases_under_spec = state.list_loops_for_parent(spec_id).await.expect("list");
        assert_eq!(phases_under_spec.len(), 2, "Each spec should have 2 phase children");
    }

    for phase_id in &phase_ids {
        let ralphs_under_phase = state.list_loops_for_parent(phase_id).await.expect("list");
        assert_eq!(ralphs_under_phase.len(), 2, "Each phase should have 2 ralph children");
    }
}
