#!/usr/bin/env python3
"""ring3_build.py — Compile C programs for PORTIX ring-3.

Pipeline:
  1. Compile C → COFF .o  (using MSYS2 gcc)
  2. Convert COFF .o → ELF64 .o  (using objcopy)
  3. Link ELF64 .o → ELF64 executable (Python pyelftools)

Usage:
    python scripts/ring3_build.py input.c -o output.elf
"""

import subprocess
import sys
import os
import struct
import tempfile
import shutil

from elftools.elf.elffile import ELFFile

VADDR_DEFAULT = 0x20000000
STACK_SIZE_DEFAULT = 0x10000

MSYS2 = r'C:\msys64\mingw64\bin'
GCC = os.path.join(MSYS2, 'gcc.exe')
OBJCOPY = os.path.join(MSYS2, 'objcopy.exe')

def compile_c_to_elf_o(c_path, elf_o_path, extra_flags=None, tmp_dir=None):
    flags = extra_flags or []
    base = ['-nostdlib', '-ffreestanding', '-mno-red-zone',
            '-O2', '-fomit-frame-pointer', '-fno-pic', '-fno-pie',
            '-m64', '-c']
    src_abs = os.path.abspath(c_path)
    work = tmp_dir or tempfile.mkdtemp()
    coff_o = os.path.join(work, 'out.o')
    subprocess.run([GCC] + base + flags + ['-o', coff_o, src_abs],
                   check=True, capture_output=True, text=True)
    subprocess.run([OBJCOPY, '-O', 'elf64-x86-64', coff_o, elf_o_path],
                   check=True, capture_output=True, text=True)

def link_elf(o_paths, output_path, vaddr=VADDR_DEFAULT, stack=STACK_SIZE_DEFAULT):
    sections_data = {}  # name -> bytes
    symbols = {}  # name -> (shndx, value, size)
    relocs = []  # (offset, type, sym_name, addend, shndx)

    for path in o_paths:
        with open(path, 'rb') as f:
            elf = ELFFile(f)
            if elf.elfclass != 64:
                raise ValueError(f"Expected 64-bit ELF, got {elf.elfclass}")
            symtab = None
            for sec in elf.iter_sections():
                if sec.name == '.symtab':
                    symtab = sec
                elif sec.sh_type in ('SHT_RELA', 'SHT_REL'):
                    pass  # skip, we handle symbols directly

            if symtab:
                for sym in symtab.iter_symbols():
                    name = sym.name
                    if name and name not in symbols:
                        symbols[name] = (sym.entry.st_shndx,
                                         sym.entry.st_value,
                                         sym.entry.st_size)

            for sec in elf.iter_sections():
                name = sec.name
                skip = ('', '.symtab', '.strtab', '.shstrtab',
                        '.rela.text', '.rela.rodata', '.rela.data', '.rela.bss',
                        '.rel.text', '.rel.rodata', '.rel.data', '.rel.bss',
                        '.rela.pdata', '.pdata', '.xdata',
                        '.note.GNU-stack', '.comment', '.debug_abbrev',
                        '.debug_info', '.debug_line', '.debug_str',
                        '.debug_loc', '.debug_ranges')
                if name in skip:
                    continue
                if sec.sh_type == 'SHT_NOBITS':
                    sections_data[name] = b''
                elif sec.data_size > 0:
                    sections_data[name] = sec.data()

    text = sections_data.get('.text', b'')
    rodata = sections_data.get('.rodata', b'')
    data = sections_data.get('.data', b'')

    text_vaddr = vaddr
    text_offset = 64 + 56  # ELF header + program header
    text_size = len(text)

    if rodata:
        rodata_vaddr = align_up(text_vaddr + text_size, 16)
        rodata_offset = text_offset + text_size
    else:
        rodata_vaddr = text_vaddr + text_size
        rodata_offset = text_offset + text_size
    rodata_size = len(rodata)

    data_vaddr = align_up(rodata_vaddr + rodata_size, 16)
    data_offset = text_offset + text_size + rodata_size
    data_size = len(data)

    total_file_size = data_offset + data_size
    total_mem_size = data_vaddr + data_size
    bss_size = 0

    entry = text_vaddr
    if '_start' in symbols:
        entry = text_vaddr + symbols['_start'][1]

    elf = bytearray(64 + 56)
    elf[0:4] = b'\x7fELF'
    elf[4] = 2; elf[5] = 1; elf[6] = 1; elf[7] = 0
    struct.pack_into('<H', elf, 16, 2)
    struct.pack_into('<H', elf, 18, 0x3E)
    struct.pack_into('<I', elf, 20, 1)
    struct.pack_into('<Q', elf, 24, entry)
    struct.pack_into('<Q', elf, 32, 64)
    struct.pack_into('<Q', elf, 40, 64 + 56)
    struct.pack_into('<I', elf, 48, 0)
    struct.pack_into('<H', elf, 52, 64)
    struct.pack_into('<H', elf, 54, 56)
    struct.pack_into('<H', elf, 56, 1)
    struct.pack_into('<H', elf, 58, 0)
    struct.pack_into('<H', elf, 60, 0)
    struct.pack_into('<H', elf, 62, 0)

    ph = bytearray(56)
    struct.pack_into('<I', ph, 0, 1)  # PT_LOAD
    struct.pack_into('<I', ph, 4, 7)  # RWX
    struct.pack_into('<Q', ph, 8, 0)  # p_offset
    struct.pack_into('<Q', ph, 16, vaddr)
    struct.pack_into('<Q', ph, 24, vaddr)
    struct.pack_into('<Q', ph, 32, total_file_size)
    struct.pack_into('<Q', ph, 36, total_mem_size)
    struct.pack_into('<Q', ph, 40, 0x1000)

    payload = text + rodata + data
    result = bytes(elf[:64]) + bytes(ph) + payload

    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    with open(output_path, 'wb') as f:
        f.write(result)
    return len(result), entry

def align_up(x, a):
    return (x + a - 1) & ~(a - 1)

def main():
    import argparse
    ap = argparse.ArgumentParser(description='Compile C to PORTIX ring-3 ELF')
    ap.add_argument('input', nargs='+')
    ap.add_argument('-o', '--output', required=True)
    ap.add_argument('--vaddr', type=lambda x: int(x, 0), default=VADDR_DEFAULT)
    ap.add_argument('--stack', type=lambda x: int(x, 0), default=STACK_SIZE_DEFAULT)
    ap.add_argument('--no-strip', action='store_true')
    args = ap.parse_args()

    tmp_dir = tempfile.mkdtemp()
    try:
        elf_os = []
        for c_path in args.input:
            elf_o = os.path.join(tmp_dir, os.path.basename(c_path) + '.elf.o')
            base_flags = []
            compile_c_to_elf_o(c_path, elf_o, base_flags)
            elf_os.append(elf_o)

        size, entry = link_elf(elf_os, args.output, args.vaddr, args.stack)
        print(f"[OK] {args.output} — {size} bytes, entry=0x{entry:x}, stack={hex(args.stack)}")
    except Exception as e:
        print(f"[ERR] {e}", file=sys.stderr)
        sys.exit(1)
    finally:
        shutil.rmtree(tmp_dir, ignore_errors=True)

if __name__ == '__main__':
    main()
