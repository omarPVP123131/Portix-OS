//! Estado final al recibir una señal durante el cierre de un grupo.

/// Sólo un SIGKILL interno de un líder marcado para cierre diferido preserva
/// el estado ya decidido para el grupo. Una señal externa normal, o la salida
/// aislada de un hilo, mantiene el comportamiento de señal fatal habitual.
pub(super) fn estado_por_senal(
    sig: u32,
    salir_solo: bool,
    es_hilo: bool,
    lider_salio: bool,
    codigo_guardado: i32,
    senal_guardada: u8,
) -> (i32, u8) {
    if sig == 9 && salir_solo && !es_hilo && lider_salio {
        (codigo_guardado, senal_guardada)
    } else {
        (128 + sig as i32, (sig & 0x7f) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::estado_por_senal;

    #[test]
    fn salida_grupo_cero_no_se_convierte_en_sigkill() {
        assert_eq!(estado_por_senal(9, true, false, true, 0, 0), (0, 0));
    }
    #[test]
    fn salida_grupo_no_cero_se_preserva() {
        assert_eq!(estado_por_senal(9, true, false, true, 42, 0), (42, 0));
    }
    #[test]
    fn senal_fatal_del_grupo_se_preserva() {
        assert_eq!(estado_por_senal(9, true, false, true, 139, 11), (139, 11));
    }
    #[test]
    fn sigkill_externo_sigue_siendo_senal_nueve() {
        assert_eq!(estado_por_senal(9, false, false, true, 42, 0), (137, 9));
    }
    #[test]
    fn hilo_solo_no_hereda_estado_del_lider() {
        assert_eq!(estado_por_senal(9, true, true, true, 42, 0), (137, 9));
    }
    #[test]
    fn lider_no_marcado_como_salido_no_preserva_codigo() {
        assert_eq!(estado_por_senal(9, true, false, false, 42, 0), (137, 9));
    }
    #[test]
    fn otra_senal_no_se_confunde_con_kill_interno() {
        assert_eq!(estado_por_senal(11, true, false, true, 0, 0), (139, 11));
    }
}
