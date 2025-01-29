use std::collections::HashMap;
use std::io;
use std::io::Read;
use std::cmp::max;
use crate::parsing::*;
use crate::ast::*;

/// Parse an atomic expression
fn parse_atom(input: &mut Input) -> Result<ExprBox, ParseError>
{
    input.eat_ws()?;
    let ch = input.peek_ch();
    let pos = input.get_pos();

    // Hexadecimal integer literal
    if input.match_token("0x")? {
        let int_val = input.parse_int(16)?;
        return Ok(ExprBox::new(
            Expr::Int(int_val),
            pos
        ));
    }

    // Binary integer literal
    if input.match_token("0b")? {
        let int_val = input.parse_int(2)?;
        return Ok(ExprBox::new(
            Expr::Int(int_val),
            pos
        ));
    }

    // Decimal numeric value
    if ch.is_digit(10) {
        let num_str = input.read_numeric();
        //println!("{}", num_str);

        // If we can parse this value as an integer
        if let Ok(int_val) = num_str.parse::<i128>() {
            return Ok(ExprBox::new(
                Expr::Int(int_val),
                pos
            ));
        }

        // Parse this value as a floating-point number
        let float_val: f64 = num_str.parse().unwrap();

        return Ok(ExprBox::new(
            Expr::Float64(float_val),
            pos
        ));
    }

    if input.match_keyword("true")? {
        return Ok(ExprBox::new(
            Expr::True,
            pos
        ));
    }

    if input.match_keyword("false")? {
        return Ok(ExprBox::new(
            Expr::False,
            pos
        ));
    }

    if input.match_keyword("none")? {
        return Ok(ExprBox::new(
            Expr::None,
            pos
        ));
    }

    // String literal
    if ch == '\"' || ch == '\'' {
        let mut str_val = "".to_string();

        // As a convenience feature, we concatenate multiple
        // double-quoted strings in a row, but not single-quoted strings.
        // This makes it less error-prone to write machine code with
        // single-quoted VM instructions.
        loop
        {
            str_val += &input.parse_str(ch)?;

            if ch == '\'' {
                break;
            }

            input.eat_ws()?;
            if input.peek_ch() != ch {
                break;
            }
        }

        return Ok(ExprBox::new(
            Expr::String(str_val),
            pos,
        ));
    }

    // Parenthesized expression or type casting expression
    if ch == '(' {
        input.eat_ch();
        let expr = parse_expr(input)?;
        input.expect_token(")")?;
        return Ok(expr);
    }

    // Array literal
    if input.match_char('[') {
        let exprs = parse_expr_list(input, "]")?;
        return Ok(ExprBox::new(
            Expr::Array { frozen: true,  exprs },
            pos,
        ));
    }
    if input.match_token("*[")? {
        let exprs = parse_expr_list(input, "]")?;
        return Ok(ExprBox::new(
            Expr::Array { frozen: false,  exprs },
            pos,
        ));
    }

    // Object literal
    if input.match_char('{') {
        return parse_object(input, false, pos);
    }
    if input.match_token("+{")? {
        return parse_object(input, true, pos);
    }

    // Host function call
    if ch == '$' {
        input.eat_ch();
        let fun_name = input.parse_ident()?;
        input.expect_token("(")?;
        let arg_exprs = parse_expr_list(input, ")")?;

        return ExprBox::new_ok(
            Expr::HostCall {
                fun_name,
                args: arg_exprs
            },
            pos
        );
    }

    // Function expression
    if input.match_keyword("fun")? {
        input.eat_ws()?;

        let mut name = "".to_string();
        if input.peek_ch() != '(' {
            name = input.parse_ident()?;
        }

        let fun = parse_function(input, name, pos)?;

        return ExprBox::new_ok(
            Expr::Fun(Box::new(fun)),
            pos
        );
    }

    // Identifier (variable reference)
    if is_ident_start(ch) {
        let ident = input.parse_ident()?;
        return Ok(ExprBox::new(
            Expr::Ident(ident),
            pos,
        ));
    }

    input.parse_error("unknown atomic expression")
}

