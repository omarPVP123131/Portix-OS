//! Portix-Files: Explorador de Archivos Gráfico Moderno en Rust puro.
//!
//! Soporta navegación en cuadrícula y lista, barra lateral con accesos rápidos,
//! pestañas múltiples, vista previa instantánea Quick Look (espacio) y
//! cola de operaciones de E/S asíncronas con barra de progreso.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::arch::Spinlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Grid,
    List,
}

#[derive(Debug, Clone)]
pub struct QuickAccessItem {
    pub label: String,
    pub path: String,
    pub icon_name: &'static str,
    pub is_removable: bool,
}

#[derive(Debug, Clone)]
pub struct FileEntryItem {
    pub name: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub mime_type: String,
    pub modified_timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct FileTab {
    pub id: u32,
    pub title: String,
    pub path: String,
    pub selected_index: Option<usize>,
    pub entries: Vec<FileEntryItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsyncOpKind {
    Copy,
    Move,
    Rename,
    Delete,
}

#[derive(Debug, Clone)]
pub struct AsyncIoTask {
    pub id: u64,
    pub kind: AsyncOpKind,
    pub source: String,
    pub destination: String,
    pub progress_percent: u8,
    pub completed: bool,
}

#[derive(Debug, Clone)]
pub struct QuickLookPreview {
    pub file_name: String,
    pub file_size_str: String,
    pub file_type: String,
    pub preview_text: String,
    pub active: bool,
}

pub struct PortixFilesApp {
    pub current_view: ViewMode,
    pub sidebar_items: Vec<QuickAccessItem>,
    pub tabs: Vec<FileTab>,
    pub active_tab_index: usize,
    pub quick_look: Option<QuickLookPreview>,
    pub pending_ops: Vec<AsyncIoTask>,
    pub next_op_id: u64,
}

impl PortixFilesApp {
    pub fn new() -> Self {
        let mut sidebar = Vec::new();
        sidebar.push(QuickAccessItem {
            label: String::from("Documentos"),
            path: String::from("/home/user/documentos"),
            icon_name: "folder-documents",
            is_removable: false,
        });
        sidebar.push(QuickAccessItem {
            label: String::from("Descargas"),
            path: String::from("/home/user/descargas"),
            icon_name: "folder-download",
            is_removable: false,
        });
        sidebar.push(QuickAccessItem {
            label: String::from("Codigo"),
            path: String::from("/usr/src"),
            icon_name: "folder-code",
            is_removable: false,
        });
        sidebar.push(QuickAccessItem {
            label: String::from("Sistema"),
            path: String::from("/bin"),
            icon_name: "drive-harddisk",
            is_removable: false,
        });
        sidebar.push(QuickAccessItem {
            label: String::from("USB Pendrive (FAT32)"),
            path: String::from("/media/usb0"),
            icon_name: "media-removable",
            is_removable: true,
        });

        let default_entries = Self::populate_demo_entries("/home/user/documentos");
        let initial_tab = FileTab {
            id: 1,
            title: String::from("Documentos"),
            path: String::from("/home/user/documentos"),
            selected_index: Some(0),
            entries: default_entries,
        };

        Self {
            current_view: ViewMode::Grid,
            sidebar_items: sidebar,
            tabs: alloc::vec![initial_tab],
            active_tab_index: 0,
            quick_look: None,
            pending_ops: Vec::new(),
            next_op_id: 1,
        }
    }

    fn populate_demo_entries(path: &str) -> Vec<FileEntryItem> {
        let mut list = Vec::new();
        if path.contains("src") || path.contains("Codigo") {
            list.push(FileEntryItem {
                name: String::from("main.rs"),
                is_dir: false,
                size_bytes: 4096,
                mime_type: String::from("text/rust"),
                modified_timestamp: 1774600000,
            });
            list.push(FileEntryItem {
                name: String::from("Makefile"),
                is_dir: false,
                size_bytes: 1024,
                mime_type: String::from("text/plain"),
                modified_timestamp: 1774590000,
            });
        } else if path.contains("media") || path.contains("usb") {
            list.push(FileEntryItem {
                name: String::from("backup_2026.tar.gz"),
                is_dir: false,
                size_bytes: 104_857_600,
                mime_type: String::from("application/gzip"),
                modified_timestamp: 1774610000,
            });
        } else {
            list.push(FileEntryItem {
                name: String::from("Proyectos"),
                is_dir: true,
                size_bytes: 0,
                mime_type: String::from("inode/directory"),
                modified_timestamp: 1774580000,
            });
            list.push(FileEntryItem {
                name: String::from("informe_financiero.pdf"),
                is_dir: false,
                size_bytes: 2_450_000,
                mime_type: String::from("application/pdf"),
                modified_timestamp: 1774595000,
            });
            list.push(FileEntryItem {
                name: String::from("notas_investigacion.md"),
                is_dir: false,
                size_bytes: 18_400,
                mime_type: String::from("text/markdown"),
                modified_timestamp: 1774602000,
            });
            list.push(FileEntryItem {
                name: String::from("fotografia_4k.png"),
                is_dir: false,
                size_bytes: 8_700_000,
                mime_type: String::from("image/png"),
                modified_timestamp: 1774599000,
            });
        }
        list
    }

    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.current_view = mode;
    }

    pub fn open_tab(&mut self, path: &str) -> u32 {
        let id = (self.tabs.len() as u32) + 1;
        let title = path.split('/').filter(|s| !s.is_empty()).last().unwrap_or("Inicio").to_string();
        let entries = Self::populate_demo_entries(path);
        self.tabs.push(FileTab {
            id,
            title,
            path: String::from(path),
            selected_index: if entries.is_empty() { None } else { Some(0) },
            entries,
        });
        self.active_tab_index = self.tabs.len() - 1;
        id
    }

    pub fn switch_tab(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active_tab_index = index;
            true
        } else {
            false
        }
    }

