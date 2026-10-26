//! PC-keyed actions the runner applies before fetching: the "no glitch" digest skips (decisions.md 3).
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SetGpr { reg: usize, val: u64 },
    Log(&'static str),
    /// Prints the listed GPRs to stderr (debug tracing, `runcb --log-pc`).
    LogRegs(Vec<usize>),
    /// HLE: continue at `pc` instead (skips code that only talks to hardware we do not model).
    Jump(u64),
}

/// A `SetGpr` that changed the register: `old` is what the emulated code actually computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub pc: u64,
    pub reg: usize,
    pub old: u64,
}

#[derive(Default)]
pub struct Hooks {
    map: HashMap<u64, Vec<Action>>,
    overrides: RefCell<Vec<Override>>,
}

impl Hooks {
    pub fn new() -> Hooks {
        Hooks::default()
    }

    pub fn add(&mut self, pc: u64, a: Action) {
        self.map.entry(pc).or_default().push(a);
    }

    pub fn record(&self, o: Override) {
        self.overrides.borrow_mut().push(o);
    }

    pub fn overrides(&self) -> Vec<Override> {
        self.overrides.borrow().clone()
    }

    pub fn at(&self, pc: u64) -> &[Action] {
        self.map.get(&pc).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_is_an_action() {
        let mut h = Hooks::new();
        h.add(0x100, Action::Jump(0x200));
        assert_eq!(h.at(0x100), &[Action::Jump(0x200)]);
    }

    #[test]
    fn at_returns_actions_for_pc_only() {
        let mut h = Hooks::new();
        h.add(0x100, Action::SetGpr { reg: 3, val: 0 });
        assert_eq!(h.at(0x100).len(), 1);
        assert!(h.at(0x104).is_empty());
    }
}
