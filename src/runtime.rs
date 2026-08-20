use std::mem::size_of;
use crate::alloc::Tag;
use crate::array::Array;
use crate::ast::*;
use crate::vm::Actor;
use crate::value::*;
use crate::str::Str;
use crate::*;

fn identity_method(actor: &mut Actor, self_val: Value) -> Result<Value, String>
{
    Ok(self_val)
}

fn true_to_s(actor: &mut Actor, _v: Value) -> Result<Value, String>
{
    Ok(actor.intern_str("true"))
}

fn false_to_s(actor: &mut Actor, _v: Value) -> Result<Value, String>
{
    Ok(actor.intern_str("false"))
}

fn nil_to_s(actor: &mut Actor, _v: Value) -> Result<Value, String>
{
    Ok(actor.intern_str("nil"))
}

fn int64_abs(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    Ok(actor.int64(if v > 0 { v } else { -v }))
}

fn int64_min(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let other = unwrap_i64!(other);
    Ok(actor.int64(v.min(other)))
}

fn int64_max(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let other = unwrap_i64!(other);
    Ok(actor.int64(v.max(other)))
}

fn int64_to_f(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    Ok(actor.float64(v as f64))
}

fn int64_to_s(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let s = format!("{}", v);

    actor.gc_check(Str::alloc_size(32), &mut []);
    Ok(actor.alloc.str_val(&s))
}

fn int64_to_hex(actor: &mut Actor, v: Value, digits: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let digits = unwrap_usize!(digits);
    let s = format!("{:0width$X}", v, width = digits);

    actor.gc_check(Str::alloc_size(32 + digits), &mut []);
    Ok(actor.alloc.str_val(&s))
}

fn float64_abs(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(if v > 0.0 { v } else { -v }))
}

fn float64_ceil(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    // TODO: check that float value fits in integer range
    let v = unwrap_f64!(v);
    let int_val = v.ceil() as i64;
    Ok(actor.int64(int_val))
}

fn float64_floor(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    // TODO: check that float value fits in integer range
    let v = unwrap_f64!(v);
    let int_val = v.floor() as i64;
    Ok(actor.int64(int_val))
}

fn float64_trunc(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    // TODO: check that float value fits in integer range
    let v = unwrap_f64!(v);
    let int_val = v.trunc() as i64;
    Ok(actor.int64(int_val))
}

fn float64_sin(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.sin()))
}

fn float64_cos(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.cos()))
}

fn float64_tan(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.tan()))
}

fn float64_atan(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.atan()))
}

fn float64_sqrt(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.sqrt()))
}

fn float64_min(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let other = unwrap_f64!(other);
    Ok(actor.float64(v.min(other)))
}

fn float64_max(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let other = unwrap_f64!(other);
    Ok(actor.float64(v.max(other)))
}

fn float64_clip(actor: &mut Actor, v: Value, min: Value, max: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let min = unwrap_f64!(min);
    let max = unwrap_f64!(max);
    Ok(actor.float64(v.clamp(min, max)))
}

fn float64_pow(actor: &mut Actor, v: Value, exponent: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let exponent = unwrap_f64!(exponent);
    Ok(actor.float64(v.powf(exponent)))
}

fn float64_exp(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.exp()))
}

fn float64_ln(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.ln()))
}

fn float64_to_s(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let s = format!("{}", v);
    actor.gc_check(Str::alloc_size(1024), &mut []);
    Ok(actor.alloc.str_val(&s))
}

fn float64_format_decimals(actor: &mut Actor, v: Value, decimals: Value) -> Result<Value, String>
{
    let num = unwrap_f64!(v);
    let decimals = unwrap_usize!(decimals);
    let s = format!("{:.*}", decimals, num);
    actor.gc_check(Str::alloc_size(std::cmp::max(1024, decimals)), &mut []);
    Ok(actor.alloc.str_val(&s))
}

