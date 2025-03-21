class Node
{
    init(self, value, next)
    {
        self.value = value;
        self.next = next;
    }
}

// Create a linked list of specified length with sequential values
fun make_list(length)
{
    let var head = nil;

    // Build the list from tail to head (so newest node becomes head)
    for (let var i = length - 1; i >= 0; --i)
    {
        let node = new Node(i, head);
        head = node;
    }

    return head;
}

// Linear search: find the node with specified value
fun linear_search(head, target)
{
    let var current = head;

    while (current != nil)
    {
        if (current.value == target)
        {
            return true;
        }

        current = current.next;
    }

    return false;
}

let list = make_list(20_000);

for (let var i = 0; i < 2_000; ++i)
{
    assert(linear_search(list, 19_999) == true);
}
