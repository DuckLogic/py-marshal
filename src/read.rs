#![allow(clippy::wildcard_imports)] // read::errors
pub mod errors {


    #[derive(thiserror::Error, Debug)]
    pub enum Error<'a> {
        #[error("Invalid type: {spec:X}")]
        InvalidType {
            spec: u8
        },
        #[error("Recursion limit exceeded")]
        RecursionLimitExceeded,
        #[error("Digit out of range: {digit}")]
        DigitOutOfRange {
            digit: u16
        },
        #[error("Unnormalized long")]
        UnnormalizedLong,
        #[error("Unexpected null")]
        UnexpectedNull,
        #[error("Unexpected use of unhashable type: {0:?}")]
        Unhashable(crate::Obj<'a>),
        #[error("Internal type error for {0:?}")]
        TypeError(crate::Obj<'a>),
        #[error("Invalid reference")]
        InvalidRef,
        #[error(transparent)]
        Io(#[from] std::io::Error),
        #[error(transparent)]
        Utf8(#[from] std::str::Utf8Error),
        #[error(transparent)] // TODO: Is this redundant?
        StringUtf8(#[from] ::std::string::FromUtf8Error),
        #[error("Unable to parse float: {0}")]
        ParseFloat(#[from] std::num::ParseFloatError)

    }

    pub type Result<'a, T> = std::result::Result<T, Error<'a>>;
}

use self::errors::*;
use crate::{utils, Code, CodeFlags, Depth, Obj, ObjHashable, Type};
use num_bigint::BigInt;
use num_complex::Complex;
use num_traits::{FromPrimitive, Zero};
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    io::Read,
    str::FromStr,
};

struct RFile<'a, R: Read> {
    arena: &'a super::ObjArena<'a>,
    depth: Depth<'a>,
    readable: R,
    refs: Vec<Obj<'a>>,
    has_posonlyargcount: bool,
}

macro_rules! define_r {
    ($ident:ident -> $ty:ty; $n:literal) => {
        fn $ident<'a>(p: &mut RFile<'a, impl Read>) -> Result<'a, $ty> {
            let mut buf: [u8; $n] = [0; $n];
            p.readable.read_exact(&mut buf)?;
            Ok(<$ty>::from_le_bytes(buf))
        }
    };
}

define_r! { r_byte      -> u8 ; 1 }
define_r! { r_short     -> u16; 2 }
define_r! { r_long      -> u32; 4 }
define_r! { r_long64    -> u64; 8 }
define_r! { r_float_bin -> f64; 8 }

fn r_bytes<'a>(n: usize, p: &mut RFile<'a, impl Read>) -> Result<'a, &'a [u8]> {
    let buf = p.arena.as_bumpalo().alloc_slice_fill_copy(n, 0);
    p.readable.read_exact(&mut buf)?;
    Ok(&*buf)
}

fn r_string<'a>(n: usize, p: &mut RFile<'a, impl Read>) -> Result<'a, &'a str> {
    let buf = r_bytes(n, p)?;
    Ok(std::str::from_utf8(buf)?)
}

fn r_float_str<'a>(p: &mut RFile<impl Read>) -> Result<'a, f64> {
    let n = r_byte(p)?;
    let s = r_string(n as usize, p)?;
    Ok(f64::from_str(&s)?)
}

// TODO: test
/// May misbehave on 16-bit platforms.
fn r_pylong<'a>(p: &mut RFile<'a, impl Read>) -> Result<'a, &'a BigInt> {
    #[allow(clippy::cast_possible_wrap)]
    let n = r_long(p)? as i32;
    if n == 0 {
        return Ok(p.arena.alloc(BigInt::zero()));
    };
    #[allow(clippy::cast_sign_loss)]
    let size = n.wrapping_abs() as u32;
    let mut digits = Vec::<u16>::with_capacity(size as usize);
    for _ in 0..size {
        let d = r_short(p)?;
        if d > (1 << 15) {
            return Err(Error::DigitOutOfRange { digit: d });
        }
        digits.push(d);
    }
    if digits[(size - 1) as usize] == 0 {
        return Err(Error::UnnormalizedLong.into());
    }
    Ok(p.arena.alloc(BigInt::from_biguint(
        utils::sign_of(&n),
        utils::biguint_from_pylong_digits(&digits),
    )))
}

