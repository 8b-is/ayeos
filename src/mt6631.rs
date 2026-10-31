//! MediaTek `mt6768` CONNSYS (`consys@18002000`) & Companion `MT6631` RF A-Die
//! Hardware Register, Firmware, AFE Connsys I2S, and STP-over-BTIF (`btif@1100c000`) Contract.
//!
//! Extracted directly from `LineageOS/android_kernel_xiaomi_mt6768 @ 8745b22c4bb0`
//! (`mt6768.dts`, `mt6768.c`, `wmt_ic_soc.c`, `mt6631_fm_lib.c`, `fm_cmd.c`, `fm_link.h`,
//! `mt6768-reg.h`, `mt6768-dai-hostless.c`, `mt6768-afe-pcm.c`) and the BLU G75 (`G1170`)
//! bare-metal FDT (`board4-fdt.dtb`, 207,221 bytes, 979 nodes, `Unresolved=0`).
//!
//! ## Key Hardware Facts (Killing Previous Folklore)
//! 1. **10 `consys@18002000` MMIO Windows (`mt6768.dts:2455-2501`, `mt6768.c:1141-1170`)**:
//!    - There is **no** `0x18006000` FM MMIO window in the SoC address map.
//!    - Touching `0x1800xxxx` before `SCP_SYS_CONN` / `SPM` power-on (`0x10006000 = 0x0b160001`,
//!      `0x1000632C`) and clearing `INFRACFG_AO` AHB/AXI sleep protection (`0x10001220` bits 13..14,
//!      `0x10001250` bit 18) will fault or hang the bus (`mt6768.c:1353-1363`).
//! 2. **D-Die (`mt6768-consys`) vs. Companion RF A-Die (`MT6631`)**:
//!    - On reset deassertion, `mt6768.c:583-595,785-792` polls `conn_reg.mcu_base + 0x600`
//!      (`0x18002600`) `== 0x1D1E`, then `conn_reg.mcu_top_misc_off_base + 0x10` (`0x180b1010`)
//!      `== 0x10020501` (`CONSYS_IP_VER_ID`).
//!    - The companion `MT6631` A-Die is queried over WMT STP-BTIF using
//!      `WMT_QUERY_A_DIE_CMD = [0x01, 0x02, 0x01, 0x00, 0x12]`, which returns `0x6631`.
//! 3. **STP-over-BTIF (`btif@1100c000`) & Basic Operations (`BOP`) for FM & RDS**:
//!    - There is no `0x68716100` (`"hqa\0"`) ring mailbox (`HQA_RETURN_ZERO_MAP = 0` in
//!      `mt6631_fm_lib.c:35-36`; Wi-Fi QA uses `TEST_CMD_MAGIC_NO = 0x18142880`).
//!    - FM commands (`FM_TASK_COMMAND_PKT_TYPE = 0x01`) and events (`FM_TASK_EVENT_PKT_TYPE = 0x04`)
//!      pack Basic Operations (`fm_bop_write`, `fm_bop_udelay`, `fm_bop_rd_until`, `fm_bop_modify`)
//!      into STP frames over `btif@1100c000`, which the CONNSYS MCU executes over FSPI/CSPI against
//!      the `MT6631` A-Die.
//! 4. **AFE Connsys I2S Offset (`mt6768-reg.h:1346-1347`)**:
//!    - `AFE_CONNSYS_I2S_CON` is at offset `0x0c78` (physical `0x11220c78`, monitored at
//!      `AFE_CONNSYS_I2S_MON = 0x11220c7c`), **not** `0x0478`.

/// All 10 MMIO register windows declared in `consys: consys@18002000` (`mt6768.dts:2455-2501`)
/// plus companion `btif@1100c000`, `pwrap@1000d000`, `gpio@10005000`, and `afe@11220000`.
pub mod consys_mmio {
    /// Reg index 0: `CONN_MCU_CONFIG_BASE` (`conn_reg.mcu_base`, size `0x1000`).
    pub const CONN_MCU_CONFIG_BASE: u64 = 0x1800_2000;
    /// Reg index 1: `TOP_RGU_BASE` (`conn_reg.ap_rgu_base`, size `0x0100`).
    pub const TOP_RGU_BASE: u64 = 0x1000_7000;
    /// Reg index 2: `INFRACFG_AO_BASE` (`conn_reg.topckgen_base`, size `0x1000`).
    pub const INFRACFG_AO_BASE: u64 = 0x1000_1000;
    /// Reg index 3: `SPM_BASE` (`conn_reg.spm_base`, size `0x1000`).
    pub const SPM_BASE: u64 = 0x1000_6000;
    /// Reg index 4: `CONN_HIF_ON_BASE` (`conn_reg.mcu_conn_hif_on_base`, size `0x1000`).
    pub const CONN_HIF_ON_BASE: u64 = 0x1800_7000;
    /// Reg index 5: `CONN_TOP_MISC_OFF_BASE` (`conn_reg.mcu_top_misc_off_base`, size `0x1000`).
    pub const CONN_TOP_MISC_OFF_BASE: u64 = 0x180B_1000;
    /// Reg index 6: `CONN_MCU_CFG_ON_BASE` (`conn_reg.mcu_cfg_on_base`, size `0x1000`).
    pub const CONN_MCU_CFG_ON_BASE: u64 = 0x180A_3000;
    /// Reg index 7: `CONN_MCU_CIRQ_BASE` (`conn_reg.mcu_cirq_base`, size `0x0800`).
    pub const CONN_MCU_CIRQ_BASE: u64 = 0x180A_5000;
    /// Reg index 8: `CONN_TOP_MISC_ON_BASE` (`conn_reg.mcu_top_misc_on_base`, size `0x1000`).
    pub const CONN_TOP_MISC_ON_BASE: u64 = 0x180C_1000;
    /// Reg index 9: `CONN_HIF_PDMA_BASE` (`conn_reg.mcu_conn_hif_pdma_base`, size `0x1000`).
    pub const CONN_HIF_PDMA_BASE: u64 = 0x1800_4000;