/// Parse a postfix expression
fn parse_postfix(input: &mut Input) -> Result<ExprBox, ParseError>
{
    let mut base_expr = parse_atom(input)?;

    loop
    {
        input.eat_ws()?;
        let pos = input.get_pos();

        // If this is a function call
        if input.match_token("(")? {
            let arg_exprs = parse_expr_list(input, ")")?;

            base_expr = ExprBox::new(
                Expr::Call {
                    callee: base_expr,
                    args: arg_exprs
                },
                pos
            );

            continue;
        }

        // Array indexing
        if input.match_token("[")? {
            let index_expr = parse_expr(input)?;
            input.expect_token("]")?;

            base_expr = ExprBox::new(
                Expr::Index {
                    base: base_expr,
                    index: index_expr
                },
                pos
            );

            continue;
        }

        // Member operator (a.b)
        if input.match_token(".")? {
            let field_name = input.parse_ident()?;

            base_expr = ExprBox::new(
                Expr::Member {
                    base: base_expr,
                    field: field_name
                },
                pos
            );

            continue;
        }

        /*
        // Postfix increment expression
        if input.match_token("++")? {
            // Let users know this is not supported. We use panic!() because
            // backtracking may override our error message.
            panic!(concat!(
                "the postfix increment operator (i.e. i++) is not supported, ",
                "use prefix increment (i.e. ++i) instead."
            ));
        }

        // Postfix decrement expression
        if input.match_token("--")? {
            // Let users know this is not supported. We use panic!() because
            // backtracking may override our error message.
            panic!(concat!(
                "the postfix increment operator (i.e. i--) is not supported, ",
                "use prefix increment (i.e. --i) instead."
            ));
        }
        */

        break;
    }

    Ok(base_expr)
}

/// Parse an prefix expression
/// Note: this function should only call parse_postfix directly
/// to respect the priority of operations in C
fn parse_prefix(input: &mut Input) -> Result<ExprBox, ParseError>
{
    input.eat_ws()?;
    let ch = input.peek_ch();
    let pos = input.get_pos();

    // Unary not expression (bitwise or logical not)
    if ch == '!' {
        input.eat_ch();
        let child = parse_prefix(input)?;

        return ExprBox::new_ok(
            Expr::Unary {
                op: UnOp::Not,
                child
            },
            pos,
        );
    }

    /*
    // Pre-increment expression
    if input.match_token("++")? {
        let sub_expr = parse_prefix(input)?;

        // Transform into i = i + 1
        return Ok(
            Expr::Binary{
                op: BinOp::Assign,
                lhs: Box::new(sub_expr.clone()),
                rhs: Box::new(Expr::Binary{
                    op: BinOp::Add,
                    lhs: Box::new(sub_expr.clone()),
                    rhs: Box::new(Expr::Int(1))
                })
            }
        );
    }
    */

    /*
    // Pre-decrement expression
    if input.match_token("--")? {
        let sub_expr = parse_prefix(input)?;

        // Transform into i = i - 1
        return Ok(
            Expr::Binary{
                op: BinOp::Assign,
                lhs: Box::new(sub_expr.clone()),
                rhs: Box::new(Expr::Binary{
                    op: BinOp::Sub,
                    lhs: Box::new(sub_expr.clone()),
                    rhs: Box::new(Expr::Int(1))
                })
            }
        );
    }
    */

    // Unary minus expression
    if ch == '-' {
        input.eat_ch();
        let sub_expr = parse_prefix(input)?;

        // If this is an integer or floating-point value, negate it
        let expr = match *sub_expr.expr {
            Expr::Int(int_val) => Expr::Int(-int_val),
            Expr::Float64(f_val) => Expr::Float64(-f_val),
            _ => Expr::Unary{
                op: UnOp::Minus,
                child: sub_expr.clone(),
            }
        };

        return ExprBox::new_ok(
            expr,
            sub_expr.pos
        );
    }

    /*
    // Unary plus expression
    if ch == '+' {
        input.eat_ch();
        let sub_expr = parse_prefix(input)?;

        // If this is an integer or floating-point value, negate it
        let expr = match sub_expr {
            Expr::Int(int_val) => sub_expr,
            Expr::Float32(f_val) => sub_expr,
            _ => return input.parse_error("plus operator applied to non-constant value")
        };

        return Ok(expr)
    }
    */

    if input.match_keyword("typeof")? {
        let child = parse_prefix(input)?;

        return ExprBox::new_ok(
            Expr::Unary {
                op: UnOp::TypeOf,
                child
            },
            pos,
        );
    }

    // Try to parse this as a postfix expression
    parse_postfix(input)
}