fn r_vec<'a>(n: usize, p: &mut RFile<'a, impl Read>) -> Result<'a, &'a [Obj<'a>]> {
    let mut vec = Vec::with_capacity(n);
    for _ in 0..n {
        vec.push(r_object_not_null(p)?);
    }
    Ok(p.arena.alloc(vec))
}

fn r_hashmap<'a>(p: &mut RFile<'a, impl Read>) -> Result<'a, &'a [(Obj<'a>, Obj<'a>)]> {
    let mut map = Vec::new();
    loop {
        match r_object(p)? {
            None => break,
            Some(key) => match r_object(p)? {
                None => break, // TODO: Can we have key with no value??
                Some(value) => {
                    map.push((key, value))
                }
            },
        }
    }
    Ok(map)
}

fn r_hashset(n: usize, p: &mut RFile<impl Read>) -> Result<[ObjHashable<'a>]> {
    let mut set = HashSet::new();
    r_hashset_into(&mut set, n, p)?;
    Ok(set)
}
fn r_hashset_into(
    set: &mut HashSet<ObjHashable>,
    n: usize,
    p: &mut RFile<impl Read>,
) -> Result<()> {
    for _ in 0..n {
        set.insert(
            ObjHashable::try_from(&r_object_not_null(p)?)
                .map_err(Error::Unhashable)?,
        );
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn r_object(p: &mut RFile<impl Read>) -> Result<Option<Obj>> {
    let code: u8 = r_byte(p)?;
    let _depth_handle = p
        .depth
        .try_clone()
        .map_or(Err(Error::RecursionLimitExceeded), Ok)?;
    let (flag, type_) = {
        let flag: bool = (code & Type::FLAG_REF) != 0;
        let type_u8: u8 = code & !Type::FLAG_REF;
        let type_: Type =
            Type::from_u8(type_u8).map_or(Err(Error::InvalidType { spec: type_u8 }), Ok)?;
        (flag, type_)
    };
    let mut idx: Option<usize> = match type_ {
        // R_REF/r_ref_reserve before reading contents
        // See https://github.com/sollyucko/py-marshal/issues/2
        Type::SmallTuple | Type::Tuple | Type::List | Type::Dict | Type::Set | Type::FrozenSet | Type::Code if flag => {
            let i = p.refs.len();
            p.refs.push(Obj::None);
            Some(i)
        }
        _ => None,
    };
    #[allow(clippy::cast_possible_wrap)]
    let retval = match type_ {
        Type::Null => None,
        Type::None => Some(Obj::None),
        Type::StopIter => Some(Obj::StopIteration),
        Type::Ellipsis => Some(Obj::Ellipsis),
        Type::False => Some(Obj::Bool(false)),
        Type::True => Some(Obj::Bool(true)),
        Type::Int => Some(Obj::Long(Arc::new(BigInt::from(r_long(p)? as i32)))),
        Type::Int64 => Some(Obj::Long(Arc::new(BigInt::from(r_long64(p)? as i64)))),
        Type::Long => Some(Obj::Long(Arc::new(r_pylong(p)?))),
        Type::Float => Some(Obj::Float(r_float_str(p)?)),
        Type::BinaryFloat => Some(Obj::Float(r_float_bin(p)?)),
        Type::Complex => Some(Obj::Complex(Complex {
            re: r_float_str(p)?,
            im: r_float_str(p)?,
        })),
        Type::BinaryComplex => Some(Obj::Complex(Complex {
            re: r_float_bin(p)?,
            im: r_float_bin(p)?,
        })),
        Type::String => Some(Obj::Bytes(Arc::new(r_bytes(r_long(p)? as usize, p)?))),
        Type::AsciiInterned | Type::Ascii | Type::Interned | Type::Unicode => {
            Some(Obj::String(Arc::new(r_string(r_long(p)? as usize, p)?)))
        }
        Type::ShortAsciiInterned | Type::ShortAscii => {
            Some(Obj::String(Arc::new(r_string(r_byte(p)? as usize, p)?)))
        }
        Type::SmallTuple => Some(Obj::Tuple(Arc::new(r_vec(r_byte(p)? as usize, p)?))),
        Type::Tuple => Some(Obj::Tuple(Arc::new(r_vec(r_long(p)? as usize, p)?))),
        Type::List => Some(Obj::List(Arc::new(RwLock::new(r_vec(
            r_long(p)? as usize,
            p,
        )?)))),
        Type::Set => {
            let set = Arc::new(RwLock::new(HashSet::new()));

            if flag {
                idx = Some(p.refs.len());
                p.refs.push(Obj::Set(Arc::clone(&set)));
            }

            r_hashset_into(&mut *set.write().unwrap(), r_long(p)? as usize, p)?;
            Some(Obj::Set(set))
        }
        Type::FrozenSet => Some(Obj::FrozenSet(Arc::new(r_hashset(r_long(p)? as usize, p)?))),
        Type::Dict => Some(Obj::Dict(Arc::new(RwLock::new(r_hashmap(p)?)))),
        Type::Code => Some(Obj::Code(Arc::new(Code {
            argcount: r_long(p)?,
            posonlyargcount: if p.has_posonlyargcount { r_long(p)? } else { 0 },
            kwonlyargcount: r_long(p)?,
            nlocals: r_long(p)?,
            stacksize: r_long(p)?,
            flags: CodeFlags::from_bits_truncate(r_long(p)?),
            code: r_object_extract_bytes(p)?,
            consts: r_object_extract_tuple(p)?,
            names: r_object_extract_tuple_string(p)?,
            varnames: r_object_extract_tuple_string(p)?,
            freevars: r_object_extract_tuple_string(p)?,
            cellvars: r_object_extract_tuple_string(p)?,
            filename: r_object_extract_string(p)?,
            name: r_object_extract_string(p)?,
            firstlineno: r_long(p)?,
            lnotab: r_object_extract_bytes(p)?,
        }))),

        Type::Ref => {
            let n = r_long(p)? as usize;
            let result = p.refs.get(n).ok_or(Error::InvalidRef)?.clone();
            if result.is_none() {
                return Err(Error::InvalidRef.into());
            } else {
                Some(result)
            }
        }
        Type::Unknown => return Err(Error::InvalidType { spec: Type::Unknown as u8 }.into()),
    };
    match (&retval, idx) {
        (None, _)
        | (Some(Obj::None), _)
        | (Some(Obj::StopIteration), _)
        | (Some(Obj::Ellipsis), _)
        | (Some(Obj::Bool(_)), _) => {}
        (Some(x), Some(i)) if flag => {
            p.refs[i] = x.clone();
        }
        (Some(x), None) if flag => {
            p.refs.push(x.clone());
        }
        (Some(_), _) => {}
    };
    Ok(retval)
}

fn r_object_not_null(p: &mut RFile<impl Read>) -> Result<Obj> {
    Ok(r_object(p)?.ok_or(Error::UnexpectedNull)?)
}
fn r_object_extract_string(p: &mut RFile<impl Read>) -> Result<Arc<String>> {
    r_object_not_null(p)?
        .extract_string()
        .map_err(Error::TypeError)
}
fn r_object_extract_bytes(p: &mut RFile<impl Read>) -> Result<Arc<Vec<u8>>> {
    Ok(r_object_not_null(p)?
        .extract_bytes()
        .map_err(Error::TypeError)?)
}
fn r_object_extract_tuple(p: &mut RFile<impl Read>) -> Result<Arc<Vec<Obj>>> {
    Ok(r_object_not_null(p)?
        .extract_tuple()
        .map_err(Error::TypeError)?)
}
fn r_object_extract_tuple_string(p: &mut RFile<impl Read>) -> Result<Vec<Arc<String>>> {
    Ok(r_object_extract_tuple(p)?
        .iter()
        .map(|x| {
            x.clone()
                .extract_string()
                .map_err(Error::TypeError)
        })
        .collect::<Result<Vec<Arc<String>>>>()?)
}

fn read_object(p: &mut RFile<impl Read>) -> Result<Obj> {
    r_object_not_null(p)
}

#[derive(Copy, Clone, Debug)]
pub struct MarshalLoadExOptions {
    pub has_posonlyargcount: bool,
}
/// Assume latest version
impl Default for MarshalLoadExOptions {
    fn default() -> Self {
        Self {
            has_posonlyargcount: true,
        }
    }
}

/// # Errors
/// See [`ErrorKind`].
pub fn marshal_load_ex(readable: impl Read, opts: MarshalLoadExOptions) -> Result<Obj> {
    let mut rf = RFile {
        depth: Depth::new(),
        readable,
        refs: Vec::<Obj>::new(),
        has_posonlyargcount: opts.has_posonlyargcount,
    };
    read_object(&mut rf)
}

/// # Errors
/// See [`ErrorKind`].
pub fn marshal_load(readable: impl Read) -> Result<Obj> {
    marshal_load_ex(readable, MarshalLoadExOptions::default())
}

/// Allows coercion from array reference to slice.
/// # Errors
/// See [`ErrorKind`].
pub fn marshal_loads(bytes: &[u8]) -> Result<Obj> {
    marshal_load(bytes)
}

// Ported from <https://github.com/python/cpython/blob/master/Lib/test/test_marshal.py>
#[cfg(test)]
mod test {
    use super::{
        errors, marshal_load, marshal_load_ex, marshal_loads, Code, CodeFlags,
        MarshalLoadExOptions, Obj, ObjHashable,
    };
    use num_bigint::BigInt;
    use num_traits::Pow;
    use std::{
        io::{self, Read},
        ops::Deref,
        sync::Arc,
    };

    macro_rules! assert_match {
        ($expr:expr, $pat:pat) => {
            match $expr {
                $pat => {}
                _ => panic!(),
            }
        };
    }

    fn load_unwrap(r: impl Read) -> Obj {
        marshal_load(r).unwrap()
    }

    fn loads_unwrap(s: &[u8]) -> Obj {
        load_unwrap(s)
    }

    #[test]
    fn test_ints() {
        assert_eq!(BigInt::parse_bytes(b"85070591730234615847396907784232501249", 10).unwrap(), *loads_unwrap(b"l\t\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\xf0\x7f\xff\x7f\xff\x7f\xff\x7f?\x00").extract_long().unwrap());
    }

    #[allow(clippy::unreadable_literal)]
    #[test]
    fn test_int64() {
        for mut base in [i64::MAX, i64::MIN, -i64::MAX, -(i64::MIN >> 1)]
            .iter()
            .copied()
        {
            while base != 0 {
                let mut s = Vec::<u8>::new();
                s.push(b'I');
                s.extend_from_slice(&base.to_le_bytes());
                assert_eq!(
                    BigInt::from(base),
                    *loads_unwrap(&s).extract_long().unwrap()
                );

                if base == -1 {
                    base = 0
                } else {
                    base >>= 1
                }
            }
        }

        assert_eq!(
            BigInt::from(0x1032547698badcfe_i64),
            *loads_unwrap(b"I\xfe\xdc\xba\x98\x76\x54\x32\x10")
                .extract_long()
                .unwrap()
        );
        assert_eq!(
            BigInt::from(-0x1032547698badcff_i64),
            *loads_unwrap(b"I\x01\x23\x45\x67\x89\xab\xcd\xef")
                .extract_long()
                .unwrap()
        );
        assert_eq!(
            BigInt::from(0x7f6e5d4c3b2a1908_i64),
            *loads_unwrap(b"I\x08\x19\x2a\x3b\x4c\x5d\x6e\x7f")
                .extract_long()
                .unwrap()
        );
        assert_eq!(
            BigInt::from(-0x7f6e5d4c3b2a1909_i64),
            *loads_unwrap(b"I\xf7\xe6\xd5\xc4\xb3\xa2\x91\x80")
                .extract_long()
                .unwrap()
        );
    }

    #[test]
    fn test_bool() {
        assert_eq!(true, loads_unwrap(b"T").extract_bool().unwrap());
        assert_eq!(false, loads_unwrap(b"F").extract_bool().unwrap());
    }

    #[allow(clippy::float_cmp, clippy::cast_precision_loss)]
    #[test]
    fn test_floats() {
        assert_eq!(
            (i64::MAX as f64) * 3.7e250,
            loads_unwrap(b"g\x11\x9f6\x98\xd2\xab\xe4w")
                .extract_float()
                .unwrap()
        );
    }

    #[test]
    fn test_unicode() {
        assert_eq!("", *loads_unwrap(b"\xda\x00").extract_string().unwrap());
        assert_eq!(
            "Andr\u{e8} Previn",
            *loads_unwrap(b"u\r\x00\x00\x00Andr\xc3\xa8 Previn")
                .extract_string()
                .unwrap()
        );
        assert_eq!(
            "abc",
            *loads_unwrap(b"\xda\x03abc").extract_string().unwrap()
        );
        assert_eq!(
            " ".repeat(10_000),
            *loads_unwrap(&[b"a\x10'\x00\x00" as &[u8], &[b' '; 10_000]].concat())
                .extract_string()
                .unwrap()
        );
    }

    #[test]
    fn test_string() {
        assert_eq!("", *loads_unwrap(b"\xda\x00").extract_string().unwrap());
        assert_eq!(
            "Andr\u{e8} Previn",
            *loads_unwrap(b"\xf5\r\x00\x00\x00Andr\xc3\xa8 Previn")
                .extract_string()
                .unwrap()
        );
        assert_eq!(
            "abc",
            *loads_unwrap(b"\xda\x03abc").extract_string().unwrap()
        );
        assert_eq!(
            " ".repeat(10_000),
            *loads_unwrap(&[b"\xe1\x10'\x00\x00" as &[u8], &[b' '; 10_000]].concat())
                .extract_string()
                .unwrap()
        );
    }

    #[test]
    fn test_bytes() {
        assert_eq!(
            b"",
            &loads_unwrap(b"\xf3\x00\x00\x00\x00")
                .extract_bytes()
                .unwrap()[..]
        );
        assert_eq!(
            b"Andr\xe8 Previn",
            &loads_unwrap(b"\xf3\x0c\x00\x00\x00Andr\xe8 Previn")
                .extract_bytes()
                .unwrap()[..]
        );
        assert_eq!(
            b"abc",
            &loads_unwrap(b"\xf3\x03\x00\x00\x00abc")
                .extract_bytes()
                .unwrap()[..]
        );
        assert_eq!(
            b" ".repeat(10_000),
            &loads_unwrap(&[b"\xf3\x10'\x00\x00" as &[u8], &[b' '; 10_000]].concat())
                .extract_bytes()
                .unwrap()[..]
        );
    }

    #[test]
    fn test_exceptions() {
        loads_unwrap(b"S").extract_stop_iteration().unwrap();
    }

    fn assert_test_exceptions_code_valid(code: &Code) {
        assert_eq!(code.argcount, 1);
        assert!(code.cellvars.is_empty());
        assert_eq!(*code.code, b"t\x00\xa0\x01t\x00\xa0\x02t\x03\xa1\x01\xa1\x01}\x01|\x00\xa0\x04t\x03|\x01\xa1\x02\x01\x00d\x00S\x00");
        assert_eq!(code.consts.len(), 1);
        assert!(code.consts[0].is_none());
        assert_eq!(*code.filename, "<string>");
        assert_eq!(code.firstlineno, 3);
        assert_eq!(
            code.flags,
            CodeFlags::NOFREE | CodeFlags::NEWLOCALS | CodeFlags::OPTIMIZED
        );
        assert!(code.freevars.is_empty());
        assert_eq!(code.kwonlyargcount, 0);
        assert_eq!(*code.lnotab, b"\x00\x01\x10\x01");
        assert_eq!(*code.name, "test_exceptions");
        assert!(code.names.iter().map(Deref::deref).eq(vec![
            "marshal",
            "loads",
            "dumps",
            "StopIteration",
            "assertEqual"
        ]
        .iter()));
        assert_eq!(code.nlocals, 2);
        assert_eq!(code.stacksize, 5);
        assert!(code
            .varnames
            .iter()
            .map(Deref::deref)
            .eq(vec!["self", "new"].iter()));
    }

    #[test]
    fn test_code() {
        // ExceptionTestCase.test_exceptions
        // { 'co_argcount': 1, 'co_cellvars': (), 'co_code': b't\x00\xa0\x01t\x00\xa0\x02t\x03\xa1\x01\xa1\x01}\x01|\x00\xa0\x04t\x03|\x01\xa1\x02\x01\x00d\x00S\x00', 'co_consts': (None,), 'co_filename': '<string>', 'co_firstlineno': 3, 'co_flags': 67, 'co_freevars': (), 'co_kwonlyargcount': 0, 'co_lnotab': b'\x00\x01\x10\x01', 'co_name': 'test_exceptions', 'co_names': ('marshal', 'loads', 'dumps', 'StopIteration', 'assertEqual'), 'co_nlocals': 2, 'co_stacksize': 5, 'co_varnames': ('self', 'new') }
        let mut input: &[u8] = b"\xe3\x01\x00\x00\x00\x00\x00\x00\x00\x02\x00\x00\x00\x05\x00\x00\x00C\x00\x00\x00s \x00\x00\x00t\x00\xa0\x01t\x00\xa0\x02t\x03\xa1\x01\xa1\x01}\x01|\x00\xa0\x04t\x03|\x01\xa1\x02\x01\x00d\x00S\x00)\x01N)\x05\xda\x07marshal\xda\x05loads\xda\x05dumps\xda\rStopIteration\xda\x0bassertEqual)\x02\xda\x04self\xda\x03new\xa9\x00r\x08\x00\x00\x00\xda\x08<string>\xda\x0ftest_exceptions\x03\x00\x00\x00s\x04\x00\x00\x00\x00\x01\x10\x01";
        println!("{}", input.len());
        let code_result = marshal_load_ex(
            &mut input,
            MarshalLoadExOptions {
                has_posonlyargcount: false,
            },
        );
        println!("{}", input.len());
        let code = code_result.unwrap().extract_code().unwrap();
        assert_test_exceptions_code_valid(&code);
    }

    #[test]
    fn test_many_codeobjects() {
        let mut input: &[u8] = &[b"(\x88\x13\x00\x00\xe3\x01\x00\x00\x00\x00\x00\x00\x00\x02\x00\x00\x00\x05\x00\x00\x00C\x00\x00\x00s \x00\x00\x00t\x00\xa0\x01t\x00\xa0\x02t\x03\xa1\x01\xa1\x01}\x01|\x00\xa0\x04t\x03|\x01\xa1\x02\x01\x00d\x00S\x00)\x01N)\x05\xda\x07marshal\xda\x05loads\xda\x05dumps\xda\rStopIteration\xda\x0bassertEqual)\x02\xda\x04self\xda\x03new\xa9\x00r\x08\x00\x00\x00\xda\x08<string>\xda\x0ftest_exceptions\x03\x00\x00\x00s\x04\x00\x00\x00\x00\x01\x10\x01" as &[u8], &b"r\x00\x00\x00\x00".repeat(4999)].concat();
        let result = marshal_load_ex(
            &mut input,
            MarshalLoadExOptions {
                has_posonlyargcount: false,
            },
        );
        let tuple = result.unwrap().extract_tuple().unwrap();
        for o in &*tuple {
            assert_test_exceptions_code_valid(&o.clone().extract_code().unwrap());
        }
    }

    #[test]
    fn test_different_filenames() {
        let mut input: &[u8] = b")\x02c\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00@\x00\x00\x00s\x08\x00\x00\x00e\x00\x01\x00d\x00S\x00)\x01N)\x01\xda\x01x\xa9\x00r\x01\x00\x00\x00r\x01\x00\x00\x00\xda\x02f1\xda\x08<module>\x01\x00\x00\x00\xf3\x00\x00\x00\x00c\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00@\x00\x00\x00s\x08\x00\x00\x00e\x00\x01\x00d\x00S\x00)\x01N)\x01\xda\x01yr\x01\x00\x00\x00r\x01\x00\x00\x00r\x01\x00\x00\x00\xda\x02f2r\x03\x00\x00\x00\x01\x00\x00\x00r\x04\x00\x00\x00";
        println!("{}", input.len());
        let result = marshal_load_ex(
            &mut input,
            MarshalLoadExOptions {
                has_posonlyargcount: false,
            },
        );
        println!("{}", input.len());
        let tuple = result.unwrap().extract_tuple().unwrap();
        assert_eq!(tuple.len(), 2);
        assert_eq!(*tuple[0].clone().extract_code().unwrap().filename, "f1");
        assert_eq!(*tuple[1].clone().extract_code().unwrap().filename, "f2");
    }

    #[allow(clippy::float_cmp)]
    #[test]
    fn test_dict() {
        let mut input: &[u8] = b"{\xda\x07astring\xfa\x10foo@bar.baz.spam\xda\x06afloat\xe7H\xe1z\x14ns\xbc@\xda\x05anint\xe9\x00\x00\x10\x00\xda\nashortlong\xe9\x02\x00\x00\x00\xda\x05alist[\x01\x00\x00\x00\xfa\x07.zyx.41\xda\x06atuple\xa9\n\xfa\x07.zyx.41r\x0c\x00\x00\x00r\x0c\x00\x00\x00r\x0c\x00\x00\x00r\x0c\x00\x00\x00r\x0c\x00\x00\x00r\x0c\x00\x00\x00r\x0c\x00\x00\x00r\x0c\x00\x00\x00r\x0c\x00\x00\x00\xda\x08abooleanF\xda\x08aunicode\xf5\r\x00\x00\x00Andr\xc3\xa8 Previn0";
        println!("{}", input.len());
        let result = marshal_load(&mut input);
        println!("{}", input.len());
        let dict_ref = result.unwrap().extract_dict().unwrap();
        let dict = dict_ref.try_read().unwrap();
        assert_eq!(dict.len(), 8);
        assert_eq!(
            *dict[&ObjHashable::String(Arc::new("astring".to_owned()))]
                .clone()
                .extract_string()
                .unwrap(),
            "foo@bar.baz.spam"
        );
        assert_eq!(
            dict[&ObjHashable::String(Arc::new("afloat".to_owned()))]
                .clone()
                .extract_float()
                .unwrap(),
            7283.43_f64
        );
        assert_eq!(
            *dict[&ObjHashable::String(Arc::new("anint".to_owned()))]
                .clone()
                .extract_long()
                .unwrap(),
            BigInt::from(2).pow(20_u8)
        );
        assert_eq!(
            *dict[&ObjHashable::String(Arc::new("ashortlong".to_owned()))]
                .clone()
                .extract_long()
                .unwrap(),
            BigInt::from(2)
        );

        let list_ref = dict[&ObjHashable::String(Arc::new("alist".to_owned()))]
            .clone()
            .extract_list()
            .unwrap();
        let list = list_ref.try_read().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(*list[0].clone().extract_string().unwrap(), ".zyx.41");

        let tuple = dict[&ObjHashable::String(Arc::new("atuple".to_owned()))]
            .clone()
            .extract_tuple()
            .unwrap();
        assert_eq!(tuple.len(), 10);
        for o in &*tuple {
            assert_eq!(*o.clone().extract_string().unwrap(), ".zyx.41");
        }
        assert_eq!(
            dict[&ObjHashable::String(Arc::new("aboolean".to_owned()))]
                .clone()
                .extract_bool()
                .unwrap(),
            false
        );
        assert_eq!(
            *dict[&ObjHashable::String(Arc::new("aunicode".to_owned()))]
                .clone()
                .extract_string()
                .unwrap(),
            "Andr\u{e8} Previn"
        );
    }

    /// Tests hash implementation
    #[test]
    fn test_dict_tuple_key() {
        let dict = loads_unwrap(b"{\xa9\x02\xda\x01a\xda\x01b\xda\x01c0")
            .extract_dict()
            .unwrap();
        assert_eq!(dict.read().unwrap().len(), 1);
        assert_eq!(
            *dict.read().unwrap()[&ObjHashable::Tuple(Arc::new(vec![
                ObjHashable::String(Arc::new("a".to_owned())),
                ObjHashable::String(Arc::new("b".to_owned()))
            ]))]
                .clone()
                .extract_string()
                .unwrap(),
            "c"
        );
    }

    // TODO: test_list and test_tuple

    #[test]
    fn test_sets() {
        let set = loads_unwrap(b"<\x08\x00\x00\x00\xda\x05alist\xda\x08aboolean\xda\x07astring\xda\x08aunicode\xda\x06afloat\xda\x05anint\xda\x06atuple\xda\nashortlong").extract_set().unwrap();
        assert_eq!(set.read().unwrap().len(), 8);
        let frozenset = loads_unwrap(b">\x08\x00\x00\x00\xda\x06atuple\xda\x08aunicode\xda\x05anint\xda\x08aboolean\xda\x06afloat\xda\x05alist\xda\nashortlong\xda\x07astring").extract_frozenset().unwrap();
        assert_eq!(frozenset.len(), 8);
        // TODO: check values
    }

    // TODO: test_bytearray, test_memoryview, test_array

    #[test]
    fn test_patch_873224() {
        assert_match!(
            marshal_loads(b"0").unwrap_err(),
            errors::Error::UnexpectedNull
        );
        let f_err = marshal_loads(b"f").unwrap_err();
        match f_err {
            errors::Error::Io(io_err) => {
                assert_eq!(io_err.kind(), io::ErrorKind::UnexpectedEof);
            }
            _ => panic!(),
        }
        let int_err =
            marshal_loads(b"l\x05\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00 ").unwrap_err();
        match int_err {
            errors::Error::Io(io_err) => {
                assert_eq!(io_err.kind(), io::ErrorKind::UnexpectedEof);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_fuzz() {
        for i in 0..=u8::MAX {
            println!("{:?}", marshal_loads(&[i]));
        }
    }

    /// Warning: this has to be run on a release build to avoid a stack overflow.
    #[cfg(not(debug_assertions))]
    #[test]
    fn test_loads_recursion() {
        loads_unwrap(&[&b")\x01".repeat(100)[..], b"N"].concat());
        loads_unwrap(&[&b"(\x01\x00\x00\x00".repeat(100)[..], b"N"].concat());
        loads_unwrap(&[&b"[\x01\x00\x00\x00".repeat(100)[..], b"N"].concat());
        loads_unwrap(&[&b"{N".repeat(100)[..], b"N", &b"0".repeat(100)[..]].concat());
        loads_unwrap(&[&b">\x01\x00\x00\x00".repeat(100)[..], b"N"].concat());

        assert_match!(
            marshal_loads(&[&b")\x01".repeat(1048576)[..], b"N"].concat())
                .unwrap_err(),
            errors::Error::RecursionLimitExceeded
        );
        assert_match!(
            marshal_loads(&[&b"(\x01\x00\x00\x00".repeat(1048576)[..], b"N"].concat())
                .unwrap_err(),
            errors::Error::RecursionLimitExceeded
        );
        assert_match!(
            marshal_loads(&[&b"[\x01\x00\x00\x00".repeat(1048576)[..], b"N"].concat())
                .unwrap_err(),
            errors::Error::RecursionLimitExceeded
        );
        assert_match!(
            marshal_loads(
                &[&b"{N".repeat(1048576)[..], b"N", &b"0".repeat(1048576)[..]].concat()
            )
            .unwrap_err(),
            errors::Error::RecursionLimitExceeded
        );
        assert_match!(
            marshal_loads(&[&b">\x01\x00\x00\x00".repeat(1048576)[..], b"N"].concat())
                .unwrap_err(),
            errors::Error::RecursionLimitExceeded
        );
    }

    #[test]
    fn test_invalid_longs() {
        assert_match!(
            marshal_loads(b"l\x02\x00\x00\x00\x00\x00\x00\x00")
                .unwrap_err(),
            errors::Error::UnnormalizedLong
        );
    }
    
    // See https://github.com/sollyucko/py-marshal/issues/2
    #[test]
    fn test_issue_2_ref_demarshalling_ordering_previously_broken() {
        let list_ref = marshal_loads(b"\xdb\x02\x00\x00\x00\xda\x01ar\x01\x00\x00\x00").unwrap().extract_list().unwrap();
        let list = list_ref.try_read().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(*list[0].clone().extract_string().unwrap(), "a");
        assert_eq!(*list[1].clone().extract_string().unwrap(), "a");
    }
    #[test]
    fn test_issue_2_ref_demarshalling_ordering_previously_working() {
        let list_ref = marshal_loads(b"[\x02\x00\x00\x00\xda\x01ar\x00\x00\x00\x00").unwrap().extract_list().unwrap();
        let list = list_ref.try_read().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(*list[0].clone().extract_string().unwrap(), "a");
        assert_eq!(*list[1].clone().extract_string().unwrap(), "a");
    }
}
