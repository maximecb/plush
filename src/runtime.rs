use crate::ast::*;
use crate::vm::{Value, Actor};
use crate::{error, unwrap_usize, unwrap_str};

fn identity_method(actor: &mut Actor, self_val: Value) -> Result<Value, String>
{
    Ok(self_val)
}

fn true_to_s(actor: &mut Actor, _v: Value) -> Result<Value, String>
{
    Ok(actor.alloc.str_val("true"))
}

fn false_to_s(actor: &mut Actor, _v: Value) -> Result<Value, String>
{
    Ok(actor.alloc.str_val("false"))
}

fn nil_to_s(actor: &mut Actor, _v: Value) -> Result<Value, String>
{
    Ok(actor.alloc.str_val("nil"))
}

fn int64_abs(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_i64();
    Ok(Value::Int64(if v > 0 { v } else { -v }))
}

fn int64_min(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = v.unwrap_i64();
    let other = other.unwrap_i64();
    Ok(Value::Int64(v.min(other)))
}

fn int64_max(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = v.unwrap_i64();
    let other = other.unwrap_i64();
    Ok(Value::Int64(v.max(other)))
}

fn int64_to_f(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_i64();
    Ok(Value::Float64(v as f64))
}

fn int64_to_s(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_i64();
    let s = format!("{}", v);
    Ok(actor.alloc.str_val(&s))
}

fn float64_abs(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    Ok(Value::Float64(if v > 0.0 { v } else { -v }))
}

fn float64_ceil(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    // TODO: check that float value fits in integer range
    let v = v.unwrap_f64();
    let int_val = v.ceil() as i64;
    Ok(Value::Int64(int_val))
}

fn float64_floor(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    // TODO: check that float value fits in integer range
    let v = v.unwrap_f64();
    let int_val = v.floor() as i64;
    Ok(Value::Int64(int_val))
}

fn float64_trunc(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    // TODO: check that float value fits in integer range
    let v = v.unwrap_f64();
    let int_val = v.trunc() as i64;
    Ok(Value::Int64(int_val))
}

fn float64_sin(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    Ok(Value::Float64(v.sin()))
}

fn float64_cos(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    Ok(Value::Float64(v.cos()))
}

fn float64_tan(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    Ok(Value::Float64(v.tan()))
}

fn float64_atan(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    Ok(Value::Float64(v.atan()))
}

fn float64_sqrt(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    Ok(Value::Float64(v.sqrt()))
}

fn float64_min(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    let other = other.unwrap_f64();
    Ok(Value::Float64(v.min(other)))
}

fn float64_max(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    let other = other.unwrap_f64();
    Ok(Value::Float64(v.max(other)))
}

fn float64_clip(actor: &mut Actor, v: Value, min: Value, max: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    let min = min.unwrap_f64();
    let max = max.unwrap_f64();
    Ok(Value::Float64(v.clamp(min, max)))
}

fn float64_pow(actor: &mut Actor, v: Value, exponent: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    let exponent = exponent.unwrap_f64();
    Ok(Value::Float64(v.powf(exponent)))
}

fn float64_exp(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    Ok(Value::Float64(v.exp()))
}

fn float64_ln(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    Ok(Value::Float64(v.ln()))
}

fn float64_to_s(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = v.unwrap_f64();
    let s = format!("{}", v);
    Ok(actor.alloc.str_val(&s))
}

fn float64_format_decimals(actor: &mut Actor, v: Value, decimals: Value) -> Result<Value, String>
{
    let num = v.unwrap_f64();
    let decimals = unwrap_usize!(decimals);
    let s = format!("{:.*}", decimals, num);
    Ok(actor.alloc.str_val(&s))
}

/// Create a single-character string from a codepoint integer value
fn string_from_codepoint(actor: &mut Actor, _class: Value, codepoint: Value) -> Result<Value, String>
{
    // TODO: eventually we can add caching for this,
    // at least for ASCII character values, we can
    // easily intern those strings

    let codepoint = codepoint.unwrap_u32();
    let ch = char::from_u32(codepoint).expect("Invalid Unicode codepoint");

    let mut s = String::new();
    s.push(ch);

    Ok(actor.alloc.str_val(&s))
}

/// Get the UTF-8 byte at the given index
fn string_byte_at(actor: &mut Actor, s: Value, idx: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let idx = unwrap_usize!(idx);
    let byte = s.as_bytes().get(idx).unwrap();
    Ok(Value::from(*byte))
}

