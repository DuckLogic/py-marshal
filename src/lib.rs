// Ported from <https://github.com/python/cpython/blob/master/Python/marshal.c>
use bitflags::bitflags;
use num_bigint::BigInt;
use num_complex::Complex;
use num_derive::{FromPrimitive, ToPrimitive};
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    fmt,
    hash::{Hash, Hasher},
    iter::FromIterator,
    sync::{Arc, RwLock},
};

/// `Arc` = immutable
/// `ArcRwLock` = mutable
pub type ArcRwLock<T> = Arc<RwLock<T>>;

#[derive(FromPrimitive, ToPrimitive, Debug, Copy, Clone)]
#[repr(u8)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
enum Type {
    Null               = b'0',
    None               = b'N',
    False              = b'F',
    True               = b'T',
    StopIter           = b'S',
    Ellipsis           = b'.',
    Int                = b'i',
    Int64              = b'I',
    Float              = b'f',
    BinaryFloat        = b'g',
    Complex            = b'x',
    BinaryComplex      = b'y',
    Long               = b'l',
    String             = b's',
    Interned           = b't',
    Ref                = b'r',
    Tuple              = b'(',
    List               = b'[',
    Dict               = b'{',
    Code               = b'c',
    Unicode            = b'u',
    Unknown            = b'?',
    Set                = b'<',
    FrozenSet          = b'>',
    Ascii              = b'a',
    AsciiInterned      = b'A',
    SmallTuple         = b')',
    ShortAscii         = b'z',
    ShortAsciiInterned = b'Z',
}
impl Type {
    const FLAG_REF: u8 = b'\x80';
}

struct Depth(Arc<()>);
impl Depth {
    const MAX: usize = 900;

    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(()))
    }

    pub fn try_clone(&self) -> Option<Self> {
        if Arc::strong_count(&self.0) > Self::MAX {
            None
        } else {
            Some(Self(self.0.clone()))
        }
    }
}
impl fmt::Debug for Depth {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_tuple("Depth")
            .field(&Arc::strong_count(&self.0))
            .finish()
    }
}

bitflags! {
    #[cfg_attr(feature = "serialize", derive(serde::Serialize))]
    pub struct CodeFlags: u32 {
        const OPTIMIZED                   = 0x1;
        const NEWLOCALS                   = 0x2;
        const VARARGS                     = 0x4;
        const VARKEYWORDS                 = 0x8;
        const NESTED                     = 0x10;
        const GENERATOR                  = 0x20;
        const NOFREE                     = 0x40;
        const COROUTINE                  = 0x80;
        const ITERABLE_COROUTINE        = 0x100;
        const ASYNC_GENERATOR           = 0x200;
        // TODO: old versions
        const GENERATOR_ALLOWED        = 0x1000;
        const FUTURE_DIVISION          = 0x2000;
        const FUTURE_ABSOLUTE_IMPORT   = 0x4000;
        const FUTURE_WITH_STATEMENT    = 0x8000;
        const FUTURE_PRINT_FUNCTION   = 0x10000;
        const FUTURE_UNICODE_LITERALS = 0x20000;
        const FUTURE_BARRY_AS_BDFL    = 0x40000;
        const FUTURE_GENERATOR_STOP   = 0x80000;
        #[allow(clippy::unreadable_literal)]
        const FUTURE_ANNOTATIONS     = 0x100000;
    }
}

#[rustfmt::skip]
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct Code {
    pub argcount:        u32,
    pub posonlyargcount: u32,
    pub kwonlyargcount:  u32,
    pub nlocals:         u32,
    pub stacksize:       u32,
    pub flags:           CodeFlags,
    pub code:            Arc<Vec<u8>>,
    pub consts:          Arc<Vec<Obj>>,
    pub names:           Vec<Arc<String>>,
    pub varnames:        Vec<Arc<String>>,
    pub freevars:        Vec<Arc<String>>,
    pub cellvars:        Vec<Arc<String>>,
    pub filename:        Arc<String>,
    pub name:            Arc<String>,
    pub firstlineno:     u32,
    pub lnotab:          Arc<Vec<u8>>,
}

