# Design Notes

These are some notes about key principles and ideas that factor into
the design of the Plush language and its VM.
Although the language is dynamically typed, it is designed in part to be easy
to optimize. This is why, for instance, functions and global
variables are immutable by default. This removes the need to guard against
functions being redefined or to implement a deoptimization mechanism for a
rarely needed feature. Redefining a global function is
also something that can make code hard to follow and probably should not
be done. That being said, it is possible to define a mutable global variable
and assign a function to it, but if this is done, it will be clear both in the
source code and to the VM that the function can be redefined.

## Language Design

One of the core design principles is minimalism. That is, try to keep both the
language and the VM's implementation relatively simple. In my experience, those
things go hand in hand. A language with straightforward semantics, fewer corner
cases and less "automagic" behavior is both easier to implement and easier to
optimize. It also tends to be more intuitive for programmers, in keeping with the
[principle of least surprise](https://en.wikipedia.org/wiki/Principle_of_least_astonishment).
JavaScript [is famous](https://javascriptwtf.com/) for having many of the
kinds of corner cases we want to avoid.
In addition to corner cases in the language, Plush also tries to have as
little undefined behavior as possible to maximize portability.

Plush is also designed to use familiar syntax and concepts. Its syntax
draws from Rust, JavaScript, and Lox. Basically, don't
try to reinvent things that don't need to be reinvented or go against
established practice for no particular reason. Don't try to be original just
for the sake of being original. The purpose of language is to be understood,
and in that spirit, it's useful to start from a place people are most likely to
understand. An example here would be how dictionaries in Plush use the familiar
JSON syntax. This is very likely to be familiar to most developers, and it's
also great from an interoperability perspective.
Plush takes into account the concept of a "weirdness budget".
The most uncommon feature in Plush is likely actor-based parallelism, but Plush
actors are conceptually similar to forked processes.

Another key idea is that Plush should err on the side of being too restrictive
about what it accepts or exposes rather than too permissive.
This aligns with minimalism: it is hard to remove a language feature or quirk once people start
relying on it. We should avoid introducing quirks and explicitly reject
combinations of types or inputs to which we cannot assign a clear, useful
meaning. We can always remove restrictions in the future, but it's
hard to add new restrictions later.

## Host Functions

Unlike CPython, CRuby and others, Plush has no Foreign Function Interface (FFI),
and this is intentional. This choice may be controversial, but there are
several reasons for it. The first is portability. Only
the VM itself needs to be ported. The second is ease of use. It can happen fairly
often in CPython or CRuby that you need to install host packages to install some
libraries. However, these dependencies can sometimes fail to install for a variety
of reasons, and some users
don't have the root/sudo access required to install these packages. This makes
software easier to break and harder to install. Another reason is security and
sandboxing, which I'll talk about in the next section.

Instead of an FFI, Plush provides a set of primitives known as host functions.
These can be called from the language using the `$` dollar sign prefix, e.g.
`$print()`. These functions are low-level abstractions designed to have simple
semantics and a small API surface, and to be as portable as possible. Keeping the API
surface small makes it harder to get the implementation wrong or
accidentally introduce unexpected interactions and corner cases. An example
would be the windowing API, which supports drawing a frame buffer. Currently,
only one pixel format is supported, which is BGRA32, with the idea that this
pixel format will be sufficient for most use cases. This may again be controversial,
because we're not exposing host UI primitives, but it does make the software
much more portable, and extremely fast UI rendering is very much possible,
even with an interpreter.

Not having an FFI has the additional benefit that we don't risk exposing a ton
of VM internals to the outside world.
In CPython, numbers like integers are heap-allocated objects, and these objects
have been exposed to libraries through its C API. This has a performance cost, but
this cost is hard to ever fully remove, because doing so would break Python's C APIs
on which hundreds of thousands of libraries now depend. They've also gone as far
as to expose CPython's bytecode to C libraries, which means it would be extremely
hard for them to ever switch to a faster register-based interpreter design, for
example.

Host functions are non-reentrant. They are leaf methods, meaning that they cannot
call back into Plush code. This means that a function like `Array.map(f)` must be
implemented in Plush, rather than as a host function. The motivation for this is
that it makes for a simpler JIT, a simpler GC, and simpler stack unwinding. It
also enables simpler program analysis, because we don't have to assume that every
host call can have random side effects. This design choice offers many benefits
in terms of both performance and implementation simplicity.

## Sandboxing and Security

In terms of sandboxing, Plush currently places a number of limits on which files
can be read and where. In particular, it denies access to files outside the current
working directory, and explicitly denies access to a user's home directory. The goal
is to eventually make it so that the language is sandboxed by default, and the
VM will have a capabilities model. This is meant to make it much safer for users to
try programs written by strangers on the internet. This is in contrast to the current
state of open source software, where you can clone or install a program, and it instantly
has full access to your entire home directory, where it could easily steal your SSH
keys or install a keylogger.

Because Plush has host functions and no FFI, sandboxing can be as straightforward as
tying certain capabilities to certain host functions. For example, the `$net_listen()`
host function to open a listening/server socket could be tied to a `net_server` capability
which is disabled by default. The ability to open an audio input device could be tied to
an `audio_input` capability, etc.
In terms of file access, a program could eventually be limited to only the directory
where it was installed by default, where its own bundled input data and config files are
located. We could also add a file picker API which opens a dialog box for the user to select
a file. This makes it so users can explicitly choose which files to open, which makes it
impossible to just open a file anywhere without the user knowing. Programs will also be
able to prompt users to request additional permissions, like on mobile phones.

## VM Design and Performance

One of the goals of the VM design is to minimize dependencies, maximize
portability, and reduce the risk of breakage. There are also security reasons for this in
light of supply chain attacks. Some dependencies are unavoidable in order to
interface with the outside world, but the default should be to avoid introducing
many small dependencies that do things we could easily implement ourselves.

The instruction set is designed to be easy to compile with a JIT. Plush is
dynamically typed, but the number of types you can pass to some instructions is
intentionally kept small. We also try to avoid situations where random instructions
can do host function calls, as this makes optimization more difficult. For example,
if the `add` instruction can call an add method on its first argument, then you may
be forced to assume that any `add` operation could have side effects. You may also
need special deoptimization logic for every arithmetic operation. This means no
operator overloading for now. This may seem limiting in some ways, but it also goes
with the principle of least surprise, i.e. an addition can't have a method call hidden
inside of it.

At the time of this writing, there is no support for var-arg functions. It should be
fairly easy to add support for optional arguments and do this in a way that performs
well. In terms of passing lists of arguments, extra arguments will likely be passed
into an array that gets allocated by the VM and passed into a special list argument
to the callee. This has the benefit that the interpreter doesn't need to have special
logic to handle stack frames of variable sizes, and it will have zero impact on the
performance of normal function calls. There are currently no plans for keyword
arguments because those are fairly complex to implement efficiently. Currently, in
CPython and CRuby, they can have a real performance cost, which most programmers are
unaware of.