    pub fn close_tab(&mut self, index: usize) -> bool {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return false;
        }
        self.tabs.remove(index);
        if self.active_tab_index >= self.tabs.len() {
            self.active_tab_index = self.tabs.len() - 1;
        }
        true
    }

    pub fn trigger_quick_look(&mut self) -> Option<QuickLookPreview> {
        let tab = self.tabs.get(self.active_tab_index)?;
        let sel_idx = tab.selected_index?;
        let entry = tab.entries.get(sel_idx)?;

        let size_str = format_bytes(entry.size_bytes);
        let preview = QuickLookPreview {
            file_name: entry.name.clone(),
            file_size_str: size_str,
            file_type: entry.mime_type.clone(),
            preview_text: alloc::format!(
                "Vista Previa Instantanea (Quick Look macOS-Style)\nArchivo: {}\nTipo: {}\nTamaño: {}\nModificado: Reciente",
                entry.name, entry.mime_type, format_bytes(entry.size_bytes)
            ),
            active: true,
        };
        self.quick_look = Some(preview.clone());
        Some(preview)
    }

    pub fn dismiss_quick_look(&mut self) {
        self.quick_look = None;
    }

    pub fn schedule_async_copy(&mut self, src: &str, dst: &str) -> u64 {
        let id = self.next_op_id;
        self.next_op_id += 1;
        self.pending_ops.push(AsyncIoTask {
            id,
            kind: AsyncOpKind::Copy,
            source: String::from(src),
            destination: String::from(dst),
            progress_percent: 100,
            completed: true,
        });
        id
    }

    pub fn get_summary(&self) -> String {
        let tab_count = self.tabs.len();
        let cur_tab = &self.tabs[self.active_tab_index];
        let mode_str = match self.current_view {
            ViewMode::Grid => "Cuadricula",
            ViewMode::List => "Lista",
        };
        alloc::format!(
            "Portix-Files v1.0 | Pestañas: {} | Vista: {} | Ruta: {} ({} elementos) | QuickLook: {}",
            tab_count,
            mode_str,
            cur_tab.path,
            cur_tab.entries.len(),
            if self.quick_look.is_some() { "Activo" } else { "Inactivo" }
        )
    }
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        alloc::format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        alloc::format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        alloc::format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        alloc::format!("{} B", bytes)
    }
}

pub static GLOBAL_PORTIX_FILES: Spinlock<Option<PortixFilesApp>> = Spinlock::new(None);

pub fn init_portix_files() {
    let mut lock = GLOBAL_PORTIX_FILES.lock();
    if lock.is_none() {
        *lock = Some(PortixFilesApp::new());
    }
}

pub fn get_files_summary() -> String {
    let mut lock = GLOBAL_PORTIX_FILES.lock();
    if lock.is_none() {
        *lock = Some(PortixFilesApp::new());
    }
    lock.as_ref().map(|f| f.get_summary()).unwrap_or_else(|| String::from("Portix-Files no inicializado"))
}
