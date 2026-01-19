//! Tree data structure for hierarchical loop display
//!
//! Builds a tree from flat LoopExecution records using parent-child relationships.
//! Supports expand/collapse state and efficient tree traversal.

use std::collections::{HashMap, HashSet};

use super::state::ExecutionItem;

/// A node in the loop tree
#[derive(Debug, Clone)]
pub struct TreeNode {
    /// The execution item data
    pub item: ExecutionItem,
    /// Child node IDs (in display order)
    pub children: Vec<String>,
    /// Depth in the tree (0 = root)
    pub depth: usize,
    /// Is this node expanded (children visible)?
    pub expanded: bool,
    /// Number of completed children (for progress display)
    pub completed_children: usize,
    /// Total number of children
    pub total_children: usize,
}

impl TreeNode {
    /// Create a new tree node from an execution item
    pub fn new(item: ExecutionItem, depth: usize) -> Self {
        Self {
            item,
            children: Vec::new(),
            depth,
            expanded: true, // Default to expanded
            completed_children: 0,
            total_children: 0,
        }
    }

    /// Check if this node has children
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Get progress string for display (e.g., "[2/5]" or "[-]" for no children)
    pub fn progress_string(&self) -> String {
        if self.total_children == 0 {
            "[-]".to_string()
        } else {
            format!("[{}/{}]", self.completed_children, self.total_children)
        }
    }

    /// Check if this is a draft (Plan in draft state with no children)
    pub fn is_draft(&self) -> bool {
        self.item.status == "draft"
    }
}

/// The complete loop tree
#[derive(Debug, Default)]
pub struct LoopTree {
    /// All nodes indexed by ID
    nodes: HashMap<String, TreeNode>,
    /// Root node IDs (nodes with no parent)
    roots: Vec<String>,
    /// Expand/collapse state (persists across rebuilds)
    expand_state: HashMap<String, bool>,
    /// Currently selected node ID
    selected_id: Option<String>,
    /// Flattened visible nodes for rendering (updated on expand/collapse)
    visible_nodes: Vec<String>,
}

impl LoopTree {
    /// Create a new empty tree
    pub fn new() -> Self {
        Self::default()
    }

    /// Build tree from flat list of execution items
    ///
    /// This is the main entry point for constructing the tree.
    /// It preserves expand state from previous builds.
    pub fn build_from_items(&mut self, items: Vec<ExecutionItem>) {
        // Save current expand state
        let prev_expand_state = std::mem::take(&mut self.expand_state);
        let prev_selected = self.selected_id.clone();

        // Clear existing nodes
        self.nodes.clear();
        self.roots.clear();

        // Index items by parent for efficient lookup
        let mut children_by_parent: HashMap<Option<String>, Vec<&ExecutionItem>> = HashMap::new();
        for item in &items {
            children_by_parent.entry(item.parent_id.clone()).or_default().push(item);
        }

        // Create nodes and detect roots
        let mut parent_ids: HashSet<String> = HashSet::new();
        for item in &items {
            if let Some(ref parent_id) = item.parent_id {
                parent_ids.insert(parent_id.clone());
            }
        }

        // Build tree recursively starting from roots (items with no parent)
        if let Some(root_items) = children_by_parent.get(&None) {
            for item in root_items {
                self.build_subtree(item, 0, &children_by_parent, &prev_expand_state);
                self.roots.push(item.id.clone());
            }
        }

        // Handle orphaned nodes (parent ID exists but parent not found)
        // These are displayed at root level with a warning
        for item in &items {
            if let Some(ref parent_id) = item.parent_id
                && !self.nodes.contains_key(parent_id)
                && !self.nodes.contains_key(&item.id)
            {
                // Orphaned: parent doesn't exist, treat as root
                tracing::warn!("Orphaned execution {} has invalid parent {}", item.id, parent_id);
                self.build_subtree(item, 0, &children_by_parent, &prev_expand_state);
                self.roots.push(item.id.clone());
            }
        }

        // Calculate completion counts for all nodes
        self.calculate_completion_counts();

        // Restore selection if still valid
        if let Some(ref id) = prev_selected
            && self.nodes.contains_key(id)
        {
            self.selected_id = prev_selected;
        }

        // If no selection, select first root
        if self.selected_id.is_none() && !self.roots.is_empty() {
            self.selected_id = Some(self.roots[0].clone());
        }

        // Rebuild visible nodes list
        self.rebuild_visible_nodes();
    }