/// Get a string containing the single character at the given byte index
/// Returns nil if not a valid character boundary or character
fn string_char_at(actor: &mut Actor, s: Value, byte_idx: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let byte_idx = unwrap_usize!(byte_idx);

    if byte_idx >= s.len() {
        return Err("string byte index out of bounds".into());
    }

    // Indexing in the middle of a character
    if !s.is_char_boundary(byte_idx) {
        return Ok(Value::Nil);
    }

    let ch = s[byte_idx..].chars().next();

    let ch = match ch {
        // Not a valid character
        None => return Ok(Value::Nil),
        Some(ch) => ch,
    };

    Ok(actor.alloc.str_val(&ch.to_string()))
}

/// Try to parse the string as an integer with the given radix
fn string_parse_int(actor: &mut Actor, s: Value, radix: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let radix = radix.unwrap_u32();

    match i64::from_str_radix(s, radix) {
        Ok(int_val) => Ok(Value::from(int_val)),
        Err(_) => Ok(Value::Nil),
    }
}

/// Trim whitespace
fn string_trim(actor: &mut Actor, s: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let s = s.trim().to_string();
    Ok(actor.alloc.str_val(&s))
}

/// Split a string by a separator and return an array of strings
fn string_split(actor: &mut Actor, s: Value, sep: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let sep = unwrap_str!(sep);

    let parts: Vec<Value> = s.split(sep).map(|part| {
        actor.alloc.str_val(&part.to_string())
    }).collect();

    let arr = crate::array::Array { elems: parts };
    Ok(Value::Array(actor.alloc.alloc(arr)))
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
    ui_class.reg_field("text");
    prog.reg_class(ui_class);

    // AudioNeeded
    // Note: in the future we may move this into
    // an importable module instead of making it a core
    // runtime object class
    let mut audio_needed = Class::default();
    audio_needed.id = AUDIO_NEEDED_ID;
    audio_needed.reg_field("num_samples");
    audio_needed.reg_field("num_channels");
    audio_needed.reg_field("device_id");
    prog.reg_class(audio_needed);
}

fn dict_has(actor: &mut Actor, mut d: Value, key: Value) -> Result<Value, String>
{
    let d = d.unwrap_dict();
    let key = unwrap_str!(key);
    Ok(Value::from(d.has(key)))
}

