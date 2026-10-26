//! Memória do sistema.
//!
//! O barramento (`bus`) cuida do mapa de regiões, aliases e acesso a RAM e
//! dispositivos. As demais peças ficam separadas por responsabilidade:
//!
//! - `tlb`: tradução de software do CB/CD e janelas de cache travado;
//! - `walk`: passeio pela tabela de páginas hasheada (SDR1);
//! - `cipher`: visões cifradas das regiões 1 e 3 pelo security engine;
//! - `shared`: reservas entre threads e janela do IIC.

mod bus;
mod cipher;
mod shared;
mod tlb;
mod walk;

pub use bus::{Bus, BusError, Device};

#[cfg(test)]
mod tests;
