from pathlib import Path
import json
import marshal
import sys
import base64


def explicitly_typed_object(explicit_type, value):
    assert isinstance(explicit_type, (type, str)), f"Unexpected explcit type: {explicit_type!r}"
    type_name = explicit_type.__name__ if isinstance(explicit_type, type) else explicit_type
    return {"type": type_name, "value": value}

class TypedEncoder(json.JSONEncoder):
    def default(self, obj):
        if isinstance(obj, (set, list, frozenset, tuple)):
            value = list(obj)
            if isinstance(obj, (set, frozenset)):
                value.sort()
            return explicitly_typed_object(type(obj), value)
        elif isinstance(obj, dict):
            return explicitly_typed_object(dict, obj)
        elif isinstance(obj, (bytes, bytearray)):
            return explicitly_typed_object(type(obj), base64.b64encode(obj).decode('utf8'))
        elif isinstance(obj, complex):
            return explicitly_typed_object(type(obj), [obj.real, obj.imag])
        elif hasattr(obj, 'co_flags'):
            res = {}
            for key in dir(obj):
                if key.startswith("co_"):
                    res[key] = getattr(obj, key)
            return explicitly_typed_object(type(obj), res)
        elif isinstance(obj, str, int, float, bool, type(None)):
            return super().default(obj)
        else:
            raise TypeError("Foo: {}", )

_VALID_FORAMTS = {"plain", "bytecode"}

def marshal2json(args):
    format_type = "plain"
    if len(args) >= 1 and args[0].startswith('--'):
        opt = args.pop(0)
        if opt == "--format":
            format_type = args.pop(0)
            if format_type not in _VALID_FORAMTS:
                print(f"Unknown format: {format_type!r}", file=sys.stderr)
                sys.exit(1)
        else:
            print("Unknown option:", opt, file=sys.stderr)
            sys.exit(1)
    if not args:
        data = sys.stdin.buffer.read()
    else:
        target = args[0]
        with open(target, 'rb') as f:
            data = f.read()
    if format_type == "plain":
        pass
    elif format_type == "bytecode":
        # TODO: This works fine for "timestamp" based bytecode,
        # but not for other types of bytecode..
        data = data[4 + (4 * 3):]
    data = marshal.loads(data)
    print(TypedEncoder().encode(data))



VALID_COMMANDS = {
    'marshal2json': marshal2json
}

if __name__ == "__main__":
    cmd_name = None
    try:
        cmd_name = sys.argv[1]
        cmd = VALID_COMMANDS[cmd_name]
    except (IndexError, KeyError):
        error_detail = "insufficent args" if cmd_name is None else repr(cmd_name) + " is not valid"
        print("Must specify a valid command to execute:", error_detail)
        sys.exit(1)
    else:
        cmd(sys.argv[2:])
