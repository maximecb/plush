# Binary tree traversal benchmark
# Heavy on pointer chasing, object field accesses and recursive calls.
# The tree is built once and kept small enough to avoid triggering GC,
# so this measures traversal and not allocation.

class Node
  attr_accessor :left, :right, :val

  def initialize(left, right, val)
    @left = left
    @right = right
    @val = val
  end
end

# Build a perfect binary tree of a given depth.
# A tree of depth d holds 2^(d+1) - 1 nodes.
def make_tree(depth, val)
  if depth == 0
    return Node.new(nil, nil, val)
  end

  return Node.new(
    make_tree(depth - 1, val * 2),
    make_tree(depth - 1, val * 2 + 1),
    val
  )
end

# Sum the values of every node in the tree
def check_tree(node)
  if node.left == nil
    return node.val
  end

  return node.val + check_tree(node.left) + check_tree(node.right)
end

def main
  depth = 14
  num_itrs = 2000

  num_nodes = (1 << (depth + 1)) - 1
  tree = make_tree(depth, 1)

  start_time = Process.clock_gettime(Process::CLOCK_MONOTONIC)

  sum = 0
  num_itrs.times do
    sum = sum + check_tree(tree)
  end

  end_time = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  elapsed_ms = ((end_time - start_time) * 1000).to_i

  puts "#{num_itrs} traversals of a #{num_nodes} node tree took #{elapsed_ms} ms"

  # Each traversal visits every node exactly once
  expected = check_tree(tree) * num_itrs
  if sum != expected
    puts "checksum FAILED, got #{sum}, expected #{expected}"
  end
end

main
