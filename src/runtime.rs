use crate::array::Array;
use crate::ast::*;
use crate::vm::Actor;
use crate::value::*;
use crate::str::Str;
use crate::*;
use crate::host::HostFnId;

pub(crate) fn identity_method(_actor: &mut Actor, self_val: Value) -> Result<Value, String>
{
    Ok(self_val)
}

// `to_f` on a float and `to_s` on a string both hand back self. They are
// named here so that the host function table can find them by id
pub(crate) use self::identity_method as float64_to_f;
pub(crate) use self::identity_method as string_to_s;

pub(crate) fn true_to_s(actor: &mut Actor, _v: Value) -> Result<Value, String>
{
    Ok(actor.intern_str("true"))
}

pub(crate) fn false_to_s(actor: &mut Actor, _v: Value) -> Result<Value, String>
{
    Ok(actor.intern_str("false"))
}

pub(crate) fn nil_to_s(actor: &mut Actor, _v: Value) -> Result<Value, String>
{
    Ok(actor.intern_str("nil"))
}

pub(crate) fn int64_abs(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    Ok(actor.int64(if v > 0 { v } else { -v }))
}

pub(crate) fn int64_min(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let other = unwrap_i64!(other);
    Ok(actor.int64(v.min(other)))
}

pub(crate) fn int64_max(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let other = unwrap_i64!(other);
    Ok(actor.int64(v.max(other)))
}

/// Truncated integer division. The `/` operator always yields a float, so
/// this is how integer code divides without leaving the integer domain.
pub(crate) fn int64_idiv(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let other = unwrap_i64!(other);

    match v.checked_div(other) {
        Some(q) => Ok(actor.int64(q)),
        None if other == 0 => Err("division by zero in idiv()".into()),
        None => Err("integer overflow in idiv()".into()),
    }
}

pub(crate) fn int64_clip(actor: &mut Actor, v: Value, min: Value, max: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let min = unwrap_i64!(min);
    let max = unwrap_i64!(max);

    if min > max {
        return Err("min must be less than or equal to max in clip()".into());
    }

    Ok(actor.int64(v.clamp(min, max)))
}

pub(crate) fn int64_to_f(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    Ok(actor.float64(v as f64))
}

pub(crate) fn int64_to_s(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let s = format!("{}", v);

    actor.gc_check(Str::alloc_size(32), &mut []);
    Ok(Str::new(&s, &mut actor.alloc))
}

pub(crate) fn int64_comma_sep(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let s = utils::thousands_sep(v);

    actor.gc_check(Str::alloc_size(s.len()), &mut []);
    Ok(Str::new(&s, &mut actor.alloc))
}

pub(crate) fn int64_to_hex(actor: &mut Actor, v: Value, digits: Value) -> Result<Value, String>
{
    let v = unwrap_i64!(v);
    let digits = unwrap_usize!(digits);
    let s = format!("{:0width$X}", v, width = digits);

    actor.gc_check(Str::alloc_size(32 + digits), &mut []);
    Ok(Str::new(&s, &mut actor.alloc))
}

pub(crate) fn float64_abs(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(if v > 0.0 { v } else { -v }))
}

pub(crate) fn float64_ceil(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    // TODO: check that float value fits in integer range
    let v = unwrap_f64!(v);
    let int_val = v.ceil() as i64;
    Ok(actor.int64(int_val))
}

pub(crate) fn float64_floor(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    // TODO: check that float value fits in integer range
    let v = unwrap_f64!(v);
    let int_val = v.floor() as i64;
    Ok(actor.int64(int_val))
}

pub(crate) fn float64_trunc(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    // TODO: check that float value fits in integer range
    let v = unwrap_f64!(v);
    let int_val = v.trunc() as i64;
    Ok(actor.int64(int_val))
}

pub(crate) fn float64_sin(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.sin()))
}

pub(crate) fn float64_cos(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.cos()))
}

pub(crate) fn float64_tan(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.tan()))
}

pub(crate) fn float64_atan(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.atan()))
}

pub(crate) fn float64_sqrt(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.sqrt()))
}

pub(crate) fn float64_min(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let other = unwrap_f64!(other);
    Ok(actor.float64(v.min(other)))
}

pub(crate) fn float64_max(actor: &mut Actor, v: Value, other: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let other = unwrap_f64!(other);
    Ok(actor.float64(v.max(other)))
}

pub(crate) fn float64_clip(actor: &mut Actor, v: Value, min: Value, max: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let min = unwrap_f64!(min);
    let max = unwrap_f64!(max);
    Ok(actor.float64(v.clamp(min, max)))
}

