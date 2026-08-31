# Binary tree traversal benchmark
# Heavy on pointer chasing, object field accesses and recursive calls.
# The tree is built once and kept small enough to avoid triggering GC,
# so this measures traversal and not allocation.

import time


class Node:
    def __init__(self, left, right, val):
        self.left = left
        self.right = right
        self.val = val


# Build a perfect binary tree of a given depth.
# A tree of depth d holds 2^(d+1) - 1 nodes.
def make_tree(depth, val):
    if depth == 0:
        return Node(None, None, val)

    return Node(
        make_tree(depth - 1, val * 2),
        make_tree(depth - 1, val * 2 + 1),
        val
    )


# Sum the values of every node in the tree
def check_tree(node):
    if node.left is None:
        return node.val

    return node.val + check_tree(node.left) + check_tree(node.right)


def main():
    DEPTH = 14
    NUM_ITRS = 2000

    num_nodes = (1 << (DEPTH + 1)) - 1
    tree = make_tree(DEPTH, 1)

    start_time = time.time()

    sum = 0
    for i in range(NUM_ITRS):
        sum = sum + check_tree(tree)

    end_time = time.time()
    elapsed_ms = int((end_time - start_time) * 1000)

    print(f"{NUM_ITRS} traversals of a {num_nodes} node tree took {elapsed_ms} ms")

    # Each traversal visits every node exactly once
    expected = check_tree(tree) * NUM_ITRS
    if sum != expected:
        print(f"checksum FAILED, got {sum}, expected {expected}")


main()
