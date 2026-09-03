use rustc_hash::FxHashMap as HashMap;
use std::fs;
use std::fmt;
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct FileIdMap
{
    // Map of file names to unique ids
    name_to_id: HashMap<String, u32>,

    // Map of integer ids to file names
    id_to_name: Vec<String>,
}

// Define the global hash map using OnceLock with u32 keys
static FILE_ID_MAP: OnceLock<Mutex<FileIdMap>> = OnceLock::new();

/// Helper function to get or initialize the global map
fn get_file_id_map() -> &'static Mutex<FileIdMap>
{
    FILE_ID_MAP.get_or_init(|| Mutex::new(FileIdMap::default()))
}

/// Get a unique id for a given file name
fn get_file_id(name: &str) -> u32
{
    let mut map = get_file_id_map().lock().unwrap();

    if let Some(id) = map.name_to_id.get(name) {
        return *id;
    }

    let new_id = map.id_to_name.len() as u32;
    assert!(new_id < MAX_FILES, "too many source files");
    map.id_to_name.push(name.to_owned());
    map.name_to_id.insert(name.to_owned(), new_id);
    new_id
}

/// Get the file name associated with a unique id
pub fn name_from_id(id: u32) -> String
{
    let id = id as usize;
    let map = get_file_id_map().lock().unwrap();
    assert!(id < map.id_to_name.len());
    map.id_to_name[id].clone()
}

/// Source position, packed into a single 64-bit word.
///
/// One of these is stored on every AST node, so its size drives the size
/// of the whole tree: `ExprBox` is a pointer plus a position, and the
/// expression variants are made of `ExprBox`es in turn. Keeping it to one
/// word instead of three cuts the tree by about 15%.
///
/// The bits go to the line number, because that is where the length of a
/// file actually goes: 2^32 lines is over 100GB of ordinary source. A
/// column past `MAX_COL_NO` is clamped rather than rejected, so a file
/// with one very long line (a minified program, or a generated table on a
/// single line) still compiles and still reports the right line
///
/// Layout: col_no in bits 0..14, line_no in 14..46, file_id in 46..64
#[derive(Copy, Clone, Default, Eq, PartialEq, Hash)]
pub struct SrcPos(u64);

const COL_BITS: u32 = 14;
const LINE_BITS: u32 = 32;
const FILE_BITS: u32 = 18;

/// Highest column a position can represent. Positions further right on
/// the same line all report this column
pub const MAX_COL_NO: u32 = (1 << COL_BITS) - 1;

/// Highest line a position can represent
pub const MAX_LINE_NO: u32 = ((1u64 << LINE_BITS) - 1) as u32;

/// Number of distinct source files a program can be built from
pub const MAX_FILES: u32 = 1 << FILE_BITS;

const _: () = assert!(COL_BITS + LINE_BITS + FILE_BITS == 64);

impl SrcPos
{
    pub fn new(file_id: u32, line_no: u32, col_no: u32) -> Self
    {
        // Running out of file ids is a hard limit rather than something
        // to degrade: a clamped id would name the wrong file
        assert!(file_id < MAX_FILES, "too many source files");

        let col_no = std::cmp::min(col_no, MAX_COL_NO) as u64;
        let line_no = std::cmp::min(line_no, MAX_LINE_NO) as u64;

        Self(
            col_no
            | (line_no << COL_BITS)
            | ((file_id as u64) << (COL_BITS + LINE_BITS))
        )
    }

    pub fn get_src_name(&self) -> String
    {
        name_from_id(self.file_id())
    }

    pub fn file_id(&self) -> u32 { (self.0 >> (COL_BITS + LINE_BITS)) as u32 }
    pub fn line_no(&self) -> u32 { ((self.0 >> COL_BITS) as u32) & MAX_LINE_NO }
    pub fn col_no(&self) -> u32 { (self.0 as u32) & MAX_COL_NO }
}

impl fmt::Display for SrcPos
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let src_name = name_from_id(self.file_id());
        write!(f, "{}@{}:{}", src_name, self.line_no(), self.col_no())
    }
}

impl fmt::Debug for SrcPos
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file_id(), self.line_no(), self.col_no())
    }
}

#[derive(Debug, Clone)]
pub struct ParseError
{
    pub msg: String,
    pub pos: SrcPos,
}

impl ParseError
{
    pub fn new(input: &Lexer, msg: &str) -> Self
    {
        ParseError {
            msg: msg.to_string(),
            pos: input.get_pos(),
        }
    }

