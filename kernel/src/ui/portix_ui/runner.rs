// kernel/src/ui/portix_ui/runner.rs — Lanzador Rápido Global Portix-Runner (Estilo Spotlight)
// Fases 701 - 750: Atajo Super + Espacio, Búsqueda Fuzzy, Evaluador Matemático y Acciones Rápidas.

use alloc::vec::Vec;
use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::arch::spinlock::Spinlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerResultType {
    Application,
    DocumentFile,
    MathCalculation,
    SystemAction,
}

#[derive(Debug, Clone)]
pub struct RunnerItem {
    pub title: String,
    pub description: String,
    pub item_type: RunnerResultType,
    pub command: String,
}

pub struct PortixRunner {
    pub is_visible: AtomicBool,
    pub current_query: String,
    pub catalog: Vec<RunnerItem>,
}

impl PortixRunner {
    pub fn new() -> Self {
        let mut catalog = Vec::new();
        // Aplicaciones principales
        catalog.push(RunnerItem {
            title: String::from("Terminal GPU (Portix-Term)"),
            description: String::from("Emulador de terminal acelerado por Vulkan a 240 FPS"),
            item_type: RunnerResultType::Application,
            command: String::from("/bin/sh"),
        });
        catalog.push(RunnerItem {
            title: String::from("Explorador de Archivos (Portix-Files)"),
            description: String::from("Gestor de archivos VFS multi-pestaña"),
            item_type: RunnerResultType::Application,
            command: String::from("files"),
        });
        catalog.push(RunnerItem {
            title: String::from("Calculadora (Portix-Calc)"),
            description: String::from("Calculadora matemática de alta precisión"),
            item_type: RunnerResultType::Application,
            command: String::from("/bin/calc"),
        });
        catalog.push(RunnerItem {
            title: String::from("Monitor del Sistema (Portix-Monitor)"),
            description: String::from("Monitor en tiempo real a 60 FPS de CPU, RAM y GPU"),
            item_type: RunnerResultType::Application,
            command: String::from("monitor"),
        });
        catalog.push(RunnerItem {
            title: String::from("Editor de Código (Portix-Code)"),
            description: String::from("Entorno de desarrollo nativo con LSP"),
            item_type: RunnerResultType::Application,
            command: String::from("/bin/pvi"),
        });
        catalog.push(RunnerItem {
            title: String::from("Tienda de Software (Portix-Store)"),
            description: String::from("Gestor de paquetes nativos y Flatpak en 1 clic"),
            item_type: RunnerResultType::Application,
            command: String::from("/bin/pkg"),
        });
        // Acciones del sistema
        catalog.push(RunnerItem {
            title: String::from("Bloquear Pantalla"),
            description: String::from("Activa Portix-DM y bloquea la sesión"),
            item_type: RunnerResultType::SystemAction,
            command: String::from("lock"),
        });
        catalog.push(RunnerItem {
            title: String::from("Suspender Equipo"),
            description: String::from("Entrar en suspensión profunda ACPI S3"),
            item_type: RunnerResultType::SystemAction,
            command: String::from("sleep"),
        });

        Self {
            is_visible: AtomicBool::new(false),
            current_query: String::new(),
            catalog,
        }
    }

    pub fn toggle_visibility(&self) -> bool {
        let cur = self.is_visible.load(Ordering::Relaxed);
        self.is_visible.store(!cur, Ordering::SeqCst);
        !cur
    }

    /// Busca coincidencias fuzzy en el catálogo o evalúa expresiones matemáticas
    pub fn search(&self, query: &str) -> Vec<RunnerItem> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.catalog.iter().take(5).cloned().collect();
        }

        // Detección simple de cálculo aritmético (si contiene operadores o dígitos)
        if q.chars().all(|c| c.is_ascii_digit() || c == '+' || c == '-' || c == '*' || c == '/' || c == ' ') {
            let result_str = evaluate_simple_math(&q);
            return alloc::vec![RunnerItem {
                title: alloc::format!("= {}", result_str),
                description: String::from("Resultado matemático inmediato"),
                item_type: RunnerResultType::MathCalculation,
                command: result_str,
            }];
        }

        let mut matches = Vec::new();
        for item in &self.catalog {
            if item.title.to_lowercase().contains(&q) || item.command.to_lowercase().contains(&q) {
                matches.push(item.clone());
            }
        }
        matches
    }
}

fn evaluate_simple_math(expr: &str) -> String {
    // Parser elemental para sumas o multiplicaciones básicas
    if let Some(pos) = expr.find('+') {
        let a = expr[..pos].trim().parse::<i64>().unwrap_or(0);
        let b = expr[pos+1..].trim().parse::<i64>().unwrap_or(0);
        return alloc::format!("{}", a + b);
    }
    if let Some(pos) = expr.find('*') {
        let a = expr[..pos].trim().parse::<i64>().unwrap_or(0);
        let b = expr[pos+1..].trim().parse::<i64>().unwrap_or(0);
        return alloc::format!("{}", a * b);
    }
    String::from(expr)
}

pub static PORTIX_RUNNER: Spinlock<Option<PortixRunner>> = Spinlock::new(None);

pub fn init_runner() {
    let mut lock = PORTIX_RUNNER.lock();
    if lock.is_none() {
        *lock = Some(PortixRunner::new());
    }
}

pub fn get_runner_summary() -> String {
    let lock = PORTIX_RUNNER.lock();
    match &*lock {
        Some(runner) => {
            let vis = if runner.is_visible.load(Ordering::Relaxed) { "Visible" } else { "Oculto" };
            let total = runner.catalog.len();
            alloc::format!(
                "Portix-Runner: Lanzador Global Spotlight ({}) | Atajo: Super+Espacio | Catálogo Indexado: {} acciones",
                vis, total
            )
        }
        None => String::from("Portix-Runner: No inicializado"),
    }
}