    /// Recursively build a subtree
    fn build_subtree(
        &mut self,
        item: &ExecutionItem,
        depth: usize,
        children_by_parent: &HashMap<Option<String>, Vec<&ExecutionItem>>,
        prev_expand_state: &HashMap<String, bool>,
    ) {
        let mut node = TreeNode::new(item.clone(), depth);

        // Restore expand state or use default based on status
        // Default: expand active nodes, collapse drafts and completed
        let default_expand = !matches!(item.status.as_str(), "draft" | "complete" | "failed");
        node.expanded = prev_expand_state.get(&item.id).copied().unwrap_or(default_expand);

        // Build children
        if let Some(child_items) = children_by_parent.get(&Some(item.id.clone())) {
            for child in child_items {
                node.children.push(child.id.clone());
                self.build_subtree(child, depth + 1, children_by_parent, prev_expand_state);
            }
        }

        // Store expand state for persistence
        self.expand_state.insert(item.id.clone(), node.expanded);

        self.nodes.insert(item.id.clone(), node);
    }

    /// Calculate completion counts for all nodes (bottom-up)
    fn calculate_completion_counts(&mut self) {
        // Process nodes in reverse depth order (leaves first)
        let mut nodes_by_depth: Vec<Vec<String>> = Vec::new();
        for (id, node) in &self.nodes {
            while nodes_by_depth.len() <= node.depth {
                nodes_by_depth.push(Vec::new());
            }
            nodes_by_depth[node.depth].push(id.clone());
        }

        // Process from deepest to shallowest
        for depth_nodes in nodes_by_depth.iter().rev() {
            for id in depth_nodes {
                let children: Vec<String> = self.nodes.get(id).map(|n| n.children.clone()).unwrap_or_default();
                let total = children.len();
                let completed = children
                    .iter()
                    .filter(|child_id| self.nodes.get(*child_id).is_some_and(|n| n.item.status == "complete"))
                    .count();

                if let Some(node) = self.nodes.get_mut(id) {
                    node.total_children = total;
                    node.completed_children = completed;
                }
            }
        }
    }

    /// Rebuild the list of visible nodes for rendering
    fn rebuild_visible_nodes(&mut self) {
        self.visible_nodes.clear();
        for root_id in &self.roots.clone() {
            self.add_visible_nodes_recursive(root_id);
        }
    }

    /// Recursively add visible nodes
    fn add_visible_nodes_recursive(&mut self, id: &str) {
        self.visible_nodes.push(id.to_string());

        if let Some(node) = self.nodes.get(id)
            && node.expanded
        {
            let children = node.children.clone();
            for child_id in children {
                self.add_visible_nodes_recursive(&child_id);
            }
        }
    }

    /// Get the list of visible nodes in display order
    pub fn visible_nodes(&self) -> &[String] {
        &self.visible_nodes
    }

    /// Get a node by ID
    pub fn get(&self, id: &str) -> Option<&TreeNode> {
        self.nodes.get(id)
    }

    /// Get the currently selected node ID
    pub fn selected_id(&self) -> Option<&String> {
        self.selected_id.as_ref()
    }

    /// Get the currently selected node
    pub fn selected_node(&self) -> Option<&TreeNode> {
        self.selected_id.as_ref().and_then(|id| self.nodes.get(id))
    }

