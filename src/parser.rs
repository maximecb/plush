use std::collections::HashMap;
use std::io;
use std::io::Read;
use std::cmp::max;
use crate::lexer::*;
use crate::ast::*;

/// Parse an atomic expression
fn parse_atom(input: &mut Lexer, prog: &mut Program) -> Result<ExprBox, ParseError>
{
    input.eat_ws()?;
    let ch = input.peek_ch();
    let pos = input.get_pos();

    // Hexadecimal integer literal
    if input.match_token("0x")? {
        let int_val = input.parse_int(16)?;

        if int_val < i64::MIN.into() || int_val > i64::MAX.into() {
            return input.parse_error("integer literal outside of int64 range")
        }

        return Ok(ExprBox::new(
            Expr::Int64(int_val as i64),
            pos
        ));
    }

    // Binary integer literal
    if input.match_token("0b")? {
        let int_val = input.parse_int(2)?;

        if int_val < i64::MIN.into() || int_val > i64::MAX.into() {
            return input.parse_error("integer literal outside of int64 range")
        }

        return Ok(ExprBox::new(
            Expr::Int64(int_val as i64),
            pos
        ));
    }

    // Decimal numeric value
    if ch.is_digit(10) {
        let num_str = input.read_numeric();
        //println!("{}", num_str);

        // If we can parse this value as an integer
        if let Ok(int_val) = num_str.parse::<i64>() {
            return Ok(ExprBox::new(
                Expr::Int64(int_val),
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

    if input.match_keyword("nil")? {
        return Ok(ExprBox::new(
            Expr::Nil,
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
        let expr = parse_expr(input, prog)?;
        input.expect_token(")")?;
        return Ok(expr);
    }

    // Array literal
    if input.match_char('[') {
        let exprs = parse_expr_list(input, prog, "]")?;
        return Ok(ExprBox::new(
            Expr::Array { exprs },
            pos,
        ));
    }

    // Dictionary literal
    if input.match_char('{') {
        return parse_dict(input, prog, pos);
    }

    // Byte array literal
    if input.match_chars(&['#', '[']) {
        let expr = parse_bytearray(input, prog, pos)?;
        return Ok(expr);
    }

    // Host constant
    if ch == '$' {
        input.eat_ch();
        let name = input.parse_ident()?;
        let expr = crate::host::get_host_const(&name);

        return ExprBox::new_ok(
            expr,
            pos
        );
    }

    // Lambda expression
    if ch == '|' {
        input.eat_ws()?;

        let fun_id = parse_lambda(input, prog, pos)?;

        return ExprBox::new_ok(
            Expr::Fun {
                fun_id,
                captured: Vec::default()
            },
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
fn parse_postfix(input: &mut Lexer, prog: &mut Program) -> Result<ExprBox, ParseError>
{
    let mut base_expr = parse_atom(input, prog)?;

    loop
    {
        input.eat_ws()?;
        let pos = input.get_pos();

        // If this is a function call
        if input.match_token("(")? {
            let arg_exprs = parse_expr_list(input, prog, ")")?;

            // Add one to account for self in constructor and method calls
            if arg_exprs.len() + 1 > u8::MAX.into() {
                return input.parse_error("too many arguments in function call");
            }

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
            let index_expr = parse_expr(input, prog)?;
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

        // Instanceof operator
        if input.match_token("instanceof")? {
            input.eat_ws()?;
            let class_name = input.parse_ident()?;

            base_expr = ExprBox::new(
                Expr::InstanceOf {
                    val: base_expr,
                    class_name,
                    class_id: ClassId::default(),
                },
                pos
            );

            continue;
        }

        // Postfix increment expression
        if input.match_token("++")? {
            return input.parse_error(&concat!(
                "the postfix increment operator (i.e. i++) is not supported, ",
                "use prefix increment (i.e. ++i) instead."
            ));
        }

        // Postfix decrement expression
        if input.match_token("--")? {
            return input.parse_error(&concat!(
                "the postfix increment operator (i.e. i++) is not supported, ",
                "use prefix increment (i.e. ++i) instead."
            ));
        }

        break;
    }

    Ok(base_expr)
}

/// Parse an prefix expression
/// Note: this function should only call parse_postfix directly
/// to respect the priority of operations in C
fn parse_prefix(input: &mut Lexer, prog: &mut Program) -> Result<ExprBox, ParseError>
{
    input.eat_ws()?;
    let ch = input.peek_ch();
    let pos = input.get_pos();

    // Unary not expression (bitwise or logical not)
    if ch == '!' {
        input.eat_ch();
        let child = parse_prefix(input, prog)?;

        return ExprBox::new_ok(
            Expr::Unary {
                op: UnOp::Not,
                child
            },
            pos,
        );
    }

    // Pre-increment expression
    if input.match_token("++")? {
        let sub_expr = parse_prefix(input, prog)?;

        // Transform into i = i + 1
        return ExprBox::new_ok(
            Expr::Binary {
                op: BinOp::Assign,
                lhs: sub_expr.clone(),
                rhs: ExprBox::new(
                    Expr::Binary{
                        op: BinOp::Add,
                        lhs: sub_expr.clone(),
                        rhs: ExprBox::new(Expr::Int64(1), sub_expr.pos)
                    },
                    sub_expr.pos
                )
            },
            sub_expr.pos
        );
    }

    // Pre-decrement expression
    if input.match_token("--")? {
        let sub_expr = parse_prefix(input, prog)?;

        // Transform into i = i - 1
        return ExprBox::new_ok(
            Expr::Binary {
                op: BinOp::Assign,
                lhs: sub_expr.clone(),
                rhs: ExprBox::new(
                    Expr::Binary{
                        op: BinOp::Sub,
                        lhs: sub_expr.clone(),
                        rhs: ExprBox::new(Expr::Int64(1), sub_expr.pos)
                    },
                    sub_expr.pos
                )
            },
            sub_expr.pos
        );
    }

    // Unary minus expression
    if ch == '-' {
        input.eat_ch();
        let sub_expr = parse_prefix(input, prog)?;

        // If this is an integer or floating-point value, negate it
        let expr = match *sub_expr.expr {
            Expr::Int64(int_val) => Expr::Int64(-int_val),
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

    // Unary plus expression
    if ch == '+' {
        input.eat_ch();
        let sub_expr = parse_prefix(input, prog)?;

        // If this is an integer or floating-point value, do nothing
        let expr = match sub_expr.expr.as_ref() {
            Expr::Int64(int_val) => sub_expr,
            Expr::Float64(f_val) => sub_expr,
            _ => return input.parse_error("plus operator applied to non-constant value")
        };

        return Ok(expr)
    }

    // Try to parse this as a postfix expression
    parse_postfix(input, prog)
}

/// Parse a list of argument expressions
fn parse_expr_list(input: &mut Lexer, prog: &mut Program, end_token: &str) -> Result<Vec<ExprBox>, ParseError>
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
        arg_exprs.push(parse_expr(input, prog)?);

        if input.match_token(end_token)? {
            break;
        }

        // If this isn't the last argument, there
        // has to be a comma separator
        input.expect_token(",")?;
    }

    Ok(arg_exprs)
}

// Parse a dictionary literal
fn parse_dict(
    input: &mut Lexer,
    prog: &mut Program,
    pos: SrcPos,
) -> Result<ExprBox, ParseError>
{
    // List of key, value pairs
    let mut pairs = Vec::default();

    loop
    {
        input.eat_ws()?;

        if input.eof() {
            return input.parse_error("unexpected end of input inside dictionary literal");
        }

        if input.match_token("}")? {
            break;
        }

        // Parse a field name
        input.eat_ws()?;
        let field_name = input.parse_ident()?;

        // Parse the field value
        input.expect_token(":")?;
        let field_expr = parse_expr(input, prog)?;
        pairs.push((field_name, field_expr));

        if input.match_token("}")? {
            break;
        }

        // If this isn't the last field, there
        // has to be a comma separator
        input.expect_token(",")?;
    }

    ExprBox::new_ok(
        Expr::Dict { pairs },
        pos
    )
}

// Parse a byte array literal
fn parse_bytearray(
    input: &mut Lexer,
    prog: &mut Program,
    pos: SrcPos,
) -> Result<ExprBox, ParseError>
{
    let mut bytes: Vec<u8> = Vec::default();

    fn parse_ascii(input: &mut Lexer, bytes: &mut Vec<u8>) -> Result<(), ParseError>
    {
        loop {
            let ch = input.peek_ch();

            if ch == ']' {
                break;
            }

            if input.peek_chars(&['\\', 'x']) {
                break;
            }

            if input.peek_chars(&['\\', 'b']) {
                break;
            }

            if input.peek_chars(&['/', '/']) {
                break;
            }

            if input.peek_chars(&['/', '*']) {
                break;
            }

            // End of line terminates the ascii sequence
            if ch == '\r' || ch == '\n' {
                if let Some(last_byte) = bytes.last() {
                    if *last_byte == ' ' as u8 {
                        return input.parse_error("spaces cannot immediately precede end of line in ascii sequence");
                    }
                }

                break;
            }

            if ch == '\t' {
                return input.parse_error("tabs disallowed inside bytearray ASCII sequences");
            }

            // Escape sequence
            if ch == '\\' {
                input.eat_ch();
                let ch = match input.eat_ch() {
                    '\\' => '\\',
                    '\'' => '\'',
                    '\"' => '\"',
                    't'  => '\t',
                    'r'  => '\r',
                    'n'  => '\n',
                    '0'  => '\0',
                    _ => return input.parse_error("unknown escape sequence")
                };

                bytes.push(ch.try_into().unwrap());
                continue;
            }

            if !ch.is_ascii_graphic() && ch != ' ' {
                break;
            }

            input.eat_ch();
            bytes.push(ch.try_into().unwrap());
        }

        Ok(())
    }

    fn parse_hex(input: &mut Lexer, bytes: &mut Vec<u8>) -> Result<(), ParseError>
    {
        loop {
            // Ignore whitespace
            input.eat_ws()?;

            let ch = input.peek_ch();

            if ch == ']' {
                break;
            }

            if !ch.is_ascii_alphanumeric() {
                break;
            }

            // Read one hex byte
            let ch0 = input.eat_ch().to_digit(16);
            let ch1 = input.eat_ch().to_digit(16);

            if ch0 == None || ch1 == None {
                return input.parse_error("invalid or incomplete hex byte")
            }

            let byte = (ch0.unwrap() * 16 + ch1.unwrap()) as u8;
            bytes.push(byte);
        }

        Ok(())
    }

    fn parse_bin(input: &mut Lexer, bytes: &mut Vec<u8>) -> Result<(), ParseError>
    {
        loop {
            // Ignore whitespace
            input.eat_ws()?;

            let ch = input.peek_ch();

            if ch == ']' {
                break;
            }

            if ch != '0' && ch != '1' {
                break;
            }

            // Read one binary byte
            let mut byte = 0;
            for mut i in 0..8 {
                let d = input.eat_ch().to_digit(2);

                if d == None {
                    return input.parse_error("each binary byte must contain exactly 8 bits")
                }

                byte = (byte << 1) + d.unwrap() as u8;
            }

            bytes.push(byte);
        }

        Ok(())
    }

    loop
    {
        // Ignore whitespace
        input.eat_ws()?;

        if input.eof() {
            return input.parse_error("unexpected end of input inside byte array literal");
        }

        if input.match_token("]")? {
            break;
        }

        let ch = input.eat_ch();

        if ch != '\\' {
            return input.parse_error("expected control sequence inside bytearray literal")
        }

        match input.eat_ch() {
            'a' => parse_ascii(input, &mut bytes)?,
            'x' => parse_hex(input, &mut bytes)?,
            'b' => parse_bin(input, &mut bytes)?,
            _ => return input.parse_error("unknown control sequence in bytearray literal")
        }
    }

    ExprBox::new_ok(
        Expr::ByteArray(bytes),
        pos
    )
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
const BIN_OPS: [OpInfo; 20] = [
    OpInfo { op_str: "*", prec: 3, op: BinOp::Mul, rtl: false },
    OpInfo { op_str: "/", prec: 3, op: BinOp::Div, rtl: false },
    OpInfo { op_str: "_/", prec: 3, op: BinOp::IntDiv, rtl: false },
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

    // Logical AND, logical OR
    // We place these before bitwise ops because they are longer tokens
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
fn match_bin_op(input: &mut Lexer) -> Result<Option<OpInfo>, ParseError>
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
fn parse_expr(input: &mut Lexer, prog: &mut Program) -> Result<ExprBox, ParseError>
{
    // Operator stack
    let mut op_stack: Vec<OpInfo> = Vec::default();

    // Expression stack
    let mut expr_stack: Vec<ExprBox> = Vec::default();

    // Parse the prefix sub-expression
    expr_stack.push(parse_prefix(input, prog)?);

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

        // Ternary operator
        if input.match_token("?")? {
            // We have to evaluate lower-precedence operators now
            // in order to use the resulting value for the boolean test
            eval_lower_prec(&mut op_stack, &mut expr_stack, TERNARY_PREC);

            let test_expr = expr_stack.pop().unwrap();
            let then_expr = parse_expr(input, prog)?;
            input.expect_token(":")?;
            let else_expr = parse_expr(input, prog)?;

            let pos = test_expr.pos.clone();
            expr_stack.push(ExprBox::new(
                Expr::Ternary {
                    test_expr,
                    then_expr,
                    else_expr,
                },
                pos
            ));

            break;
        }

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
            let rhs = parse_expr(input, prog)?;
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
        expr_stack.push(parse_prefix(input, prog)?);
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
fn parse_block_stmt(input: &mut Lexer, prog: &mut Program) -> Result<StmtBox, ParseError>
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

        stmts.push(parse_stmt(input, prog)?);
    }

    return StmtBox::new_ok(
        Stmt::Block(stmts),
        pos,
    );
}

/// Parse a statement
fn parse_stmt(input: &mut Lexer, prog: &mut Program) -> Result<StmtBox, ParseError>
{
    input.eat_ws()?;
    let pos = input.get_pos();

    if input.match_keyword("return")? {
        if input.match_token(";")? {
            return StmtBox::new_ok(
                Stmt::Return(ExprBox::new(Expr::Nil, pos)),
                pos,
            );
        }
        else
        {
            let expr = parse_expr(input, prog)?;
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
        let test_expr = parse_expr(input, prog)?;
        input.expect_token(")")?;

        // Parse the then statement
        let then_stmt = parse_stmt(input, prog)?;

        // If there is an else statement
        if input.match_keyword("else")? {
            // Parse the else statement
            let else_stmt = parse_stmt(input, prog)?;

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

    // Infinite loop
    if input.match_keyword("loop")? {

        // Parse the loop body
        let body_stmt = parse_stmt(input, prog)?;

        return StmtBox::new_ok(
            Stmt::For {
                init_stmt: StmtBox::default(),
                test_expr: ExprBox::new(Expr::True, pos),
                incr_expr: ExprBox::default(),
                body_stmt,
            },
            pos
        );
    }

    // While loop
    if input.match_keyword("while")? {
        // Parse the test expression
        input.expect_token("(")?;
        let test_expr = parse_expr(input, prog)?;
        input.expect_token(")")?;

        // Parse the loop body
        let body_stmt = parse_stmt(input, prog)?;

        return StmtBox::new_ok(
            Stmt::For {
                init_stmt: StmtBox::default(),
                test_expr,
                incr_expr: ExprBox::default(),
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
            parse_stmt(input, prog)?
        };

        // Test expression
        let test_expr = if input.match_token(";")? {
            ExprBox::new(
                Expr::True,
                pos
            )
        } else {
            let expr = parse_expr(input, prog)?;
            input.expect_token(";")?;
            expr
        };

        // Increment expression
        let incr_expr = if input.match_token(")")? {
            ExprBox::default()
        } else {
            let expr = parse_expr(input, prog)?;
            input.expect_token(")")?;
            expr
        };

        // Parse the loop body
        let body_stmt = parse_stmt(input, prog)?;

        return StmtBox::new_ok(
            Stmt::For {
                init_stmt,
                test_expr,
                incr_expr,
                body_stmt,
            },
            pos
        );
    }

    // Assert statement
    if input.match_keyword("assert")? {
        // Parse the test expression
        input.expect_token("(")?;
        let test_expr = parse_expr(input, prog)?;
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
        return parse_block_stmt(input, prog);
    }

    // Variable declaration
    if input.match_keyword("let")? {
        let mutable = input.match_keyword("var")?;
        input.eat_ws()?;
        let var_name = input.parse_ident()?;
        input.expect_token("=")?;
        let init_expr = parse_expr(input, prog)?;
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
        let fun_id = parse_function(input, prog, name, pos)?;
        let fun_name = prog.funs[&fun_id].name.clone();

        let fun_expr = ExprBox::new(
            Expr::Fun {
                fun_id,
                captured: Vec::default(),
            },
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

    // Unexpected semicolon
    if input.peek_ch() == ';' {
        return input.parse_error("extraneous semicolon `;`");
    }

    // Try to parse this as an expression statement
    let expr = parse_expr(input, prog)?;
    input.expect_token(";")?;

    StmtBox::new_ok(
        Stmt::Expr(expr),
        pos,
    )
}

/// Parse a function declaration
fn parse_function(input: &mut Lexer, prog: &mut Program, name: String, pos: SrcPos) -> Result<FunId, ParseError>
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

        // Parse one parameter
        let param_name = input.parse_ident()?;
        params.push(param_name);

        if input.match_token(")")? {
            break;
        }

        // If this isn't the last argument, there
        // has to be a comma separator
        input.expect_token(",")?;
    }

    if params.len() > u8::MAX.into() {
        return input.parse_error("too many function parameters");
    }

    // Parse the function body (must be a block statement)
    let body = parse_block_stmt(input, prog)?;

    let fun = Function {
        name,
        params,
        var_arg,
        body,
        num_locals: 0,
        captured: Default::default(),
        escaping: Default::default(),
        is_unit: false,
        pos,
        id: Default::default(),
        class_id: Default::default(),
    };

    let fun_id = prog.reg_fun(fun);
    Ok(fun_id)
}

/// Parse a lambda expression
fn parse_lambda(input: &mut Lexer, prog: &mut Program, pos: SrcPos) -> Result<FunId, ParseError>
{
    let mut params = Vec::default();
    let mut var_arg = false;

    input.expect_token("|")?;

    loop
    {
        input.eat_ws()?;

        if input.eof() {
            return input.parse_error("unexpected end of input inside lambda parameter list");
        }

        if input.match_token("|")? {
            break;
        }

        // Parse one parameter
        let param_name = input.parse_ident()?;
        params.push(param_name);

        if input.match_token("|")? {
            break;
        }

        // If this isn't the last argument, there
        // has to be a comma separator
        input.expect_token(",")?;
    }

    if params.len() > u8::MAX.into() {
        return input.parse_error("too many function parameters");
    }

    input.eat_ws()?;

    let body = if input.peek_ch() == '{' {
        // Parse the function body (must be a block statement)
        parse_block_stmt(input, prog)?
    } else {
        let expr = parse_expr(input, prog)?;
        StmtBox::new(
            Stmt::Return(expr),
            pos
        )
    };

    let fun = Function {
        name: "lambda".to_owned(),
        params,
        var_arg,
        body,
        num_locals: 0,
        captured: Default::default(),
        escaping: Default::default(),
        is_unit: false,
        pos,
        id: Default::default(),
        class_id: Default::default(),
    };

    let fun_id = prog.reg_fun(fun);
    Ok(fun_id)
}

/// Parse a class declaration
fn parse_class(input: &mut Lexer, prog: &mut Program, pos: SrcPos) -> Result<(String, ClassId), ParseError>
{
    input.eat_ws()?;
    let class_name = input.parse_ident()?;
    input.expect_token("{")?;

    let mut methods = HashMap::new();

    loop
    {
        input.eat_ws()?;

        if input.eof() {
            return input.parse_error("unexpected end of input inside class declaration");
        }

        if input.match_token("}")? {
            break;
        }

        // Parse a method declaration
        let pos = input.get_pos();
        let method_name = input.parse_ident()?;
        let fun_id = parse_function(input, prog, method_name.clone(), pos)?;

        if method_name == "init" && prog.funs[&fun_id].params.len() == 0 {
            return input.parse_error("the init method must have a self parameter");
        }

        methods.insert(method_name, fun_id);
    }

    let class_id = prog.reg_class(Class {
        name: class_name.clone(),
        fields: HashMap::default(),
        methods: methods.clone(),
        pos,
        id: ClassId::default(),
    });

    // Tag each method with the class id
    for (_, fun_id) in methods {
        prog.funs.get_mut(&fun_id).unwrap().class_id = class_id;
    }

    Ok((class_name, class_id))
}

/// Parse a single unit of source code (e.g. one source file)
pub fn parse_unit(input: &mut Lexer, prog: &mut Program) -> Result<Unit, ParseError>
{
    input.eat_ws()?;
    let pos = input.get_pos();

    let mut classes = HashMap::default();
    let mut stmts = Vec::default();

    loop
    {
        input.eat_ws()?;
        let pos = input.get_pos();

        if input.eof() {
            break;
        }

        if input.match_keyword("class")? {
            let (name, id) = parse_class(input, prog, pos)?;
            classes.insert(name, id);
            stmts.push(StmtBox::new(
                Stmt::ClassDecl { class_id: id },
                pos
            ));
            continue;
        }

        stmts.push(parse_stmt(input, prog)?);
    }

    let body = StmtBox::new(
        Stmt::Block(stmts),
        pos
    );

    let unit_fn = Function {
        name: input.get_src_name(),
        params: Default::default(),
        var_arg: false,
        body,
        num_locals: 0,
        captured: Default::default(),
        escaping: Default::default(),
        is_unit: true,
        pos,
        id: Default::default(),
        class_id: Default::default(),
    };

    Ok(Unit {
        classes,
        unit_fn: prog.reg_fun(unit_fn)
    })
}

pub fn parse_program(input: &mut Lexer) -> Result<Program, ParseError>
{
    let mut prog = Program::new();
    let unit = parse_unit(input, &mut prog)?;
    prog.main_fn = unit.unit_fn;
    prog.main_unit = unit;
    Ok(prog)
}

pub fn parse_str(src: &str) -> Result<Program, ParseError>
{
    let mut input = Lexer::new(&src, "src");
    parse_program(&mut input)
}

pub fn parse_file(file_name: &str) -> Result<Program, ParseError>
{
    let mut input = Lexer::from_file(file_name)?;

    // If a shebang line is present, treat it as a comment
    if input.match_chars(&['#', '!']) {
        input.eat_comment();
    }

    parse_program(&mut input)
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn parse_ok(src: &str)
    {
        dbg!(src);
        let mut input = Lexer::new(&src, "src");
        parse_program(&mut input).unwrap();
    }

    fn parse_fails(src: &str)
    {
        dbg!(src);
        let mut input = Lexer::new(&src, "src");
        assert!(parse_program(&mut input).is_err());
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
        parse_ok("+3;");
        parse_ok("+3.5;");

        // No semicolon
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
        parse_ok("1 * -3;");

        // Should not parse
        parse_fails("1 + 2 +;");
    }

    #[test]
    fn ternary_expr()
    {
        parse_ok("1? 2:3;");
        parse_ok("let a = 1? (2+3):4;");
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

        // Invalid format
        parse_fails("let f = 4..5;");
        parse_fails("let f = 4.5.;");
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
        parse_ok("$println(123);");
    }

    #[test]
    fn fun_decl()
    {
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
        parse_fails("funx() {}");
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
        parse_ok("let f = || {};");
        parse_ok("let f = |x| {};");
        parse_ok("let f = |x| x;");
        parse_ok("let f = |x| x + 1;");
        parse_ok("let f = |x| (x+1);");
        parse_ok("let f = |x,| {};");
        parse_ok("let f = |x,y| {};");
        parse_ok("let f = |x, y| { return 1; };");
        parse_fails("let f = |x,y,1| {};");
    }

    #[test]
    fn dicts()
    {
        // Literals
        parse_ok("let o = {};");
        parse_ok("let o = { x: 1, y: 2};");

        // Member operator
        parse_ok("let v = a.b;");
        parse_ok("a.b = c;");
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
        parse_ok("let a = nil; a[0];");
        parse_ok("let a = nil; a[0][0];");
        parse_ok("let a = nil; a[0][0]();");

        // Methods
        parse_ok("let a = []; a.push(1);");
    }

    #[test]
    fn bytearrays()
    {
        parse_ok("let a = #[];");
        parse_ok("let a = #[ ];");
        parse_ok("let a = #[\\aascii string foobar];");
        parse_ok("let a = #[\\aascii string\\tfoobar];");
        parse_ok("let a = #[\\aascii\n  \\astring\n  \\aover multiple lines];");

        // Hex sequences
        parse_ok("let a = #[\\xFF];");
        parse_ok("let a = #[\\xFF AA BB];");
        parse_ok("let a = #[\\xFF\n AA\n BB];");
        parse_ok("let a = #[\\xFF\n AA\n BB\n \\aascii string];");
        parse_ok("let a = #[\\xFF\n \\aascii string\n \\xCC];");
        parse_ok("let a = #[\\x\nFF\nAA\nBB\nCC];");

        // Binary sequences
        parse_ok("let a = #[\\b00000000];");
        parse_ok("let a = #[\\b00000000 00000001];");
        parse_ok("let a = #[\\b00000000 \\xFF \\afoobar];");

        // Can't have space right before a newline in an ascii sequence
        // This is because the space would be invisible in most editors,
        // and potentially also automatically removed by the editor
        parse_fails("let a = #[\\aascii \n];");

        // Currently ASCII sequences end with every newline
        // This is to allow spaces at the beginning of each line
        // for ease of formatting
        parse_fails("let a = #[\\aascii\nfoo];");

        // Incomplete hex byte
        parse_fails("let a = #[\\xF];");

        // Incomplete binary byte
        parse_fails("let a = #[\\b0000];");
    }

    #[test]
    fn classes()
    {
        parse_ok("class Foo {}");
        parse_ok("let x = 1; class Foo {} let y = 2;");
        parse_ok("class Foo { init(self) {} }");
        parse_ok("class Foo { init(self) { self.x = 1; } }");
        parse_ok("class Foo { init(self) { self.x = 1; } inc(self) { ++self.x; } }");
        parse_ok("let o = Foo();");
        parse_ok("let o = Foo(1, 2, 3);");
    }

    #[test]
    fn instanceof()
    {
        parse_ok("a instanceof Foo;");
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
    }

    #[test]
    fn loop_stmt()
    {
        parse_ok("loop { break; }");
    }

    #[test]
    fn while_stmt()
    {
        parse_ok("while (1) { foo(); }");
        parse_ok("let i = 0; while (i < n) { foo(); i = i + 1; }");

        // Common error, don't accept
        parse_fails("while (1);");
    }

    #[test]
    fn for_stmt()
    {
        parse_ok("for (let var i = 0; i < 10; ++i) {}");
        parse_ok("for (;;) {}");

        // Common error, don't accept
        parse_fails("for (;;);");
    }

    #[test]
    fn regress_prefix_postfix()
    {
        parse_ok("return !a instanceof B;");
        parse_ok("return f() instanceof F;");
        parse_ok("return !f() instanceof F;");
    }

    #[test]
    fn tests_examples()
    {
        // Make sure that we can parse our test and example files
        parse_file("tests/empty.psh");
        parse_file("tests/fact.psh");
        parse_file("examples/helloworld.psh");
        parse_file("examples/fib.psh");
    }
}
