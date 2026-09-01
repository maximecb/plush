#!/usr/bin/env python3

"""
Generate a large Plush source file made of many randomly-generated functions.

The generated program is meant to measure parsing/codegen/startup speed, so the
code it contains is deliberately cheap to run: there is no recursion, loops run
at most 2 iterations and are never nested, and all integer values are kept small
enough that arithmetic can never overflow.

Usage:
    python3 benchmarks/gen_parse_bench.py --num-funs 50000 -o benchmarks/parse_bench.psh
"""

import argparse
import random

# Types we know how to generate expressions for
INT = "int"
FLOAT = "float"
STR = "str"
BOOL = "bool"
ARRAY = "array"
DICT = "dict"

VALUE_TYPES = [INT, FLOAT, STR, BOOL, ARRAY, DICT]

WORDS = [
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
    "plush", "vector", "matrix", "buffer", "token", "symbol", "value", "node",
    "left", "right", "up", "down", "red", "green", "blue", "gray",
]

DICT_KEYS = ["a", "b", "c", "d", "n", "x", "y", "z", "count", "name", "flag"]

# Number of leading functions that call no other function. Later functions may
# only call these, which keeps the call graph acyclic and shallow.
NUM_LEAF_FUNS = 64


class Generator:
    def __init__(self, rng, max_depth):
        self.rng = rng
        self.max_depth = max_depth
        self.lines = []
        self.indent = 0
        self.scopes = []
        self.num_vars = 0
        self.in_loop = False
        self.in_for = False
        self.callable_funs = []

    # --- Output helpers ---

    def emit(self, text):
        self.lines.append("    " * self.indent + text)

    def blank(self):
        self.lines.append("")

    # --- Scope helpers ---

    def push_scope(self):
        self.scopes.append({})

    def pop_scope(self):
        self.scopes.pop()

    def declare(self, name, var_type, mutable, meta=None):
        self.scopes[-1][name] = {"type": var_type, "mutable": mutable, "meta": meta or {}}

    def fresh_name(self):
        self.num_vars += 1
        return "v" + str(self.num_vars)

    def vars_of(self, var_type, mutable=None):
        found = []
        for scope in self.scopes:
            for name, info in scope.items():
                if info["type"] != var_type:
                    continue
                if mutable is not None and info["mutable"] != mutable:
                    continue
                found.append((name, info))
        return found

    def pick_var(self, var_type, mutable=None):
        found = self.vars_of(var_type, mutable)
        if not found:
            return None
        return self.rng.choice(found)

    # --- Literals ---

    def int_lit(self):
        r = self.rng.random()
        if r < 0.1:
            return "0x" + format(self.rng.randint(0, 255), "X")
        if r < 0.15:
            return "0b" + format(self.rng.randint(0, 63), "b")
        if r < 0.2:
            return str(self.rng.randint(1, 99)) + "_000"
        return str(self.rng.randint(-99, 99))

    def small_lit(self):
        return str(self.rng.randint(2, 9))

    def float_lit(self):
        return "{:.3f}".format(self.rng.uniform(-100, 100)).rstrip("0") + "0"

    def str_lit(self):
        quote = "'" if self.rng.random() < 0.2 else '"'
        return quote + self.rng.choice(WORDS) + quote

    # --- Expressions ---

    # Note on parenthesization: expressions whose top-level operator binds
    # looser than comparison (bitwise ops, logical ops, the ternary) are wrapped
    # in parentheses, so that embedding them anywhere keeps their type.
    def expr(self, var_type, depth):
        if var_type == INT:
            return self.int_expr(depth)
        if var_type == FLOAT:
            return self.float_expr(depth)
        if var_type == STR:
            return self.str_expr(depth)
        if var_type == BOOL:
            return self.bool_expr(depth)
        raise ValueError("no plain expression form for type " + var_type)

    # Wrap an integer expression so that its value stays small. Every integer
    # stored in a variable, passed as an argument or returned goes through this,
    # which is what makes overflow impossible in the generated program.
    def bound_int(self, expr):
        choice = self.rng.randint(0, 3)
        if choice == 0:
            return "(" + expr + ") % " + str(self.rng.choice([97, 251, 1009, 65537]))
        if choice == 1:
            return "(" + expr + ").clip(-1000, 1000)"
        if choice == 2:
            return "(" + expr + ") & 0xFFF"
        return "(" + expr + ").abs() % 1000"

    def int_leaf(self):
        var = self.pick_var(INT)
        if var and self.rng.random() < 0.55:
            return var[0]
        return self.int_lit()

    def int_expr(self, depth):
        if depth <= 0:
            return self.int_leaf()

        sub = depth - 1
        kind = self.rng.randint(0, 15)

        if kind <= 1:
            op = self.rng.choice(["+", "-"])
            return self.int_expr(sub) + " " + op + " " + self.int_expr(sub)
        if kind == 2:
            # Only multiply by a small literal, so values stay bounded
            return self.small_lit() + " * (" + self.int_expr(sub) + ")"
        if kind == 3:
            return "(" + self.int_expr(sub) + ") % " + str(self.rng.randint(2, 999))
        if kind == 4:
            return "(" + self.int_expr(sub) + ").idiv(" + str(self.rng.randint(2, 17)) + ")"
        if kind == 5:
            op = self.rng.choice(["&", "|", "^"])
            return "((" + self.int_expr(sub) + ") " + op + " 0x" + format(self.rng.randint(1, 255), "X") + ")"
        if kind == 6:
            op = self.rng.choice(["<<", ">>"])
            return "((" + self.int_expr(sub) + ") " + op + " " + str(self.rng.randint(1, 3)) + ")"
        if kind == 7:
            return "-(" + self.int_expr(sub) + ")"
        if kind == 8:
            method = self.rng.choice(["abs()", "min(" + self.int_lit() + ")",
                                      "max(" + self.int_lit() + ")", "clip(-500, 500)"])
            return "(" + self.int_expr(sub) + ")." + method
        if kind == 9:
            return "((" + self.bool_expr(sub) + ")? " + self.int_expr(sub) + " : " + self.int_expr(sub) + ")"
        if kind == 10:
            return "(" + self.float_expr(sub) + ").clip(-1000.0, 1000.0)." + \
                self.rng.choice(["floor()", "ceil()", "trunc()"])
        if kind == 11:
            return "(" + self.str_expr(sub) + ").len"
        if kind == 12:
            var = self.pick_var(ARRAY)
            if var:
                idx = self.rng.randrange(var[1]["meta"]["len"])
                return var[0] + "[" + str(idx) + "]"
        if kind == 13:
            var = self.pick_var(DICT)
            if var:
                keys = var[1]["meta"]["int_keys"]
                if keys:
                    key = self.rng.choice(keys)
                    if self.rng.random() < 0.3:
                        return var[0] + '["' + key + '"]'
                    return var[0] + "." + key
        if kind == 14:
            var = self.pick_var("fun1")
            if var:
                return var[0] + "(" + self.bound_int(self.int_expr(sub)) + ")"
        if kind == 15 and self.callable_funs:
            return self.call_expr(sub)

        return self.int_leaf()

    def call_expr(self, depth):
        name, arity = self.rng.choice(self.callable_funs)
        args = [self.bound_int(self.int_expr(depth)) for _ in range(arity)]
        return name + "(" + ", ".join(args) + ")"

    def float_expr(self, depth):
        var = self.pick_var(FLOAT)
        if depth <= 0:
            if var and self.rng.random() < 0.5:
                return var[0]
            return self.float_lit()

        sub = depth - 1
        kind = self.rng.randint(0, 8)

        if kind <= 1:
            op = self.rng.choice(["+", "-"])
            return self.float_expr(sub) + " " + op + " " + self.float_expr(sub)
        if kind == 2:
            return self.small_lit() + ".0 * (" + self.float_expr(sub) + ")"
        if kind == 3:
            return "(" + self.float_expr(sub) + ") / " + str(self.rng.randint(2, 9)) + ".0"
        if kind == 4:
            return "(" + self.int_expr(sub) + ").to_f()"
        if kind == 5:
            method = self.rng.choice(["abs()", "sin()", "cos()", "atan()",
                                      "clip(-100.0, 100.0)", "min(2.5)", "max(-2.5)"])
            return "(" + self.float_expr(sub) + ")." + method
        if kind == 6:
            # Take the absolute value first, so we never produce a NaN
            return "(" + self.float_expr(sub) + ").abs().sqrt()"
        if kind == 7:
            return "((" + self.bool_expr(sub) + ")? " + self.float_expr(sub) + " : " + self.float_expr(sub) + ")"
        if var:
            return var[0]
        return self.float_lit()

    # `allow_vars` is turned off when the result is stored in a string
    # variable. Referring to string variables there would let a string grow by
    # a constant factor per assignment, which gets very slow inside loops.
    def str_expr(self, depth, allow_vars=True):
        var = self.pick_var(STR) if allow_vars else None
        if depth <= 0:
            if var and self.rng.random() < 0.5:
                return var[0]
            return self.str_lit()

        sub = depth - 1
        kind = self.rng.randint(0, 5)

        if kind <= 1:
            return self.str_expr(sub, allow_vars) + " + " + self.str_expr(sub, allow_vars)
        if kind == 2:
            return "(" + self.int_expr(sub) + ").to_s()"
        if kind == 3:
            return "(" + self.float_expr(sub) + ").format_decimals(2)"
        if kind == 4:
            return "(" + self.str_expr(sub, allow_vars) + ")." + \
                self.rng.choice(["upper()", "lower()", "trim()"])
        return "((" + self.bool_expr(sub) + ")? " + self.str_expr(sub, allow_vars) + \
            " : " + self.str_expr(sub, allow_vars) + ")"

    def bool_expr(self, depth):
        var = self.pick_var(BOOL)
        if depth <= 0:
            if var and self.rng.random() < 0.4:
                return var[0]
            return self.rng.choice(["true", "false"])

        sub = depth - 1
        kind = self.rng.randint(0, 7)

        if kind <= 2:
            op = self.rng.choice(["<", "<=", ">", ">=", "==", "!="])
            operand_type = self.rng.choice([INT, INT, FLOAT, STR])
            return "(" + self.expr(operand_type, sub) + " " + op + " " + self.expr(operand_type, sub) + ")"
        if kind == 3:
            op = self.rng.choice(["&&", "||"])
            return "((" + self.bool_expr(sub) + ") " + op + " (" + self.bool_expr(sub) + "))"
        if kind == 4:
            return "!(" + self.bool_expr(sub) + ")"
        if kind == 5:
            dict_var = self.pick_var(DICT)
            if dict_var:
                return dict_var[0] + '.has("' + self.rng.choice(DICT_KEYS) + '")'
        if kind == 6 and var:
            return var[0]
        return self.rng.choice(["true", "false"])

    def array_expr(self, depth):
        sub = max(depth - 1, 0)
        if self.rng.random() < 0.25:
            size = self.rng.randint(1, 8)
            return "Array.with_size(" + str(size) + ", " + self.bound_int(self.int_expr(sub)) + ")", size
        size = self.rng.randint(1, 5)
        elems = [self.bound_int(self.int_expr(sub)) for _ in range(size)]
        return "[" + ", ".join(elems) + "]", size

    def dict_expr(self, depth):
        sub = max(depth - 1, 0)
        keys = self.rng.sample(DICT_KEYS, self.rng.randint(1, 4))
        int_keys = []
        fields = []
        for key in keys:
            field_type = self.rng.choice([INT, INT, FLOAT, STR, BOOL])
            if field_type == INT:
                int_keys.append(key)
            name = '"' + key + '"' if self.rng.random() < 0.2 else key
            fields.append(name + ": " + self.expr(field_type, sub))
        return "{ " + ", ".join(fields) + " }", int_keys

    # --- Statements ---

    def stmt(self, depth):
        """Emit one statement. `depth` limits nesting of blocks."""
        kinds = ["let", "let", "let_var", "assign", "compound", "incr", "if",
                 "array_op", "dict_op", "lambda", "nested_fun", "call", "assert"]
        if depth <= 0:
            kinds = ["let", "let_var", "assign", "compound", "incr", "array_op",
                     "dict_op", "lambda", "call", "assert"]
        if self.in_loop:
            # No nested loops: the point is to benchmark parsing, not running
            kinds = kinds + ["break_stmt"]
        elif depth > 0:
            kinds = kinds + ["for", "while"]

        kind = self.rng.choice(kinds)
        getattr(self, "stmt_" + kind)(depth)

    def decl_stmt(self, mutable):
        var_type = self.rng.choice(VALUE_TYPES)
        name = self.fresh_name()
        meta = {}

        if var_type == ARRAY:
            init, size = self.array_expr(self.max_depth - 1)
            meta["len"] = size
        elif var_type == DICT:
            init, int_keys = self.dict_expr(self.max_depth - 1)
            meta["int_keys"] = int_keys
        elif var_type == INT:
            init = self.bound_int(self.int_expr(self.max_depth))
        elif var_type == STR:
            init = self.str_expr(self.max_depth, allow_vars=False)
        else:
            init = self.expr(var_type, self.max_depth)

        self.emit("let " + ("var " if mutable else "") + name + " = " + init + ";")
        self.declare(name, var_type, mutable, meta)

    def stmt_let(self, depth):
        self.decl_stmt(False)

    def stmt_let_var(self, depth):
        self.decl_stmt(True)

    def stmt_assign(self, depth):
        var = self.pick_var(self.rng.choice([INT, FLOAT, STR, BOOL]), mutable=True)
        if var is None:
            self.decl_stmt(True)
            return
        name, info = var
        if info["type"] == INT:
            value = self.bound_int(self.int_expr(self.max_depth))
        elif info["type"] == STR:
            value = self.str_expr(self.max_depth, allow_vars=False)
        else:
            value = self.expr(info["type"], self.max_depth)
        self.emit(name + " = " + value + ";")

    def stmt_compound(self, depth):
        var = self.pick_var(self.rng.choice([INT, INT, FLOAT, STR]), mutable=True)
        if var is None:
            self.decl_stmt(True)
            return
        name, info = var
        if info["type"] == INT:
            # Only operators that keep the value small are used here
            op = self.rng.choice(["+=", "-=", "%=", "&=", "|=", "^="])
            if op in ("+=", "-="):
                rhs = str(self.rng.randint(1, 99))
            elif op == "%=":
                rhs = str(self.rng.randint(2, 999))
            else:
                rhs = "0x" + format(self.rng.randint(1, 255), "X")
        elif info["type"] == FLOAT:
            # No `*=`, so that repeated updates in a loop can't blow up
            op = self.rng.choice(["+=", "-=", "/="])
            rhs = self.float_lit() if op in ("+=", "-=") else str(self.rng.randint(2, 9)) + ".0"
        else:
            op = "+="
            rhs = self.str_lit()
        self.emit(name + " " + op + " " + rhs + ";")

    def stmt_incr(self, depth):
        var = self.pick_var(INT, mutable=True)
        if var is None:
            self.decl_stmt(True)
            return
        self.emit(self.rng.choice(["++", "--"]) + var[0] + ";")

    def block(self, depth, min_stmts=1, max_stmts=3):
        """Emit a braced block containing a few statements."""
        self.emit("{")
        self.indent += 1
        self.push_scope()
        for _ in range(self.rng.randint(min_stmts, max_stmts)):
            self.stmt(depth - 1)
        self.pop_scope()
        self.indent -= 1
        self.emit("}")

    def simple_stmt(self):
        """Emit a statement that is guaranteed to be a single statement."""
        kinds = ["let", "let_var", "assign", "compound", "incr", "assert",
                 "array_op", "dict_op", "return_stmt"]
        if self.in_loop:
            kinds += ["loop_jump"]
        getattr(self, "stmt_" + self.rng.choice(kinds))(0)

    def stmt_return_stmt(self, depth):
        self.emit("return " + self.bound_int(self.int_expr(self.max_depth - 1)) + ";")

    def stmt_loop_jump(self, depth):
        # `continue` is only safe in a for loop, where the loop header does the
        # incrementing. A while loop increments its counter in the body.
        self.emit(self.rng.choice(["break;", "continue;"]) if self.in_for else "break;")

    def body_or_block(self, depth):
        """Either a braced block, or a single statement without braces."""
        if depth > 0 and self.rng.random() < 0.7:
            self.block(depth)
        else:
            self.indent += 1
            self.push_scope()
            self.simple_stmt()
            self.pop_scope()
            self.indent -= 1

    def stmt_if(self, depth):
        self.emit("if (" + self.bool_expr(self.max_depth) + ")")
        self.body_or_block(depth)
        if self.rng.random() < 0.4:
            self.emit("else")
            self.body_or_block(depth)

    def stmt_for(self, depth):
        name = self.fresh_name()
        count = self.rng.randint(1, 2)
        self.emit("for (let var " + name + " = 0; " + name + " < " + str(count) + "; ++" + name + ")")
        self.emit("{")
        self.indent += 1
        self.push_scope()
        self.declare(name, INT, False)
        was_in_loop, was_in_for = self.in_loop, self.in_for
        self.in_loop, self.in_for = True, True
        for _ in range(self.rng.randint(1, 3)):
            self.stmt(depth - 1)
        if self.rng.random() < 0.2:
            self.emit("if (" + name + " > " + str(count) + ") continue;")
        self.in_loop, self.in_for = was_in_loop, was_in_for
        self.pop_scope()
        self.indent -= 1
        self.emit("}")

    def stmt_while(self, depth):
        name = self.fresh_name()
        count = self.rng.randint(1, 2)
        self.emit("let var " + name + " = 0;")
        self.declare(name, INT, False)
        self.emit("while (" + name + " < " + str(count) + ")")
        self.emit("{")
        self.indent += 1
        self.push_scope()
        was_in_loop, was_in_for = self.in_loop, self.in_for
        self.in_loop, self.in_for = True, False
        for _ in range(self.rng.randint(1, 2)):
            self.stmt(depth - 1)
        self.in_loop, self.in_for = was_in_loop, was_in_for
        # The counter is always incremented, so the loop terminates
        self.emit(name + " = " + name + " + 1;")
        self.pop_scope()
        self.indent -= 1
        self.emit("}")

    def stmt_break_stmt(self, depth):
        self.emit("if (" + self.bool_expr(1) + ") break;")

    def stmt_array_op(self, depth):
        var = self.pick_var(ARRAY)
        if var is None:
            self.decl_stmt(self.rng.random() < 0.5)
            return
        name, info = var
        kind = self.rng.randint(0, 2)
        if kind == 0:
            idx = self.rng.randrange(info["meta"]["len"])
            self.emit(name + "[" + str(idx) + "] = " + self.bound_int(self.int_expr(self.max_depth)) + ";")
        elif kind == 1:
            self.emit(name + ".push(" + self.bound_int(self.int_expr(self.max_depth)) + ");")
        else:
            new_name = self.fresh_name()
            self.emit("let " + new_name + " = " + name + ".len;")
            self.declare(new_name, INT, False)

    def stmt_dict_op(self, depth):
        var = self.pick_var(DICT)
        if var is None:
            self.decl_stmt(self.rng.random() < 0.5)
            return
        name, info = var
        # Only write integer keys that are already present, so the type of a
        # field never depends on whether a conditional branch was taken
        int_keys = info["meta"]["int_keys"]
        if not int_keys:
            self.decl_stmt(self.rng.random() < 0.5)
            return
        key = self.rng.choice(int_keys)
        value = self.bound_int(self.int_expr(self.max_depth))
        if self.rng.random() < 0.5:
            self.emit(name + "." + key + " = " + value + ";")
        else:
            self.emit(name + '["' + key + '"] = ' + value + ";")

    def stmt_lambda(self, depth):
        name = self.fresh_name()
        param = self.fresh_name()
        self.push_scope()
        self.declare(param, INT, False)
        if self.rng.random() < 0.5:
            body = self.bound_int(self.int_expr(self.max_depth - 1))
            self.pop_scope()
            self.emit("let " + name + " = |" + param + "| " + body + ";")
        else:
            body = self.bound_int(self.int_expr(self.max_depth - 1))
            self.pop_scope()
            self.emit("let " + name + " = |" + param + "| { return " + body + "; };")
        self.declare(name, "fun1", False)
        if self.rng.random() < 0.5:
            self.emit(name + "(" + self.int_lit() + ");")

    def stmt_nested_fun(self, depth):
        name = self.fresh_name()
        param = self.fresh_name()
        self.push_scope()
        self.declare(param, INT, False)
        body = self.bound_int(self.int_expr(self.max_depth - 1))
        self.pop_scope()
        self.emit("fun " + name + "(" + param + ")")
        self.emit("{")
        self.indent += 1
        self.emit("return " + body + ";")
        self.indent -= 1
        self.emit("}")
        self.declare(name, "fun1", False)

    def stmt_call(self, depth):
        var = self.pick_var("fun1")
        if var and self.rng.random() < 0.4:
            self.emit(var[0] + "(" + self.bound_int(self.int_expr(1)) + ");")
            return
        if not self.callable_funs:
            self.stmt_lambda(depth)
            return
        name = self.fresh_name()
        self.emit("let " + name + " = " + self.call_expr(self.max_depth - 1) + ";")
        self.declare(name, INT, False)

    def stmt_assert(self, depth):
        a = self.rng.randint(0, 50)
        b = self.rng.randint(51, 100)
        forms = [
            "assert(true);",
            "assert(" + str(a) + " < " + str(b) + ");",
            "assert(" + str(a) + " + " + str(b) + " == " + str(a + b) + ");",
        ]
        self.emit(self.rng.choice(forms))

    # --- Functions ---

    def gen_function(self, name, num_stmts):
        self.num_vars = 0
        self.push_scope()

        num_params = self.rng.randint(0, 3)
        params = []
        for _ in range(num_params):
            param = self.fresh_name()
            params.append(param)
            self.declare(param, INT, False)

        self.emit("fun " + name + "(" + ", ".join(params) + ")")
        self.emit("{")
        self.indent += 1

        for _ in range(num_stmts):
            self.stmt(2)

        # The return value is always bounded, so callers can use it freely
        self.emit("return " + self.bound_int(self.int_expr(self.max_depth)) + ";")

        self.indent -= 1
        self.emit("}")
        self.pop_scope()

        return num_params