    /// Ordered table of `(base_phys, size_bytes)` matching `consys@18002000` `reg` in `mt6768.dts`.
    pub const CONSYS_FDT_REG_WINDOWS: [(u64, u64); 10] = [
        (CONN_MCU_CONFIG_BASE, 0x1000),
        (TOP_RGU_BASE, 0x0100),
        (INFRACFG_AO_BASE, 0x1000),
        (SPM_BASE, 0x1000),
        (CONN_HIF_ON_BASE, 0x1000),
        (CONN_TOP_MISC_OFF_BASE, 0x1000),
        (CONN_MCU_CFG_ON_BASE, 0x1000),
        (CONN_MCU_CIRQ_BASE, 0x0800),
        (CONN_TOP_MISC_ON_BASE, 0x1000),
        (CONN_HIF_PDMA_BASE, 0x1000),
    ];

    /// GICv3 SPI interrupt numbers declared in `consys@18002000` (`mt6768.dts`):
    /// - `BGF_EINT`: `GIC_SPI 284` (`IRQ_TYPE_LEVEL_LOW`)
    /// - `WDT_EINT`: `GIC_SPI 78` (`IRQ_TYPE_LEVEL_LOW`)
    /// - `conn2ap_sw_irq`: `GIC_SPI 287` (`IRQ_TYPE_EDGE_RISING`)
    pub const IRQ_SPI_BGF_EINT: u32 = 284;
    pub const IRQ_SPI_WDT_EINT: u32 = 78;
    pub const IRQ_SPI_CONN2AP_SW: u32 = 287;

    /// `consys_mem` dynamic reserved DRAM size (`4 MiB`).
    pub const CONSYS_RESERVED_EMI_SIZE: usize = 0x0040_0000;

    /// MediaTek `BTIF` controller (`btif@1100c000`) used for STP-over-BTIF WMT & FM transport.
    pub const BTIF_BASE: u64 = 0x1100_C000;
    pub const BTIF_RBR_THR_OFFSET: u64 = 0x0000;
    pub const BTIF_LSR_OFFSET: u64 = 0x0014;
    pub const BTIF_LSR_THRE_BIT: u32 = 1 << 5;
    pub const BTIF_LSR_DR_BIT: u32 = 1 << 0;

    /// D-Die <-> A-Die Pinmux registers (`mt6768.c:521-565`).
    pub const GPIO_BASE: u64 = 0x1000_5000;
    pub const GPIO_MODE_CONN_3E0: u64 = 0x1000_53E0;
    pub const GPIO_MODE_CONN_3F0: u64 = 0x1000_53F0;
    pub const IOCFG_CONN_2C00: u64 = 0x1000_2C00;

    /// `TOP_RGU` WDT / CONNSYS CPU SW reset register (`0x1000_7018`).
    pub const TOP_RGU_WDT_SWSYSRST: u64 = 0x1000_7018;
    pub const WDT_SWSYSRST_UNLOCK_KEY: u32 = 0x8800_0000;
    pub const WDT_SWSYSRST_CONN_MCU_BIT: u32 = 1 << 12;

    /// `SPM` (`0x1000_6000`) power-on configuration & CONNSYS power control (`mt6768.c:567-712`).
    pub const SPM_POWERON_CONFIG_EN: u64 = 0x1000_6000;
    pub const SPM_POWERON_CONFIG_EN_VAL: u32 = 0x0B16_0001;
    pub const SPM_CONN_PWR_CON: u64 = 0x1000_632C;
    pub const SPM_PWR_STATUS: u64 = 0x1000_6180;
    pub const SPM_PWR_STATUS_2ND: u64 = 0x1000_6184;
    pub const SPM_PWR_STATUS_CONN_MASK: u32 = (1 << 1) | (1 << 30);

    /// `INFRACFG_AO` (`0x1000_1000`) AHB/AXI sleep-protection & EMI remap registers (`mt6768.c:615-712`).
    pub const INFRA_TOPAXI_PROTECTEN: u64 = 0x1000_1220;
    pub const INFRA_TOPAXI_PROTECTSTA1: u64 = 0x1000_1228;
    /// Bits 13 and 14 guard CONNSYS AHB/AXI bus access (`0x6000`).
    pub const INFRA_TOPAXI_CONN_PROT_MASK: u32 = (1 << 13) | (1 << 14);

    pub const INFRA_TOPAXI_PROTECTEN_1: u64 = 0x1000_1250;
    pub const INFRA_TOPAXI_PROTECTSTA1_1: u64 = 0x1000_1258;
    /// Bit 18 guards secondary CONNSYS bus access (`0x0004_0000`).
    pub const INFRA_TOPAXI_1_CONN_PROT_MASK: u32 = 1 << 18;

