//! SuperIO 芯片枚举域（从 superio.rs 拆分，2026-10-12）。
//! ChipKind / Sio / enter_chip / exit_chip / read_reg / kind_for / find_sio。

use super::sio_driver::{inp, outp};

// ── SuperIO 芯片枚举（复刻 fancmd FindSio：多芯片 × 双端口）──

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ChipKind {
    Ite,
    Nuvoton,
    Winbond,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sio {
    pub(crate) addr: u16, // 0x2E / 0x4E
    pub(crate) kind: ChipKind,
    pub(crate) env_base: u16, // LD#4 基址
    pub(crate) id: u16,       // idH<<8 | idL
}

#[cfg(windows)]
pub(crate) fn enter_chip(addr: u16, kind: ChipKind) {
    match kind {
        ChipKind::Ite => {
            outp(addr, 0x87);
            outp(addr, 0x01);
            outp(addr, 0x55);
            outp(addr, if addr == 0x4E { 0xAA } else { 0x55 });
        }
        ChipKind::Nuvoton => {
            outp(addr, 0x87);
            outp(addr, 0x87);
        }
        ChipKind::Winbond => {
            outp(addr, 0x87);
            outp(addr, 0x87);
            if addr == 0x2E {
                outp(addr, 0x87);
            }
        }
    }
}

#[cfg(windows)]
pub(crate) fn exit_chip(addr: u16) {
    outp(addr, 0x02);
    outp(addr + 1, 0x02);
}

#[cfg(windows)]
fn read_reg(addr: u16, reg: u8) -> u8 {
    outp(addr, reg);
    inp(addr + 1)
}

#[cfg(windows)]
pub(crate) fn kind_for(idh: u8) -> ChipKind {
    match idh {
        0x86 | 0x87 | 0x88 | 0x90 => ChipKind::Ite,
        0x85 | 0xA0 | 0xE0 => ChipKind::Nuvoton,
        _ => ChipKind::Ite, // 未知默认 ITE（LHM 逻辑最宽松）
    }
}

#[cfg(windows)]
pub(crate) fn find_sio() -> Option<Sio> {
    const SIO_ADDRS: [u16; 2] = [0x2E, 0x4E];
    for addr in SIO_ADDRS {
        for kind in [ChipKind::Ite, ChipKind::Nuvoton, ChipKind::Winbond] {
            enter_chip(addr, kind);
            let idh = read_reg(addr, 0x20);
            let idl = read_reg(addr, 0x21);
            if idh == 0xFF || idh == 0x00 {
                exit_chip(addr);
                continue;
            }
            // 选 LD#4（环境控制器）再读基址 0x60/0x61
            outp(addr, 0x07);
            outp(addr + 1, 0x04);
            let hi = read_reg(addr, 0x60);
            let lo = read_reg(addr, 0x61);
            exit_chip(addr);
            if hi != 0xFF || lo != 0xFF {
                return Some(Sio {
                    addr,
                    kind: kind_for(idh),
                    env_base: (u16::from(hi) << 8) | u16::from(lo),
                    id: (u16::from(idh) << 8) | u16::from(idl),
                });
            }
        }
    }
    None
}
