//! Bounded status-row scheduling; no hardware, timers or allocation.
//! Poll counts are work budgets, not durations. USB service runs before tick().
pub const COLUMNS: usize = 76;
const SNAPSHOT_INTERVAL_POLLS: u32 = 65_536;

pub struct StatusRow {
    bytes: [u8; COLUMNS],
    cursor: usize,
    cooldown: u32,
}
impl StatusRow {
    pub const fn new() -> Self {
        Self {
            bytes: [b' '; COLUMNS],
            cursor: COLUMNS,
            cooldown: 0,
        }
    }
    pub fn ready(&self) -> bool {
        self.cursor == COLUMNS && self.cooldown == 0
    }
    pub fn begin(&mut self, text: &str) {
        if !self.ready() {
            return;
        }
        self.bytes.fill(b' ');
        for (dst, src) in self.bytes.iter_mut().zip(text.bytes()) {
            *dst = if src.is_ascii_graphic() || src == b' ' {
                src
            } else {
                b'.'
            };
        }
        self.cursor = 0;
        self.cooldown = SNAPSHOT_INTERVAL_POLLS;
    }
    /// At most one 9x16 glyph cell per quiet poll; active USB pauses the row.
    pub fn tick(&mut self, idle: bool) -> Option<(usize, u8)> {
        self.cooldown = self.cooldown.saturating_sub(1);
        if !idle || self.cursor == COLUMNS {
            return None;
        }
        let column = self.cursor;
        self.cursor += 1;
        Some((column, self.bytes[column]))
    }
}
