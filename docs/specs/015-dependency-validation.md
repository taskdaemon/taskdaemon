# Spec: Dependency Graph Validation

**ID:** 015-dependency-validation
**Status:** Draft
**Dependencies:** [006-domain-types, 011-priority-scheduler]

## Summary

Implement dependency graph validation with cycle detection using topological sort. This ensures that task dependencies form a valid directed acyclic graph (DAG) and provides ordering for execution.

## Acceptance Criteria

1. **Graph Construction**
   - Build dependency graph from domain objects
   - Support multiple dependency types
   - Handle dynamic updates
   - Efficient representation

2. **Cycle Detection**
   - Detect circular dependencies
   - Provide clear cycle reporting
   - Identify all cycles in graph
   - Suggest resolution strategies

3. **Topological Ordering**
   - Generate valid execution order
   - Support partial ordering
   - Handle disconnected components
   - Provide level-based grouping

4. **Graph Operations**
   - Add/remove dependencies
   - Query reachability
   - Find critical path
   - Impact analysis

## Implementation Phases

### Phase 1: Graph Structure
- Define graph data types
- Implement adjacency list
- Add basic operations
- Create builders

### Phase 2: Cycle Detection
- Implement DFS-based detection
- Add Tarjan's algorithm
- Create cycle reporting
- Build visualizations

### Phase 3: Topological Sort
- Implement Kahn's algorithm
- Add DFS-based sort
- Support incremental updates
- Generate execution levels

### Phase 4: Advanced Analysis
- Critical path finding
- Impact analysis
- Graph metrics
- Optimization hints

## Technical Details

### Module Structure
```
src/dependencies/
├── mod.rs
├── graph.rs       # Graph data structure
├── validation.rs  # Cycle detection
├── sort.rs        # Topological sorting
├── analysis.rs    # Graph analysis
└── visual.rs      # Visualization helpers
```

### Core Types
```rust
pub struct DependencyGraph<T> {
    nodes: HashMap<Uuid, T>,
    edges: HashMap<Uuid, HashSet<Uuid>>,
    reverse_edges: HashMap<Uuid, HashSet<Uuid>>,
    node_levels: Option<HashMap<Uuid, usize>>,
}

pub struct ValidationResult {
    pub is_valid: bool,
    pub cycles: Vec<Cycle>,
    pub unreachable_nodes: HashSet<Uuid>,
    pub stats: GraphStats,
}

pub struct Cycle {
    pub nodes: Vec<Uuid>,
    pub edges: Vec<(Uuid, Uuid)>,
}

pub struct TopologicalOrder {
    pub order: Vec<Uuid>,
    pub levels: Vec<HashSet<Uuid>>,
    pub critical_path: Option<Vec<Uuid>>,
}
```

### Cycle Detection Algorithm
```rust
impl<T> DependencyGraph<T> {
    pub fn detect_cycles(&self) -> Vec<Cycle> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for node in self.nodes.keys() {
            if !visited.contains(node) {
                self.dfs_detect_cycles(
                    node,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }
        cycles
    }

    fn dfs_detect_cycles(&self, node: &Uuid, ...) {
        visited.insert(*node);
        rec_stack.insert(*node);
        path.push(*node);

        if let Some(neighbors) = self.edges.get(node) {
            for neighbor in neighbors {
                if !visited.contains(neighbor) {
                    self.dfs_detect_cycles(neighbor, ...);
                } else if rec_stack.contains(neighbor) {
                    // Found cycle
                    let cycle_start = path.iter().position(|n| n == neighbor).unwrap();
                    cycles.push(Cycle {
                        nodes: path[cycle_start..].to_vec(),
                        edges: self.get_cycle_edges(&path[cycle_start..]),
                    });
                }
            }
        }

        path.pop();
        rec_stack.remove(node);
    }
}
```

### Topological Sort
```rust
impl<T> DependencyGraph<T> {
    pub fn topological_sort(&self) -> Result<TopologicalOrder, ValidationError> {
        // First check for cycles
        let cycles = self.detect_cycles();
        if !cycles.is_empty() {
            return Err(ValidationError::CyclesDetected(cycles));
        }

        // Kahn's algorithm
        let mut in_degree = self.calculate_in_degrees();
        let mut queue: VecDeque<_> = in_degree.iter()
            .filter(|(_, &count)| count == 0)
            .map(|(node, _)| *node)
            .collect();

        let mut order = Vec::new();
        let mut levels = vec![HashSet::new()];

        while !queue.is_empty() {
            let level_size = queue.len();
            for _ in 0..level_size {
                let node = queue.pop_front().unwrap();
                order.push(node);
                levels.last_mut().unwrap().insert(node);

                if let Some(neighbors) = self.edges.get(&node) {
                    for neighbor in neighbors {
                        in_degree.entry(*neighbor).and_modify(|e| *e -= 1);
                        if in_degree[neighbor] == 0 {
                            queue.push_back(*neighbor);
                        }
                    }
                }
            }
            if !queue.is_empty() {
                levels.push(HashSet::new());
            }
        }

        Ok(TopologicalOrder { order, levels, critical_path: None })
    }
}
```

## Notes

- Graph operations should be thread-safe for concurrent access
- Consider implementing incremental validation for performance
- Provide clear visualization of cycles for debugging
- Support for transitive reduction to simplify graphs