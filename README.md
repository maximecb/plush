# Plush

Plush is an experimental toy programming language and virtual machine for fun and teaching purposes.
It follows a minimalistic design philosphy and draws inspiration from JavaScript, [Lox](https://craftinginterpreters.com/the-lox-language.html), Lua, Python and Rust. Please do note that this project is very much a fun side-project and a work in progress. You're likely to run into bugs and missing features.

If you think that Plush is cool, you can support my work via [GitHub Sponsors](https://github.com/sponsors/maximecb) :heart:

Some cool/fun/notable features:
- Simple JavaScript/lox inspired syntax and straightforward semantics
- Simple stack-based bytecode interpreter
- Memory safe, actor-based parallelism
- Easily extensible set of host functions
- Very few dependencies, good cross-platform support
- Well-commented codebase

The Plush language supports:
- Dynamic typing
- Classes
- Closures/lambdas
- Dynamic arrays aka vectors/lists
- UTF-8 strings (work in progress)
- Dictionaries with JS-like syntax (work in progress)
- Host functions defined in `src/host.rs`

Caveats and limitations:
- The language is still very much incomplete
  - You may run into bugs or missing features
- Lack of error messages or good error handling
  - You may have to look at the source code to figure things out
  - We could use your help in improving error messages
- Copying garbage collector planned but not yet implemented
  - Currently relying on alloc-until-you-crash technology (TM)
- Imports, support for multiple source files planned but not yet implemented

For a detailed guide to the Plush language, please see the [Plush Language Quickstart](docs/language.md).

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

Copy `SDL2.dll` (unzip) to the root of your cloned repo.

### Compiling the Project

```sh
cargo build
```

To run the test suite:
```sh
cargo test
```

To run a plush script:
```sh
# See example programs under /examples, /benchmarks and /tests
cargo run examples/helloworld.psh
cargo run examples/plasma.psh
```

## Open Source License

The code for Plush, its VM and associated tools is shared under the [Apache-2.0 license](https://github.com/maximecb/plush/blob/main/LICENSE). The example code in the `/examples` directory are shared under the [Creative Commons CC0](https://creativecommons.org/publicdomain/zero/1.0/) license.

## Contributing

There is a lot of work to be done to get this project going and contributions are welcome.

A good first step is to look at open issues and read the available documentation. Another easy way to contribute
is to create new example programs showcasing cool things you can do with Plush, or to
[report any bugs and issues](https://github.com/maximecb/plush/issues) you encounter.
If you do report bugs, please provide as much context as possible, and the smallest reproduction you can
come up with.

You can also search the codebase for TODO or FIXME notes:
```sh
grep -IRi "todo" .
```

In general, smaller pull requests are easier to review and have a much higher chance of getting merged than large
pull requests. If you would like to add a new, complex feature or refactor the design of Plush, I recommend opening
an issue or starting a discussion about your proposed changes first.

Also please keep in mind that one of the core principles of Plush is to minimize dependencies to keep the VM easy
to install and easy to port. Opening a PR that adds dependencies to multiple new packages and libraries is
unlikely to get merged. If you believe you have a valid argument in favor of doing so, please open an issue to
share your point of view.
