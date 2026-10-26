//! Estado compartilhado entre threads: reservas `lwarx`/`stwcx` e a janela do IIC.
//!
//! Essas duas coisas são as únicas do barramento que dependem de mais de uma
//! thread ao mesmo tempo; o resto do mapa é por endereço.

use super::{Bus, BusError};

/// Janela do controlador de interrupções entre processadores.
const IIC_WINDOW: std::ops::Range<u64> = 0x8000_0200_0005_0000..0x8000_0200_0005_8000;

impl Bus {
    pub(super) fn iic_read(&mut self, addr: u64, size: usize) -> Option<Result<u64, BusError>> {
        if !self.multicore || !IIC_WINDOW.contains(&addr) {
            return None;
        }
        if !matches!(size, 1 | 2 | 4 | 8) || (addr & 7) as usize + size > 8 {
            return Some(Err(BusError::Unmapped { addr, size, write: false }));
        }
        Some(Ok(self.iic.read(addr - IIC_WINDOW.start, size)))
    }

    pub(super) fn iic_write(&mut self, addr: u64, size: usize, val: u64) -> Option<Result<(), BusError>> {
        if !self.multicore || !IIC_WINDOW.contains(&addr) {
            return None;
        }
        if !matches!(size, 1 | 2 | 4 | 8) || (addr & 7) as usize + size > 8 {
            return Some(Err(BusError::Unmapped { addr, size, write: true }));
        }
        self.iic.write(addr - IIC_WINDOW.start, size, val);
        Some(Ok(()))
    }

    /// A store to a reserved line clears the reservation of every thread.
    pub(super) fn release_reservations(&mut self, addr: u64, size: usize) {
        for reservation in &mut self.reservations {
            if let Some(line) = *reservation {
                if addr < line.saturating_add(128) && addr.saturating_add(size as u64) > line {
                    *reservation = None;
                }
            }
        }
    }

    pub(crate) fn load_reserve(&mut self, addr: u64, size: usize) -> Result<u64, BusError> {
        self.reservations[self.active_thread] = None;
        let physical = self.translate_atomic(addr, size, false)?;
        let value = self.read(addr, size)?;
        self.reservations[self.active_thread] = Some(physical & !127);
        Ok(value)
    }

    pub(crate) fn store_conditional(&mut self, addr: u64, size: usize, val: u64) -> Result<bool, BusError> {
        let reservation = self.reservations[self.active_thread].take();
        let physical = self.translate_atomic(addr, size, true)?;
        self.ensure_mapped(physical, size, true)?;
        if reservation != Some(physical & !127) {
            return Ok(false);
        }
        self.write(addr, size, val)?;
        Ok(true)
    }
}