// Parse an object literal
fn parse_object(
    input: &mut Input,
    extensible: bool,
    pos: SrcPos,
) -> Result<ExprBox, ParseError>
{
    // List of mutable, key, value triplets
    let mut mut_key_val = Vec::default();

    loop
    {
        input.eat_ws()?;

        if input.eof() {
            return input.parse_error("unexpected end of input inside object");
        }

        if input.match_token("}")? {
            break;
        }

        let mut mutable = false;
        if input.match_char('~') {
            mutable = true;
        }

        // Parse a field name
        let field_name = input.parse_ident()?;

        // If this is a method definition
        input.eat_ws()?;
        if input.peek_ch() == '(' {

            let fun = parse_function(input, field_name.clone(), pos)?;

            let fun_expr = ExprBox::new(
                Expr::Fun(Box::new(fun)),
                pos
            );

            mut_key_val.push((mutable, field_name, fun_expr));

            if input.match_token("}")? {
                break;
            }

            continue;
        }

        // Parse the field value
        input.expect_token(":")?;
        let field_expr = parse_expr(input)?;
        mut_key_val.push((mutable, field_name, field_expr));

        if input.match_token("}")? {
            break;
        }

        // If this isn't the last field, there
        // has to be a comma separator
        input.expect_token(",")?;
    }

    let obj_expr = Expr::Object {
        extensible,
        fields: mut_key_val,
    };

    ExprBox::new_ok(
        obj_expr,
        pos
    )
}

/// Parse a list of argument expressions
fn parse_expr_list(input: &mut Input, end_token: &str) -> Result<Vec<ExprBox>, ParseError>
{
    let mut arg_exprs = Vec::default();

    loop {
        input.eat_ws()?;

        if input.eof() {
            return input.parse_error("unexpected end of input in call expression");
        }

        if input.match_token(end_token)? {
            break;
        }

        // Parse one argument
        arg_exprs.push(parse_expr(input)?);

        if input.match_token(end_token)? {
            break;
        }

        // If this isn't the last argument, there
        // has to be a comma separator
        input.expect_token(",")?;
    }

    Ok(arg_exprs)
}

struct OpInfo
{
    op_str: &'static str,
    prec: usize,
    op: BinOp,
    rtl: bool,
}

/// Binary operators and their precedence level
/// Lower numbers mean higher precedence
/// https://en.cppreference.com/w/c/language/operator_precedence
const BIN_OPS: [OpInfo; 19] = [
    OpInfo { op_str: "*", prec: 3, op: BinOp::Mul, rtl: false },
    OpInfo { op_str: "/", prec: 3, op: BinOp::Div, rtl: false },
    OpInfo { op_str: "%", prec: 3, op: BinOp::Mod, rtl: false },
    OpInfo { op_str: "+", prec: 4, op: BinOp::Add, rtl: false },
    OpInfo { op_str: "-", prec: 4, op: BinOp::Sub, rtl: false },

    OpInfo { op_str: "<<", prec: 5, op: BinOp::LShift, rtl: false },
    OpInfo { op_str: ">>", prec: 5, op: BinOp::RShift, rtl: false },

    OpInfo { op_str: "<=", prec: 6, op: BinOp::Le, rtl: false },
    OpInfo { op_str: "<" , prec: 6, op: BinOp::Lt, rtl: false },
    OpInfo { op_str: ">=", prec: 6, op: BinOp::Ge, rtl: false },
    OpInfo { op_str: ">" , prec: 6, op: BinOp::Gt, rtl: false },
    OpInfo { op_str: "==", prec: 7, op: BinOp::Eq, rtl: false },
    OpInfo { op_str: "!=", prec: 7, op: BinOp::Ne, rtl: false },

    // Logical and, logical or
    // We place these first because they are longer tokens
    OpInfo { op_str: "&&", prec: 11, op: BinOp::And, rtl: false },
    OpInfo { op_str: "||", prec: 12, op: BinOp::Or, rtl: false },

    OpInfo { op_str: "&", prec: 8, op: BinOp::BitAnd, rtl: false },
    OpInfo { op_str: "^", prec: 9, op: BinOp::BitXor, rtl: false },
    OpInfo { op_str: "|", prec: 10, op: BinOp::BitOr, rtl: false },

    // Assignment operator, evaluates right to left
    OpInfo { op_str: "=", prec: 14, op: BinOp::Assign, rtl: true },
];