#[rustfmt::skip]
#[derive(Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum Obj {
    None,
    StopIteration,
    Ellipsis,
    Bool     (bool),
    Long     (Arc<BigInt>),
    Float    (f64),
    Complex  (Complex<f64>),
    Bytes    (Arc<Vec<u8>>),
    String   (Arc<String>),
    Tuple    (Arc<Vec<Obj>>),
    List     (ArcRwLock<Vec<Obj>>),
    Dict     (ArcRwLock<HashMap<ObjHashable, Obj>>),
    Set      (ArcRwLock<HashSet<ObjHashable>>),
    FrozenSet(Arc<HashSet<ObjHashable>>),
    Code     (Arc<Code>),
    // etc.
}
macro_rules! define_extract {
    ($extract_fn:ident($variant:ident) -> ()) => {
        define_extract! { $extract_fn -> () { $variant => () } }
    };
    ($extract_fn:ident($variant:ident) -> Arc<$ret:ty>) => {
        define_extract! { $extract_fn -> Arc<$ret> { $variant(x) => x } }
    };
    ($extract_fn:ident($variant:ident) -> ArcRwLock<$ret:ty>) => {
        define_extract! { $extract_fn -> ArcRwLock<$ret> { $variant(x) => x } }
    };
    ($extract_fn:ident($variant:ident) -> $ret:ty) => {
        define_extract! { $extract_fn -> $ret { $variant(x) => x } }
    };
    ($extract_fn:ident -> $ret:ty { $variant:ident$(($($pat:pat),+))? => $expr:expr }) => {
        /// # Errors
        /// Returns a reference to self if extraction fails
        pub fn $extract_fn(self) -> Result<$ret, Self> {
            if let Self::$variant$(($($pat),+))? = self {
                Ok($expr)
            } else {
                Err(self)
            }
        }
    }
}
macro_rules! define_is {
    ($is_fn:ident($variant:ident$(($($pat:pat),+))?)) => {
        /// # Errors
        /// Returns a reference to self if extraction fails
        #[must_use]
        pub fn $is_fn(&self) -> bool {
            if let Self::$variant$(($($pat),+))? = self {
                true
            } else {
                false
            }
        }
    }
}
impl Obj {
    define_extract! { extract_none          (None)          -> ()                                    }
    define_extract! { extract_stop_iteration(StopIteration) -> ()                                    }
    define_extract! { extract_bool          (Bool)          -> bool                                  }
    define_extract! { extract_long          (Long)          -> Arc<BigInt>                           }
    define_extract! { extract_float         (Float)         -> f64                                   }
    define_extract! { extract_bytes         (Bytes)         -> Arc<Vec<u8>>                          }
    define_extract! { extract_string        (String)        -> Arc<String>                           }
    define_extract! { extract_tuple         (Tuple)         -> Arc<Vec<Self>>                        }
    define_extract! { extract_list          (List)          -> ArcRwLock<Vec<Self>>                  }
    define_extract! { extract_dict          (Dict)          -> ArcRwLock<HashMap<ObjHashable, Self>> }
    define_extract! { extract_set           (Set)           -> ArcRwLock<HashSet<ObjHashable>>       }
    define_extract! { extract_frozenset     (FrozenSet)     -> Arc<HashSet<ObjHashable>>             }
    define_extract! { extract_code          (Code)          -> Arc<Code>                             }