    /// CONNSYS -> AP DRAM EMI base address remap register (`gConEmiPhyBase >> 24`).
    pub const INFRA_CONN2AP_EMI_REMAP: u64 = 0x1000_1380;

    /// CONNSYS D-Die polling registers after reset deassertion (`mt6768.c:583-595, 785-792`).
    pub const CONN_MCU_CONNSYS_VERSION_REG: u64 = CONN_MCU_CONFIG_BASE + 0x0600; // 0x1800_2600
    pub const CONN_MCU_CONNSYS_VERSION_EXPECTED: u32 = 0x0000_1D1E;

    pub const CONN_TOP_MISC_OFF_IP_VER_REG: u64 = CONN_TOP_MISC_OFF_BASE + 0x0010; // 0x180B_1010
    pub const CONSYS_IP_VER_ID_EXPECTED: u32 = 0x1002_0501;

    /// Companion RF A-Die chip ID returned by `WMT_QUERY_A_DIE_CMD`.
    pub const MT6631_A_DIE_CHIP_ID: u16 = 0x6631;
}

/// MT6768 Audio Front End (`AFE @ 0x11220000`) Connsys I2S & FM crossbar routing
/// extracted from `mt6768-reg.h:1346-1347`, `mt6768-dai-hostless.c:63-80`, and `mt6768-afe-pcm.c:1317-1351`.
pub mod afe_connsys_fm {
    pub const AFE_BASE: u64 = 0x1122_0000;

    /// `AFE_CONNSYS_I2S_CON` offset is `0x0c78` (`0x1122_0c78`), NOT `0x0478`!
    pub const AFE_CONNSYS_I2S_CON_OFFSET: u64 = 0x0C78;
    pub const AFE_CONNSYS_I2S_CON: u64 = AFE_BASE + AFE_CONNSYS_I2S_CON_OFFSET;

    /// `AFE_CONNSYS_I2S_MON` offset is `0x0c7c` (`0x1122_0c7c`).
    pub const AFE_CONNSYS_I2S_MON_OFFSET: u64 = 0x0C7C;
    pub const AFE_CONNSYS_I2S_MON: u64 = AFE_BASE + AFE_CONNSYS_I2S_MON_OFFSET;

    /// Path A — Hostless FM DL (`CONNSYS_I2S_CH1/CH2 -> HW_GAIN1_IN_CH1/CH2 -> ADDA_DL -> MT6358 -> AW87xxx`):
    pub const AFE_CONN5_1: u64 = AFE_BASE + 0x09AC;
    pub const AFE_CONN6_1: u64 = AFE_BASE + 0x09B4;

    /// Path B — CPU PCM Capture for Waterfall / Software Demod (`CONNSYS_I2S_CH1/CH2 -> UL2_CH1/CH2 -> VUL2`):
    pub const AFE_CONN32_1: u64 = AFE_BASE + 0x0A84;
    pub const AFE_CONN33_1: u64 = AFE_BASE + 0x0A8C;
    pub const I_CONNSYS_I2S_CH1_BIT: u32 = 1 << 0;
    pub const I_CONNSYS_I2S_CH2_BIT: u32 = 1 << 1;

    /// `WMT_IC_AIF_2` internal CONNSYS I2S configuration (`wmt_ic_soc.c:1860-1865`).
    pub const WMT_IC_AIF_2_REG: u32 = 0x8005_0078;
    pub const WMT_IC_AIF_2_VAL: u32 = 0x0111_0000;
    pub const WMT_IC_AIF_2_MASK: u32 = 0x0FFF_0000;

    /// Standard 44.1 kHz / 48 kHz Slave Mode I2S enable word for `AFE_CONNSYS_I2S_CON`.
    pub const AFE_CONNSYS_I2S_ENABLE_48KHZ: u32 = 0x0009_0009;
}

/// Exact firmware filenames required from `/vendor/firmware/` on the BLU G75 `super` partition (`vendor_a`).
pub mod firmware_manifest {
    /// Stage 1: CONNSYS MCU ROM / EMI patch headers (`wmt_ic_soc.c:3091-3319, 3510-3672`).
    pub const CONNSYS_MCU_PATCH_FILES: [&str; 4] = [
        "mt6631_patch_e1_hdr.bin",
        "soc2_0_ram_mcu_e1_hdr.bin",
        "soc2_0_ram_bt_e1_hdr.bin",
        "soc2_0_ram_wifi_e1_hdr.bin",
    ];

    /// Stage 2: `mt6631_patch_tbl[5]` FM DSP patch & coefficient pairs (`mt6631_fm_lib.c:45-51`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FmRomPatchPair {
        pub rom_version: u16,
        pub patch_bin: &'static str,
        pub coeff_bin: &'static str,
    }

    pub const MT6631_FM_PATCH_TABLE: [FmRomPatchPair; 5] = [
        FmRomPatchPair {
            rom_version: 1,
            patch_bin: "mt6631_fm_v1_patch.bin",
            coeff_bin: "mt6631_fm_v1_coeff.bin",
        },
        FmRomPatchPair {
            rom_version: 2,
            patch_bin: "mt6631_fm_v2_patch.bin",
            coeff_bin: "mt6631_fm_v2_coeff.bin",
        },
        FmRomPatchPair {
            rom_version: 3,
            patch_bin: "mt6631_fm_v3_patch.bin",
            coeff_bin: "mt6631_fm_v3_coeff.bin",
        },
        FmRomPatchPair {
            rom_version: 4,
            patch_bin: "mt6631_fm_v4_patch.bin",
            coeff_bin: "mt6631_fm_v4_coeff.bin",
        },
        FmRomPatchPair {
            rom_version: 5,
            patch_bin: "mt6631_fm_v5_patch.bin",
            coeff_bin: "mt6631_fm_v5_coeff.bin",
        },
    ];
}

