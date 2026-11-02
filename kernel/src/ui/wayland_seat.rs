// kernel/src/ui/wayland_seat.rs — Soporte Multi-Seat Nativo Wayland (wl_seat) y Atajos Configurables Tiling
// Implementa el protocolo de múltiples puestos de usuario independientes (Multi-Seat):
// - Asignación de múltiples punteros (ratones) y teclados independientes (seat0, seat1...)
// - Foco de ventana y cursor aislado por puesto de trabajo sin interferencia mutua
// - Atajos de teclado configurables para el gestor de ventanas en mosaico (Sway/i3 Tiling WM)
// - Compatibilidad con el protocolo Wayland wl_seat, wl_keyboard y wl_pointer

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::String;
use crate::arch::Spinlock;

pub const SEAT_CAP_KEYBOARD: u32 = 1 << 0;
pub const SEAT_CAP_POINTER:  u32 = 1 << 1;
pub const SEAT_CAP_TOUCH:    u32 = 1 << 2;

pub const MOD_SUPER: u8 = 1 << 0; // Mod4 / Windows Key
pub const MOD_SHIFT: u8 = 1 << 1;
pub const MOD_CTRL:  u8 = 1 << 2;
pub const MOD_ALT:   u8 = 1 << 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TilingAction {
    SpawnTerminal,
    CloseFocused,
    FocusNext,
    FocusPrev,
    ToggleSplit,
    SwitchWorkspace(u32),
    MoveToWorkspace(u32),
    ToggleFullscreen,
    ReloadConfig,
}

#[derive(Clone, Copy, Debug)]
pub struct SeatKeyBinding {
    pub modifiers: u8,
    pub key: u8, // Código ASCII o scancode
    pub action: TilingAction,
}

#[derive(Clone, Debug)]
pub struct WlSeat {
    pub id: u32,
    pub name: &'static str,
    pub capabilities: u32,
    // Estado del puntero
    pub pointer_x: i32,
    pub pointer_y: i32,
    pub pointer_buttons: u8,
    pub pointer_focused_node: Option<u32>,
    // Estado del teclado
    pub keyboard_focused_node: Option<u32>,
    pub modifiers: u8,
    pub active_workspace: u32,
    pub keybindings: Vec<SeatKeyBinding>,
    pub events_processed: u64,
}

impl WlSeat {
    pub fn new(id: u32, name: &'static str, capabilities: u32) -> Self {
        let mut seat = WlSeat {
            id,
            name,
            capabilities,
            pointer_x: 100 * (id as i32 + 1),
            pointer_y: 100 * (id as i32 + 1),
            pointer_buttons: 0,
            pointer_focused_node: Some(id),
            keyboard_focused_node: Some(id),
            modifiers: 0,
            active_workspace: 1,
            keybindings: Vec::new(),
            events_processed: 0,
        };

        // Configurar atajos estándar Sway/i3 para este seat
        seat.add_default_bindings();
        seat
    }

    pub fn add_default_bindings(&mut self) {
        // Mod4 + Enter: Lanzar Terminal
        self.keybindings.push(SeatKeyBinding {
            modifiers: MOD_SUPER,
            key: b'\n', // Enter
            action: TilingAction::SpawnTerminal,
        });
        // Mod4 + Shift + Q: Cerrar ventana
        self.keybindings.push(SeatKeyBinding {
            modifiers: MOD_SUPER | MOD_SHIFT,
            key: b'Q',
            action: TilingAction::CloseFocused,
        });
        // Mod4 + J: Foco siguiente
        self.keybindings.push(SeatKeyBinding {
            modifiers: MOD_SUPER,
            key: b'j',
            action: TilingAction::FocusNext,
        });
        // Mod4 + K: Foco anterior
        self.keybindings.push(SeatKeyBinding {
            modifiers: MOD_SUPER,
            key: b'k',
            action: TilingAction::FocusPrev,
        });
        // Mod4 + V: Alternar Split V/H
        self.keybindings.push(SeatKeyBinding {
            modifiers: MOD_SUPER,
            key: b'v',
            action: TilingAction::ToggleSplit,
        });
        // Mod4 + 1..4: Cambiar de Workspace
        for ws in 1..=4u32 {
            self.keybindings.push(SeatKeyBinding {
                modifiers: MOD_SUPER,
                key: b'0' + (ws as u8),
                action: TilingAction::SwitchWorkspace(ws),
            });
        }
    }

