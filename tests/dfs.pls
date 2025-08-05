// Test of a recursive, depth-first search (DFS) of a graph

class Node {
    init(self, name) {
        self.name = name;
        self.marked = false;
        self.neighbors = [];
    }
}

// Create the graph nodes
let a = Node('A');
let b = Node('B');
let c = Node('C');
let d = Node('D');
let e = Node('E');
let f = Node('F');

// Build the graph by adding neighbors
a.neighbors.push(b);
a.neighbors.push(c);
b.neighbors.push(a);
b.neighbors.push(d);
b.neighbors.push(e);
c.neighbors.push(a);
c.neighbors.push(f);
d.neighbors.push(b);
e.neighbors.push(b);
e.neighbors.push(f);
f.neighbors.push(c);
f.neighbors.push(e);

let var visited_order = [];

fun dfs(node) {
    // Mark the node as visited
    node.marked = true;
    visited_order.push(node.name);

    let neighbors = node.neighbors;
    for (let var i = 0; i < neighbors.len; ++i) {
        let neighbor = neighbors[i];
        if (!neighbor.marked) {
            dfs(neighbor);
        }
    }
}

// Start DFS from node 'A'
dfs(a);

// Assert the order of visited nodes for a specific DFS traversal
assert(visited_order.len == 6);
assert(visited_order[0] == 'A');
assert(visited_order[1] == 'B');
assert(visited_order[2] == 'D');
assert(visited_order[3] == 'E');
assert(visited_order[4] == 'F');
assert(visited_order[5] == 'C');

// Assert that all nodes were marked as visited
assert(a.marked);
assert(b.marked);
assert(c.marked);
assert(d.marked);
assert(e.marked);
assert(f.marked);

$println("DFS graph traversal test passed!");