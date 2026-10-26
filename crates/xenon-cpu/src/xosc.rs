//! Experimental XOSC (Xbox Online Security Challenge) response model.
//!
//! The real XOSC is a hypervisor challenge-response protocol tied to the
//! console's CPU key, fuse data and an online nonce.  This module models the
//! *shape* of that path: a challenge buffer is transformed into a response
//! using a key-derived HMAC-like construction.  It is NOT a valid Live
//! response and must not be presented as one.
use crate::security::SecurityEngine;

#[derive(Clone, Copy, Debug)]
pub struct XoscChallenge {
    pub nonce: [u8; 16],
    pub flags: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct XoscResponse {
    pub status: u32,
    pub digest: [u8; 32],
}

pub struct XoscEngine {
    cpu_key: u64,
    security: SecurityEngine,
}

impl XoscEngine {
    pub fn new(cpu_key: u64) -> Self {
        Self { cpu_key, security: SecurityEngine::new(cpu_key) }
    }

    pub fn security(&mut self) -> &mut SecurityEngine { &mut self.security }

    /// Produce a deterministic response for a challenge.  Real XOSC includes
    /// fuse hashes, HV state and console-specific data; those are not modelled.
    pub fn respond(&self, challenge: &XoscChallenge) -> XoscResponse {
        let mut buf = [0u8; 64];
        buf[..16].copy_from_slice(&challenge.nonce);
        buf[16..20].copy_from_slice(&challenge.flags.to_be_bytes());
        buf[20..28].copy_from_slice(&self.cpu_key.to_be_bytes());
        // Mix with master key.
        let mut digest = [0u8; 32];
        let mut h: u64 = 0xCBF29CE484222325;
        for &b in &buf {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001B3);
        }
        // Expand to 32 bytes deterministically.
        for i in 0..4usize {
            let mut x = h ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15);
            for j in 0..8 {
                x ^= x >> 33;
                x = x.wrapping_mul(0xFF51AFD7ED558CCD);
                digest[i * 8 + j] = (x >> (j * 8)) as u8;
            }
        }
        XoscResponse { status: 0, digest }
    }

    pub fn verify(&self, challenge: &XoscChallenge, response: &XoscResponse) -> bool {
        self.respond(challenge).digest == response.digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_is_deterministic_and_key_dependent() {
        let a = XoscEngine::new(0x111);
        let b = XoscEngine::new(0x222);
        let c = XoscChallenge { nonce: [1;16], flags: 3 };
        let ra = a.respond(&c);
        let rb = b.respond(&c);
        assert_ne!(ra.digest, rb.digest);
        assert!(a.verify(&c, &ra));
        assert!(!a.verify(&c, &rb));
    }
    #[test]
    fn nonce_changes_response() {
        let e = XoscEngine::new(0x111);
        let r1 = e.respond(&XoscChallenge { nonce: [1;16], flags: 0 });
        let r2 = e.respond(&XoscChallenge { nonce: [2;16], flags: 0 });
        assert_ne!(r1.digest, r2.digest);
    }
}