def generate(num_funs, seed, max_depth, max_stmts):
    rng = random.Random(seed)
    gen = Generator(rng, max_depth)

    gen.emit("// Auto-generated by benchmarks/gen_parse_bench.py -- do not edit by hand.")
    gen.emit("// " + str(num_funs) + " randomly-generated functions, each called once at the end.")
    gen.emit("// Every function is quick to run: no recursion, and loops run at most 2 iterations.")
    gen.blank()

    funs = []
    for idx in range(num_funs):
        name = "f" + str(idx)
        # Reserve one statement slot for the return statement
        num_stmts = rng.randint(1, max_stmts - 1)
        num_params = gen.gen_function(name, num_stmts)
        gen.blank()
        funs.append((name, num_params))
        if idx < NUM_LEAF_FUNS:
            gen.callable_funs.append((name, num_params))

    gen.emit("// Call every function once")
    for name, num_params in funs:
        args = [str(rng.randint(-99, 99)) for _ in range(num_params)]
        gen.emit(name + "(" + ", ".join(args) + ");")

    gen.blank()
    gen.emit('$println("done");')

    return "\n".join(gen.lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description="Generate a large Plush file for parser benchmarking")
    parser.add_argument("-o", "--output", default="benchmarks/parse_bench.psh")
    parser.add_argument("-n", "--num-funs", type=int, default=50000)
    parser.add_argument("-s", "--seed", type=int, default=1337)
    parser.add_argument("--max-depth", type=int, default=3, help="Maximum expression tree depth")
    parser.add_argument("--max-stmts", type=int, default=10, help="Maximum statements per function")
    args = parser.parse_args()

    source = generate(args.num_funs, args.seed, args.max_depth, args.max_stmts)

    with open(args.output, "w") as file:
        file.write(source)

    num_lines = source.count("\n")
    print("Wrote {} ({} functions, {} lines, {:.1f} MiB)".format(
        args.output, args.num_funs, num_lines, len(source) / (1024 * 1024)))


if __name__ == "__main__":
    main()
