use super::{Code, CodeFlags, Obj, ObjHashable};
use num_bigint::BigInt;
use num_complex::Complex;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

#[test]
fn test_debug_repr() {
    assert_eq!(format!("{:?}", Obj::None), "None");
    assert_eq!(format!("{:?}", Obj::StopIteration), "StopIteration");
    assert_eq!(format!("{:?}", Obj::Ellipsis), "Ellipsis");
    assert_eq!(format!("{:?}", Obj::Bool(true)), "True");
    assert_eq!(format!("{:?}", Obj::Bool(false)), "False");
    assert_eq!(
        format!("{:?}", Obj::Long(Arc::new(BigInt::from(-123)))),
        "-123"
    );
    assert_eq!(format!("{:?}", Obj::Tuple(Arc::new(vec![]))), "()");
    assert_eq!(
        format!("{:?}", Obj::Tuple(Arc::new(vec![Obj::Bool(true)]))),
        "(True,)"
    );
    assert_eq!(
        format!(
            "{:?}",
            Obj::Tuple(Arc::new(vec![Obj::Bool(true), Obj::None]))
        ),
        "(True, None)"
    );
    assert_eq!(
        format!(
            "{:?}",
            Obj::List(Arc::new(RwLock::new(vec![Obj::Bool(true)])))
        ),
        "[True]"
    );
    assert_eq!(
        format!(
            "{:?}",
            Obj::Dict(Arc::new(RwLock::new(
                vec![(
                    ObjHashable::Bool(true),
                    Obj::Bytes(Arc::new(Vec::from(b"a" as &[u8])))
                )]
                .into_iter()
                .collect::<HashMap<_, _>>()
            )))
        ),
        "{True: b\"a\"}"
    );
    assert_eq!(
        format!(
            "{:?}",
            Obj::Set(Arc::new(RwLock::new(
                vec![ObjHashable::Bool(true)]
                    .into_iter()
                    .collect::<HashSet<_>>()
            )))
        ),
        "{True}"
    );
    assert_eq!(
        format!(
            "{:?}",
            Obj::FrozenSet(Arc::new(
                vec![ObjHashable::Bool(true)]
                    .into_iter()
                    .collect::<HashSet<_>>()
            ))
        ),
        "frozenset({True})"
    );
    assert_eq!(format!("{:?}", Obj::Code(Arc::new(Code {
        argcount: 0,
        posonlyargcount: 1,
        kwonlyargcount: 2,
        nlocals: 3,
        stacksize: 4,
        flags: CodeFlags::NESTED | CodeFlags::COROUTINE,
        code: Arc::new(Vec::from(b"abc" as &[u8])),
        consts: Arc::new(vec![Obj::Bool(true)]),
        names: vec![],
        varnames: vec![Arc::new("a".to_owned())],
        freevars: vec![Arc::new("b".to_owned()), Arc::new("c".to_owned())],
        cellvars: vec![Arc::new("de".to_owned())],
        filename: Arc::new("xyz.py".to_owned()),
        name: Arc::new("fgh".to_owned()),
        firstlineno: 5,
        lnotab: Arc::new(vec![255, 0, 45, 127, 0, 73]),
    }))), "code(argcount=0, posonlyargcount=1, kwonlyargcount=2, nlocals=3, stacksize=4, flags=NESTED | COROUTINE, code=b\"abc\", consts=[True], names=[], varnames=[\"a\"], freevars=[\"b\", \"c\"], cellvars=[\"de\"], filename=\"xyz.py\", name=\"fgh\", firstlineno=5, lnotab=bytes([255, 0, 45, 127, 0, 73]))");
}

#[test]
fn test_float_debug_repr() {
    assert_eq!(format!("{:?}", Obj::Float(1.23)), "1.23");
    assert_eq!(format!("{:?}", Obj::Float(f64::NAN)), "float('nan')");
    assert_eq!(format!("{:?}", Obj::Float(f64::INFINITY)), "float('inf')");
    assert_eq!(format!("{:?}", Obj::Float(-f64::INFINITY)), "-float('inf')");
    assert_eq!(format!("{:?}", Obj::Float(0.0)), "0.0");
    assert_eq!(format!("{:?}", Obj::Float(-0.0)), "-0.0");
}

