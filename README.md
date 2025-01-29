# Plush

**NOTE: this project is very much a work in progress. You're likely to run
into bugs and missing features. I'm looking for collaborators who share the vision
and want to help me make it happen.**

Plush is an experimental programming language and virtual machine for fun and teaching purposes.
It follows a minimalistic design philosph and draw inspiration from JavaScript, Lua as well as Lox.

If you think that Plush is cool, you can support my work via [GitHub Sponsors](https://github.com/sponsors/maximecb) :heart:

## Features

**TODO**

## Build Instructions

Dependencies:
- The [Rust toolchain](https://www.rust-lang.org/tools/install)
- The [SDL2 libraries](https://wiki.libsdl.org/SDL2/Installation)

### Installing Rust and SDL2 on macOS

Install the SDL2 package:
```sh
brew install sdl2
```

Add this to your `~/.zprofile`:
```sh
export LIBRARY_PATH="$LIBRARY_PATH:$(brew --prefix)/lib"
```

Install the Rust toolchain:
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Installing Rust and SDL2 on Debian/Ubuntu

Install the SDL2 package:
```sh
sudo apt-get install libsdl2-dev
```

Install the Rust toolchain:
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Installing Rust and SDL2 on Windows

Follow the Windows-specific instructions to [install the Rust toolchain](https://www.rust-lang.org/tools/install).

Get `SDL2.dll` from one of [SDL2 Releases](https://github.com/libsdl-org/SDL/releases).

Copy `SDL2.dll` (unzip) to the `vm/` folder.

### Compiling the Project

```sh
cd vm
cargo build
```

To run an asm file:
```sh
cargo run examples/fizzbuzz.asm
```

### Running the Test Suite

Run `cargo test` from the `vm`, and `plush` directories.

## Codebase Organization

The repository is organized into a 3 different subprojects, each of which is a Rust codebase which can be compiled with `cargo`:

- `/vm` : The implementation of the virtual machine
  - [`/vm/examples/*`](vm/examples): Example assembly programs that can be run by the VM

## Open Source License

The code for Plush, its VM and associated tools is shared under the [Apache-2.0 license](https://github.com/maximecb/plush/blob/main/LICENSE).

The examples under the `vm/examples` and `plusg/examples` directories are shared under the [Creative Commons CC0](https://creativecommons.org/publicdomain/zero/1.0/) license.

## Contributing

There is a lot of work to be done to get this project going and contributions are welcome.

A good first step is to look at open issues and read the available documentation. Another easy way to contribute
is to create new example programs showcasing cool things you can do with Plush, or to open issues to report bugs.
If you do report bugs, please provide as much context as possible, and the smallest reproduction you can
come up with.

You can also search the codebase for TODO or FIXME notes:
```sh
grep -IRi "todo" .
```

In general, smaller pull requests are easier to review and have a much higher chance of getting merged than large
pull requests. If you would like to add a new, complex feature or refactor the design of Plush, I recommend opening
an issue or starting a discussion about your proposed change first.

Also please keep in mind that one of the core principles of Plush is to minimize dependencies to keep the VM easy
to install and easy to port. Opening a PR that adds dependencies to multiple new packages and libraries is
unlikely to get merged. Again, if you have a valid argument in favor of doing so, please open a discussion to
share your point of view.