    /// Parse error with just an error message and position
    pub fn with_pos<T>(msg: &str, pos: &SrcPos) -> Result<T, ParseError>
    {
        Err(ParseError {
            msg: msg.to_string(),
            pos: *pos,
        })
    }

}

impl fmt::Display for ParseError
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.pos.line_no() != 0 {
            write!(f, "{}: {}",  self.pos, self.msg)
        } else
        {
            write!(f, "{}", self.msg)
        }
    }
}

/// Check if a character can be the start of an identifier
pub fn is_ident_start(ch: char) -> bool
{
    ch.is_ascii_alphabetic() || ch == '_'
}

/// Check if a character can be part of an identifier
pub fn is_ident_ch(ch: char) -> bool
{
    ch.is_ascii_alphanumeric() || ch == '_'
}

#[derive(Debug, Clone)]
pub struct Lexer
{
    // Lexer string to be parsed
    input: Vec<char>,

    // Current index in the input string
    idx: usize,

    // Source file id
    pub file_id: u32,

    // Current line number
    pub line_no: u32,

    // Current column number
    pub col_no: u32,
}

impl Lexer
{
    pub fn from_file(file_name: &str) -> Result<Self, ParseError>
    {
        let data = match fs::read_to_string(file_name) {
            Ok(data) => data,
            Err(_) => {
                return Err(ParseError {
                    msg: format!("could not read input file \"{}\"", file_name),
                    pos: SrcPos::default()
                })
            }
        };

        Ok(Self::new(&data, file_name))
    }

    pub fn new(input_str: &str, src_name: &str) -> Self
    {
        let file_id = get_file_id(src_name);

        Self {
            input: input_str.chars().collect(),
            file_id,
            idx: 0,
            line_no: 1,
            col_no: 1
        }
    }

    pub fn get_src_name(&self) -> String
    {
        name_from_id(self.file_id)
    }

    pub fn get_pos(&self) -> SrcPos
    {
        SrcPos::new(self.file_id, self.line_no, self.col_no)
    }


    /// Test if the end of the input has been reached
    pub fn eof(&self) -> bool
    {
        return self.idx >= self.input.len();
    }

    /// Peek at a character from the input
    pub fn peek_ch(&self) -> char
    {
        if self.idx >= self.input.len()
        {
            return '\0';
        }

        return self.input[self.idx];
    }

    /// Peek at a character further ahead in the input
    pub fn peek_ch_at(&self, offset: usize) -> char
    {
        if self.idx + offset >= self.input.len()
        {
            return '\0';
        }

        return self.input[self.idx + offset];
    }

    /// Consume a character from the input
    pub fn eat_ch(&mut self) -> char
    {
        let ch = self.peek_ch();

        // Move to the next char
        self.idx += 1;

        if ch == '\n'
        {
            self.line_no += 1;
            self.col_no = 1;
        }
        else
        {
            self.col_no += 1;
        }

        return ch;
    }

    /// Match a single character in the input, no preceding whitespace allowed
    pub fn match_char(&mut self, ch: char) -> bool
    {
        if self.peek_ch() == ch {
            self.eat_ch();
            return true;
        }

        return false;
    }

    /// Peek for a sequence of characters
    pub fn peek_chars(&mut self, chars: &[char]) -> bool
    {
        let end_pos = self.idx + chars.len();

        if end_pos > self.input.len() {
            return false;
        }

        // Compare the characters to match
        for i in 0..chars.len() {
            if chars[i] != self.input[self.idx + i] {
                return false;
            }
        }

        return true;
    }

    /// Match characters in the input, no preceding whitespace allowed
    pub fn match_chars(&mut self, chars: &[char]) -> bool
    {
        if !self.peek_chars(chars) {
            return false;
        }

        // Consume the matched characters
        for _ in 0..chars.len() {
            self.eat_ch();
        }

        return true;
    }

    /// Consume characters until the end of a single-line comment
    pub fn eat_comment(&mut self)
    {
        loop
        {
            // If we are at the end of the input, stop
            if self.eof() || self.eat_ch() == '\n' {
                break;
            }
        }
    }

