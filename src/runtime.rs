use crate::ast::*;
use crate::vm::{Value, Actor};

fn int64_to_s(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_i64();
    let s = format!("{}", v);
    Value::String(actor.alloc.str_const(s))
}

fn float64_sqrt(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_f64();
    Value::Float64(v.sqrt())
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

    // UIMessage
    // Note: in the future we may move this into
    // an importable module instead of making it a core
    // runtime object class
    let mut ui_class = Class::default();
    ui_class.id = UIMESSAGE_ID;
    ui_class.reg_field("event");
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
        (Value::Int64(_), "to_s") => HostFn::Fn1_1(int64_to_s),
        (Value::Float64(_), "sqrt") => HostFn::Fn1_1(float64_sqrt),

        (Value::Array(_), "push") => HostFn::Fn2_0(array_push),

        (Value::Class(BYTEARRAY_ID), "new") => HostFn::Fn1_1(ba_new),
        (Value::Class(BYTEARRAY_ID), "with_size") => HostFn::Fn2_1(ba_with_size),
        (Value::ByteArray(_), "write_u32") => HostFn::Fn3_0(ba_write_u32),
        (Value::ByteArray(_), "fill_u32") => HostFn::Fn4_0(ba_fill_u32),

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
        Value::String(_) => STRING_ID,
        Value::Array(_) => ARRAY_ID,
        Value::ByteArray(_) => BYTEARRAY_ID,

        _ => todo!()
    }
}