    define_is! { is_none          (None)          }
    define_is! { is_stop_iteration(StopIteration) }
    define_is! { is_bool          (Bool(_))       }
    define_is! { is_long          (Long(_))       }
    define_is! { is_float         (Float(_))      }
    define_is! { is_bytes         (Bytes(_))      }
    define_is! { is_string        (String(_))     }
    define_is! { is_tuple         (Tuple(_))      }
    define_is! { is_list          (List(_))       }
    define_is! { is_dict          (Dict(_))       }
    define_is! { is_set           (Set(_))        }
    define_is! { is_frozenset     (FrozenSet(_))  }
    define_is! { is_code          (Code(_))       }
}
/// Should mostly match Python's repr
///
/// # Float, Complex
/// - Uses `float('...')` instead of `...` for nan, inf, and -inf.
/// - Uses Rust's float-to-decimal conversion.
///
/// # Bytes, String
/// - Always uses double-quotes
/// - Escapes both kinds of quotes
///
/// # Code
/// - Uses named arguments for readability
/// - lnotab is formatted as bytes(...) with a list of integers, instead of a bytes literal
impl fmt::Debug for Obj {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::StopIteration => write!(f, "StopIteration"),
            Self::Ellipsis => write!(f, "Ellipsis"),
            Self::Bool(true) => write!(f, "True"),
            Self::Bool(false) => write!(f, "False"),
            Self::Long(x) => write!(f, "{}", x),
            &Self::Float(x) => python_float_repr_full(f, x),
            &Self::Complex(x) => python_complex_repr(f, x),
            Self::Bytes(x) => python_bytes_repr(f, x),
            Self::String(x) => python_string_repr(f, x),
            Self::Tuple(x) => python_tuple_repr(f, x),
            Self::List(x) => f.debug_list().entries(x.read().unwrap().iter()).finish(),
            Self::Dict(x) => f.debug_map().entries(x.read().unwrap().iter()).finish(),
            Self::Set(x) => f.debug_set().entries(x.read().unwrap().iter()).finish(),
            Self::FrozenSet(x) => python_frozenset_repr(f, x),
            Self::Code(x) => python_code_repr(f, x),
        }
    }
}
fn python_float_repr_full(f: &mut fmt::Formatter, x: f64) -> fmt::Result {
    python_float_repr_core(f, x)?;
    if x.fract() == 0. {
        write!(f, ".0")?;
    };
    Ok(())
}
fn python_float_repr_core(f: &mut fmt::Formatter, x: f64) -> fmt::Result {
    if x.is_nan() {
        write!(f, "float('nan')")
    } else if x.is_infinite() {
        if x.is_sign_positive() {
            write!(f, "float('inf')")
        } else {
            write!(f, "-float('inf')")
        }
    } else {
        // properly handle -0.0
        if x.is_sign_negative() {
            write!(f, "-")?;
        }
        write!(f, "{}", x.abs())
    }
}
fn python_complex_repr(f: &mut fmt::Formatter, x: Complex<f64>) -> fmt::Result {
    if x.re == 0. && x.re.is_sign_positive() {
        python_float_repr_core(f, x.im)?;
        write!(f, "j")?;
    } else {
        write!(f, "(")?;
        python_float_repr_core(f, x.re)?;
        if x.im >= 0. || x.im.is_nan() {
            write!(f, "+")?;
        }
        python_float_repr_core(f, x.im)?;
        write!(f, "j)")?;
    };
    Ok(())
}
fn python_bytes_repr(f: &mut fmt::Formatter, x: &[u8]) -> fmt::Result {
    write!(f, "b\"")?;
    for &byte in x.iter() {
        match byte {
            b'\t' => write!(f, "\\t")?,
            b'\n' => write!(f, "\\n")?,
            b'\r' => write!(f, "\\r")?,
            b'\'' | b'"' | b'\\' => write!(f, "\\{}", char::from(byte))?,
            b' '..=b'~' => write!(f, "{}", char::from(byte))?,
            _ => write!(f, "\\x{:02x}", byte)?,
        }
    }
    write!(f, "\"")?;
    Ok(())
}
fn python_string_repr(f: &mut fmt::Formatter, x: &str) -> fmt::Result {
    let original = format!("{:?}", x);
    let mut last_end = 0;
    // Note: the behavior is arbitrary if there are improper escapes.
    for (start, _) in original.match_indices("\\u{") {
        f.write_str(&original[last_end..start])?;
        let len = original[start..].find('}').ok_or(fmt::Error)? + 1;
        let end = start + len;
        match len - 4 {
            0..=2 => write!(f, "\\x{:0>2}", &original[start + 3..end - 1])?,
            3..=4 => write!(f, "\\u{:0>4}", &original[start + 3..end - 1])?,
            5..=8 => write!(f, "\\U{:0>8}", &original[start + 3..end - 1])?,
            _ => panic!("Internal error: length of unicode escape = {} > 8", len),
        }
        last_end = end;
    }
    f.write_str(&original[last_end..])?;
    Ok(())
}
fn python_tuple_repr(f: &mut fmt::Formatter, x: &[Obj]) -> fmt::Result {
    if x.is_empty() {
        f.write_str("()") // Otherwise this would get formatted into an empty string
    } else {
        let mut debug_tuple = f.debug_tuple("");
        for o in x.iter() {
            debug_tuple.field(&o);
        }
        debug_tuple.finish()
    }
}
fn python_frozenset_repr(f: &mut fmt::Formatter, x: &HashSet<ObjHashable>) -> fmt::Result {
    f.write_str("frozenset(")?;
    if !x.is_empty() {
        f.debug_set().entries(x.iter()).finish()?;
    }
    f.write_str(")")?;
    Ok(())
}
fn python_code_repr(f: &mut fmt::Formatter, x: &Code) -> fmt::Result {
    write!(f, "code(argcount={:?}, posonlyargcount={:?}, kwonlyargcount={:?}, nlocals={:?}, stacksize={:?}, flags={:?}, code={:?}, consts={:?}, names={:?}, varnames={:?}, freevars={:?}, cellvars={:?}, filename={:?}, name={:?}, firstlineno={:?}, lnotab=bytes({:?}))", x.argcount, x.posonlyargcount, x.kwonlyargcount, x.nlocals, x.stacksize, x.flags, Obj::Bytes(Arc::clone(&x.code)), x.consts, x.names, x.varnames, x.freevars, x.cellvars, x.filename, x.name, x.firstlineno, &x.lnotab)
}
/// This is a f64 wrapper suitable for use as a key in a (Hash)Map, since NaNs compare equal to
/// each other, so it can implement Eq and Hash. `HashF64(-0.0) == HashF64(0.0)`.
#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct HashF64(f64);
impl PartialEq for HashF64 {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 || (self.0.is_nan() && other.0.is_nan())
    }
}
impl Eq for HashF64 {}
impl Hash for HashF64 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if self.0.is_nan() {
            // Multiple NaN values exist
            state.write_u8(0);
        } else if self.0 == 0.0 {
            // 0.0 == -0.0
            state.write_u8(1);
        } else {
            state.write_u64(self.0.to_bits()); // This should be fine, since all the dupes should be accounted for.
        }
    }
}
impl From<f64> for HashF64 {
    #[inline]
    fn from(val: f64) -> Self {
        HashF64(val)
    }
}
impl From<HashF64> for f64 {
    #[inline]
    fn from(val: HashF64) -> Self {
        val.0
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
#[cfg_attr(feature = "serialize", serde(bound = "T: Eq + Hash + serde::Serialize"))]
pub struct HashableHashSet<T>(HashSet<T>);
impl<T> Hash for HashableHashSet<T>
where
    T: Hash,
{
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut xor: u64 = 0;
        let hasher = std::collections::hash_map::DefaultHasher::new();
        for value in &self.0 {
            let mut hasher_clone = hasher.clone();
            value.hash(&mut hasher_clone);
            xor ^= hasher_clone.finish();
        }
        state.write_u64(xor);
    }
}
impl<T> PartialEq for HashableHashSet<T>
where
    T: Eq + Hash,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T> Eq for HashableHashSet<T> where T: Eq + Hash {}
impl<T> FromIterator<T> for HashableHashSet<T>
where
    T: Eq + Hash,
{
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        Self(iter.into_iter().collect())
    }
}

