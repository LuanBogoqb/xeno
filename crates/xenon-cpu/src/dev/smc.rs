//! SMC mailbox behind the PCI BAR at 0xEA00_1000. Registers are little-endian (the stages use
//! lwbrx/stwbrx), so the bus-side value of "bit 2" is 0x0400_0000. Messages are four words.
//!
//! CPU -> SMC: write 4 to +0x84 (begin), write the four words to +0x80, write 0 to +0x84 (commit).
//! +0x84 reads bit 2 set when the CPU may start a transmission: nothing is waiting in either direction.
//! CB_B's wait loop (bytecode 0x2D1C) branches to its send path when that bit is set, and to its
//! receive path when +0x94 bit 2 is set.
//! SMC -> CPU: poll +0x94 bit 2 (a message is waiting), write 4 to +0x94 (begin), read four words
//! from +0x90, write 0 to +0x94 (done, drops the message).
//! Order taken from the executed bytecode (CB_B 0x2D1C..0x2DC0, trace in research/cb/README.md).
use crate::dev::soc::SocRegs;
use crate::mem::{BusError, Device};
use std::collections::VecDeque;

const TX_DATA: u64 = 0x80;
const TX_CTRL: u64 = 0x84;
const RX_DATA: u64 = 0x90;
const RX_CTRL: u64 = 0x94;
/// +0x14 takes a command (CD 0x5330 writes 0x2E00_0000, plain big-endian stw) and +0x18 bit 6 (0x0200_0000) says it is done.
const CMD_STATUS: u64 = 0x18;
const BIT2_RAW: u64 = 0x0400_0000;

type Msg = [u32; 4];

/// Reply to a command. 0x12 is the version query CB_B sends first; the bytes are a guess (nothing in
/// the dumps records the console's SMC version), kept in one place so they are easy to change.
fn reply(cmd: u8) -> Msg {
    let mut b = [0u8; 16];
    b[0] = cmd;
    if cmd == 0x12 {
        b[1..4].copy_from_slice(&[0x41, 0x02, 0x03]);
    } else {
        eprintln!("smc: unknown command {cmd:#x}, empty reply");
    }
    let w = |i: usize| u32::from_be_bytes(b[4 * i..4 * i + 4].try_into().unwrap());
    [w(0), w(1), w(2), w(3)]
}

pub struct SmcMailbox {
    regs: SocRegs,
    tx_open: bool,
    tx_fifo: Vec<u32>,
    rx_msgs: VecDeque<Msg>,
    rx_cursor: usize,
    trace: bool,
}

impl SmcMailbox {
    /// Starts with one all-zero message waiting: CB_B polls +0x94 before it sends anything, so the SMC
    /// has to have spoken first (content unknown; the code reads one word and moves on).
    pub fn new(base: u64) -> SmcMailbox {
        SmcMailbox {
            regs: SocRegs::named("smc", base),
            tx_open: false,
            tx_fifo: Vec::new(),
            rx_msgs: VecDeque::from([[0; 4]]),
            rx_cursor: 0,
            trace: std::env::var_os("XB_TRACE_MMIO").is_some(),
        }
    }
}

impl Device for SmcMailbox {
    fn read(&mut self, off: u64, size: usize) -> Result<u64, BusError> {
        let v = match off {
            TX_CTRL => Some(if self.tx_open || !self.rx_msgs.is_empty() { 0 } else { BIT2_RAW }),
            RX_CTRL => Some(if self.rx_msgs.is_empty() { 0 } else { BIT2_RAW }),
            CMD_STATUS => Some(0x0200_0000),
            RX_DATA => {
                let w = self.rx_msgs.front().and_then(|m| m.get(self.rx_cursor)).copied().unwrap_or(0);
                self.rx_cursor += 1;
                Some(w as u64)
            }
            _ => None,
        };
        if let (true, Some(v)) = (self.trace, v) {
            eprintln!("smc: read +{off:#x} = {v:#x}");
        }
        match v {
            Some(v) => Ok(v),
            None => self.regs.read(off, size),
        }
    }

