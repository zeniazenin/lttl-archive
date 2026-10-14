"""Validate the shipped PE subsystem and icon resources without opening a window."""
import argparse
from pathlib import Path
import struct

def inspect(path, subsystem):
    data = path.read_bytes()
    u16 = lambda o: struct.unpack_from('<H', data, o)[0]
    u32 = lambda o: struct.unpack_from('<I', data, o)[0]
    pe = u32(0x3c)
    assert data[pe:pe+4] == b'PE\0\0', path
    opt = pe + 24
    assert u16(opt + 68) == subsystem, f'{path}: wrong subsystem'
    directories = opt + (112 if u16(opt) == 0x20b else 96)
    resource_rva = u32(directories + 16)
    assert resource_rva, f'{path}: no resource directory'
    sections = opt + u16(pe + 20)
    for i in range(u16(pe + 6)):
        s = sections + 40 * i
        if u32(s + 12) <= resource_rva < u32(s + 12) + max(u32(s+8), u32(s+16)):
            root = u32(s+20) + resource_rva - u32(s+12)
            break
    else:
        raise AssertionError('Resource section not found')
    types = {u32(root+16+8*i) for i in range(u16(root+12)+u16(root+14))}
    assert {3,14} <= types, f'{path}: missing image/group icon resources'
    print(f'PASS: {path.name}: subsystem={subsystem}, icon and group-icon resources present')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--installer', type=Path)
    args = parser.parse_args()
    inspect(args.directory/'lttl.exe', 3)
    inspect(args.directory/'lttl-gui.exe', 2)
    if args.installer:
        inspect(args.installer, 2)