#[test]
fn test_complex_debug_repr() {
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: 2., im: 1. })),
        "(2+1j)"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: 0., im: 1. })),
        "1j"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: 2., im: 0. })),
        "(2+0j)"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: 0., im: 0. })),
        "0j"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: -2., im: 1. })),
        "(-2+1j)"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: -2., im: 0. })),
        "(-2+0j)"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: 2., im: -1. })),
        "(2-1j)"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: 0., im: -1. })),
        "-1j"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: -2., im: -1. })),
        "(-2-1j)"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: 0., im: -1. })),
        "-1j"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: -2., im: 0. })),
        "(-2+0j)"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: -0., im: 1. })),
        "(-0+1j)"
    );
    assert_eq!(
        format!("{:?}", Obj::Complex(Complex { re: -0., im: -1. })),
        "(-0-1j)"
    );
}

#[test]
fn test_bytes_string_debug_repr() {
    assert_eq!(format!("{:?}", Obj::Bytes(Arc::new(Vec::from(
                        b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~\x7f\x80\x81\x82\x83\x84\x85\x86\x87\x88\x89\x8a\x8b\x8c\x8d\x8e\x8f\x90\x91\x92\x93\x94\x95\x96\x97\x98\x99\x9a\x9b\x9c\x9d\x9e\x9f\xa0\xa1\xa2\xa3\xa4\xa5\xa6\xa7\xa8\xa9\xaa\xab\xac\xad\xae\xaf\xb0\xb1\xb2\xb3\xb4\xb5\xb6\xb7\xb8\xb9\xba\xbb\xbc\xbd\xbe\xbf\xc0\xc1\xc2\xc3\xc4\xc5\xc6\xc7\xc8\xc9\xca\xcb\xcc\xcd\xce\xcf\xd0\xd1\xd2\xd3\xd4\xd5\xd6\xd7\xd8\xd9\xda\xdb\xdc\xdd\xde\xdf\xe0\xe1\xe2\xe3\xe4\xe5\xe6\xe7\xe8\xe9\xea\xeb\xec\xed\xee\xef\xf0\xf1\xf2\xf3\xf4\xf5\xf6\xf7\xf8\xf9\xfa\xfb\xfc\xfd\xfe" as &[u8]
                        )))),
    "b\"\\x00\\x01\\x02\\x03\\x04\\x05\\x06\\x07\\x08\\t\\n\\x0b\\x0c\\r\\x0e\\x0f\\x10\\x11\\x12\\x13\\x14\\x15\\x16\\x17\\x18\\x19\\x1a\\x1b\\x1c\\x1d\\x1e\\x1f !\\\"#$%&\\\'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\\\]^_`abcdefghijklmnopqrstuvwxyz{|}~\\x7f\\x80\\x81\\x82\\x83\\x84\\x85\\x86\\x87\\x88\\x89\\x8a\\x8b\\x8c\\x8d\\x8e\\x8f\\x90\\x91\\x92\\x93\\x94\\x95\\x96\\x97\\x98\\x99\\x9a\\x9b\\x9c\\x9d\\x9e\\x9f\\xa0\\xa1\\xa2\\xa3\\xa4\\xa5\\xa6\\xa7\\xa8\\xa9\\xaa\\xab\\xac\\xad\\xae\\xaf\\xb0\\xb1\\xb2\\xb3\\xb4\\xb5\\xb6\\xb7\\xb8\\xb9\\xba\\xbb\\xbc\\xbd\\xbe\\xbf\\xc0\\xc1\\xc2\\xc3\\xc4\\xc5\\xc6\\xc7\\xc8\\xc9\\xca\\xcb\\xcc\\xcd\\xce\\xcf\\xd0\\xd1\\xd2\\xd3\\xd4\\xd5\\xd6\\xd7\\xd8\\xd9\\xda\\xdb\\xdc\\xdd\\xde\\xdf\\xe0\\xe1\\xe2\\xe3\\xe4\\xe5\\xe6\\xe7\\xe8\\xe9\\xea\\xeb\\xec\\xed\\xee\\xef\\xf0\\xf1\\xf2\\xf3\\xf4\\xf5\\xf6\\xf7\\xf8\\xf9\\xfa\\xfb\\xfc\\xfd\\xfe\""
    );
    assert_eq!(format!("{:?}", Obj::String(Arc::new(String::from(
                        "\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~\x7f")))),
                        "\"\\x00\\x01\\x02\\x03\\x04\\x05\\x06\\x07\\x08\\t\\n\\x0b\\x0c\\r\\x0e\\x0f\\x10\\x11\\x12\\x13\\x14\\x15\\x16\\x17\\x18\\x19\\x1a\\x1b\\x1c\\x1d\\x1e\\x1f !\\\"#$%&\\\'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\\\]^_`abcdefghijklmnopqrstuvwxyz{|}~\\x7f\"");
}