    /// Evalúa una tecla presionada contra la tabla de atajos de este seat
    pub fn match_binding(&self, key: u8) -> Option<TilingAction> {
        for binding in &self.keybindings {
            if binding.modifiers == self.modifiers && binding.key == key {
                return Some(binding.action);
            }
        }
        None
    }
}

pub struct MultiSeatManager {
    pub seats: Vec<WlSeat>,
    pub next_seat_id: u32,
    pub total_events: u64,
    pub actions_triggered: u64,
}

impl MultiSeatManager {
    pub const fn new() -> Self {
        MultiSeatManager {
            seats: Vec::new(),
            next_seat_id: 0,
            total_events: 0,
            actions_triggered: 0,
        }
    }

    /// Registra un nuevo puesto de usuario independiente (seat)
    pub fn register_seat(&mut self, name: &'static str, caps: u32) -> u32 {
        let id = self.next_seat_id;
        self.next_seat_id += 1;
        self.seats.push(WlSeat::new(id, name, caps));
        id
    }

    /// Actualiza posición del puntero para un seat determinado
    pub fn move_pointer(&mut self, seat_id: u32, dx: i32, dy: i32, max_w: i32, max_h: i32) {
        if let Some(seat) = self.seats.iter_mut().find(|s| s.id == seat_id) {
            seat.pointer_x = (seat.pointer_x + dx).clamp(0, max_w);
            seat.pointer_y = (seat.pointer_y + dy).clamp(0, max_h);
            seat.events_processed += 1;
            self.total_events += 1;
        }
    }

    /// Procesa evento de teclado y ejecuta atajo Tiling si coincide
    pub fn process_key(&mut self, seat_id: u32, key: u8, pressed: bool) -> Option<TilingAction> {
        self.total_events += 1;
        let seat = match self.seats.iter_mut().find(|s| s.id == seat_id) {
            Some(s) => s,
            None => return None,
        };

        seat.events_processed += 1;

        // Actualizar modificadores especiales (Super, Shift, Ctrl, Alt)
        match key {
            0x80 | 0x5B => { // Super / Mod4
                if pressed { seat.modifiers |= MOD_SUPER; } else { seat.modifiers &= !MOD_SUPER; }
                return None;
            },
            0x81 | 0x2A => { // Shift
                if pressed { seat.modifiers |= MOD_SHIFT; } else { seat.modifiers &= !MOD_SHIFT; }
                return None;
            },
            0x82 | 0x1D => { // Ctrl
                if pressed { seat.modifiers |= MOD_CTRL; } else { seat.modifiers &= !MOD_CTRL; }
                return None;
            },
            0x83 | 0x38 => { // Alt
                if pressed { seat.modifiers |= MOD_ALT; } else { seat.modifiers &= !MOD_ALT; }
                return None;
            },
            _ => {}
        }

        if pressed {
            if let Some(action) = seat.match_binding(key) {
                self.actions_triggered += 1;
                // Aplicar la acción a nivel de gestor de ventanas
                self.apply_action(seat_id, action);
                return Some(action);
            }
        }

        None
    }