    fn write(&mut self, off: u64, size: usize, val: u64) -> Result<(), BusError> {
        if self.trace && matches!(off, TX_DATA | TX_CTRL | RX_CTRL) {
            eprintln!("smc: write +{off:#x} = {val:#x}");
        }
        match off {
            TX_DATA => self.tx_fifo.push(val as u32),
            TX_CTRL if val & BIT2_RAW != 0 => {
                self.tx_open = true;
                self.tx_fifo.clear();
            }
            TX_CTRL => {
                if self.tx_open {
                    self.tx_open = false;
                    let cmd = self.tx_fifo.first().map_or(0, |w| (w >> 24) as u8);
                    self.rx_msgs.push_back(reply(cmd));
                }
            }
            RX_CTRL if val & BIT2_RAW != 0 => self.rx_cursor = 0,
            RX_CTRL => {
                self.rx_msgs.pop_front();
                self.rx_cursor = 0;
            }
            _ => return self.regs.write(off, size, val),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: u64 = 0x0400_0000;

    fn send(d: &mut SmcMailbox, words: [u32; 4]) {
        d.write(TX_CTRL, 4, B).unwrap();
        for w in words {
            d.write(TX_DATA, 4, w as u64).unwrap();
        }
        d.write(TX_CTRL, 4, 0).unwrap();
    }

    fn recv(d: &mut SmcMailbox) -> [u64; 4] {
        d.write(RX_CTRL, 4, B).unwrap();
        let m = [(); 4].map(|_| d.read(RX_DATA, 4).unwrap());
        d.write(RX_CTRL, 4, 0).unwrap();
        m
    }

    #[test]
    fn boots_with_one_message_waiting_then_goes_quiet() {
        let mut d = SmcMailbox::new(0xEA00_1000);
        assert_eq!(d.read(RX_CTRL, 4).unwrap(), B);
        assert_eq!(recv(&mut d), [0; 4]);
        assert_eq!(d.read(RX_CTRL, 4).unwrap(), 0);
    }

    #[test]
    fn tx_ctrl_is_set_only_when_idle_in_both_directions() {
        let mut d = SmcMailbox::new(0xEA00_1000);
        assert_eq!(d.read(TX_CTRL, 4).unwrap(), 0); // the power-on message is still waiting
        recv(&mut d);
        assert_eq!(d.read(TX_CTRL, 4).unwrap(), B);
        d.write(TX_CTRL, 4, B).unwrap();
        assert_eq!(d.read(TX_CTRL, 4).unwrap(), 0); // transmission open
        d.write(TX_CTRL, 4, 0).unwrap();
        assert_eq!(d.read(TX_CTRL, 4).unwrap(), 0); // reply waiting
    }

    #[test]
    fn version_query_is_answered_with_its_own_command_byte_first() {
        let mut d = SmcMailbox::new(0xEA00_1000);
        recv(&mut d);
        send(&mut d, [0x1200_0000, 0, 0, 0]);
        assert_eq!(d.read(RX_CTRL, 4).unwrap(), B);
        let m = recv(&mut d);
        assert_eq!(m[0] >> 24, 0x12);
        assert_eq!(d.read(RX_CTRL, 4).unwrap(), 0);
    }

    #[test]
    fn a_commit_without_a_begin_is_not_a_message() {
        let mut d = SmcMailbox::new(0xEA00_1000);
        recv(&mut d);
        d.write(TX_CTRL, 4, 0).unwrap();
        assert_eq!(d.read(RX_CTRL, 4).unwrap(), 0);
    }

    #[test]
    fn command_status_reports_done() {
        let mut d = SmcMailbox::new(0xEA00_1000);
        d.write(0x14, 4, 0x2E00_0000).unwrap();
        assert_eq!(d.read(0x18, 4).unwrap() & 0x0200_0000, 0x0200_0000);
    }

    #[test]
    fn other_registers_read_back() {
        let mut d = SmcMailbox::new(0xEA00_1000);
        d.write(0x1C, 4, 0xE601_0000).unwrap();
        assert_eq!(d.read(0x1C, 4).unwrap(), 0xE601_0000);
    }
}
