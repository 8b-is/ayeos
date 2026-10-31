//! Bounded, allocation-free FDT v17 reader for board bring-up diagnostics.
//! Implements the flattened-tree token format (Devicetree Specification 0.4,
//! chapter 5). No MMIO or hardware readiness inference occurs here.

pub const MAX_BYTES: usize = 1_048_576;
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error { Header, Bounds, Structure, Text, Depth }

#[derive(Debug, Clone, Copy)]
pub enum Event<'a> {
    Begin(&'a str),
    End,
    Property(&'a str, &'a [u8]),
}

fn word(b: &[u8], at: usize) -> Result<usize, Error> {
    let bytes = b.get(at..at.checked_add(4).ok_or(Error::Bounds)?).ok_or(Error::Bounds)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
}

fn region(b: &[u8], at: usize, len: usize) -> Result<&[u8], Error> {
    b.get(at..at.checked_add(len).ok_or(Error::Bounds)?).ok_or(Error::Bounds)
}

fn string(b: &[u8], at: usize) -> Result<(&str, usize), Error> {
    let rest = b.get(at..).ok_or(Error::Bounds)?;
    let len = rest.iter().take(256).position(|&v| v == 0).ok_or(Error::Text)?;
    let text = core::str::from_utf8(&rest[..len]).map_err(|_| Error::Text)?;
    if !text.bytes().all(|c| (32..=126).contains(&c)) { return Err(Error::Text); }
    Ok((text, at + len + 1))
}

fn align4(n: usize) -> Result<usize, Error> {
    Ok(n.checked_add(3).ok_or(Error::Bounds)? & !3)
}

pub fn walk<'a>(blob: &'a [u8], mut emit: impl FnMut(Event<'a>)) -> Result<(), Error> {
    if blob.len() < 40 || blob.len() > MAX_BYTES || word(blob, 0)? != 0xD00D_FEED
        || word(blob, 4)? != blob.len() || word(blob, 20)? != 17 || word(blob, 24)? > 17 {
        return Err(Error::Header);
    }
    let structure_at = word(blob, 8)?;
    let strings_at = word(blob, 12)?;
    let reserve_at = word(blob, 16)?;
    let structure_len = word(blob, 36)?;
    let strings_len = word(blob, 32)?;
    if structure_at < 40 || strings_at < 40 || reserve_at < 40
        || structure_at % 4 != 0 || reserve_at % 8 != 0 {
        return Err(Error::Header);
    }
    let structure = region(blob, structure_at, structure_len)?;
    let strings = region(blob, strings_at, strings_len)?;
    if structure_at < strings_at + strings_len && strings_at < structure_at + structure_len {
        return Err(Error::Header);
    }
    // Reservation entries are pairs of big-endian u64; terminator is (0,0).
    let mut r = reserve_at;
    loop {
        let entry = region(blob, r, 16)?;
        if (r < structure_at + structure_len && structure_at < r + 16)
            || (r < strings_at + strings_len && strings_at < r + 16) {
            return Err(Error::Header);
        }
        r += 16;
        if entry.iter().all(|&b| b == 0) { break; }
    }
    let mut pos = 0;
    let mut depth = 0usize;
    let mut saw_root = false;
    let mut child_seen = [false; MAX_DEPTH];
    loop {
        let token = word(structure, pos)?;
        pos += 4;
        match token {
            1 => {
                let (name, end) = string(structure, pos)?;
                pos = align4(end)?;
                if pos > structure.len() { return Err(Error::Bounds); }
                if depth == 0 {
                    if saw_root || !name.is_empty() { return Err(Error::Structure); }
                    saw_root = true;
                } else {
                    if name.is_empty() || name.contains('/') { return Err(Error::Structure); }
                    child_seen[depth - 1] = true;
                }
                if depth == MAX_DEPTH { return Err(Error::Depth); }
                child_seen[depth] = false;
                depth += 1;
                emit(Event::Begin(name));
            }
            2 => {
                if depth == 0 { return Err(Error::Structure); }
                depth -= 1;
                emit(Event::End);
            }
            3 => {
                if depth == 0 || child_seen[depth - 1] { return Err(Error::Structure); }
                let len = word(structure, pos)?;
                let name_at = word(structure, pos + 4)?;
                pos += 8;
                let (name, _) = string(strings, name_at)?;
                if name.is_empty() { return Err(Error::Text); }
                let value = region(structure, pos, len)?;
                pos = align4(pos.checked_add(len).ok_or(Error::Bounds)?)?;
                if pos > structure.len() { return Err(Error::Bounds); }
                emit(Event::Property(name, value));
            }
            4 => {}, // FDT_NOP
            9 => {
                if depth != 0 || !saw_root || pos != structure.len() { return Err(Error::Structure); }
                return Ok(());
            }
            _ => return Err(Error::Structure),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 56];
        for w in [1u32, 0, 3, 4, 0, 0x6768, 2, 9] { b.extend(w.to_be_bytes()); }
        b.extend(b"chip\0");
        let total = b.len() as u32;
        for (i, v) in [0xD00DFEED, total, 56, 88, 40, 17, 16, 0, 5, 32].iter().enumerate() {
            b[i * 4..i * 4 + 4].copy_from_slice(&u32::to_be_bytes(*v));
        }
        b
    }
    #[test]
    fn reads_borrowed_property() {
        let b = fixture();
        let mut found = false;
        walk(&b, |e| if let Event::Property(name, value) = e {
            assert_eq!(name, "chip"); assert_eq!(value, [0, 0, 0x67, 0x68]); found = true;
        }).unwrap();
        assert!(found);
    }
    #[test]
    fn every_truncated_prefix_fails_without_panic() {
        let b = fixture();
        for n in 0..b.len() { assert!(walk(&b[..n], |_| {}).is_err()); }
    }
    #[test]
    fn malformed_offsets_lengths_tokens_and_overlap_fail() {
        for at in [8usize, 12, 16, 32, 36, 56, 68, 72] {
            let mut b = fixture(); b[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
            assert!(walk(&b, |_| {}).is_err(), "offset {at}");
        }
        let mut b = fixture(); b[12..16].copy_from_slice(&56u32.to_be_bytes());
        assert!(walk(&b, |_| {}).is_err());
    }
    #[test]
    fn invalid_utf8_and_unbalanced_end_fail() {
        let mut b = fixture(); b[88] = 255; assert!(walk(&b, |_| {}).is_err());
        let mut b = fixture(); b[80..84].copy_from_slice(&4u32.to_be_bytes());
        assert!(walk(&b, |_| {}).is_err());
    }
}
