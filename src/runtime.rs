use crate::ast::{Program, Class, ClassId};
use crate::vm::{Value, Actor};

// Note: this maybe isn't necessary?
//const INT64_ID: ClassId = ClassId(1);

fn int64_to_s(actor: &mut Actor, v: Value) -> Value
{
    let v = v.unwrap_i64();
    let s = format!("{}", v);
    Value::String(actor.alloc.str_const(s))
}

pub fn init_runtime(prog: &mut Program)
{
    let int64_class = Class::default();

    // TODO: register to_s method for int64 class



    prog.reg_class(int64_class);






    // TODO: can we assign classes global constant indices?
    // maybe we can simply pre-register those indices ahead of time?

}

/// Get the method associated with a core value
pub fn get_method(val: Value, method_name: &str) -> Value
{
    use crate::array::*;
    use crate::host::HostFn;

    let f = match (val, method_name) {
        (Value::Int64(_), "to_s") => HostFn::Fn1_1(int64_to_s),

        (Value::Array(_), "push") => HostFn::Fn2_0(array_push),

        _ => panic!("unknown method")
    };

    Value::HostFn(f)
}





//
// NOTE: is this what we want, or do we want instanceof?
//
pub fn get_class_of(val: Value, prog: &Program)
{
    // Note: here you need some runtime class definitions accessible

    match val {



        _ => todo!()
    }
}