#[derive(PartialEq, Eq, Hash, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum ObjHashable {
    None,
    StopIteration,
    Ellipsis,
    Bool(bool),
    Long(Arc<BigInt>),
    Float(HashF64),
    Complex(Complex<HashF64>),
    String(Arc<String>),
    Tuple(Arc<Vec<ObjHashable>>),
    FrozenSet(Arc<HashableHashSet<ObjHashable>>),
    // etc.
}
impl TryFrom<&Obj> for ObjHashable {
    type Error = Obj;

    fn try_from(orig: &Obj) -> Result<Self, Obj> {
        match orig {
            Obj::None => Ok(Self::None),
            Obj::StopIteration => Ok(Self::StopIteration),
            Obj::Ellipsis => Ok(Self::Ellipsis),
            Obj::Bool(x) => Ok(Self::Bool(*x)),
            Obj::Long(x) => Ok(Self::Long(Arc::clone(x))),
            Obj::Float(x) => Ok(Self::Float(HashF64(*x))),
            Obj::Complex(Complex { re, im }) => Ok(Self::Complex(Complex {
                re: HashF64(*re),
                im: HashF64(*im),
            })),
            Obj::String(x) => Ok(Self::String(Arc::clone(x))),
            Obj::Tuple(x) => Ok(Self::Tuple(Arc::new(
                x.iter()
                    .map(Self::try_from)
                    .collect::<Result<Vec<Self>, Obj>>()?,
            ))),
            Obj::FrozenSet(x) => Ok(Self::FrozenSet(Arc::new(
                x.iter().cloned().collect::<HashableHashSet<Self>>(),
            ))),
            x => Err(x.clone()),
        }
    }
}
impl fmt::Debug for ObjHashable {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::StopIteration => write!(f, "StopIteration"),
            Self::Ellipsis => write!(f, "Ellipsis"),
            Self::Bool(true) => write!(f, "True"),
            Self::Bool(false) => write!(f, "False"),
            Self::Long(x) => write!(f, "{}", x),
            Self::Float(x) => python_float_repr_full(f, x.0),
            Self::Complex(x) => python_complex_repr(
                f,
                Complex {
                    re: x.re.0,
                    im: x.im.0,
                },
            ),
            Self::String(x) => python_string_repr(f, x),
            Self::Tuple(x) => python_tuple_hashable_repr(f, x),
            Self::FrozenSet(x) => python_frozenset_repr(f, &x.0),
        }
    }
}
fn python_tuple_hashable_repr(f: &mut fmt::Formatter, x: &[ObjHashable]) -> fmt::Result {
    if x.is_empty() {
        f.write_str("()") // Otherwise this would get formatted into an empty string
    } else {
        let mut debug_tuple = f.debug_tuple("");
        for o in x.iter() {
            debug_tuple.field(&o);
        }
        debug_tuple.finish()
    }
}

#[cfg(test)]
mod test;

mod utils;

pub mod read;
