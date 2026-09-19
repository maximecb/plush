Commenting:
- Avoid overcommenting:
  - Keep comments concise and relevant in lexical context
  - Avoid long multi-line comments
- Comments should start with an uppercase letter, e.g.
  // This is a comment

When making file edits of any kind, prefer using tools rather than running
shell commands, as shell commands hide the file edits from the user.

Benchmarking:
- Performed interleaved runs and use the median for each configuration
  to mitigate the impact of thermal noise.
