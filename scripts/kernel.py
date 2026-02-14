#!/usr/bin/env python3
"""
kernel_debug.py - Herramienta avanzada de debugging para el kernel Portix
Analiza logs de QEMU, memoria, registros, y excepciones
"""

import sys
import re
from pathlib import Path
from collections import defaultdict

# Colores ANSI
class Colors:
    RED = '\033[91m'
    GREEN = '\033[92m'
    YELLOW = '\033[93m'
    BLUE = '\033[94m'
    MAGENTA = '\033[95m'
    CYAN = '\033[96m'
    BOLD = '\033[1m'
    RESET = '\033[0m'

EXCEPTION_NAMES = {
    0x00: "Divide by Zero (#DE)",
    0x01: "Debug Exception (#DB)",
    0x02: "Non-Maskable Interrupt",
    0x03: "Breakpoint (#BP)",
    0x04: "Overflow (#OF)",
    0x05: "Bound Range Exceeded (#BR)",
    0x06: "Invalid Opcode (#UD)",
    0x07: "Device Not Available (#NM)",
    0x08: "Double Fault (#DF)",
    0x09: "Coprocessor Segment Overrun",
    0x0A: "Invalid TSS (#TS)",
    0x0B: "Segment Not Present (#NP)",
    0x0C: "Stack-Segment Fault (#SS)",
    0x0D: "General Protection Fault (#GP)",
    0x0E: "Page Fault (#PF)",
    0x10: "x87 FPU Error (#MF)",
    0x11: "Alignment Check (#AC)",
    0x12: "Machine Check (#MC)",
    0x13: "SIMD Floating-Point (#XM)",
}

def parse_qemu_log(log_file):
    """Parse QEMU CPU log"""
    try:
        with open(log_file, 'r', encoding='utf-8', errors='ignore') as f:
            return f.readlines()
    except FileNotFoundError:
        print(f"{Colors.RED}Error: Log file '{log_file}' not found{Colors.RESET}")
        return []

def analyze_exceptions(lines):
    """Analizar excepciones en el log"""
    exceptions = []
    
    for i, line in enumerate(lines):
        if 'check_exception' in line:
            match = re.search(r'old: (0x[0-9a-f]+) new (0x[0-9a-f]+)', line)
            if match:
                old_exc = int(match.group(1), 16) if match.group(1) != '0xffffffff' else None
                new_exc = int(match.group(2), 16)
                
                # Leer la siguiente línea para obtener contexto
                if i + 1 < len(lines):
                    ctx_line = lines[i + 1]
                    match_ctx = re.search(r'v=([0-9a-f]+).*EIP=([0-9a-f]+).*ESP=([0-9a-f]+)', ctx_line)
                    if match_ctx:
                        exceptions.append({
                            'old': old_exc,
                            'new': new_exc,
                            'vector': int(match_ctx.group(1), 16),
                            'eip': match_ctx.group(2),
                            'esp': match_ctx.group(3),
                            'line_num': i
                        })
    
    return exceptions

def analyze_registers(lines):
    """Extraer estado de registros en puntos clave"""
    states = []
    
    for i, line in enumerate(lines):
        if re.match(r'^\s*\d+:', line):
            # Línea con número de instrucción
            regs = {}
            
            # Extraer registros de propósito general
            if i + 1 < len(lines):
                reg_line = lines[i + 1]
                matches = re.findall(r'([A-Z]{2,3})=([0-9a-f]{8})', reg_line)
                for reg, val in matches:
                    regs[reg] = val
            
            # Extraer EIP, ESP
            eip_match = re.search(r'EIP=([0-9a-f]{8})', line)
            esp_match = re.search(r'ESP=([0-9a-f]{8})', line)
            
            if eip_match and esp_match:
                regs['EIP'] = eip_match.group(1)
                regs['ESP'] = esp_match.group(1)
                states.append(regs)
    
    return states

