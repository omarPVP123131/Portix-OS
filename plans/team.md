# Portix OS — Organización del Equipo

> **Versión:** 1.0 · **Estado:** Activo · **Clasificación:** Interno

---

## ¿Por qué existe este documento?

Portix ya no es solo una idea. Hay un bootloader que arranca, un kernel que carga, un repositorio activo y personas que quieren contribuir. Eso significa que necesitamos estructura real, no solo buenas intenciones.

Este documento existe por una razón simple: **cada quien necesita saber exactamente qué le toca hacer, por qué importa, y qué se espera de él o ella.** No hay ambigüedad. No hay "yo pensé que alguien más lo hacía."

El liderazgo técnico es de Omar. Las decisiones de arquitectura, las del kernel y las del build system no se discuten en paralelo ni se modifican sin aprobación. Eso no es rigidez — es lo que mantiene el proyecto vivo y funcional.

---

## El Equipo

### Visión Rápida

| Integrante | Rol | Disponibilidad Diaria | Área Principal |
| :--- | :--- | :---: | :--- |
| **Omar** | Founder & Lead Systems Engineer | ~18 hrs | Arquitectura, Bootloader, Kernel |
| **Monse** | Junior Systems Developer | ~5 hrs | Rust, componentes del sistema |
| **Ali** | Product, Community & Release Lead | ~6 hrs | Visibilidad, marketing, releases |
| **Melanie** | Technical Documentation Lead | Variable | Portal web, diagramas, guías |
| **Bri** | Research & QA Engineer | Variable | Investigación, testing |
| **Hugo** | QA Tester | ~1–2 hrs | Pruebas en VM, reporte de bugs |
| **Diego** | Documentation Contributor | Ocasional | Correcciones de texto |

---

## Roles en Detalle

### 👑 Omar — Founder & Lead Systems Engineer

Omar es la columna vertebral del proyecto. Tiene el mapa completo de la arquitectura en la cabeza, la mayor disponibilidad del equipo y el historial técnico que ningún otro integrante tiene aún. Sin Omar, el proyecto se detiene. Con Omar delegando bien, el proyecto escala.

**Responsabilidades:**
- Diseño y mantenimiento de la arquitectura general, bootloader y kernel.
- Mantenimiento del build system y definición del roadmap técnico.
- Code review de todas las contribuciones técnicas.
- Decisiones técnicas finales y firma de releases.

**Lo que se espera en los próximos 3 meses:**
Delegar al menos el 20% de las tareas secundarias del sistema a Monse. Si Omar sigue siendo el único que puede tocar el código, el proyecto tiene un punto único de falla.

---

### ⚡ Monse — Junior Systems Developer

Monse tiene el mayor potencial de crecimiento técnico dentro del equipo. Tiene bases de C++, disponibilidad real y actitud para aprender. Su rol ahora mismo no es escribir código de producción — es prepararse para hacerlo bien.

**Responsabilidades:**
- Aprendizaje activo de Rust y la arquitectura de Portix.
- Desarrollo de scripts internos, herramientas auxiliares y componentes modulares bajo supervisión de Omar.
- Ejecución de pruebas técnicas unitarias.

**Lo que se espera:**
Convertirse en co-desarrolladora del kernel. Ese es el objetivo real. Cada semana sin avance en Rust es una semana que el proyecto sigue dependiendo de una sola persona.

---

### 📢 Ali — Product, Community & Release Lead

Ali no escribe código y no necesita hacerlo. Su trabajo es igualmente crítico: si nadie sabe que Portix existe, si los releases no salen limpios, si la comunidad no crece, el proyecto muere en privado aunque el kernel sea perfecto.

**Responsabilidades:**
- Diseño y mantenimiento de la landing page.
- Gestión de redes sociales, comunidad y estrategia de visibilidad.
- Redacción de changelogs y roadmaps públicos.
- Coordinación logística de releases.

**Lo que se espera:**
Transformar el trabajo técnico en algo visible y atractivo. Cada release necesita un comunicado. Cada hito necesita llegar a la audiencia correcta.

---

### 🎨 Melanie — Technical Documentation Lead

La documentación técnica es uno de los problemas más ignorados en proyectos de sistemas operativos open-source. Melanie resuelve eso. Su trabajo determina si un desarrollador externo puede entender Portix en minutos o si se rinde a los 10.

**Responsabilidades:**
- Diseño y maquetación del portal técnico web.
- Creación de diagramas de arquitectura y flujos de datos.
- Guías de contribución y documentación para desarrolladores externos.

**Lo que se espera:**
Que cualquier desarrollador externo pueda entender cómo funciona Portix sin necesitar leer el código fuente. Si eso no está pasando, la documentación no está cumpliendo su función.