    /// Consume characters until the end of a multi-line comment
    pub fn eat_multi_comment(&mut self) -> Result<(), ParseError>
    {
        let mut depth = 1;

        loop
        {
            if self.eof() {
                return self.parse_error(&format!("unexpected end of input inside multi-line comment"));
            }
            else if self.match_chars(&['/', '*']) {
                depth += 1;
            }
            else if self.match_chars(&['*', '/']) {
                depth -= 1;

                if depth == 0 {
                    break
                }
            }
            else
            {
                self.eat_ch();
            }
        }

        Ok(())
    }

    /// Consume whitespace
    pub fn eat_ws(&mut self) -> Result<(), ParseError>
    {
        // Until the end of the whitespace
        loop
        {
            // If we are at the end of the input, stop
            if self.eof()
            {
                break;
            }

            // Single-line comment
            if self.match_chars(&['/', '/'])
            {
                self.eat_comment();
                continue;
            }

            // Multi-line comment
            if self.match_chars(&['/', '*'])
            {
                self.eat_multi_comment()?;
                continue;
            }

            let ch = self.peek_ch();

            // Consume ASCII whitespace characters
            // Explicitly reject non-ASCII whitespace
            if ch.is_ascii_whitespace()
            {
                self.eat_ch();
                continue;
            }

            // This isn't whitespace, stop
            break;
        }

        Ok(())
    }

    /// Match a string in the input, ignoring preceding whitespace
    /// Do not use this method to match a keyword which could be
    /// an identifier.
    pub fn match_token(&mut self, token: &str) -> Result<bool, ParseError>
    {
        // Consume preceding whitespace
        self.eat_ws()?;

        let token_chars: Vec<char> = token.chars().collect();
        return Ok(self.match_chars(&token_chars));
    }

    /// Match a keyword in the input, ignoring preceding whitespace
    /// This is different from match_token because there can't be a
    /// match if the following chars are also valid identifier chars.
    pub fn match_keyword(&mut self, keyword: &str) -> Result<bool, ParseError>
    {
        // Consume preceding whitespace
        self.eat_ws()?;

        let chars: Vec<char> = keyword.chars().collect();
        let end_pos = self.idx + chars.len();

        // We can't match as a keyword if the next chars are
        // valid identifier characters
        if end_pos < self.input.len() && is_ident_ch(self.input[end_pos]) {
            return Ok(false);
        }

        return Ok(self.match_chars(&chars));
    }

    /// Shortcut for yielding a parse error wrapped in a result type
    pub fn parse_error<T>(&self, msg: &str) -> Result<T, ParseError>
    {
        Err(ParseError::new(self, msg))
    }

    /// Produce an error if the input doesn't match a given token
    pub fn expect_token(&mut self, token: &str) -> Result<(), ParseError>
    {
        if self.match_token(token)? {
            return Ok(())
        }

        self.parse_error(&format!("expected token \"{}\"", token))
    }

    /// Parse a decimal integer value
    pub fn parse_int(&mut self, radix: u32) -> Result<i128, ParseError>
    {
        let mut int_val: i128 = 0;

        if self.eof() || self.peek_ch().to_digit(radix).is_none() {
            return self.parse_error("expected digit");
        }

        loop
        {
            if self.eof() {
                break;
            }

            let ch = self.peek_ch();

            // Allow underscores as separators
            if ch == '_' {
                self.eat_ch();
                continue;
            }

            let digit = ch.to_digit(radix);

            if digit.is_none() {
                break
            }

            // Guard against overflowing the accumulator, so that an
            // absurdly long literal is a parse error and not a panic
            int_val = match int_val
                .checked_mul(radix as i128)
                .and_then(|v| v.checked_add(digit.unwrap() as i128))
            {
                Some(v) => v,
                None => return self.parse_error("integer literal is too large")
            };

            self.eat_ch();
        }

        return Ok(int_val);
    }

