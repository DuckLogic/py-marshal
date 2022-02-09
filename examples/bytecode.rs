//! Example parsing of of Python bytecode.
//!
//! This test is somewhat convoluted, because it tries to make its output
//! match the "reference" implementation in bytecode/reference.py
//!
//! This is further complicated by the excessive `Arc` in `Obj`
//! which makes it harder for us to use the serde ecosystem :(
use std::env;
use std::io::Read;
use std::sync::Arc;
use std::collections::HashSet;
use num_bigint::BigInt;

use anyhow::{Context, anyhow};
use byteorder::{ReadBytesExt, LittleEndian};

fn fatal(msg: impl std::fmt::Display) -> ! {
    eprintln!("{}", msg);
    std::process::exit(1);
}   

#[derive(Copy, Clone, Debug)]
enum InputFormat {
    Bytecode,
    Plain
}

fn main() -> Result<(), anyhow::Error> {
    let mut args = env::args().peekable();
    args.next(); // Skip program
    let mut input_format = InputFormat::Plain;
    while args.peek().map_or(false, |arg| arg.starts_with("--")) {
        let flag = args.next().unwrap();
        match &*flag {
            "--format" => {
                let format = args.next()
                    .ok_or_else(|| anyhow!("Expected an argument to --format flag"))?;
                input_format = match &*format {
                    "plain" => InputFormat::Plain,
                    "bytecode" => InputFormat::Bytecode,
                    _ => fatal(format_args!("Unknown format: {format:?}"))
                };

            }
            "--" => break, // End of special arg processing
            _ => {
                fatal(format_args!("Invalid flag: {:?}", flag));
            }
        }
    }
    let mut input = if let Some(file_name) = args.next() {
        let reader = std::io::BufReader::new(
            std::fs::File::open(&file_name)
                    .with_context(|| format!("Unable to open file: {:?}", file_name))?
            );
        Box::new(reader) as Box<dyn Read>
    } else {
        Box::new(std::io::stdin()) as Box<dyn Read>
    };
    match input_format {
        InputFormat::Bytecode => {
            skip_bytecode_header(&mut input)
                .context("Unable to read bytecode header")?;
        },
        InputFormat::Plain => {}
    }
    let value = py_marshal::read::marshal_load(&mut input)
        .context("Unable to read marshaled input (via py_marshal lib)")?;
    let serialized = serialize_obj(&value);
    println!("{}", ::serde_json::to_string(&serialized).unwrap());
    Ok(())
}
use num_traits::ToPrimitive;
use py_marshal::{Obj, ObjHashable};
fn serialize_obj(obj: &Obj) -> serde_json::Value {
    use serde_json::json;
    match *obj {
        Obj::None => {
            json!({"type": "NoneType", "value": null})
        },
        Obj::StopIteration => {
            json!({"type": "StopIteration", "value": null})
        },
        Obj::Ellipsis => {
            json!({"type": "ellipsis", "value": null})
        },
        Obj::Bool(val) => json!(val),
        Obj::Long(ref val) => {
            let val: serde_json::Number = val.to_i64()
                .unwrap_or_else(|| panic!("Integer too large for i64: {}", val))
                .into();
            json!(val)
        },
        Obj::Float(val) => json!(val),
        Obj::Complex(val) => {
            json!({"type": "complex", "value": [val.re, val.im]})
        },
        Obj::Bytes(ref val) => {
            let val = base64::encode(&**val);
            json!({"type": "bytes", "value": val})
        },
        Obj::String(ref val) => json!(&&*val),
        Obj::Tuple(ref objs) => {
            let value = serialize_obj_iter(objs.iter());
            json!({"type": "tuple", "value": value})
        },
        Obj::List(ref objs) => {
            let guard = objs.read().unwrap();
            let value = serialize_obj_iter(guard.iter());
            json!({"type": "list", "value": value})
        },
        Obj::Set(ref set) => {
            let sorted = sorted_objs(set.read().unwrap()
                .iter().cloned().map(hashable_to_obj));
            let value = serialize_obj_iter(sorted.iter());
            json!({"type": "set", "value": value})
        },
        Obj::FrozenSet(ref set) => {
            let sorted = sorted_objs(set
                .iter().cloned().map(hashable_to_obj));
            let value = serialize_obj_iter(sorted.iter());
            json!({"type": "frozenset", "value": value})
        }
        Obj::Dict(ref objs) => {
            let value = serde_json::Value::Object(objs.read().unwrap()
                .iter()
                .map(|(key, value)| (
                    String::clone(&*hashable_to_obj(key.clone())
                        .extract_string().unwrap()),
                    serialize_obj(value)
                )).collect());
            json!({"type": "dict", "value": value})
        },
        Obj::Code(ref original_code) => {
            let mut code = Arc::clone(original_code);
            let mut code = Arc::make_mut(&mut code);
            code.consts = Arc::new(vec![]);
            let mut value = serde_json::to_value(&*code)
                .expect("Unable to serialize code obj");
            const BYTES_FIELDS: &[&str] = &["code", "lnotab"];
            value.as_object_mut().unwrap().insert(
                "consts".into(),
                original_code.consts.iter()
                    .map(|obj| serialize_obj(obj))
                    .collect()
            );
            fn rewrite_value(name: &str, value: &serde_json::Value) -> serde_json::Value {
                if BYTES_FIELDS.contains(&name) {
                    let mut bytes: Vec<u8> = Vec::new();
                    let vals = value.as_array()
                        .unwrap_or_else(|| panic!("Expected a byte array for field {:?}", name));
                    for val in vals {
                        match &*val {
                            serde_json::Value::Number(num) 
                                if num.as_i64().filter(|&val| val < 256).is_some() => {
                                bytes.push(num.as_i64().unwrap() as u8);

                            },
                            _ => panic!("Expected a byte for field {:?}, but got {:?}", name, val)
                        }
                    }
                    let encoded = serde_json::Value::String(base64::encode(
                        &bytes
                    ));
                    json!({"type": "bytes", "value": encoded})
                } else {
                    value.clone()
                }
            }
            let value = serde_json::Value::Object(
                value.as_object().unwrap().iter()
                    .map(|(name, value)| (format!("co_{}", name), rewrite_value(name, value)))
                    .collect()
            );
            json!({"type": "code", "value": value})
        }

    }
}
#[derive(PartialEq, Hash, Eq, Ord, PartialOrd, Clone)]
enum OrdObj {
    Unordered,
    Bool(bool),
    Bytes(Arc<Vec<u8>>),
    String(Arc<String>),
    Integer(Arc<BigInt>),
    Float(ordered_float::OrderedFloat<f64>),
    FrozenSet(Vec<OrdObj>),
    Tuple(Vec<OrdObj>)
}
impl From<Obj> for OrdObj {
    fn from(obj: Obj) -> Self {
        match obj {
            Obj::None | Obj::StopIteration | Obj::Ellipsis => OrdObj::Unordered,
            Obj::Bool(val) => OrdObj::Bool(val),
            Obj::Long(val) => OrdObj::Integer(val),
            Obj::Float(val) => OrdObj::Float(val.into()),
            // Technically speaking complex numbers aren't ordered
            Obj::Complex(_) => OrdObj::Unordered, 
            Obj::Bytes(b) => OrdObj::Bytes(b),
            Obj::String(s) => OrdObj::String(s),
            Obj::Tuple(v) => OrdObj::Tuple(v.iter().cloned()
                .map(OrdObj::from).collect()),
            Obj::List(_) |
            Obj::Dict(_) |
            Obj::Set(_) => OrdObj::Unordered,
            Obj::FrozenSet(ref set) => {
                let objs = sorted_objs(set.iter().cloned().map(hashable_to_obj));
                OrdObj::FrozenSet(objs.into_iter().map(OrdObj::from).collect())
            },
            Obj::Code(_) => OrdObj::Unordered,
        }
    }
}
fn hashable_to_obj(obj: ObjHashable) -> Obj {
    match obj {
        ObjHashable::None => Obj::None,
        ObjHashable::StopIteration => Obj::StopIteration,
        ObjHashable::Ellipsis => Obj::Ellipsis,
        ObjHashable::Bool(val) => Obj::Bool(val),
        ObjHashable::Long(l) => Obj::Long(l),
        ObjHashable::Float(f) => Obj::Float(f.into()),
        ObjHashable::Complex(c) => Obj::Complex(num_complex::Complex {
            re: c.re.into(),
            im: c.im.into()
        }),
        ObjHashable::String(s) => Obj::String(s),
        ObjHashable::Tuple(t) => Obj::Tuple(Arc::new(t.iter().cloned()
            .map(hashable_to_obj).collect())),
        ObjHashable::FrozenSet(s) => Obj::FrozenSet(Arc::new(
            {
                let s: &HashSet<ObjHashable> = (&*s).as_ref();
                s.iter().cloned().collect()
            }
        )),
    }
}
fn sorted_objs<'a>(objs: impl Iterator<Item=Obj>) -> Vec<Obj> {
    let mut v: Vec<Obj> = objs.collect();
    v.sort_by_cached_key(|obj| OrdObj::from(obj.clone()));
    v
}
fn serialize_obj_iter<'a>(objs: impl Iterator<Item=&'a Obj>) -> serde_json::Value {
    serde_json::Value::Array(objs.map(serialize_obj).collect())
}


struct BytecodeHeader {
    #[allow(dead_code)]
    magic_number: u32

}
fn skip_bytecode_header(rd: &mut dyn Read) -> Result<BytecodeHeader, anyhow::Error> {
    /*
     * See source code in importlib/_bootstrap_external.py in CPython
     * 
     * Specifically _code_to_timestamp_pyc in 3.9/3.10
     */
    let magic_number = rd.read_u16::<LittleEndian>()? as u32;
    let mut buf: [u8; 2] = [0; 2];
    rd.read_exact(&mut buf)?;
    anyhow::ensure!(
        buf == *b"\r\n",
        "Expected \\r\\n after magic number {}, but got {:?}",
        magic_number, buf
    );
    let flags = rd.read_u32::<LittleEndian>()?;
    // Ensure that we're actually using a timestamp based 
    anyhow::ensure!(
        flags == 0,
        "Unexpected flags {} for bytecode header (NOTE: Only timestamp-based caching is supported)",
        flags
    );
    let _mtime = rd.read_u32::<LittleEndian>()?;
    let _source_size = rd.read_u32::<LittleEndian>()?;
    Ok(BytecodeHeader {
        magic_number
    })
}