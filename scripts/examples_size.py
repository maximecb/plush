#!/usr/bin/env python3

# Report the size of every file under examples/, largest first, and the total.

import argparse
import os

def repo_root():
    return os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

def format_size(num_bytes):
    for unit in ["B", "KiB", "MiB", "GiB"]:
        if num_bytes < 1024 or unit == "GiB":
            if unit == "B":
                return f"{num_bytes} B"
            return f"{num_bytes:.1f} {unit}"
        num_bytes /= 1024

def collect_files(root):
    files = []
    for dir_path, _, file_names in os.walk(root):
        for name in sorted(file_names):
            path = os.path.join(dir_path, name)
            if os.path.islink(path):
                continue
            files.append((path, os.path.getsize(path)))
    return files

def main():
    parser = argparse.ArgumentParser(description="Compute the total size of the examples folder")
    parser.add_argument("--dir", default=os.path.join(repo_root(), "examples"))
    parser.add_argument("--bytes", action="store_true", help="Print raw byte counts")
    args = parser.parse_args()

    root = os.path.abspath(args.dir)
    if not os.path.isdir(root):
        parser.error(f"not a directory: {root}")

    # Largest first, and files of the same size by name
    files = collect_files(root)
    files.sort(key=lambda entry: (-entry[1], os.path.relpath(entry[0], root)))

    total = 0
    for path, size in files:
        total += size
        size_str = str(size) if args.bytes else format_size(size)
        print(f"{size_str:>10}  {os.path.relpath(path, root)}")

    total_str = str(total) if args.bytes else format_size(total)
    print(f"\n{len(files)} files, {total_str} total")

if __name__ == "__main__":
    main()