    /// Aplica una acción de atajo sobre el estado del Tiling Window Manager
    fn apply_action(&mut self, seat_id: u32, action: TilingAction) {
        let mut wm = crate::ui::tiling_wm::TILING_WM.lock();
        match action {
            TilingAction::SpawnTerminal => {
                let id = wm.add_window("Terminal MultiSeat (/bin/sh)");
                if let Some(seat) = self.seats.iter_mut().find(|s| s.id == seat_id) {
                    seat.keyboard_focused_node = Some(id);
                    seat.pointer_focused_node = Some(id);
                }
            },
            TilingAction::ToggleSplit => {
                wm.split_mode = match wm.split_mode {
                    crate::ui::tiling_wm::SplitDirection::Vertical => crate::ui::tiling_wm::SplitDirection::Horizontal,
                    crate::ui::tiling_wm::SplitDirection::Horizontal => crate::ui::tiling_wm::SplitDirection::Vertical,
                };
                wm.recalculate_layout();
            },
            TilingAction::SwitchWorkspace(ws) => {
                wm.active_workspace = ws;
                wm.recalculate_layout();
                if let Some(seat) = self.seats.iter_mut().find(|s| s.id == seat_id) {
                    seat.active_workspace = ws;
                }
            },
            TilingAction::FocusNext => {
                if !wm.nodes.is_empty() {
                    let ws = wm.active_workspace;
                    let nodes_in_ws: Vec<u32> = wm.nodes.iter().filter(|n| n.workspace == ws).map(|n| n.id).collect();
                    if !nodes_in_ws.is_empty() {
                        if let Some(seat) = self.seats.iter_mut().find(|s| s.id == seat_id) {
                            let curr = seat.keyboard_focused_node.unwrap_or(nodes_in_ws[0]);
                            let next_idx = (nodes_in_ws.iter().position(|&id| id == curr).unwrap_or(0) + 1) % nodes_in_ws.len();
                            seat.keyboard_focused_node = Some(nodes_in_ws[next_idx]);
                        }
                    }
                }
            },
            TilingAction::FocusPrev => {
                if !wm.nodes.is_empty() {
                    let ws = wm.active_workspace;
                    let nodes_in_ws: Vec<u32> = wm.nodes.iter().filter(|n| n.workspace == ws).map(|n| n.id).collect();
                    if !nodes_in_ws.is_empty() {
                        if let Some(seat) = self.seats.iter_mut().find(|s| s.id == seat_id) {
                            let curr = seat.keyboard_focused_node.unwrap_or(nodes_in_ws[0]);
                            let pos = nodes_in_ws.iter().position(|&id| id == curr).unwrap_or(0);
                            let prev_idx = if pos == 0 { nodes_in_ws.len() - 1 } else { pos - 1 };
                            seat.keyboard_focused_node = Some(nodes_in_ws[prev_idx]);
                        }
                    }
                }
            },
            _ => {}
        }
    }
}

pub static MULTI_SEAT: Spinlock<MultiSeatManager> = Spinlock::new(MultiSeatManager::new());

pub fn wayland_seat_init() {
    let mut msm = MULTI_SEAT.lock();
    // Registrar puesto primario (seat0)
    msm.register_seat("seat0", SEAT_CAP_KEYBOARD | SEAT_CAP_POINTER);
    // Registrar puesto secundario independiente (seat1)
    msm.register_seat("seat1", SEAT_CAP_KEYBOARD | SEAT_CAP_POINTER);
}

pub fn get_multi_seat_status() -> String {
    let msm = MULTI_SEAT.lock();
    use core::fmt::Write;
    let mut s = String::new();
    let _ = write!(s, "[WAYLAND MULTI-SEAT] Protocolo wl_seat & Gestor Tiling\n");
    let _ = write!(s, "  Puestos activos:        {}\n", msm.seats.len());
    let _ = write!(s, "  Eventos procesados:     {}\n", msm.total_events);
    let _ = write!(s, "  Atajos Sway/i3 lanzados:{}\n", msm.actions_triggered);
    for seat in &msm.seats {
        let caps_str = match seat.capabilities {
            c if (c & SEAT_CAP_KEYBOARD != 0) && (c & SEAT_CAP_POINTER != 0) => "Teclado + Raton",
            c if (c & SEAT_CAP_KEYBOARD != 0) => "Solo Teclado",
            c if (c & SEAT_CAP_POINTER != 0) => "Solo Raton",
            _ => "Touch / Otros",
        };
        let _ = write!(s, "    - [Seat {}] {}: Caps: [{}] | Ptr: ({},{}) | Foco Kbd: #{:?} | Atajos: {}\n",
                       seat.id, seat.name, caps_str, seat.pointer_x, seat.pointer_y,
                       seat.keyboard_focused_node, seat.keybindings.len());
    }
    s
}
