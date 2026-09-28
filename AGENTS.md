Commenting:
- Comments should start with an uppercase letter, e.g.
  // This is a comment
- Try to write a short explanatory comment describing the
  purpose of every function or method above it, except for trivial
  methods such as accessors

Code style:
- Separate function and method declarations with one line of whitespace
- Separate parts of a function that are logically distinct using one line
  of whitespace
- Declare at most one variable per line of code
- Separate operators using one space, e.g.
  let a = 1;
  a = b + c;

Benchmarking:
- Performed interleaved runs and use the median for each configuration
  to mitigate the impact of thermal noise.

When making file edits of any kind, prefer using tools rather than running
shell commands, as shell commands hide the file edits from the user.