/// Precedence level of the ternary operator (a? b:c)
const TERNARY_PREC: usize = 13;

/// Try to match a binary operator in the input
fn match_bin_op(input: &mut Input) -> Result<Option<OpInfo>, ParseError>
{
    for op_info in BIN_OPS {
        if input.match_token(op_info.op_str)? {
            return Ok(Some(op_info));
        }
    }

    Ok(None)
}

/// Parse a complex infix expression
/// This uses the shunting yard algorithm to parse infix expressions:
/// https://en.wikipedia.org/wiki/Shunting_yard_algorithm
fn parse_expr(input: &mut Input) -> Result<ExprBox, ParseError>
{
    // Operator stack
    let mut op_stack: Vec<OpInfo> = Vec::default();

    // Expression stack
    let mut expr_stack: Vec<ExprBox> = Vec::default();

    // Parse the prefix sub-expression
    expr_stack.push(parse_prefix(input)?);

    // Evaluate the operators on the stack with lower
    // precedence than a new operator we just read
    fn eval_lower_prec(op_stack: &mut Vec<OpInfo>, expr_stack: &mut Vec<ExprBox>, new_op_prec: usize)
    {
        while op_stack.len() > 0 {
            // Get the operator at the top of the stack
            let top_op = &op_stack[op_stack.len() - 1];

            if top_op.prec <= new_op_prec {
                assert!(expr_stack.len() >= 2);
                let rhs = expr_stack.pop().unwrap();
                let lhs = expr_stack.pop().unwrap();
                let top_op = op_stack.pop().unwrap();

                let pos = lhs.pos.clone();
                let bin_expr = Expr::Binary {
                    op: top_op.op,
                    lhs,
                    rhs,
                };
                expr_stack.push(ExprBox::new(bin_expr, pos));
            }
            else {
                break;
            }
        }
    }

    loop
    {
        if input.eof() {
            break;
        }

        /*
        // Ternary operator
        if input.match_token("?")? {
            // We have to evaluate lower-precedence operators now
            // in order to use the resulting value for the boolean test
            eval_lower_prec(&mut op_stack, &mut expr_stack, TERNARY_PREC);

            let test_expr = expr_stack.pop().unwrap();
            let then_expr = parse_expr(input)?;
            input.expect_token(":")?;
            let else_expr = parse_expr(input)?;

            expr_stack.push(Expr::Ternary {
                test_expr: Box::new(test_expr),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            });

            break;
        }
        */

        let new_op = match_bin_op(input)?;

        // If no operator could be matched, stop
        if new_op.is_none() {
            break;
        }
        let new_op = new_op.unwrap();

        // If this operator evaluates right-to-left,
        // e.g. an assignment operator
        if new_op.rtl == true {
            // Recursively parse the rhs expression,
            // forcing it to be evaluated before the lhs
            let rhs = parse_expr(input)?;
            let lhs = expr_stack.pop().unwrap();

            let pos = lhs.pos.clone();
            let bin_expr = Expr::Binary {
                op: new_op.op,
                lhs,
                rhs,
            };
            expr_stack.push(ExprBox::new(bin_expr, pos));

            break;
        }

        // Evaluate the operators with lower precedence than
        // the new operator we just read
        eval_lower_prec(&mut op_stack, &mut expr_stack, new_op.prec);

        op_stack.push(new_op);

        // There must be another prefix sub-expression following
        expr_stack.push(parse_prefix(input)?);
    }

    // Emit all operators remaining on the operator stack
    while op_stack.len() > 0 {
        assert!(expr_stack.len() >= 2);
        let rhs = expr_stack.pop().unwrap();
        let lhs = expr_stack.pop().unwrap();
        let top_op = op_stack.pop().unwrap();

        let pos = lhs.pos.clone();
        let bin_expr = Expr::Binary {
            op: top_op.op,
            lhs,
            rhs,
        };
        expr_stack.push(ExprBox::new(bin_expr, pos));
    }

    assert!(expr_stack.len() == 1);
    Ok(expr_stack.pop().unwrap())
}