    /// Read the characters of a numeric value into a string
    pub fn read_numeric(&mut self) -> Result<String, ParseError>
    {
        /// Read a run of digits, returns false if there is no digit to read
        fn read_digits(input: &mut Lexer) -> bool
        {
            let ch = input.peek_ch();

            // The first char must be a digit
            if !ch.is_ascii_digit() {
                return false;
            }

            loop
            {
                let ch = input.peek_ch();
                if !ch.is_ascii_digit() && ch != '_' {
                    break;
                }
                input.eat_ch();
            }

            true
        }

        fn read_sign(input: &mut Lexer)
        {
            let _ = input.match_char('+') || input.match_char('-');
        }

        let start_idx = self.idx;

        // Read optional sign
        read_sign(self);

        // Read decimal part
        read_digits(self);

        // Fractional part. A digit has to follow the dot, otherwise the dot
        // belongs to a method call: `10.idiv(3)` is an integer with a method
        // on it, not the float `10.` followed by a stray name
        if self.peek_ch() == '.' && self.peek_ch_at(1).is_ascii_digit() {
            self.eat_ch();
            read_digits(self);
        }

        // Exponent. Unlike the fractional part above, there is no valid
        // syntax where an `e` follows a number, so rather than backtrack
        // we report a missing exponent as an error
        if self.match_char('e') || self.match_char('E') {
            read_sign(self);

            if !read_digits(self) {
                return self.parse_error("expected digits in floating-point exponent");
            }
        }

        let end_idx = self.idx;
        let num_str: String = self.input[start_idx..end_idx].iter().collect();

        // Remove any underscore separators
        let num_str = num_str.replace("_", "");

        return Ok(num_str);
    }

    /// Parse a string literal
    pub fn parse_str(&mut self, end_ch: char) -> Result<String, ParseError>
    {
        // Eat the opening character
        self.eat_ch();

        let mut out = String::new();

        loop
        {
            if self.eof() {
                return self.parse_error("unexpected end of input while parsing string literal");
            }

            let ch = self.eat_ch();

            if ch == end_ch {
                break;
            }

            if ch == '\\' {
                match self.eat_ch() {
                    '\\' => out.push('\\'),
                    '\'' => out.push('\''),
                    '\"' => out.push('\"'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'n' => out.push('\n'),
                    '0' => out.push('\0'),

                    // Hexadecimal escape sequence
                    'x' => {
                        let digit0 = self.eat_ch().to_digit(16);
                        let digit1 = self.eat_ch().to_digit(16);

                        match (digit0, digit1) {
                            (Some(d0), Some(d1)) => {
                                let byte_val = ((d0 << 4) + d1) as u8;
                                out.push(byte_val as char);
                            }
                            _ => return self.parse_error("invalid hexadecimal escape sequence")
                        }
                    }

                    _ => return self.parse_error("unknown escape sequence")
                }

                continue;
            }

            out.push(ch);
        }

        return Ok(out);
    }

    /// Parse a C-style alphanumeric identifier
    pub fn parse_ident(&mut self) -> Result<String, ParseError>
    {
        let mut ident = String::new();

        if self.eof() || !is_ident_start(self.peek_ch()) {
            return self.parse_error("expected identifier");
        }

        loop
        {
            if self.eof() {
                break;
            }

            let ch = self.peek_ch();

            if !is_ident_ch(ch) {
                break;
            }

            ident.push(ch);
            self.eat_ch();
        }

        return Ok(ident);
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn src_pos_size()
    {
        assert_eq!(std::mem::size_of::<SrcPos>(), 8);
    }

    #[test]
    fn src_pos_roundtrip()
    {
        for (file_id, line_no, col_no) in [
            (0, 0, 0),
            (0, 1, 1),
            (7, 1234, 56),
            (MAX_FILES - 1, MAX_LINE_NO, MAX_COL_NO),
            (MAX_FILES - 1, 1, 1),
            (0, MAX_LINE_NO, 0),
            (0, 0, MAX_COL_NO),
        ] {
            let pos = SrcPos::new(file_id, line_no, col_no);
            assert_eq!(pos.file_id(), file_id);
            assert_eq!(pos.line_no(), line_no);
            assert_eq!(pos.col_no(), col_no);
        }
    }

    // A column past the limit is clamped, so that a file with one very
    // long line still compiles and still reports the right line
    #[test]
    fn src_pos_clamps_col()
    {
        let pos = SrcPos::new(3, 42, MAX_COL_NO + 1);
        assert_eq!(pos.file_id(), 3);
        assert_eq!(pos.line_no(), 42);
        assert_eq!(pos.col_no(), MAX_COL_NO);

        let pos = SrcPos::new(3, 42, 1_000_000);
        assert_eq!(pos.line_no(), 42);
        assert_eq!(pos.col_no(), MAX_COL_NO);
    }

    // A default position reads as line zero, which is what tells the
    // parser that an error has no position to report
    #[test]
    fn src_pos_default_has_no_line()
    {
        assert_eq!(SrcPos::default().line_no(), 0);
    }

    #[test]
    #[should_panic(expected = "too many source files")]
    fn src_pos_rejects_bad_file_id()
    {
        SrcPos::new(MAX_FILES, 1, 1);
    }
}