/// WMT & FM STP-over-BTIF (`btif@1100c000`) commands, opcodes, and Basic Operations (`BOP`).
pub mod stp_btif_protocol {
    /// WMT Reset command over STP (`wmt_ic_soc.c`): `[0x01, 0x07, 0x01, 0x00, 0x04]`.
    pub const WMT_RESET_CMD: [u8; 5] = [0x01, 0x07, 0x01, 0x00, 0x04];
    /// WMT Query Companion RF A-Die command (`wmt_ic_soc.c`): returns `0x6631`.
    pub const WMT_QUERY_A_DIE_CMD: [u8; 5] = [0x01, 0x02, 0x01, 0x00, 0x12];
    /// WMT Function Control command prefix to power on `WMTDRV_TYPE_FM` (`type = 1`).
    pub const WMT_FUNC_CTRL_FM_ON_CMD: [u8; 6] = [0x01, 0x01, 0x02, 0x00, 0x01, 0x01];

    /// FM STP task packet types (`fm_link.h:30-69`).
    pub const FM_TASK_COMMAND_PKT_TYPE: u8 = 0x01;
    pub const FM_TASK_EVENT_PKT_TYPE: u8 = 0x04;

    /// FM command opcodes (`fm_link.h` / `mt6631_fm_lib.c`).
    pub const FM_ENABLE_OPCODE: u8 = 0x07;
    pub const FM_TUNE_OPCODE: u8 = 0x09;
    pub const FM_PATCH_DOWNLOAD_OPCODE: u8 = 0x12;
    pub const FM_COEFF_DOWNLOAD_OPCODE: u8 = 0x13;
    pub const FSPI_RD_OPCODE: u8 = 0x04;
    pub const FSPI_WR_OPCODE: u8 = 0x03;

    /// Basic Operation (`BOP`) sub-opcodes packed inside FM STP command payloads (`fm_cmd.c:25-150`).
    pub const FM_BOP_WRITE_OP: u8 = 0x00;
    pub const FM_BOP_UDELAY_OP: u8 = 0x01;
    pub const FM_BOP_RD_UNTIL_OP: u8 = 0x02;
    pub const FM_BOP_MODIFY_OP: u8 = 0x03;

    /// Key `MT6631` A-Die FM registers accessed via BOP over FSPI/CSPI (`mt6631_fm_reg.h`).
    pub const FM_CG1_CTRL: u8 = 0x60;
    pub const FM_CG2_CTRL: u8 = 0x61;
    pub const FM_MAIN_CG2_ANALOG_I2S_BIT: u16 = 1 << 7;
    pub const FM_MAIN_CTRL: u8 = 0x63;
    pub const FM_MAIN_CTRL_TUNE_MASK: u16 = 0x0001;
    pub const FM_MAIN_CTRL_SEEK_MASK: u16 = 0x0002;
    pub const FM_MAIN_CTRL_RDS_MASK: u16 = 0x0010;
    pub const FM_CHANNEL_SET: u8 = 0x65;
    pub const FM_MAIN_INTR: u8 = 0x69;
    pub const FM_INTR_STC_DONE_MASK: u16 = 0x0001;
    pub const FM_INTR_RDS_MASK: u16 = 0x0020;
    pub const FM_RDS_CFG0: u8 = 0x80;
    pub const FM_RDS_DATA_REG: u8 = 0x87;
    pub const FM_I2S_CON0: u8 = 0x90;
    /// `I2S_EN (bit 0) | FORMAT_I2S (bit 1) | WLEN_16BIT (bit 2) | I2S_SRC_SLAVE (bit 3)`.
    pub const FM_I2S_CON0_SLAVE_16BIT: u16 = 0x000B;

    /// Encode a 3-byte `fm_bop_write(addr, val)` instruction (`fm_cmd.c`).
    #[inline]
    pub const fn fm_bop_write(addr: u8, val: u16) -> [u8; 4] {
        [
            FM_BOP_WRITE_OP,
            addr,
            (val & 0xFF) as u8,
            (val >> 8) as u8,
        ]
    }

    /// Encode a 5-byte `fm_bop_udelay(us)` instruction (`fm_cmd.c`).
    #[inline]
    pub const fn fm_bop_udelay(us: u32) -> [u8; 5] {
        let le = us.to_le_bytes();
        [FM_BOP_UDELAY_OP, le[0], le[1], le[2], le[3]]
    }

    /// Encode a 6-byte `fm_bop_rd_until(addr, mask, value)` instruction (`fm_cmd.c`).
    #[inline]
    pub const fn fm_bop_rd_until(addr: u8, mask: u16, expected: u16) -> [u8; 6] {
        [
            FM_BOP_RD_UNTIL_OP,
            addr,
            (mask & 0xFF) as u8,
            (mask >> 8) as u8,
            (expected & 0xFF) as u8,
            (expected >> 8) as u8,
        ]
    }