/// Parse a block statement
fn parse_block_stmt(input: &mut Input) -> Result<StmtBox, ParseError>
{
    input.eat_ws()?;
    let pos = input.get_pos();
    input.expect_token("{")?;

    let mut stmts = Vec::default();

    loop
    {
        if input.eof() {
            return input.parse_error("unexpected end of input in block statement");
        }

        if input.match_token("}")? {
            break;
        }

        if input.match_token(";")? {
            // Empty statements are ignored
            continue;
        }

        stmts.push(parse_stmt(input)?);
    }

    return StmtBox::new_ok(
        Stmt::Block(stmts),
        pos,
    );
}

/// Parse a statement
fn parse_stmt(input: &mut Input) -> Result<StmtBox, ParseError>
{
    input.eat_ws()?;
    let pos = input.get_pos();

    if input.match_keyword("return")? {
        if input.match_token(";")? {
            return StmtBox::new_ok(
                Stmt::Return(ExprBox::new(Expr::None, pos)),
                pos,
            );
        }
        else
        {
            let expr = parse_expr(input)?;
            input.expect_token(";")?;
            return StmtBox::new_ok(
                Stmt::Return(expr),
                pos
            );
        }
    }

    if input.match_keyword("break")? {
        input.expect_token(";")?;
        return StmtBox::new_ok(Stmt::Break, pos);
    }

    if input.match_keyword("continue")? {
        input.expect_token(";")?;
        return StmtBox::new_ok(Stmt::Continue, pos);
    }

    // If-else statement
    if input.match_keyword("if")? {
        // Parse the test expression
        input.expect_token("(")?;
        let test_expr = parse_expr(input)?;
        input.expect_token(")")?;

        // Parse the then statement
        let then_stmt = parse_stmt(input)?;

        // If there is an else statement
        if input.match_keyword("else")? {
            // Parse the else statement
            let else_stmt = parse_stmt(input)?;

            return StmtBox::new_ok(
                Stmt::If {
                    test_expr,
                    then_stmt,
                    else_stmt: Some(else_stmt),
                },
                pos
            );
        }
        else
        {
            return StmtBox::new_ok(
                Stmt::If {
                    test_expr,
                    then_stmt,
                    else_stmt: None
                },
                pos
            );
        }
    }

    // While loop
    if input.match_keyword("while")? {
        // Parse the test expression
        input.expect_token("(")?;
        let test_expr = parse_expr(input)?;
        input.expect_token(")")?;

        // Parse the loop body
        let body_stmt = parse_stmt(input)?;

        return StmtBox::new_ok(
            Stmt::While {
                test_expr,
                body_stmt,
            },
            pos
        );
    }

    // For loop
    if input.match_keyword("for")? {
        input.expect_token("(")?;

        // Initialization statement
        let init_stmt = if input.match_token(";")? {
            StmtBox::default()
        } else {
            parse_stmt(input)?
        };

        // Test expression
        let test_expr = if input.match_token(";")? {
            ExprBox::new(
                Expr::True,
                pos
            )
        } else {
            let expr = parse_expr(input)?;
            input.expect_token(";")?;
            expr
        };

        // Increment expression
        let incr_expr = if input.match_token(")")? {
            ExprBox::default()
        } else {
            let expr = parse_expr(input)?;
            input.expect_token(")")?;
            expr
        };

        // Parse the loop body
        let body_stmt = parse_stmt(input)?;

        // The loop gets generated as:
        // { init_stmt, while (test_expr) { loop_body; incr_expr; }  }

        // Place the increment expression inside the body statement
        let body_stmt = StmtBox::new(
            Stmt::Block(vec![
                body_stmt,
                StmtBox::new(Stmt::Expr(incr_expr), pos)
            ]),
            pos,
        );

        // Create the while statement
        let while_stmt = StmtBox::new(
            Stmt::While { test_expr, body_stmt },
            pos
        );

        // Wrap the init and loop inside a block
        let for_stmt = StmtBox::new(
            Stmt::Block(vec![
                init_stmt,
                while_stmt
            ]),
            pos,
        );

        return Ok(for_stmt);
    }

    // Assert statement
    if input.match_keyword("assert")? {
        // Parse the test expression
        input.expect_token("(")?;
        let test_expr = parse_expr(input)?;
        input.expect_token(")")?;
        input.expect_token(";")?;

        return StmtBox::new_ok(
            Stmt::Assert {
                test_expr,
            },
            pos
        );
    }

    // Block statement
    if input.peek_ch() == '{' {
        return parse_block_stmt(input);
    }

    // Variable declaration
    if input.match_keyword("let")? {
        let mutable = input.match_keyword("var")?;
        input.eat_ws()?;
        let var_name = input.parse_ident()?;
        input.expect_token("=")?;
        let init_expr = parse_expr(input)?;
        input.expect_token(";")?;

        return StmtBox::new_ok(
            Stmt::Let {
                mutable,
                var_name,
                init_expr,
                decl: None,
            },
            pos,
        )
    }

    // Function declaration
    if input.match_keyword("fun")? {
        input.eat_ws()?;
        let name = input.parse_ident()?;
        let fun = parse_function(input, name, pos)?;
        let fun_name = fun.name.clone();

        let fun_expr = ExprBox::new(
            Expr::Fun(Box::new(fun)),
            pos
        );

        return StmtBox::new_ok(
            Stmt::Let {
                mutable: false,
                var_name: fun_name,
                init_expr: fun_expr,
                decl: None,
            },
            pos,
        );
    }

    // Try to parse this as an expression statement
    let expr = parse_expr(input)?;
    input.expect_token(";")?;

    StmtBox::new_ok(
        Stmt::Expr(expr),
        pos,
    )
}

