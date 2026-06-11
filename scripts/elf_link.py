#!/usr/bin/env python3
"""elf_link.py — Link ELF64 relocatable objects into an ELF64 executable.

Usage:
    python scripts/elf_link.py input.o -o output.elf [--vaddr=0x20000000] [--stack=0x10000]
"""

import struct
import sys
import os

from elftools.elf.elffile import ELFFile
from elftools.elf.constants import SH_TYPE, SHT_TYPE, PT_TYPE, P_TYPE
from elftools.elf.enums import ENUM_RELOC_TYPE_x64
from elftools.elf.relocation import RelocationSection

VADDR = 0x20000000
STACK_SIZE = 0x10000

def align_up(x, a):
    return (x + a - 1) & ~(a - 1)

class ElfLinker:
    def __init__(self):
        self.sections = []  # (name, type, data, flags, addr)
        self.symbols = {}   # name -> (section_idx, value, size)
        self.relocs = []    # (offset, type, sym_name, addend, section_idx)

    def add_object(self, path):
        with open(path, 'rb') as f:
            elf = ELFFile(f)
            if elf.elfclass != 64:
                raise ValueError(f"Expected 64-bit ELF, got class {elf.elfclass}")
            symtab = None
            for sec in elf.iter_sections():
                if sec.name == '.symtab':
                    symtab = sec
            for sec in elf.iter_sections():
                if sec.sh_type in ('SHT_RELA', 'SHT_REL'):
                    self._add_relocs(sec, elf)
            if symtab:
                self._add_symbols(symtab)
            for sec in elf.iter_sections():
                name = sec.name
                if name in ('', '.symtab', '.strtab', '.shstrtab', '.rela.text',
                            '.rela.rodata', '.rela.data', '.rela.bss',
                            '.rel.text', '.rel.rodata', '.rel.data', '.rel.bss',
                            '.rela.pdata', '.pdata', '.xdata',
                            '.note.GNU-stack', '.comment', '.debug_*'):
                    continue
                if sec.sh_type == 'SHT_NOBITS':
                    data = b''
                else:
                    data = sec.data() if sec.data_size > 0 else b''
                if name == '.text':
                    flags = 6  # AX
                elif name == '.rodata':
                    flags = 4  # A
                elif name == '.data':
                    flags = 3  # WA
                elif name == '.bss':
                    flags = 3  # WA
                else:
                    flags = 0
                self.sections.append((name, sec.sh_type, data, flags, 0))

    def _add_relocs(self, sec, elf):
        if not isinstance(sec, RelocationSection):
            try:
                from elftools.elf.relocation import RelocationSection as RS
                if not isinstance(sec, RS):
                    return
            except ImportError:
                return
        symtab = elf.get_section(sec.symbol_table_index)
        for rel in sec.iter_relocations():
            sym = symtab.get_symbol(rel.symbol_index)
            sym_name = sym.name if sym else ''
            self.relocs.append((rel.entry.r_offset, rel.entry.r_info_type,
                                sym_name, rel.entry.r_addend, sec.sh_info))

    def _add_symbols(self, symtab):
        for sym in symtab.iter_symbols():
            name = sym.name
            if not name:
                continue
            if name in self.symbols:
                continue
            self.symbols[name] = (sym.entry.st_shndx, sym.entry.st_value,
                                  sym.entry.st_size)

    def link(self, output_path):
        text_data = b''
        rodata_data = b''
        data_data = b''
        bss_size = 0
        for name, stype, data, flags, addr in self.sections:
            if name == '.text':
                text_data = data
            elif name == '.rodata':
                rodata_data = data
            elif name == '.data':
                data_data = data
            elif name == '.bss':
                bss_size = data_size if hasattr(self, 'data_size') else 0

        text_vaddr = VADDR
        text_size = len(text_data)
        text_paddr = 64 + 56  # after ELF header + program header

        if rodata_data:
            rodata_vaddr = align_up(text_vaddr + text_size, 16)
            rodata_paddr = text_paddr + text_size
        else:
            rodata_vaddr = text_vaddr + text_size
            rodata_paddr = text_paddr + text_size
        rodata_size = len(rodata_data)

        data_vaddr = align_up(rodata_vaddr + rodata_size, 16)
        data_paddr = text_paddr + text_size + rodata_size
        data_size = len(data_data)

        total_file_size = data_paddr + data_size
        total_mem_size = data_vaddr + data_size + bss_size

        entry = text_vaddr
        if '_start' in self.symbols:
            entry = text_vaddr + self.symbols['_start'][1]

        elf = bytearray(64 + 56)
        # ELF header
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
        struct.pack_into('<Q', ph, 16, VADDR)
        struct.pack_into('<Q', ph, 24, VADDR)
        struct.pack_into('<Q', ph, 32, total_file_size)
        struct.pack_into('<Q', ph, 36, total_mem_size)
        struct.pack_into('<Q', ph, 40, 0x1000)

        payload = text_data + rodata_data + data_data
        result = bytes(elf[:64]) + bytes(ph) + payload

        os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
        with open(output_path, 'wb') as f:
            f.write(result)
        print(f"[OK] {output_path} — {len(result)} bytes, entry=0x{entry:x}")

def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument('input', nargs='+')
    ap.add_argument('-o', '--output', required=True)
    ap.add_argument('--vaddr', type=lambda x: int(x, 0), default=VADDR)
    ap.add_argument('--stack', type=lambda x: int(x, 0), default=STACK_SIZE)
    args = ap.parse_args()

    global VADDR, STACK_SIZE
    VADDR = args.vaddr
    STACK_SIZE = args.stack

    linker = Elflinker()
    for path in args.input:
        linker.add_object(path)
    linker.link(args.output)

if __name__ == '__main__':
    main()