def detect_patterns(exceptions):
    """Detectar patrones en las excepciones"""
    patterns = {
        'triple_fault': False,
        'gp_loop': False,
        'stack_overflow': False,
        'invalid_segment': False,
        'exception_cascade': []
    }
    
    # Detectar cascada de excepciones
    if len(exceptions) > 3:
        patterns['exception_cascade'] = [e['new'] for e in exceptions[-5:]]
        
        # Triple fault: 3+ excepciones en secuencia
        if len(exceptions) >= 3:
            patterns['triple_fault'] = True
        
        # GP loop: múltiples #GP
        gp_count = sum(1 for e in exceptions if e['new'] == 0x0D)
        if gp_count >= 5:
            patterns['gp_loop'] = True
    
    # Detectar stack overflow
    if len(exceptions) >= 2:
        first_esp = int(exceptions[0]['esp'], 16)
        last_esp = int(exceptions[-1]['esp'], 16)
        stack_used = first_esp - last_esp
        
        if stack_used > 200:  # Más de 200 bytes usados en excepciones
            patterns['stack_overflow'] = True
    
    return patterns

def print_exception_summary(exceptions):
    """Imprimir resumen de excepciones"""
    print(f"\n{Colors.BOLD}{Colors.CYAN}╔═══════════════════════════════════════════════════════════════╗")
    print(f"║               ANÁLISIS DE EXCEPCIONES                        ║")
    print(f"╚═══════════════════════════════════════════════════════════════╝{Colors.RESET}\n")
    
    if not exceptions:
        print(f"{Colors.GREEN}✓ No se detectaron excepciones{Colors.RESET}")
        return
    
    print(f"{Colors.RED}Total de excepciones: {len(exceptions)}{Colors.RESET}\n")
    
    # Contar tipos
    exc_counts = defaultdict(int)
    for exc in exceptions:
        exc_counts[exc['new']] += 1
    
    print(f"{Colors.BOLD}Tipos de excepciones:{Colors.RESET}")
    for exc_num, count in sorted(exc_counts.items(), key=lambda x: -x[1]):
        exc_name = EXCEPTION_NAMES.get(exc_num, f"Unknown (0x{exc_num:02x})")
        color = Colors.RED if count > 3 else Colors.YELLOW
        print(f"  {color}• {exc_name}: {count} veces{Colors.RESET}")
    
    # Mostrar últimas 5 excepciones
    print(f"\n{Colors.BOLD}Últimas 5 excepciones:{Colors.RESET}")
    for exc in exceptions[-5:]:
        exc_name = EXCEPTION_NAMES.get(exc['new'], f"0x{exc['new']:02x}")
        print(f"  {Colors.YELLOW}→{Colors.RESET} {exc_name}")
        print(f"     EIP=0x{exc['eip']} ESP=0x{exc['esp']}")

def print_diagnosis(patterns, exceptions):
    """Imprimir diagnóstico de problemas"""
    print(f"\n{Colors.BOLD}{Colors.MAGENTA}╔═══════════════════════════════════════════════════════════════╗")
    print(f"║                    DIAGNÓSTICO                                ║")
    print(f"╚═══════════════════════════════════════════════════════════════╝{Colors.RESET}\n")
    
    has_problems = False
    
    if patterns['triple_fault']:
        has_problems = True
        print(f"{Colors.RED}{Colors.BOLD}✗ TRIPLE FAULT DETECTADO{Colors.RESET}")
        print(f"  {Colors.RED}El sistema está en un loop de excepciones{Colors.RESET}")
        print(f"  Esto causa que QEMU reinicie la CPU\n")
        
        print(f"{Colors.YELLOW}  Causas comunes:{Colors.RESET}")
        print(f"  1. IDT mal configurada o corrupta")
        print(f"  2. Handlers de excepciones que fallan")
        print(f"  3. Falta de TSS válido para Double Fault")
        print(f"  4. Segmentos inválidos (ES, DS, etc.)\n")
    
    if patterns['gp_loop']:
        has_problems = True
        print(f"{Colors.RED}{Colors.BOLD}✗ LOOP DE GENERAL PROTECTION FAULT{Colors.RESET}")
        print(f"  {Colors.RED}El handler de #GP está causando más #GP{Colors.RESET}\n")
        
        print(f"{Colors.YELLOW}  Soluciones:{Colors.RESET}")
        print(f"  1. Verificar que la IDT apunta a código válido")
        print(f"  2. Verificar que los handlers preservan registros")
        print(f"  3. Asegurar que ES=DS=SS=0x10 en modo protegido\n")
    
    if patterns['stack_overflow']:
        has_problems = True
        print(f"{Colors.RED}{Colors.BOLD}✗ POSIBLE STACK OVERFLOW{Colors.RESET}")
        print(f"  {Colors.RED}El stack se está llenando rápidamente{Colors.RESET}\n")
        
        print(f"{Colors.YELLOW}  Acciones:{Colors.RESET}")
        print(f"  1. Aumentar el tamaño del stack")
        print(f"  2. Reducir uso de variables locales grandes")
        print(f"  3. Verificar recursión infinita\n")
    
    if patterns['exception_cascade']:
        has_problems = True
        print(f"{Colors.YELLOW}⚠ CASCADA DE EXCEPCIONES{Colors.RESET}")
        cascade = " → ".join([EXCEPTION_NAMES.get(e, f"0x{e:02x}") for e in patterns['exception_cascade']])
        print(f"  {cascade}\n")
    
    if not has_problems:
        print(f"{Colors.GREEN}✓ No se detectaron problemas críticos{Colors.RESET}")
    
    # Análisis de la dirección problemática
    if exceptions:
        problem_eip = exceptions[-1]['eip']
        print(f"\n{Colors.BOLD}Dirección del código problemático:{Colors.RESET}")
        print(f"  EIP = 0x{problem_eip}")
        print(f"\n  Para ver el código assembly en esa dirección:")
        print(f"  {Colors.CYAN}objdump -D build/kernel.elf | grep -A 10 '{problem_eip}'{Colors.RESET}")