/// Parse a function declaration
fn parse_function(input: &mut Input, name: String, pos: SrcPos) -> Result<Function, ParseError>
{
    let mut params = Vec::default();
    let mut var_arg = false;

    input.expect_token("(")?;

    loop
    {
        input.eat_ws()?;

        if input.eof() {
            return input.parse_error("unexpected end of input inside function parameter list");
        }

        if input.match_token(")")? {
            break;
        }

        // Parse one parameter and its type
        let param_name = input.parse_ident()?;
        params.push(param_name);

        if input.match_token(")")? {
            break;
        }

        // If this isn't the last argument, there
        // has to be a comma separator
        input.expect_token(",")?;
    }

    // Parse the function body (must be a block statement)
    let body = parse_block_stmt(input)?;

    Ok(Function
    {
        name,
        params,
        var_arg,
        body,
        num_locals: 0,
        is_unit: false,
        pos,
        id: crate::ast::next_id(),
    })
}

/// Parse a single unit of source code (e.g. one source file)
pub fn parse_unit(input: &mut Input) -> Result<Unit, ParseError>
{
    input.eat_ws()?;
    let pos = input.get_pos();

    let mut stmts = Vec::default();

    loop
    {
        input.eat_ws()?;

        if input.eof() {
            break;
        }

        stmts.push(parse_stmt(input)?);
    }

    let body = StmtBox::new(
        Stmt::Block(stmts),
        pos
    );

    let unit_fn = Function {
        name: input.get_src_name(),
        params: Vec::default(),
        var_arg: false,
        body,
        num_locals: 0,
        is_unit: true,
        pos,
        id: crate::ast::next_id(),
    };

    Ok(Unit {
        unit_fn
    })
}