/// Get the method associated with a core value
pub fn get_method(val: Value, method_name: &str) -> Value
{
    use crate::host::HostFn;
    use crate::host::FnPtr::*;
    use crate::array::*;
    use crate::bytearray::*;

    static TRUE_TO_S: HostFn = HostFn { name: "to_s", f: Fn1(true_to_s) };
    static FALSE_TO_S: HostFn = HostFn { name: "to_s", f: Fn1(false_to_s) };
    static NIL_TO_S: HostFn = HostFn { name: "to_s", f: Fn1(nil_to_s) };

    static INT64_ABS: HostFn = HostFn { name: "abs", f: Fn1(int64_abs) };
    static INT64_MIN: HostFn = HostFn { name: "min", f: Fn2(int64_min) };
    static INT64_MAX: HostFn = HostFn { name: "max", f: Fn2(int64_max) };
    static INT64_TO_F: HostFn = HostFn { name: "to_f", f: Fn1(int64_to_f) };
    static INT64_TO_S: HostFn = HostFn { name: "to_s", f: Fn1(int64_to_s) };

    static FLOAT64_ABS: HostFn = HostFn { name: "abs", f: Fn1(float64_abs) };
    static FLOAT64_CEIL: HostFn = HostFn { name: "ceil", f: Fn1(float64_ceil) };
    static FLOAT64_FLOOR: HostFn = HostFn { name: "floor", f: Fn1(float64_floor) };
    static FLOAT64_TRUNC: HostFn = HostFn { name: "trunc", f: Fn1(float64_trunc) };
    static FLOAT64_SIN: HostFn = HostFn { name: "sin", f: Fn1(float64_sin) };
    static FLOAT64_COS: HostFn = HostFn { name: "cos", f: Fn1(float64_cos) };
    static FLOAT64_TAN: HostFn = HostFn { name: "tan", f: Fn1(float64_tan) };
    static FLOAT64_ATAN: HostFn = HostFn { name: "atan", f: Fn1(float64_atan) };
    static FLOAT64_SQRT: HostFn = HostFn { name: "sqrt", f: Fn1(float64_sqrt) };
    static FLOAT64_MIN: HostFn = HostFn { name: "min", f: Fn2(float64_min) };
    static FLOAT64_MAX: HostFn = HostFn { name: "max", f: Fn2(float64_max) };
    static FLOAT64_CLIP: HostFn = HostFn { name: "clip", f: Fn3(float64_clip) };
    static FLOAT64_POW: HostFn = HostFn { name: "pow", f: Fn2(float64_pow) };
    static FLOAT64_EXP: HostFn = HostFn { name: "exp", f: Fn1(float64_exp) };
    static FLOAT64_LN: HostFn = HostFn { name: "ln", f: Fn1(float64_ln) };
    static FLOAT64_TO_F: HostFn = HostFn { name: "to_f", f: Fn1(identity_method) };
    static FLOAT64_TO_S: HostFn = HostFn { name: "to_s", f: Fn1(float64_to_s) };
    static FLOAT64_FORMAT_DECIMALS: HostFn = HostFn { name: "format_decimals", f: Fn2(float64_format_decimals) };

    static STRING_FROM_CODEPOINT: HostFn = HostFn { name: "from_codepoint", f: Fn2(string_from_codepoint) };
    static STRING_BYTE_AT: HostFn = HostFn { name: "byte_at", f: Fn2(string_byte_at) };
    static STRING_CHAR_AT: HostFn = HostFn { name: "char_at", f: Fn2(string_char_at) };
    static STRING_PARSE_INT: HostFn = HostFn { name: "parse_int", f: Fn2(string_parse_int) };
    static STRING_TRIM: HostFn = HostFn { name: "trim", f: Fn1(string_trim) };
    static STRING_SPLIT: HostFn = HostFn { name: "split", f: Fn2(string_split) };
    static STRING_TO_S: HostFn = HostFn { name: "to_s", f: Fn1(identity_method) };

    static ARRAY_WITH_SIZE: HostFn = HostFn { name: "with_size", f: Fn3(array_with_size) };
    static ARRAY_PUSH: HostFn = HostFn { name: "push", f: Fn2(array_push) };
    static ARRAY_POP: HostFn = HostFn { name: "pop", f: Fn1(array_pop) };
    static ARRAY_REMOVE: HostFn = HostFn { name: "remove", f: Fn2(array_remove) };
    static ARRAY_INSERT: HostFn = HostFn { name: "insert", f: Fn3(array_insert) };
    static ARRAY_APPEND: HostFn = HostFn { name: "append", f: Fn2(array_append) };

    static BA_NEW: HostFn = HostFn { name: "new", f: Fn1(ba_new) };
    static BA_WITH_SIZE: HostFn = HostFn { name: "with_size", f: Fn2(ba_with_size) };
    static BA_FILL_U32: HostFn = HostFn { name: "fill_u32", f: Fn4(ba_fill_u32) };
    static BA_READ_U32: HostFn = HostFn { name: "load_u32", f: Fn2(ba_load_u32) };
    static BA_WRITE_U32: HostFn = HostFn { name: "store_u32", f: Fn3(ba_store_u32) };
    static BA_READ_U16: HostFn = HostFn { name: "load_u16", f: Fn2(ba_load_u16) };
    static BA_WRITE_U16: HostFn = HostFn { name: "store_u16", f: Fn3(ba_store_u16) };
    static BA_READ_F32: HostFn = HostFn { name: "load_f32", f: Fn2(ba_load_f32) };
    static BA_WRITE_F32: HostFn = HostFn { name: "store_f32", f: Fn3(ba_store_f32) };
    static BA_MEMCPY: HostFn = HostFn { name: "memcpy", f: Fn5(ba_memcpy) };
    static BA_RESIZE: HostFn = HostFn { name: "resize", f: Fn2(ba_resize) };
    static BA_ZERO_FILL: HostFn = HostFn { name: "zero_fill", f: Fn1(ba_zero_fill) };
    static BA_BLIT_BGRA32: HostFn = HostFn { name: "blit_bgra32", f: Fn8(ba_blit_bgra32) };

    static DICT_HAS: HostFn = HostFn { name: "has", f: Fn2(dict_has) };

    let f = match (val, method_name) {
        (Value::Int64(_), "abs") => &INT64_ABS,
        (Value::Int64(_), "min") => &INT64_MIN,
        (Value::Int64(_), "max") => &INT64_MAX,
        (Value::Int64(_), "to_f") => &INT64_TO_F,
        (Value::Int64(_), "to_s") => &INT64_TO_S,

        (Value::Float64(_), "abs") => &FLOAT64_ABS,
        (Value::Float64(_), "ceil") => &FLOAT64_CEIL,
        (Value::Float64(_), "floor") => &FLOAT64_FLOOR,
        (Value::Float64(_), "trunc") => &FLOAT64_TRUNC,
        (Value::Float64(_), "sin") => &FLOAT64_SIN,
        (Value::Float64(_), "cos") => &FLOAT64_COS,
        (Value::Float64(_), "tan") => &FLOAT64_TAN,
        (Value::Float64(_), "atan") => &FLOAT64_ATAN,
        (Value::Float64(_), "sqrt") => &FLOAT64_SQRT,
        (Value::Float64(_), "min") => &FLOAT64_MIN,
        (Value::Float64(_), "max") => &FLOAT64_MAX,
        (Value::Float64(_), "clip") => &FLOAT64_CLIP,
        (Value::Float64(_), "pow") => &FLOAT64_POW,
        (Value::Float64(_), "exp") => &FLOAT64_EXP,
        (Value::Float64(_), "ln") => &FLOAT64_LN,
        (Value::Float64(_), "to_f") => &FLOAT64_TO_F,
        (Value::Float64(_), "to_s") => &FLOAT64_TO_S,
        (Value::Float64(_), "format_decimals") => &FLOAT64_FORMAT_DECIMALS,

        (Value::Class(STRING_ID), "from_codepoint") => &STRING_FROM_CODEPOINT,
        (Value::String(_), "byte_at") => &STRING_BYTE_AT,
        (Value::String(_), "char_at") => &STRING_CHAR_AT,
        (Value::String(_), "parse_int") => &STRING_PARSE_INT,
        (Value::String(_), "trim") => &STRING_TRIM,
        (Value::String(_), "split") => &STRING_SPLIT,
        (Value::String(_), "to_s") => &STRING_TO_S,

        (Value::Class(ARRAY_ID), "with_size") => &ARRAY_WITH_SIZE,
        (Value::Array(_), "push") => &ARRAY_PUSH,
        (Value::Array(_), "pop") => &ARRAY_POP,
        (Value::Array(_), "remove") => &ARRAY_REMOVE,
        (Value::Array(_), "insert") => &ARRAY_INSERT,
        (Value::Array(_), "append") => &ARRAY_APPEND,

        (Value::Class(BYTEARRAY_ID), "new") => &BA_NEW,
        (Value::Class(BYTEARRAY_ID), "with_size") => &BA_WITH_SIZE,
        (Value::ByteArray(_), "fill_u32") => &BA_FILL_U32,
        (Value::ByteArray(_), "load_u32") => &BA_READ_U32,
        (Value::ByteArray(_), "store_u32") => &BA_WRITE_U32,
        (Value::ByteArray(_), "load_u16") => &BA_READ_U16,
        (Value::ByteArray(_), "store_u16") => &BA_WRITE_U16,
        (Value::ByteArray(_), "load_f32") => &BA_READ_F32,
        (Value::ByteArray(_), "store_f32") => &BA_WRITE_F32,
        (Value::ByteArray(_), "memcpy") => &BA_MEMCPY,
        (Value::ByteArray(_), "resize") => &BA_RESIZE,
        (Value::ByteArray(_), "zero_fill") => &BA_ZERO_FILL,
        (Value::ByteArray(_), "blit_bgra32") => &BA_BLIT_BGRA32,

        (Value::Dict(_), "has") => &DICT_HAS,

        (Value::True, "to_s") => &TRUE_TO_S,
        (Value::False, "to_s") => &FALSE_TO_S,
        (Value::Nil, "to_s") => &NIL_TO_S,

        // Method not defined on type
        _ => return Value::Nil,
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

        Value::Nil => NIL_ID,
        Value::True => BOOL_ID,
        Value::False => BOOL_ID,
        Value::Int64(_) => INT64_ID,
        Value::Float64(_) => FLOAT64_ID,
        Value::String(_) => STRING_ID,
        Value::Array(_) => ARRAY_ID,
        Value::ByteArray(_) => BYTEARRAY_ID,
        Value::Dict(_) => DICT_ID,

        _ => todo!("get_class_id for unsupported type")
    }
}
