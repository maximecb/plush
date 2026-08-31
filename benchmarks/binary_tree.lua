-- Binary tree traversal benchmark
-- Heavy on pointer chasing, object field accesses and recursive calls.
-- The tree is built once and kept small enough to avoid triggering GC,
-- so this measures traversal and not allocation.

-- Build a perfect binary tree of a given depth.
-- A tree of depth d holds 2^(d+1) - 1 nodes.
local function make_tree(depth, val)
    if depth == 0 then
        return { left = nil, right = nil, val = val }
    end

    return {
        left = make_tree(depth - 1, val * 2),
        right = make_tree(depth - 1, val * 2 + 1),
        val = val
    }
end

-- Sum the values of every node in the tree
local function check_tree(node)
    if node.left == nil then
        return node.val
    end

    return node.val + check_tree(node.left) + check_tree(node.right)
end

local function main()
    local DEPTH = 14
    local NUM_ITRS = 2000

    local num_nodes = (1 << (DEPTH + 1)) - 1
    local tree = make_tree(DEPTH, 1)

    local start_time = os.clock()

    local sum = 0
    for i = 1, NUM_ITRS do
        sum = sum + check_tree(tree)
    end

    local end_time = os.clock()
    local elapsed_ms = math.floor((end_time - start_time) * 1000)

    print(NUM_ITRS .. " traversals of a " .. num_nodes ..
          " node tree took " .. elapsed_ms .. " ms")

    -- Each traversal visits every node exactly once
    local expected = check_tree(tree) * NUM_ITRS
    if sum ~= expected then
        print("checksum FAILED, got " .. sum .. ", expected " .. expected)
    end
end

main()