pub fn parse_str(src: &str) -> Result<Unit, ParseError>
{
    let mut input = Input::new(&src, "src");
    parse_unit(&mut input)
}

pub fn parse_file(file_name: &str) -> Result<Unit, ParseError>
{
    let mut input = Input::from_file(file_name)?;
    parse_unit(&mut input)
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn parse_ok(src: &str)
    {
        dbg!(src);
        let mut input = Input::new(&src, "src");
        parse_unit(&mut input).unwrap();
    }

    fn parse_fails(src: &str)
    {
        dbg!(src);
        let mut input = Input::new(&src, "src");
        assert!(parse_unit(&mut input).is_err());
    }

    fn parse_file(file_name: &str)
    {
        dbg!(file_name);
        super::parse_file(file_name).unwrap();
    }

    #[test]
    fn simple_unit()
    {
        parse_ok("");
        parse_ok(" ");
        parse_ok("// Hi!\n ");
        parse_ok("/* Hi! */");
        parse_ok("/* Hi\nthere */");
        parse_ok("/* Hi\n/*there*/ */");
        parse_ok("x;");
        parse_ok("1;");
        parse_ok("1; ");
        parse_ok(" \"foobar\";");
        parse_ok("'foo\tbar\nbif';");
        parse_ok("1_000_000;");

        parse_fails("x");
    }

    #[test]
    fn infix_exprs()
    {
        // Should parse
        parse_ok("1 + 2;");
        parse_ok("1 + 2 * 3;");
        parse_ok("1 + 2 + 3;");
        parse_ok("1 + 2 + 3 + 4;");
        parse_ok("(1) + 2 + 3 * 4;");

        // Should not parse
        parse_fails("1 + 2 +;");
    }

    #[test]
    fn globals()
    {
        parse_ok("let x = 1;");
        parse_ok("let x = 20;");
        parse_ok("let x = true; let y = false;");
        parse_ok("let var x = 1;");
        parse_ok("let var x = 2; fun main() {}");
        parse_ok("let v = -1;");
        parse_ok("let str = \"FOO\n\";");
        parse_ok("let str = 'foo\nbar';");

        // Regressions
        parse_ok("let g0 = 1;//\n//\n//\nlet g1 = 2;");

        // Missing semicolon
        parse_fails("let g = 2");
    }

    #[test]
    fn numeric_literals()
    {
        parse_ok("let g = 400_000;");
        parse_ok("let g = 400_000_;");
        parse_ok("let f = 0.2;");
        parse_ok("let f = 4.567;");
        parse_ok("let f = 4.56e78;");
        parse_ok("let f = 4.5_6e8_;");

        parse_fails("let f = 4..5;");
    }

    #[test]
    fn strings()
    {
        parse_ok("let c = 'f';");
        parse_ok("let c = '\n';");
        parse_ok("let s = \"foo\";");

        // Double-quoted strings get concatenated
        parse_ok("let s = \"foo\" \"bar\";");
        parse_ok("let s = \"foo\"\n\"bar\";");

        // Single-quoted strings do not get concatenated
        parse_fails("let s = 'foo' 'bar';");
    }

    #[test]
    fn call_expr()
    {
        parse_ok("foo();");
        parse_ok("foo(0);");
        parse_ok("foo(0,);");
        parse_ok("foo(0,1);");
        parse_ok("foo( 0 , 1 , 2 , );");
        parse_ok("foo(0,1,2) + 3;");
        parse_ok("foo(0,1,2) + bar();");
    }

    #[test]
    fn host_call()
    {
        parse_ok("$print_endl();");
        parse_ok("$print_i64(123);");
        parse_ok("1 + $get_int() + 2;");
    }

    #[test]
    fn fun_decl()
    {
        parse_ok("let main = fun() {};");
        parse_ok("let main = fun() { return; };");
        parse_ok("let main = fun() { return 0; };");

        parse_ok("fun main() {}");
        parse_ok("fun main(argc, argv) { return 0; }");
        parse_ok("fun main(argc, argv) {}");

        parse_ok("fun foo() { /* hello! */}");
        parse_ok("fun foo() { {} }");
        parse_ok("fun foo() { return (0); }");
        parse_ok("fun foo() { return 0; }");
        parse_ok("fun foo() { return -2; }");
        parse_ok("fun foo() { return !1; }");
        parse_ok("fun foo() { \"foo\"; return 77; }");
        parse_ok("fun foo() { 333; return 77; }");
        parse_ok("fun foo() { return none; }");
        parse_ok("fun foo( a , b ) { return 77; }");

        // Should fail to parse
        parse_fails("fun foo();");
        parse_fails("fun foo() return 0;");
        parse_fails("fun f foo();");
    }

    #[test]
    fn local_vars()
    {
        parse_ok("fun main() { let x = 0; return; }");
        parse_ok("fun main() { let crc = 0xFFFFFFFF; return; }");
        parse_ok("fun main() { let x = 0; let y = x + 1; return; }");
        parse_ok("fun main() { let x = 0; foo(x); return; }");
        parse_ok("let global = 1; fun main() { let p = global + 1; return; }");
    }

    #[test]
    fn stmts()
    {
        parse_ok("let x = 3;");
        parse_ok("let str = 'foo';");
        parse_ok("let x = 3; let y = 5;");
        parse_ok("{ let x = 3; x; } let y = 4;");

        parse_ok("let x = 3;");
        parse_ok("let x = 3; return x;");
        parse_ok("let x = 3; if (!x) x = 1;");
    }

    #[test]
    fn fun_expr()
    {
        parse_ok("let f = fun() {};");
        parse_ok("let f = fun(x) {};");
        parse_ok("let f = fun(x,) {};");
        parse_ok("let f = fun(x,y) {};");
        parse_ok("let f = fun(x,y) { return 1; };");
        parse_fails("let f = fun(x,y,1) {};");
    }

    #[test]
    fn objects()
    {
        // Literals
        parse_ok("let o = {};");
        parse_ok("let o = +{};");
        parse_ok("let o = +{ x: 1, ~y: 2};");

        // Member operator
        parse_ok("let v = a.b;");
        parse_ok("a.b = c;");

        // Method definitions
        parse_ok("let o = { m() {} };");
        parse_ok("let o = { m() {} x:1, y:2 };");
        parse_ok("let o = { x1:1, x2:2, m1() {} m2(x,y,z) {} };");
    }

    #[test]
    fn arrays()
    {
        // Literals
        parse_ok("let a = [];");
        parse_ok("let a = [1];");
        parse_ok("let a = [1, 2];");
        parse_ok("let a = [ 1 , 2, 3 ];");

        // Single and double indexing
        parse_ok("let a = none; a[0];");
        parse_ok("let a = none; a[0][0];");
        parse_ok("let a = none; a[0][0]();");

        // Methods
        parse_ok("let a = []; a.push(1);");
    }

    #[test]
    fn assign_stmt()
    {
        parse_ok("let var x = 1; x = 2;");
        parse_ok("let var x = 2; let var y = 3; x = 1; y = 2; x = x + y;");
    }

    #[test]
    fn if_stmt()
    {
        parse_ok("if (1) {}");
        parse_ok("if (1) {} else {}");
        parse_ok("if (1) { foo(); }");
        parse_ok("if (1) { foo(); } else { bar(); }");
        parse_ok("if (typeof true == 'bool') {}");
    }

    #[test]
    fn while_stmt()
    {
        parse_ok("while (1) { foo(); }");
        parse_ok("let i = 0; while (i < n) { foo(); i = i + 1; }");
    }
}
