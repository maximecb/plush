use crate::ast::*;
use crate::vm::{Value, Actor};

fn int64_to_s(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_i64();
    let s = format!("{}", v);
    Value::String(actor.alloc.str_const(s))
}

pub fn init_runtime(prog: &mut Program)
{
    // Int64
    let mut int64_class = Class::default();
    int64_class.id = INT64_ID;
    prog.reg_class(int64_class);

    // ByteArray
    let mut ba_class = Class::default();
    ba_class.id = BYTEARRAY_ID;



    prog.reg_class(ba_class);





}

/// Get the method associated with a core value
pub fn get_method(val: Value, method_name: &str) -> Value
{
    use crate::host::HostFn;
    use crate::array::*;
    use crate::bytearray::*;

    let f = match (val, method_name) {
        (Value::Int64(_), "to_s") => HostFn::Fn1_1(int64_to_s),

        (Value::Array(_), "push") => HostFn::Fn2_0(array_push),

        _ => panic!("unknown method")
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