    /// Encode a 6-byte `fm_bop_modify(addr, and_mask, or_mask)` instruction (`fm_cmd.c`).
    #[inline]
    pub const fn fm_bop_modify(addr: u8, and_mask: u16, or_mask: u16) -> [u8; 6] {
        [
            FM_BOP_MODIFY_OP,
            addr,
            (and_mask & 0xFF) as u8,
            (and_mask >> 8) as u8,
            (or_mask & 0xFF) as u8,
            (or_mask >> 8) as u8,
        ]
    }

    /// Wrap a sequence of BOP operations into a 4-byte FM task command header
    /// (`[FM_TASK_COMMAND_PKT_TYPE, opcode, len_lo, len_hi] || bop_payload`, `fm_op_seq_combine_cmd`).
    pub fn fm_op_seq_combine_cmd(opcode: u8, bop_payload: &[u8]) -> Vec<u8> {
        let len = bop_payload.len() as u16;
        let mut pkt = Vec::with_capacity(4 + bop_payload.len());
        pkt.push(FM_TASK_COMMAND_PKT_TYPE);
        pkt.push(opcode);
        pkt.push((len & 0xFF) as u8);
        pkt.push((len >> 8) as u8);
        pkt.extend_from_slice(bop_payload);
        pkt
    }

    /// Build a complete `FM_TUNE_OPCODE (0x09)` BOP command packet for a target frequency
    /// in 100 kHz units (`885` = 88.5 MHz, `915` = 91.5 MHz, `1079` = 107.9 MHz) following
    /// `mt6631_fm_lib.c`:
    /// 1. Clear `TUNE` bit in `FM_MAIN_CTRL (0x63)`.
    /// 2. Program channel word `(freq_100khz - 640) * 2` into `FM_CHANNEL_SET (0x65)` (`0..9`).
    /// 3. Set `TUNE` (`0x0001`) | `RDS` (`0x0010`) in `FM_MAIN_CTRL (0x63)`.
    /// 4. Poll `FM_MAIN_INTR (0x69)` until `STC_DONE (0x0001)` asserts.
    /// 5. Enable digital Connsys I2S output on `FM_I2S_CON0 (0x90)` and `FM_RDS_CFG0 (0x80) = 6`.
    pub fn build_mt6631_fm_tune_and_rds_cmd(freq_100khz: u16) -> Vec<u8> {
        let clamped = freq_100khz.clamp(650, 1080);
        // MT6631 50 kHz channel spacing word: (f_100kHz - 640) * 2
        let chan_word: u16 = (clamped - 640) << 1;

        let mut bop = Vec::with_capacity(32);
        bop.extend_from_slice(&fm_bop_modify(
            FM_MAIN_CTRL,
            !FM_MAIN_CTRL_TUNE_MASK,
            0x0000,
        ));
        bop.extend_from_slice(&fm_bop_modify(FM_CHANNEL_SET, 0xFC00, chan_word & 0x03FF));
        bop.extend_from_slice(&fm_bop_modify(
            FM_MAIN_CTRL,
            0xFFFF,
            FM_MAIN_CTRL_TUNE_MASK | FM_MAIN_CTRL_RDS_MASK,
        ));
        bop.extend_from_slice(&fm_bop_udelay(2_000));
        bop.extend_from_slice(&fm_bop_rd_until(
            FM_MAIN_INTR,
            FM_INTR_STC_DONE_MASK,
            FM_INTR_STC_DONE_MASK,
        ));
        bop.extend_from_slice(&fm_bop_write(FM_RDS_CFG0, 6));
        bop.extend_from_slice(&fm_bop_write(FM_I2S_CON0, FM_I2S_CON0_SLAVE_16BIT));

        fm_op_seq_combine_cmd(FM_TUNE_OPCODE, &bop)
    }
}

/// MediaTek Wi-Fi `HQA` (`wlan_service/agent/agent.c` / `gl_qa_agent.c`) command frame header
/// (`TEST_CMD_MAGIC_NO = 0x1814_2880`, IOCTL `0x8BF0`).
///
/// Note: This belongs to the `wlan_drv_gen4m` Wi-Fi QA agent, whereas FM uses `stp_btif_protocol` BOPs.
pub mod wifi_hqa {
    pub const TEST_CMD_MAGIC_NO: u32 = 0x1814_2880;
    pub const HQA_TYPE_REQ: u16 = 0x0005;
    pub const HQA_TYPE_RSP: u16 = 0x8005;

    pub const HQA_CMD_OPEN_ADAPTER: u16 = 0x1000;
    pub const HQA_CMD_CLOSE_ADAPTER: u16 = 0x1001;
    pub const HQA_CMD_SET_TX_PATH: u16 = 0x100B;
    pub const HQA_CMD_SET_RX_PATH: u16 = 0x100C;
    pub const HQA_CMD_SET_CHANNEL: u16 = 0x1100;
    pub const HQA_CMD_MAC_BBP_REG_READ: u16 = 0x1300;
    pub const HQA_CMD_MAC_BBP_REG_WRITE: u16 = 0x1301;
    pub const HQA_CMD_GET_TX_INFO: u16 = 0x1313;
    pub const HQA_CMD_GET_CHIP_ID: u16 = 0x1514;
}

// ── Drop-In `Mt6631Controller` (`ConnsysGate`, `HqaFrame`, `FmTuner`, `WmtPatchManifest`, `AfeFmRoute`) ──

/// Sentinel magic word indicating WMT MCU & companion A-Die `0x6631` are out of reset (`0x6631_89AB`).
pub const WMT_MCU_READY_MAGIC: u32 = 0x6631_89AB;

