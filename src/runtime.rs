use crate::ast::*;
use crate::vm::{Value, Actor};

fn int64_abs(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_i64();
    Value::Int64(if v > 0 { v } else { -v })
}

fn int64_to_f(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_i64();
    Value::Float64(v as f64)
}

fn int64_to_s(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_i64();
    let s = format!("{}", v);
    Value::String(actor.alloc.str_const(s))
}

fn float64_abs(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_f64();
    Value::Float64(if v > 0.0 { v } else { -v })
}

fn float64_floor(actor: &mut Actor, v: Value) -> Value
{
    // TODO: check that value fits in integer range
    let v = v.unwrap_f64();
    let int_val = v.floor() as i64;
    Value::Int64(int_val)
}

fn float64_sin(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_f64();
    Value::Float64(v.sin())
}

fn float64_sqrt(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_f64();
    Value::Float64(v.sqrt())
}

fn float64_to_s(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_f64();
    let s = format!("{}", v);
    Value::String(actor.alloc.str_const(s))
}

pub fn init_runtime(prog: &mut Program)
{
    /*
    // Int64
    let mut int64_class = Class::default();
    int64_class.id = INT64_ID;
    prog.reg_class(int64_class);

    // ByteArray
    let mut ba_class = Class::default();
    ba_class.id = BYTEARRAY_ID;
    prog.reg_class(ba_class);
    */

    // UIEvent
    // Note: in the future we may move this into
    // an importable module instead of making it a core
    // runtime object class
    let mut ui_class = Class::default();
    ui_class.id = UIEVENT_ID;
    ui_class.reg_field("kind");
    ui_class.reg_field("window_id");
    ui_class.reg_field("key");
    ui_class.reg_field("button");
    ui_class.reg_field("x");
    ui_class.reg_field("y");
    prog.reg_class(ui_class);
}

/// Get the method associated with a core value
pub fn get_method(val: Value, method_name: &str) -> Value
{
    use crate::host::HostFn;
    use crate::array::*;
    use crate::bytearray::*;

    let f = match (val, method_name) {
        (Value::Int64(_), "abs") => HostFn::Fn1_1(int64_abs),
        (Value::Int64(_), "to_f") => HostFn::Fn1_1(int64_to_f),
        (Value::Int64(_), "to_s") => HostFn::Fn1_1(int64_to_s),

        (Value::Float64(_), "floor") => HostFn::Fn1_1(float64_floor),
        (Value::Float64(_), "sin") => HostFn::Fn1_1(float64_sin),
        (Value::Float64(_), "sqrt") => HostFn::Fn1_1(float64_sqrt),
        (Value::Float64(_), "to_s") => HostFn::Fn1_1(float64_to_s),

        (Value::Class(ARRAY_ID), "with_size") => HostFn::Fn3_1(array_with_size),
        (Value::Array(_), "push") => HostFn::Fn2_0(array_push),
        (Value::Array(_), "pop") => HostFn::Fn1_1(array_pop),

        (Value::Class(BYTEARRAY_ID), "new") => HostFn::Fn1_1(ba_new),
        (Value::Class(BYTEARRAY_ID), "with_size") => HostFn::Fn2_1(ba_with_size),
        (Value::ByteArray(_), "write_u32") => HostFn::Fn3_0(ba_write_u32),
        (Value::ByteArray(_), "fill_u32") => HostFn::Fn4_0(ba_fill_u32),
        (Value::ByteArray(_), "memcpy") => HostFn::Fn5_0(ba_memcpy),

        _ => panic!("unknown method {}", method_name)
    };

    Value::HostFn(f)
}

pub fn get_class_id(val: Value) -> ClassId
{
    match val {
        Value::Object(p) => {
            let obj = unsafe { &*p };
            obj.class_id
        }

        Value::Int64(_) => INT64_ID,
        Value::Float64(_) => FLOAT64_ID,
        Value::String(_) => STRING_ID,
        Value::Array(_) => ARRAY_ID,
        Value::ByteArray(_) => BYTEARRAY_ID,

        _ => todo!()
    }
}