pub(crate) fn float64_pow(actor: &mut Actor, v: Value, exponent: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let exponent = unwrap_f64!(exponent);
    Ok(actor.float64(v.powf(exponent)))
}

pub(crate) fn float64_exp(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.exp()))
}

pub(crate) fn float64_ln(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    Ok(actor.float64(v.ln()))
}

pub(crate) fn float64_to_s(actor: &mut Actor, v: Value) -> Result<Value, String>
{
    let v = unwrap_f64!(v);
    let s = format!("{}", v);
    actor.gc_check(Str::alloc_size(1024), &mut []);
    Ok(Str::new(&s, &mut actor.alloc))
}

pub(crate) fn float64_format_decimals(actor: &mut Actor, v: Value, decimals: Value) -> Result<Value, String>
{
    let num = unwrap_f64!(v);
    let decimals = unwrap_usize!(decimals);
    let s = format!("{:.*}", decimals, num);
    actor.gc_check(Str::alloc_size(std::cmp::max(1024, decimals)), &mut []);
    Ok(Str::new(&s, &mut actor.alloc))
}

/// Create a single-character string from a codepoint integer value
pub(crate) fn string_from_codepoint(actor: &mut Actor, _class: Value, codepoint: Value) -> Result<Value, String>
{
    // TODO: eventually we can add caching for this,
    // at least for ASCII character values, we can
    // easily intern those strings

    let codepoint = unwrap_u32!(codepoint);
    let ch = char::from_u32(codepoint).expect("Invalid Unicode codepoint");

    let mut s = String::new();
    s.push(ch);

    actor.gc_check(Str::alloc_size(s.len()), &mut []);
    Ok(Str::new(&s, &mut actor.alloc))
}

/// Get the UTF-8 byte at the given index
pub(crate) fn string_byte_at(_actor: &mut Actor, s: Value, idx: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let idx = unwrap_usize!(idx);
    let byte = s.as_bytes().get(idx).unwrap();
    Ok(Value::from(*byte))
}

/// Get a string containing the single character at the given byte index
/// Returns nil if not a valid character boundary or character
pub(crate) fn string_char_at(actor: &mut Actor, s: Value, byte_idx: Value) -> Result<Value, String>
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
    Ok(Str::new(&ch_s, &mut actor.alloc))
}

/// Try to parse the string as an integer with the given radix
pub(crate) fn string_parse_int(actor: &mut Actor, s: Value, radix: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let radix = unwrap_u32!(radix);

    match i64::from_str_radix(s, radix) {
        Ok(int_val) => Ok(actor.int64(int_val)),
        Err(_) => Ok(Value::NIL),
    }
}

/// Try to parse the string as a float
pub(crate) fn string_parse_float(actor: &mut Actor, s: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);

    match s.parse::<f64>() {
        Ok(float_val) => Ok(actor.float64(float_val)),
        Err(_) => Ok(Value::NIL),
    }
}

/// Trim whitespace
pub(crate) fn string_trim(actor: &mut Actor, s: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let s = s.trim().to_string();
    actor.gc_check(Str::alloc_size(s.len()), &mut []);
    Ok(Str::new(&s, &mut actor.alloc))
}

/// Uppercase a String
pub(crate) fn string_upper(actor: &mut Actor, s: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let s = s.to_uppercase();
    actor.gc_check(Str::alloc_size(s.len()), &mut []);
    Ok(Str::new(&s, &mut actor.alloc))
}

/// Lowercase a String
pub(crate) fn string_lower(actor: &mut Actor, s: Value) -> Result<Value, String>
{
    let s = unwrap_str!(s);
    let s = s.to_lowercase();
    actor.gc_check(Str::alloc_size(s.len()), &mut []);
    Ok(Str::new(&s, &mut actor.alloc))
}

/// Split a string by a separator and return an array of strings
pub(crate) fn string_split(actor: &mut Actor, input: Value, sep: Value) -> Result<Value, String>
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

    let array = Array::with_capacity(num_strs, &mut actor.alloc);
    for part in str_parts {
        let str_val = Str::new(part, &mut actor.alloc);
        array.as_arr().push(str_val, &mut actor.alloc);
    }

    Ok(array)
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

    // AudioData
    // Note: the field order must match the slots written
    // by the audio input callback
    let mut audio_data = Class::default();
    audio_data.id = AUDIO_DATA_ID;
    audio_data.reg_field("device_id");
    audio_data.reg_field("num_samples");
    prog.reg_class(audio_data);
}

pub(crate) fn dict_has(_actor: &mut Actor, d: Value, key: Value) -> Result<Value, String>
{
    let d = unwrap_dict!(d);
    let key = unwrap_str!(key);
    Ok(Value::from(d.has(key)))
}