/// Create a single-character string from a codepoint integer value
fn string_from_codepoint(actor: &mut Actor, _class: Value, codepoint: Value) -> Result<Value, String>
{
    // TODO: eventually we can add caching for this,
    // at least for ASCII character values, we can
    // easily intern those strings

    let codepoint = unwrap_u32!(codepoint);
    let ch = char::from_u32(codepoint).expect("Invalid Unicode codepoint");

    let mut s = String::new();
    s.push(ch);

    actor.gc_check(Str::alloc_size(s.len()), &mut []);
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
        return Ok(Value::NIL);
    }

    let ch = s[byte_idx..].chars().next();

    let ch = match ch {
        // Not a valid character
        None => return Ok(Value::NIL),
        Some(ch) => ch,
    };

    let ch_s = ch.to_string();
    actor.gc_check(Str::alloc_size(ch_s.len()), &mut []);
    Ok(actor.alloc.str_val(&ch_s))
}

/// Try to parse the string as an integer with the given radix
fn string_parse_int(actor: &mut Actor, s: Value, radix: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let radix = unwrap_u32!(radix);

    match i64::from_str_radix(s, radix) {
        Ok(int_val) => Ok(actor.int64(int_val)),
        Err(_) => Ok(Value::NIL),
    }
}

/// Try to parse the string as a float
fn string_parse_float(actor: &mut Actor, s: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);

    match s.parse::<f64>() {
        Ok(float_val) => Ok(actor.float64(float_val)),
        Err(_) => Ok(Value::NIL),
    }
}

/// Trim whitespace
fn string_trim(actor: &mut Actor, s: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let s = s.trim().to_string();
    actor.gc_check(Str::alloc_size(s.len()), &mut []);
    Ok(actor.alloc.str_val(&s))
}

/// Uppercase a String
fn string_upper(actor: &mut Actor, s: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let s = s.to_uppercase();
    actor.gc_check(Str::alloc_size(s.len()), &mut []);
    Ok(actor.alloc.str_val(&s))
}

/// Lowercase a String
fn string_lower(actor: &mut Actor, s: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let s = s.to_lowercase();
    actor.gc_check(Str::alloc_size(s.len()), &mut []);
    Ok(actor.alloc.str_val(&s))
}

