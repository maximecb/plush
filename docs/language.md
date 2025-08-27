# Plush Language Quickstart

This document provides a quick overview of the Plush programming language, its syntax, features, and built-in capabilities. It is intended for developers who want to get started with Plush and learn the basics of the language.

## Introduction

Plush is an experimental toy programming language and virtual machine inspired by JavaScript, Lox, Lua, and Python. It features a simple, minimalistic design with a stack-based bytecode interpreter, actor-based parallelism, and an easily extensible set of host functions.

## Getting Started

To run a Plush script, you can use the `cargo run` command, followed by the path to the script. For example:

```sh
cargo run examples/helloworld.psh
```

This will execute the `helloworld.psh` script and print "Hello, World!" to the console. More example programs
can be found in the [`examples/`](/examples) directory. These examples are available under the
[CC0 license](https://creativecommons.org/public-domain/cc0/) (public domain).

## Language Basics

### Variables

Variables are declared using the `let` keyword. By default, variables are immutable. To declare a mutable variable, use `let var`.

```plush
let x = 10;          // Immutable variable
let var y = 20;      // Mutable variable
y = 30;              // Reassigning a mutable variable
```

Loop counters, which are mutable, must be declared with `let var`, e.g.

```plush
for (let var i = 0; i < 10; ++i)
    $println(i);
```

### Data Types

Plush is a dynamically typed language and supports the following data types:

-   **Int64**: 64-bit signed integers (e.g., `10`, `-5`).
-   **Float64**: 64-bit floating-point numbers (e.g., `3.14`, `-0.5`).
-   **String**: UTF-8 encoded strings (e.g., `"hello"`, `'world'`).
-   **Bool**: `true` or `false`.
-   **Nil**: Represents the absence of a value.
-   **Array**: Ordered collections of values (e.g., `[1, 2, 3]`).
-   **ByteArray**: Raw, mutable byte buffers.
-   **Object**: Instances of classes.

### Operators

Plush supports a range of arithmetic, comparison, and logical operators:

-   **Arithmetic**: `+`, `-`, `*`, `/`, `%`
-   **Comparison**: `==`, `!=`, `<`, `>`, `<=`, `>=`.
-   **Logical**: `&&`, `||`, `!`

Note that unlike in JavaScript, the `==` operator performs reference equality for objects and arrays, not structural equality.

### Arrays

The syntax for array literals is similar to that of JavaScript, e.g.

```
let a = [0, 1, 2, 3, 4];
```

Array elements an be accessed using the indexing operator with square brackets, e.g. `a[0] = 1`.
ByteArrays can also be indexed using square brackets to read and write individual bytes.
The length of arrays and ByteArrays is accessed via the `.len` field.

### Control Flow

Plush provides `if`/`else` statements for conditional execution and `for` and `while` loops for iteration.

```plush
if (x > 5) {
    $println("x is greater than 5");
} else {
    $println("x is not greater than 5");
}

for (let var i = 0; i < 10; ++i) {
    $println(i);
}

let var i = 0;
while (i < 10) {
    $println(i);
    i = i + 1;
}
```

### Functions

Functions are defined using the `fun` keyword. They can take arguments and return values.

```plush
fun add(a, b) {
    return a + b;
}

let result = add(5, 10);
$println(result); // 15
```

### Classes

Plush supports object-oriented programming with classes. Classes are defined using the `class` keyword, and instances are created by calling the class name as a function. Note that the first argument to a method, including `init`, is the explicit `self` argument representing the current object. This argument can have any name, which avoids the JavaScript issue with closures shadowing an implicit `this` argument.

```plush
class Point {
    init(self, x, y) {
        self.x = x;
        self.y = y;
    }

    to_s(self) {
        return "(" + self.x.to_s() + ", " + self.y.to_s() + ")";
    }
}

let p = Point(10, 20);
$println(p.to_s()); // (10, 20)
```

## Built-in Functions and Methods

Plush provides a set of built-in host functions and methods that can be accessed from your code. Host functions are prefixed with a `$` sign.

### Host Functions

These host functions are defined in [`src/host.rs`](/src/host.rs):

-   `$time_current_ms()`: Returns the current time in milliseconds since the Unix epoch.
-   `$cmd_num_args()`: Get the number of command-line arguments available to the program.
-   `$cmd_get_arg(idx)`: Get the command-line argument at the given index.
-   `$print(value)`: Prints a value to the console.
-   `$println(value)`: Prints a value to the console, followed by a newline.
-   `$readln()`: Read one line of input into a string.
-   `$actor_id()`: Returns the ID of the current actor.
-   `$actor_parent()`: Returns the ID of the parent actor.
-   `$actor_sleep(msecs)`: Pauses the current actor for the specified number of milliseconds.
-   `$actor_spawn(function)`: Spawns a new actor that executes the given function.
-   `$actor_join(actor_id)`: Waits for an actor to finish and returns its result.
-   `$actor_send(actor_id, message)`: Sends a message to the specified actor.
-   `$actor_recv()`: Receives a message from the current actor's mailbox, blocking if empty.
-   `$actor_poll()`: Polls the actor's mailbox for a message, returning `nil` if empty.
-   `$window_create(width, height, title, flags)`: Creates a new window.
-   `$window_draw_frame(window_id, frame_buffer)`: Draws a frame buffer to the specified window.
-   `$exit(code)`: End program execution and produce the given exit code.

### Core Methods

-   **Int64**
    -   `abs()`: Get the absolute value of this number.
    -   `to_f()`: Converts the integer to a float.
    -   `to_s()`: Converts the integer to a string.
-   **Float64**
    -   `abs()`: Get the absolute value of this number.
    -   `ceil()`: Returns the smallest integer greater than or equal to the float.
    -   `floor()`: Returns the largest integer less than or equal to the float.
    -   `sin()`: Returns the sine of the float.
    -   `cos()`: Returns the cosine of the float.
    -   `sqrt()`: Returns the square root of the float.
    -   `to_s()`: Returns a string representation of the float.
-   **String**
    -   `parse_int(radix)`: Try to parse the entire string as an integer of the given `radix`. Returns `nil` on failure.
    -   `trim()`: Produce a new string without whitespace at the beginning or end.
-   **Array**
    -   `with_size(size, value)`: Creates a new array of the given size, filled with the given value.
    -   `push(value)`: Adds a value to the end of the array.
    -   `pop()`: Removes and returns the last value from the array.
-   **ByteArray**
    -   `with_size(size)`: Creates a new `ByteArray` of the given size.
    -   `fill_u32(start_index, count, value)`: Fills a portion of the `ByteArray` with a repeated 32-bit unsigned integer value.
    -   `read_u32(index)`: Reads a 32-bit unsigned integer from the `ByteArray` at the given index.
    -   `write_u32(index, value)`: Writes a 32-bit unsigned integer to the `ByteArray` at the given index.
    -   `memcpy(dst_idx, src_bytes, src_idx, len)`: Copies a block of memory from a source `ByteArray` to this one.
    -   `zero_fill()`: Overwrite the contents of the `ByteArray` with zeros.

## Concurrency with Actors

Plush supports actor-based concurrency, which allows you to write parallel programs that are safe and easy to reason about. Actors are independent processes that communicate by sending and receiving messages.

```plush
fun worker() {
    let msg = $actor_recv();
    $println("Received: " + msg);
}

let worker_id = $actor_spawn(worker);
$actor_send(worker_id, "Hello from the main actor!");
$actor_join(worker_id);
```

This example spawns a new worker actor, sends it a message, and then waits for it to complete. The worker receives the message and prints it to the console.

## Debugging

At the moment there is no debugger and you may find that error messages are lackluster. Unsupported behaviors can
result in Rust panics, sometimes without helpful messages. PRs to improve this are welcome.

To help in debugging, you can print values with `$println` and you can use the built in `assert` statement to
validate your assumptions.
