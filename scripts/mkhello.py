#!/usr/bin/env python3
"""Generate a minimal ELF64 executable for a persistent ring-3 shell.

The shell loops forever: read 1 byte from stdin, echo it to stdout.
"""

import struct, sys, os

VADDR = 0x20000000

# ── Helpers ─────────────────────────────────────────────────────────────────
def emit(code, b):
    code.extend(b)

def mov_eax(code, v):
    emit(code, b'\xb8' + struct.pack('<I', v))

def mov_edi(code, v):
    emit(code, b'\xbf' + struct.pack('<I', v))

def mov_esi(code, v):
    emit(code, b'\xbe' + struct.pack('<I', v))

def mov_edx(code, v):
    emit(code, b'\xba' + struct.pack('<I', v))

def int80(code):
    emit(code, b'\xcd\x80')

# ── Code ────────────────────────────────────────────────────────────────────
code = bytearray()

# Labels and relocation entries
labels = {}
relocs = []  # (offset_of_rel8, target_label)

# Write 'OK\n' to stdout to confirm shell is alive
emit(code, b'\xb0\x4f')      # mov al, 'O'
emit(code, b'\x50')           # push rax
emit(code, b'\x48\x89\xe6')   # mov rsi, rsp
mov_eax(code, 1)
mov_edi(code, 1)
mov_edx(code, 1)
int80(code)
emit(code, b'\x58')           # pop rax
emit(code, b'\xb0\x4b')      # mov al, 'K'
emit(code, b'\x50')
emit(code, b'\x48\x89\xe6')
mov_eax(code, 1)
mov_edi(code, 1)
mov_edx(code, 1)
int80(code)
emit(code, b'\x58')
emit(code, b'\xb0\x0a')      # mov al, '\n'
emit(code, b'\x50')
emit(code, b'\x48\x89\xe6')
mov_eax(code, 1)
mov_edi(code, 1)
mov_edx(code, 1)
int80(code)
emit(code, b'\x58')

# loop: sub rsp, 16
labels['loop'] = len(code)
emit(code, b'\x48\x83\xec\x10')

# .read: SYS_READ(0, rsp, 1)
labels['read'] = len(code)
mov_eax(code, 5)
mov_edi(code, 0)
emit(code, b'\x48\x89\xe6')
mov_edx(code, 1)
int80(code)

# test rax, rax
emit(code, b'\x48\x85\xc0')

# jnz .echo  (2-byte placeholder)
relocs.append((len(code) + 1, 'echo'))  # rel8 byte at offset+1
emit(code, b'\x75\x00')

# .yield: SYS_YIELD
labels['yield'] = len(code)
mov_eax(code, 3)
int80(code)

# jmp .read  (2-byte placeholder)
relocs.append((len(code) + 1, 'read'))
emit(code, b'\xeb\x00')

# .echo: SYS_WRITE(1, rsp, 1)
labels['echo'] = len(code)
mov_eax(code, 1)
mov_edi(code, 1)
emit(code, b'\x48\x89\xe6')
mov_edx(code, 1)
int80(code)

# add rsp, 16
emit(code, b'\x48\x83\xc4\x10')

# jmp loop  (2-byte placeholder)
relocs.append((len(code) + 1, 'loop'))
emit(code, b'\xeb\x00')

# ── Fix relocations ─────────────────────────────────────────────────────────
for rel8_offset, label in relocs:
    target = labels[label]
    insn_end = rel8_offset + 1  # rel8 is at offset, instruction spans (offset-1, offset)
    disp = target - insn_end
    code[rel8_offset] = disp & 0xff

# ── ELF header ──────────────────────────────────────────────────────────────
code_offset = 64 + 56  # ELF header + program header
file_size = code_offset + len(code)

elf = bytearray(64)
elf[0:4] = b'\x7fELF'
elf[4] = 2; elf[5] = 1; elf[6] = 1; elf[7] = 0
struct.pack_into('<H', elf, 16, 2)
struct.pack_into('<H', elf, 18, 0x3E)
struct.pack_into('<I', elf, 20, 1)
struct.pack_into('<Q', elf, 24, VADDR + code_offset)
struct.pack_into('<Q', elf, 32, 64)
struct.pack_into('<Q', elf, 40, 0)
struct.pack_into('<I', elf, 48, 0)
struct.pack_into('<H', elf, 52, 64)
struct.pack_into('<H', elf, 54, 56)
struct.pack_into('<H', elf, 56, 1)
struct.pack_into('<H', elf, 58, 0)
struct.pack_into('<H', elf, 60, 0)
struct.pack_into('<H', elf, 62, 0)

ph = bytearray(56)
struct.pack_into('<I', ph, 0, 1)
struct.pack_into('<I', ph, 4, 7)
struct.pack_into('<Q', ph, 8, 0)
struct.pack_into('<Q', ph, 16, VADDR)
struct.pack_into('<Q', ph, 24, VADDR)
struct.pack_into('<Q', ph, 32, file_size)
struct.pack_into('<Q', ph, 40, file_size)
struct.pack_into('<Q', ph, 48, 0x1000)

result = bytes(elf) + bytes(ph) + bytes(code)

out_path = sys.argv[1] if len(sys.argv) > 1 else 'build/hello.elf'
os.makedirs(os.path.dirname(out_path) or '.', exist_ok=True)
with open(out_path, 'wb') as f:
    f.write(result)

print(f"[OK]    hello.elf — {len(result)} bytes, entry=0x{VADDR + code_offset:x}, persistent shell")