/// Split a string by a separator and return an array of strings
fn string_split(actor: &mut Actor, mut input: Value, sep: Value) -> Result<Value, String>
{
    let s = unwrap_str!(input);
    let sep = unwrap_str!(sep);

    // Copy the input in case we have to trigger GC
    let s = s.to_owned();

    // Split the string into tokens
    let str_parts: Vec<&str> = s.split(sep).collect();
    let num_strs = str_parts.len();
    let total_str_len: usize = str_parts.iter().map(|s| s.len()).sum();

    // We need to keep the input string alive because we're
    // relying on string slices for the string parts
    // The extra 8 bytes per string cover rounding each one up to the
    // allocation alignment
    actor.gc_check(
        Array::alloc_size(num_strs) +
        num_strs * Str::alloc_size(8) +
        total_str_len,
        &mut []
    );

    let mut array = Array::with_capacity(num_strs, &mut actor.alloc);
    for part in str_parts {
        array.push(actor.alloc.str_val(part), &mut actor.alloc);
    }

    Ok(Value::array(actor.alloc.alloc(array, Tag::Array)))
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
    let d = unwrap_dict!(d);
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
    static INT64_TO_HEX: HostFn = HostFn { name: "to_hex", f: Fn2(int64_to_hex) };

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
    static STRING_PARSE_FLOAT: HostFn = HostFn { name: "parse_float", f: Fn1(string_parse_float) };
    static STRING_TRIM: HostFn = HostFn { name: "trim", f: Fn1(string_trim) };
    static STRING_UPPER: HostFn = HostFn { name: "upper", f: Fn1(string_upper) };
    static STRING_LOWER: HostFn = HostFn { name: "lower", f: Fn1(string_lower) };
    static STRING_SPLIT: HostFn = HostFn { name: "split", f: Fn2(string_split) };
    static STRING_TO_S: HostFn = HostFn { name: "to_s", f: Fn1(identity_method) };

    static ARRAY_WITH_SIZE: HostFn = HostFn { name: "with_size", f: Fn3(array_with_size) };
    static ARRAY_PUSH: HostFn = HostFn { name: "push", f: Fn2(array_push) };
    static ARRAY_POP: HostFn = HostFn { name: "pop", f: Fn1(array_pop) };
    static ARRAY_REMOVE: HostFn = HostFn { name: "remove", f: Fn2(array_remove) };
    static ARRAY_INSERT: HostFn = HostFn { name: "insert", f: Fn3(array_insert) };
    static ARRAY_APPEND: HostFn = HostFn { name: "append", f: Fn2(array_append) };
    static ARRAY_RESIZE: HostFn = HostFn { name: "resize", f: Fn3(array_resize) };

    static BA_WITH_SIZE: HostFn = HostFn { name: "with_size", f: Fn2(ba_with_size) };
    static BA_READ_U32: HostFn = HostFn { name: "load_u32", f: Fn2(ba_load_u32) };
    static BA_WRITE_U32: HostFn = HostFn { name: "store_u32", f: Fn3(ba_store_u32) };
    static BA_READ_U16: HostFn = HostFn { name: "load_u16", f: Fn2(ba_load_u16) };
    static BA_WRITE_U16: HostFn = HostFn { name: "store_u16", f: Fn3(ba_store_u16) };
    static BA_READ_F32: HostFn = HostFn { name: "load_f32", f: Fn2(ba_load_f32) };
    static BA_WRITE_F32: HostFn = HostFn { name: "store_f32", f: Fn3(ba_store_f32) };
    static BA_GET_U32: HostFn = HostFn { name: "get_u32", f: Fn2(ba_get_u32) };
    static BA_SET_U32: HostFn = HostFn { name: "set_u32", f: Fn3(ba_set_u32) };
    static BA_GET_F32: HostFn = HostFn { name: "get_f32", f: Fn2(ba_get_f32) };
    static BA_SET_F32: HostFn = HostFn { name: "set_f32", f: Fn3(ba_set_f32) };
    static BA_NUM_U32: HostFn = HostFn { name: "num_u32", f: Fn1(ba_num_u32) };
    static BA_MEMCPY: HostFn = HostFn { name: "memcpy", f: Fn5(ba_memcpy) };
    static BA_RESIZE: HostFn = HostFn { name: "resize", f: Fn2(ba_resize) };
    static BA_ZERO_FILL: HostFn = HostFn { name: "zero_fill", f: Fn1(ba_zero_fill) };
    static BA_FILL_U32: HostFn = HostFn { name: "fill_u32", f: Fn4(ba_fill_u32) };
    static BA_BLIT_BGRA32: HostFn = HostFn { name: "blit_bgra32", f: Fn8(ba_blit_bgra32) };

    static DICT_HAS: HostFn = HostFn { name: "has", f: Fn2(dict_has) };

    // Dispatch on the language-level type first, so that a value that
    // has no methods at all costs one branch and no string compares
    let f = match (val.type_of(), method_name) {
        (Type::Int64, "abs") => &INT64_ABS,
        (Type::Int64, "min") => &INT64_MIN,
        (Type::Int64, "max") => &INT64_MAX,
        (Type::Int64, "to_f") => &INT64_TO_F,
        (Type::Int64, "to_s") => &INT64_TO_S,
        (Type::Int64, "to_hex") => &INT64_TO_HEX,

        (Type::Float64, "abs") => &FLOAT64_ABS,
        (Type::Float64, "ceil") => &FLOAT64_CEIL,
        (Type::Float64, "floor") => &FLOAT64_FLOOR,
        (Type::Float64, "trunc") => &FLOAT64_TRUNC,
        (Type::Float64, "sin") => &FLOAT64_SIN,
        (Type::Float64, "cos") => &FLOAT64_COS,
        (Type::Float64, "tan") => &FLOAT64_TAN,
        (Type::Float64, "atan") => &FLOAT64_ATAN,
        (Type::Float64, "sqrt") => &FLOAT64_SQRT,
        (Type::Float64, "min") => &FLOAT64_MIN,
        (Type::Float64, "max") => &FLOAT64_MAX,
        (Type::Float64, "clip") => &FLOAT64_CLIP,
        (Type::Float64, "pow") => &FLOAT64_POW,
        (Type::Float64, "exp") => &FLOAT64_EXP,
        (Type::Float64, "ln") => &FLOAT64_LN,
        (Type::Float64, "to_f") => &FLOAT64_TO_F,
        (Type::Float64, "to_s") => &FLOAT64_TO_S,
        (Type::Float64, "format_decimals") => &FLOAT64_FORMAT_DECIMALS,

        (Type::String, "byte_at") => &STRING_BYTE_AT,
        (Type::String, "char_at") => &STRING_CHAR_AT,
        (Type::String, "parse_int") => &STRING_PARSE_INT,
        (Type::String, "parse_float") => &STRING_PARSE_FLOAT,
        (Type::String, "trim") => &STRING_TRIM,
        (Type::String, "upper") => &STRING_UPPER,
        (Type::String, "lower") => &STRING_LOWER,
        (Type::String, "split") => &STRING_SPLIT,
        (Type::String, "to_s") => &STRING_TO_S,

        (Type::Array, "push") => &ARRAY_PUSH,
        (Type::Array, "pop") => &ARRAY_POP,
        (Type::Array, "remove") => &ARRAY_REMOVE,
        (Type::Array, "insert") => &ARRAY_INSERT,
        (Type::Array, "append") => &ARRAY_APPEND,
        (Type::Array, "resize") => &ARRAY_RESIZE,

        (Type::ByteArray, "load_u32") => &BA_READ_U32,
        (Type::ByteArray, "store_u32") => &BA_WRITE_U32,
        (Type::ByteArray, "load_u16") => &BA_READ_U16,
        (Type::ByteArray, "store_u16") => &BA_WRITE_U16,
        (Type::ByteArray, "load_f32") => &BA_READ_F32,
        (Type::ByteArray, "store_f32") => &BA_WRITE_F32,
        (Type::ByteArray, "get_u32") => &BA_GET_U32,
        (Type::ByteArray, "set_u32") => &BA_SET_U32,
        (Type::ByteArray, "get_f32") => &BA_GET_F32,
        (Type::ByteArray, "set_f32") => &BA_SET_F32,
        (Type::ByteArray, "num_u32") => &BA_NUM_U32,
        (Type::ByteArray, "num_f32") => &BA_NUM_U32,
        (Type::ByteArray, "memcpy") => &BA_MEMCPY,
        (Type::ByteArray, "resize") => &BA_RESIZE,
        (Type::ByteArray, "zero_fill") => &BA_ZERO_FILL,
        (Type::ByteArray, "fill_u32") => &BA_FILL_U32,
        (Type::ByteArray, "blit_bgra32") => &BA_BLIT_BGRA32,

        (Type::Dict, "has") => &DICT_HAS,

        (Type::Bool, "to_s") => if val.as_bool() { &TRUE_TO_S } else { &FALSE_TO_S },
        (Type::Nil, "to_s") => &NIL_TO_S,

        // Static methods, called on the class itself
        (Type::Class, _) => match (val.as_class(), method_name) {
            (STRING_ID, "from_codepoint") => &STRING_FROM_CODEPOINT,
            (ARRAY_ID, "with_size") => &ARRAY_WITH_SIZE,
            (BYTEARRAY_ID, "with_size") => &BA_WITH_SIZE,
            _ => return Value::NIL,
        }

        // Method not defined on type
        _ => return Value::NIL,
    };

    Value::host_fn(f)
}

pub fn get_class_id(val: Value) -> ClassId
{
    match val.type_of() {
        Type::Object => val.as_obj().class_id,

        Type::Nil => NIL_ID,
        Type::Bool => BOOL_ID,
        Type::Int64 => INT64_ID,
        Type::Float64 => FLOAT64_ID,
        Type::String => STRING_ID,
        Type::Array => ARRAY_ID,
        Type::ByteArray => BYTEARRAY_ID,
        Type::Dict => DICT_ID,

        t => todo!("get_class_id for {:?} values", t)
    }
}