> **Nota sobre disponibilidad:** La disponibilidad variable de Melanie es un riesgo real. Si las entregas de documentación se retrasan consistentemente, se revisará el rol.

---

### 🔍 Bri — Research & QA Engineer

Bri está aprendiendo programación desde cero. Eso es válido, y hay trabajo real que puede hacer ahora mismo sin tocar el código core. Su rol libera tiempo de investigación a Omar y establece un estándar básico de calidad.

**Responsabilidades:**
- Investigación de tecnologías, estándares y proyectos competidores.
- Documentación de hallazgos en formato claro y consultable.
- Diseño de planes de prueba y reporte formal de bugs.
- Estudio progresivo de Rust.

**Lo que se espera:**
Entregas consistentes de investigación. Si la disponibilidad es variable y los entregables no llegan, el rol se reevalúa.

---

### 🧪 Hugo — QA Tester

Hugo tiene disponibilidad limitada y un rol acotado. Eso está bien — hay trabajo valioso que hacer en ese espacio. Pero la expectativa es proporcional: poco tiempo disponible significa pocos entregables esperados, y también significa que si el rol queda vacío, el proyecto no se detiene.

**Responsabilidades:**
- Pruebas de compatibilidad en QEMU y VirtualBox.
- Validación de imágenes ISO.
- Reproducción y reporte sistemático de bugs.

---

### ✍️ Diego — Documentation Contributor

Diego colabora de forma intermitente y con baja disponibilidad. Su rol es de soporte no crítico: correcciones de estilo, ortografía y gramática en textos del proyecto cuando hay tiempo y disposición.

**Expectativa real:** Las contribuciones de Diego son bienvenidas pero no planeadas. Si hay entrega, bien. Si no, el proyecto no lo siente.

---

## Cadena de Decisiones

No todo requiere aprobación de Omar, pero lo que toca la arquitectura core sí.

```
OMAR
├── Decisiones de arquitectura, kernel y build system → Solo Omar
├── Code review de contribuciones técnicas → Solo Omar
│
├── MONSE (sistemas auxiliares, bajo supervisión)
│   └── BRI (research, QA, testing)
│       └── HUGO (QA final, validación de ISO)
│
├── ALI (producto, comunidad, releases)
│
└── MELANIE (documentación técnica, portal web)
    └── DIEGO (correcciones ad-hoc)
```

**Regla de oro:** Si una decisión toca el MBR, el linker script, la IDT/GDT, el buddy allocator, las direcciones de memoria o el UEFI loader — requiere aprobación de Omar vía Issue antes de cualquier modificación.

---

## Zonas Protegidas del Código

Los siguientes componentes **no se modifican sin aprobación explícita de Omar:**

| Componente | Razón |
| :--- | :--- |
| MBR y etapa 1 del bootloader | Cualquier cambio rompe el boot |
| Linker script | Afecta direcciones de carga del kernel |
| IDT / GDT | Rompe el manejo de interrupciones |
| Buddy allocator | Corrupción de memoria en cascada |
| Direcciones de memoria fijas | Incompatibilidades con el mapa de carga |
| UEFI Loader | Afecta el dual-boot BIOS/UEFI |

---

## Estado Actual del Proyecto

Portix ha superado la fase de idea abstracta. Esto es lo que ya existe y funciona:

- ✅ Bootloader de dos etapas (`boot.asm` + `stage2.asm`)
- ✅ UEFI EFI Loader con soporte dual-boot
- ✅ Kernel funcional en fase de carga
- ✅ Drivers: ATA, FAT32, VFS, PCI, ACPI
- ✅ Gráficos: framebuffer, VGA con fallback GOP/VBE
- ✅ UI: tabs, terminal, IDE básico
- ✅ Buddy system memory allocator
- ✅ Manejo de interrupciones, IRQ1/IRQ12, PIT
- ✅ Repositorio activo con documentación pública

El siguiente hito es eliminar la dependencia exclusiva en Omar para el desarrollo del sistema.

---

## Expectativas Generales del Equipo

Ser parte del equipo de Portix no es honorífico — es funcional. Estas reglas aplican para todos:

1. **Entrega o comunica.** Si no puedes completar algo, avisa antes, no después.
2. **No modifiques lo que no entiendes.** Si tienes duda, pregunta primero.
3. **Las decisiones técnicas finales son de Omar.** Puedes proponer, argumentar y discutir — la decisión final es suya.
4. **La documentación es código.** Un módulo sin documentar no existe para el equipo externo.
5. **Los roles se evalúan por entregables, no por intención.** Participación sin resultados no sostiene un rol.

---

*Documento interno de Portix OS · No distribuir fuera del equipo*