pub(crate) fn fun_dump_bytecode(actor: &mut Actor, f: Value) -> Result<Value, String>
{
    let dump = actor.dump_fun_bytecode(f)?;
    print!("{}", dump);
    Ok(Value::NIL)
}

/// Get the method associated with a core value
pub fn get_method(val: Value, method_name: &str) -> Option<HostFnId>
{
    use crate::host::HostFnId::*;

    // Dispatch on the language-level type first, so that a value that
    // has no methods at all costs one branch and no string compares
    let f = match (val.type_of(), method_name) {
        (Type::Int64, "abs") => int64_abs,
        (Type::Int64, "min") => int64_min,
        (Type::Int64, "max") => int64_max,
        (Type::Int64, "clip") => int64_clip,
        (Type::Int64, "idiv") => int64_idiv,
        (Type::Int64, "to_f") => int64_to_f,
        (Type::Int64, "to_s") => int64_to_s,
        (Type::Int64, "comma_sep") => int64_comma_sep,
        (Type::Int64, "to_hex") => int64_to_hex,

        (Type::Float64, "abs") => float64_abs,
        (Type::Float64, "ceil") => float64_ceil,
        (Type::Float64, "floor") => float64_floor,
        (Type::Float64, "trunc") => float64_trunc,
        (Type::Float64, "sin") => float64_sin,
        (Type::Float64, "cos") => float64_cos,
        (Type::Float64, "tan") => float64_tan,
        (Type::Float64, "atan") => float64_atan,
        (Type::Float64, "sqrt") => float64_sqrt,
        (Type::Float64, "min") => float64_min,
        (Type::Float64, "max") => float64_max,
        (Type::Float64, "clip") => float64_clip,
        (Type::Float64, "pow") => float64_pow,
        (Type::Float64, "exp") => float64_exp,
        (Type::Float64, "ln") => float64_ln,
        (Type::Float64, "to_f") => float64_to_f,
        (Type::Float64, "to_s") => float64_to_s,
        (Type::Float64, "format_decimals") => float64_format_decimals,

        (Type::String, "byte_at") => string_byte_at,
        (Type::String, "char_at") => string_char_at,
        (Type::String, "parse_int") => string_parse_int,
        (Type::String, "parse_float") => string_parse_float,
        (Type::String, "trim") => string_trim,
        (Type::String, "upper") => string_upper,
        (Type::String, "lower") => string_lower,
        (Type::String, "split") => string_split,
        (Type::String, "to_s") => string_to_s,

        (Type::Array, "push") => array_push,
        (Type::Array, "pop") => array_pop,
        (Type::Array, "remove") => array_remove,
        (Type::Array, "insert") => array_insert,
        (Type::Array, "append") => array_append,
        (Type::Array, "resize") => array_resize,

        (Type::ByteArray, "load_u32") => ba_load_u32,
        (Type::ByteArray, "store_u32") => ba_store_u32,
        (Type::ByteArray, "load_u16") => ba_load_u16,
        (Type::ByteArray, "store_u16") => ba_store_u16,
        (Type::ByteArray, "load_f32") => ba_load_f32,
        (Type::ByteArray, "store_f32") => ba_store_f32,
        (Type::ByteArray, "get_u32") => ba_get_u32,
        (Type::ByteArray, "set_u32") => ba_set_u32,
        (Type::ByteArray, "get_f32") => ba_get_f32,
        (Type::ByteArray, "set_f32") => ba_set_f32,
        (Type::ByteArray, "dot_f32") => ba_dot_f32,
        (Type::ByteArray, "num_u32") => ba_num_u32,
        (Type::ByteArray, "num_f32") => ba_num_u32,
        (Type::ByteArray, "memcpy") => ba_memcpy,
        (Type::ByteArray, "resize") => ba_resize,
        (Type::ByteArray, "zero_fill") => ba_zero_fill,
        (Type::ByteArray, "fill_u32") => ba_fill_u32,

        (Type::Dict, "has") => dict_has,

        (Type::Fun, "dump_bytecode") => fun_dump_bytecode,
        (Type::Closure, "dump_bytecode") => fun_dump_bytecode,

        (Type::Bool, "to_s") => if val.as_bool() { true_to_s } else { false_to_s },
        (Type::Nil, "to_s") => nil_to_s,

        // Static methods, called on the class itself
        (Type::Class, _) => match (val.as_class(), method_name) {
            (STRING_ID, "from_codepoint") => string_from_codepoint,
            (ARRAY_ID, "with_size") => array_with_size,
            (BYTEARRAY_ID, "with_size") => ba_with_size,
            _ => return None,
        }

        // Method not defined on type
        _ => return None,
    };

    Some(f)
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