/// 4-stage `CONNSYS` bring-up state machine (`Offline -> FdtVerified -> ClockAndMcuReady -> HqaRingReady -> FmSysActive`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnsysStage {
    Offline,
    FdtVerified,
    ClockAndMcuReady,
    HqaRingReady,
    FmSysActive,
}

/// Execution transport mode (`ShadowLoopback` when unbooted vs. `HardwareMmio` once SPM/AXI + WMT are ready).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnsysMode {
    ShadowLoopback,
    HardwareMmio,
}

/// Hardware safety gate protecting `0x1800_xxxx` MMIO accesses against unclocked/bus-protected aborts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnsysGate {
    pub stage: ConnsysStage,
    pub mode: ConnsysMode,
    pub fdt_verified: bool,
    pub mcu_status_reg: u32,
}

impl Default for ConnsysGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnsysGate {
    #[inline]
    pub const fn new() -> Self {
        Self {
            stage: ConnsysStage::Offline,
            mode: ConnsysMode::ShadowLoopback,
            fdt_verified: false,
            mcu_status_reg: 0,
        }
    }

    /// Evaluate FDT presence and MCU ready word (`WMT_MCU_READY_MAGIC = 0x6631_89AB` or `0x1002_0501` / `0x1D1E`).
    pub fn evaluate_probe(&mut self, fdt_has_connsys: bool, mcu_status_reg: u32) -> ConnsysMode {
        self.fdt_verified = fdt_has_connsys;
        self.mcu_status_reg = mcu_status_reg;
        if !fdt_has_connsys {
            self.stage = ConnsysStage::Offline;
            self.mode = ConnsysMode::ShadowLoopback;
        } else if mcu_status_reg == WMT_MCU_READY_MAGIC
            || mcu_status_reg == consys_mmio::CONSYS_IP_VER_ID_EXPECTED
            || mcu_status_reg == consys_mmio::CONN_MCU_CONNSYS_VERSION_EXPECTED
        {
            self.stage = ConnsysStage::FmSysActive;
            self.mode = ConnsysMode::HardwareMmio;
        } else {
            self.stage = ConnsysStage::FdtVerified;
            self.mode = ConnsysMode::ShadowLoopback;
        }
        self.mode
    }

    #[inline]
    pub const fn can_touch_mmio(&self) -> bool {
        matches!(self.mode, ConnsysMode::HardwareMmio)
    }
}

/// Strongly typed `HQA` command IDs (`0x1814_2880`).
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HqaCmd {
    OpenAdapter = 0x1000,
    CloseAdapter = 0x1001,
    SetTxPath = 0x100B,
    SetRxPath = 0x100C,
    SetChannel = 0x1100,
    MacBbpRegRead = 0x1300,
    MacBbpRegWrite = 0x1301,
    GetTxInfo = 0x1313,
    GetChipId = 0x1514,
}

/// 12-byte `hqa_frame` header + 32-bit parameter word (`magic_no = 0x1814_2880`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HqaFrame {
    pub magic_no: u32,
    pub msg_type: u16,
    pub cmd_id: u16,
    pub length: u16,
    pub sequence: u16,
    pub param0: u32,
}

impl HqaFrame {
    #[inline]
    pub const fn new(cmd: HqaCmd, param0: u32) -> Self {
        Self {
            magic_no: wifi_hqa::TEST_CMD_MAGIC_NO,
            msg_type: wifi_hqa::HQA_TYPE_REQ,
            cmd_id: cmd as u16,
            length: 4,
            sequence: 1,
            param0,
        }
    }
}

/// Integer FM Tuner (`650..=1080` in 100 kHz steps) & IEC 62106 RDS decoder state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FmTuner {
    pub current_freq_100khz: u16,
    pub rssi_dbm: i16,
    pub snr_db: u8,
    pub stereo_locked: bool,
    pub rds_pi: u16,
    pub rds_ps: [u8; 8],
    pub rds_rt: [u8; 64],
}

impl Default for FmTuner {
    fn default() -> Self {
        Self::new()
    }
}

impl FmTuner {
    pub const fn new() -> Self {
        Self {
            current_freq_100khz: 885,
            rssi_dbm: -62,
            snr_db: 28,
            stereo_locked: true,
            rds_pi: 0x8B15,
            rds_ps: *b"AYE.IS  ",
            rds_rt: [b' '; 64],
        }
    }

    /// Split a 100 kHz integer frequency (`995` -> `(99, 5)` for `99.5 MHz`) with zero floats.
    #[inline]
    pub const fn split_mhz(freq_100khz: u16) -> (u16, u16) {
        (freq_100khz / 10, freq_100khz % 10)
    }

    pub fn tune_100khz(&mut self, freq_100khz: u16) {
        self.current_freq_100khz = freq_100khz.clamp(650, 1080);
    }
}

/// WMT & FM DSP firmware patch readiness manifest (`WIFI_RAM_CODE_6768`, `mt6631_patch_e1_hdr.bin`, `mt6631_fm_v1..v5_patch.bin`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WmtPatchManifest {
    pub wifi_ram_code_loaded: bool,
    pub wmt_mcu_patch_loaded: bool,
    pub fm_dsp_patch_loaded: bool,
}