def print_recommendations():
    """Imprimir recomendaciones"""
    print(f"\n{Colors.BOLD}{Colors.GREEN}╔═══════════════════════════════════════════════════════════════╗")
    print(f"║                  RECOMENDACIONES                              ║")
    print(f"╚═══════════════════════════════════════════════════════════════╝{Colors.RESET}\n")
    
    print(f"{Colors.BOLD}1. Arreglar segmentos en stage2.asm:{Colors.RESET}")
    print(f"   Asegurar que ES=DS=FS=GS=SS=0x10 en modo protegido\n")
    
    print(f"{Colors.BOLD}2. Agregar TSS válido:{Colors.RESET}")
    print(f"   Necesario para manejar Double Faults correctamente\n")
    
    print(f"{Colors.BOLD}3. Revisar handlers de IDT:{Colors.RESET}")
    print(f"   - Deben preservar TODOS los registros (pusha/popa)")
    print(f"   - Usar CLI al inicio del handler")
    print(f"   - Terminar con IRET correctamente\n")
    
    print(f"{Colors.BOLD}4. Verificar el kernel:{Colors.RESET}")
    print(f"   nm build/kernel.elf | grep _start")
    print(f"   objdump -D build/kernel.bin | head -n 30\n")

def main():
    if len(sys.argv) < 2:
        print(f"Uso: {sys.argv[0]} <qemu_log.txt>")
        print(f"\nPara generar el log de QEMU:")
        print(f"  qemu-system-i386 -drive file=portix.img,format=raw -d int,cpu_reset -D qemu_log.txt")
        sys.exit(1)
    
    log_file = sys.argv[1]
    
    print(f"{Colors.BOLD}{Colors.BLUE}")
    print("╔══════════════════════════════════════════════════════════════════╗")
    print("║             PORTIX KERNEL DEBUGGER v1.0                          ║")
    print("╚══════════════════════════════════════════════════════════════════╝")
    print(Colors.RESET)
    
    # Leer y parsear log
    lines = parse_qemu_log(log_file)
    
    if not lines:
        sys.exit(1)
    
    print(f"Analizando {len(lines)} líneas del log...\n")
    
    # Analizar excepciones
    exceptions = analyze_exceptions(lines)
    
    # Detectar patrones
    patterns = detect_patterns(exceptions)
    
    # Imprimir resultados
    print_exception_summary(exceptions)
    print_diagnosis(patterns, exceptions)
    print_recommendations()
    
    print(f"\n{Colors.BOLD}Log analizado: {log_file}{Colors.RESET}\n")

if __name__ == '__main__':
    main()