    /// Get the index of the selected node in visible_nodes
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_id
            .as_ref()
            .and_then(|id| self.visible_nodes.iter().position(|n| n == id))
    }

    /// Select a node by ID
    pub fn select(&mut self, id: &str) {
        if self.nodes.contains_key(id) {
            self.selected_id = Some(id.to_string());
        }
    }

    /// Select by visible index
    pub fn select_by_index(&mut self, index: usize) {
        if let Some(id) = self.visible_nodes.get(index).cloned() {
            self.selected_id = Some(id);
        }
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if let Some(current_idx) = self.selected_index()
            && current_idx > 0
        {
            self.select_by_index(current_idx - 1);
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if let Some(current_idx) = self.selected_index()
            && current_idx + 1 < self.visible_nodes.len()
        {
            self.select_by_index(current_idx + 1);
        }
    }

    /// Move selection to first visible node
    pub fn select_first(&mut self) {
        self.select_by_index(0);
    }

    /// Move selection to last visible node
    pub fn select_last(&mut self) {
        if !self.visible_nodes.is_empty() {
            self.select_by_index(self.visible_nodes.len() - 1);
        }
    }

    /// Toggle expand/collapse for the selected node
    pub fn toggle_selected(&mut self) {
        if let Some(id) = self.selected_id.clone() {
            self.toggle(&id);
        }
    }

    /// Toggle expand/collapse for a specific node
    pub fn toggle(&mut self, id: &str) {
        if let Some(node) = self.nodes.get_mut(id)
            && node.has_children()
        {
            node.expanded = !node.expanded;
            self.expand_state.insert(id.to_string(), node.expanded);
            self.rebuild_visible_nodes();
        }
    }

    /// Expand the selected node
    pub fn expand_selected(&mut self) {
        if let Some(id) = self.selected_id.clone()
            && let Some(node) = self.nodes.get_mut(&id)
            && node.has_children()
            && !node.expanded
        {
            node.expanded = true;
            self.expand_state.insert(id.clone(), true);
            self.rebuild_visible_nodes();
        }
    }

    /// Collapse the selected node
    pub fn collapse_selected(&mut self) {
        if let Some(id) = self.selected_id.clone()
            && let Some(node) = self.nodes.get_mut(&id)
            && node.has_children()
            && node.expanded
        {
            node.expanded = false;
            self.expand_state.insert(id.clone(), false);
            self.rebuild_visible_nodes();
        }
    }

    /// Check if the tree is empty
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get total number of nodes
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Get number of visible nodes
    pub fn visible_len(&self) -> usize {
        self.visible_nodes.len()
    }

    /// Check if a node is the last child of its parent
    pub fn is_last_child(&self, id: &str) -> bool {
        if let Some(node) = self.nodes.get(id)
            && let Some(ref parent_id) = node.item.parent_id
            && let Some(parent) = self.nodes.get(parent_id)
        {
            return parent.children.last().is_some_and(|last| last == id);
        }
        // Root nodes: check if last in roots
        self.roots.last().is_some_and(|last| last == id)
    }

    /// Get the parent chain for a node (for rendering tree lines)
    pub fn get_ancestor_chain(&self, id: &str) -> Vec<(String, bool)> {
        let mut chain = Vec::new();
        let mut current_id = id.to_string();

        while let Some(node) = self.nodes.get(&current_id) {
            if let Some(ref parent_id) = node.item.parent_id {
                let is_last = self.is_last_child(&current_id);
                chain.push((current_id.clone(), is_last));
                current_id = parent_id.clone();
            } else {
                break;
            }
        }

        chain.reverse();
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(id: &str, parent: Option<&str>, status: &str, loop_type: &str) -> ExecutionItem {
        ExecutionItem {
            id: id.to_string(),
            name: id.to_string(),
            loop_type: loop_type.to_string(),
            iteration: "1/10".to_string(),
            status: status.to_string(),
            duration: "0:00".to_string(),
            parent_id: parent.map(|s| s.to_string()),
            progress: String::new(),
        }
    }

    #[test]
    fn test_build_simple_tree() {
        let items = vec![
            make_item("plan1", None, "running", "plan"),
            make_item("spec1", Some("plan1"), "running", "spec"),
            make_item("phase1", Some("spec1"), "complete", "phase"),
            make_item("ralph1", Some("phase1"), "complete", "ralph"),
        ];

        let mut tree = LoopTree::new();
        tree.build_from_items(items);

        assert_eq!(tree.len(), 4);
        assert_eq!(tree.roots.len(), 1);
        assert_eq!(tree.roots[0], "plan1");

        // Check hierarchy
        let plan = tree.get("plan1").unwrap();
        assert_eq!(plan.children, vec!["spec1"]);
        assert_eq!(plan.depth, 0);

        let spec = tree.get("spec1").unwrap();
        assert_eq!(spec.children, vec!["phase1"]);
        assert_eq!(spec.depth, 1);
    }

    #[test]
    fn test_completion_counts() {
        let items = vec![
            make_item("plan1", None, "running", "plan"),
            make_item("spec1", Some("plan1"), "running", "spec"),
            make_item("phase1", Some("spec1"), "complete", "phase"),
            make_item("phase2", Some("spec1"), "running", "phase"),
            make_item("phase3", Some("spec1"), "complete", "phase"),
        ];

        let mut tree = LoopTree::new();
        tree.build_from_items(items);

        let spec = tree.get("spec1").unwrap();
        assert_eq!(spec.total_children, 3);
        assert_eq!(spec.completed_children, 2); // phase1 and phase3 are complete
    }

    #[test]
    fn test_visible_nodes() {
        let items = vec![
            make_item("plan1", None, "running", "plan"),
            make_item("spec1", Some("plan1"), "running", "spec"),
            make_item("phase1", Some("spec1"), "complete", "phase"),
        ];

        let mut tree = LoopTree::new();
        tree.build_from_items(items);

        // All expanded by default for running nodes
        assert_eq!(tree.visible_len(), 3);

        // Collapse spec1
        tree.toggle("spec1");
        assert_eq!(tree.visible_len(), 2); // plan1, spec1 (phase1 hidden)

        // Expand spec1
        tree.toggle("spec1");
        assert_eq!(tree.visible_len(), 3);
    }

    #[test]
    fn test_navigation() {
        let items = vec![
            make_item("plan1", None, "running", "plan"),
            make_item("spec1", Some("plan1"), "running", "spec"),
            make_item("spec2", Some("plan1"), "running", "spec"),
        ];

        let mut tree = LoopTree::new();
        tree.build_from_items(items);

        // Should start at first node
        assert_eq!(tree.selected_id(), Some(&"plan1".to_string()));
        assert_eq!(tree.selected_index(), Some(0));

        // Move down
        tree.select_next();
        assert_eq!(tree.selected_id(), Some(&"spec1".to_string()));

        tree.select_next();
        assert_eq!(tree.selected_id(), Some(&"spec2".to_string()));

        // Move up
        tree.select_prev();
        assert_eq!(tree.selected_id(), Some(&"spec1".to_string()));

        // Jump to last
        tree.select_last();
        assert_eq!(tree.selected_id(), Some(&"spec2".to_string()));

        // Jump to first
        tree.select_first();
        assert_eq!(tree.selected_id(), Some(&"plan1".to_string()));
    }

    #[test]
    fn test_orphaned_nodes() {
        let items = vec![
            make_item("plan1", None, "running", "plan"),
            make_item("orphan", Some("nonexistent"), "running", "spec"),
        ];

        let mut tree = LoopTree::new();
        tree.build_from_items(items);

        // Orphan should be treated as root
        assert_eq!(tree.roots.len(), 2);
        assert!(tree.roots.contains(&"orphan".to_string()));
    }

    #[test]
    fn test_is_last_child() {
        let items = vec![
            make_item("plan1", None, "running", "plan"),
            make_item("spec1", Some("plan1"), "running", "spec"),
            make_item("spec2", Some("plan1"), "running", "spec"),
        ];

        let mut tree = LoopTree::new();
        tree.build_from_items(items);

        assert!(!tree.is_last_child("spec1")); // Not last
        assert!(tree.is_last_child("spec2")); // Last child of plan1
    }

    #[test]
    fn test_draft_default_collapsed() {
        let items = vec![make_item("plan1", None, "draft", "plan")];

        let mut tree = LoopTree::new();
        tree.build_from_items(items);

        let plan = tree.get("plan1").unwrap();
        assert!(!plan.expanded); // Drafts should be collapsed by default
    }
}