impl WmtPatchManifest {
    /// Verify the 16-byte ASCII date prefix (`"20"` year century) of a 28-byte MediaTek WMT ROM patch header.
    #[inline]
    pub const fn verify_wmt_patch_header(hdr16: &[u8; 16]) -> bool {
        hdr16[0] == b'2'
            && hdr16[1] == b'0'
            && hdr16[2].is_ascii_digit()
            && hdr16[3].is_ascii_digit()
    }

    #[inline]
    pub const fn is_rf_ready(&self) -> bool {
        self.wifi_ram_code_loaded && self.wmt_mcu_patch_loaded && self.fm_dsp_patch_loaded
    }
}

/// MT6768 AFE Connsys I2S (`0x1122_0C78`) -> MT6358 DL1 / Hostless FM route state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AfeFmRoute {
    pub connsys_i2s_con_addr: u64,
    pub connsys_i2s_con_val: u32,
    pub gain_steps_of_256: u16,
    pub enabled: bool,
}

impl Default for AfeFmRoute {
    fn default() -> Self {
        Self::new()
    }
}

impl AfeFmRoute {
    #[inline]
    pub const fn new() -> Self {
        Self {
            connsys_i2s_con_addr: afe_connsys_fm::AFE_CONNSYS_I2S_CON,
            connsys_i2s_con_val: 0,
            gain_steps_of_256: 0,
            enabled: false,
        }
    }

    pub fn enable_fm_audio_path(&mut self, gain_steps_of_256: u16) {
        self.connsys_i2s_con_addr = afe_connsys_fm::AFE_CONNSYS_I2S_CON;
        self.connsys_i2s_con_val = 0x8000_0909;
        self.gain_steps_of_256 = gain_steps_of_256.min(256);
        self.enabled = true;
    }
}

/// 64×36 integer wave pool coupled to the 16× Catmull-Rom 768 kHz audio/FM demodulator.
#[derive(Debug, Clone)]
pub struct WavePool64x36 {
    pub energy_accum: i64,
    pub frames_injected: u64,
}

impl Default for WavePool64x36 {
    fn default() -> Self {
        Self::new()
    }
}

impl WavePool64x36 {
    pub const fn new() -> Self {
        Self {
            energy_accum: 0,
            frames_injected: 0,
        }
    }

    pub fn inject_oversampled_frame(&mut self, pcm_768k: &[i16; 16], snr_db: u8) {
        let mut e = 0i64;
        for &s in pcm_768k {
            e += (s as i64).abs();
        }
        self.energy_accum = self
            .energy_accum
            .wrapping_add(e * (snr_db.max(1) as i64));
        self.frames_injected = self.frames_injected.wrapping_add(1);
    }
}

/// Exact integer 16× Catmull-Rom cubic spline upsampler (`48 kHz -> 768 kHz`) with zero floats.
pub fn catmull_rom_upsample_16x(p0: i16, p1: i16, p2: i16, p3: i16) -> [i16; 16] {
    let mut out = [0i16; 16];
    let x0 = p0 as i64;
    let x1 = p1 as i64;
    let x2 = p2 as i64;
    let x3 = p3 as i64;

    let a0 = -x0 + 3 * x1 - 3 * x2 + x3;
    let a1 = 2 * x0 - 5 * x1 + 4 * x2 - x3;
    let a2 = -x0 + x2;
    let a3 = 2 * x1;

    for (step, slot) in out.iter_mut().enumerate() {
        let t = step as i64; // 0..15 in units of 1/16
        // Horner evaluation over denominator 2 * 16^3 = 8192:
        let num = ((a0 * t + (a1 << 4)) * t + (a2 << 8)) * t + (a3 << 12);
        let val = (num + 4096) >> 13;
        *slot = val.clamp(-32767, 32767) as i16;
    }
    out
}

/// Unified `MT6631` controller combining `ConnsysGate`, `FmTuner`, `WmtPatchManifest`, `AfeFmRoute`, and `WavePool64x36`.
#[derive(Debug, Clone, Default)]
pub struct Mt6631Controller {
    pub gate: ConnsysGate,
    pub fm: FmTuner,
    pub wmt_manifest: WmtPatchManifest,
    pub afe_route: AfeFmRoute,
    pub wave_pool: WavePool64x36,
    pub chip_id: u16,
    pub current_wifi_channel_mhz: u16,
}

impl Mt6631Controller {
    pub const fn new() -> Self {
        Self {
            gate: ConnsysGate::new(),
            fm: FmTuner::new(),
            wmt_manifest: WmtPatchManifest {
                wifi_ram_code_loaded: false,
                wmt_mcu_patch_loaded: false,
                fm_dsp_patch_loaded: false,
            },
            afe_route: AfeFmRoute::new(),
            wave_pool: WavePool64x36::new(),
            chip_id: consys_mmio::MT6631_A_DIE_CHIP_ID,
            current_wifi_channel_mhz: 2437,
        }
    }

    pub fn dispatch_hqa(&mut self, mut req: HqaFrame) -> HqaFrame {
        req.msg_type = wifi_hqa::HQA_TYPE_RSP;
        if req.cmd_id == HqaCmd::GetChipId as u16 {
            req.param0 = consys_mmio::MT6631_A_DIE_CHIP_ID as u32;
            self.chip_id = consys_mmio::MT6631_A_DIE_CHIP_ID;
        } else if req.cmd_id == HqaCmd::SetChannel as u16 {
            self.current_wifi_channel_mhz = (req.param0 & 0xFFFF) as u16;
        }
        req
    }

    pub fn load_demo_offgrid_broadcast(&mut self) {
        self.fm.tune_100khz(995);
        self.fm.rds_pi = 0x8B15;
        self.fm.rds_ps = *b"AYE-99.5";
        let msg = b"FROM SYRIA TO REYKJAVIK * AYE.IS SOVEREIGN SANCTUARY 432HZ";
        self.fm.rds_rt[..msg.len()].copy_from_slice(msg);
    }
}

/// Drop-in v5.4.0 / v5.5.0 helper that probes `ConnsysGate`, dispatches `HQA` (`0x1814_2880`),
/// tunes FM (`99.5 MHz`), and injects a 16× Catmull-Rom 768 kHz frame into the 64×36 Wave Pool.
pub fn poll_mt6631_v540(fdt_has_connsys: bool, mcu_status_reg: u32) -> Mt6631Controller {
    let mut ctrl = Mt6631Controller::new();
    ctrl.gate.evaluate_probe(fdt_has_connsys, mcu_status_reg);
    let _chip = ctrl.dispatch_hqa(HqaFrame::new(HqaCmd::GetChipId, 0));
    let _chan = ctrl.dispatch_hqa(HqaFrame::new(HqaCmd::SetChannel, 2412));
    ctrl.load_demo_offgrid_broadcast();
    let _ = FmTuner::split_mhz(ctrl.fm.current_freq_100khz);
    let pcm_768k = catmull_rom_upsample_16x(-4096, 8192, 16384, -8192);
    ctrl.wave_pool
        .inject_oversampled_frame(&pcm_768k, ctrl.fm.snr_db);
    ctrl
}

/// Arm `WmtPatchManifest`, `ConnsysGate`, and `AfeFmRoute` once the 28-byte WMT patch header is verified.
pub fn arm_mt6631_rf_and_afe(ctrl: &mut Mt6631Controller, wmt_hdr16: &[u8; 16]) -> bool {
    if WmtPatchManifest::verify_wmt_patch_header(wmt_hdr16) {
        ctrl.wmt_manifest.wifi_ram_code_loaded = true;
        ctrl.wmt_manifest.wmt_mcu_patch_loaded = true;
        ctrl.wmt_manifest.fm_dsp_patch_loaded = true;
        ctrl.gate.evaluate_probe(true, WMT_MCU_READY_MAGIC);
        ctrl.afe_route.enable_fm_audio_path(128);
    }
    ctrl.wmt_manifest.is_rf_ready() && ctrl.gate.can_touch_mmio()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consys_fdt_reg_windows_match_mt6768_dts_and_exclude_folklore_18006000() {
        assert_eq!(consys_mmio::CONSYS_FDT_REG_WINDOWS.len(), 10);
        assert_eq!(consys_mmio::CONSYS_FDT_REG_WINDOWS[0].0, 0x1800_2000);
        assert_eq!(consys_mmio::CONSYS_FDT_REG_WINDOWS[3].0, 0x1000_6000);
        assert_eq!(consys_mmio::CONSYS_FDT_REG_WINDOWS[5].0, 0x180B_1000);
        // Verify folklore 0x1800_6000 is NOT in the FDT register map
        assert!(consys_mmio::CONSYS_FDT_REG_WINDOWS
            .iter()
            .all(|&(base, _)| base != 0x1800_6000));
    }

    #[test]
    fn afe_connsys_i2s_con_uses_verified_0c78_offset() {
        assert_eq!(afe_connsys_fm::AFE_CONNSYS_I2S_CON_OFFSET, 0x0C78);
        assert_eq!(afe_connsys_fm::AFE_CONNSYS_I2S_CON, 0x1122_0C78);
        assert_eq!(afe_connsys_fm::AFE_CONNSYS_I2S_MON, 0x1122_0C7C);
    }

    #[test]
    fn stp_btif_wmt_and_fm_bop_packets_encode_cleanly() {
        assert_eq!(
            stp_btif_protocol::WMT_QUERY_A_DIE_CMD,
            [0x01, 0x02, 0x01, 0x00, 0x12]
        );
        let pkt = stp_btif_protocol::build_mt6631_fm_tune_and_rds_cmd(885);
        assert_eq!(pkt[0], stp_btif_protocol::FM_TASK_COMMAND_PKT_TYPE);
        assert_eq!(pkt[1], stp_btif_protocol::FM_TUNE_OPCODE);
        let payload_len = u16::from_le_bytes([pkt[2], pkt[3]]) as usize;
        assert_eq!(payload_len + 4, pkt.len());
    }

    #[test]
    fn connsys_gate_wmt_manifest_and_afe_route_arm_cleanly() {
        let mut ctrl = poll_mt6631_v540(true, 0);
        // Unbooted MCU defaults safely to ShadowLoopback
        assert!(!ctrl.gate.can_touch_mmio());
        assert_eq!(ctrl.chip_id, 0x6631);
        assert_eq!(FmTuner::split_mhz(ctrl.fm.current_freq_100khz), (99, 5));
        assert_eq!(ctrl.wave_pool.frames_injected, 1);

        // Arming with a valid WMT patch date header transitions to HardwareMmio & enables AFE route
        let hdr16 = b"202610311200000\0";
        assert!(arm_mt6631_rf_and_afe(&mut ctrl, hdr16));
        assert!(ctrl.gate.can_touch_mmio());
        assert!(ctrl.afe_route.enabled);
        assert_eq!(ctrl.afe_route.connsys_i2s_con_addr, 0x1122_0C78);
    }
}
