//! Bounded RFC 8484 DNS messages; the configured resolver is not vendor-specific.
use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};
fn bad() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid DNS response")
}
fn number(data: &[u8], offset: usize) -> io::Result<u16> {
    let b = data.get(offset..offset + 2).ok_or_else(bad)?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}
pub(super) fn query(host: &str, kind: u16) -> io::Result<Vec<u8>> {
    let mut data = vec![0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    if host.is_empty() || host.len() > 253 {
        return Err(bad());
    }
    for label in host.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 || !label.is_ascii() {
            return Err(bad());
        }
        data.push(label.len() as u8);
        data.extend_from_slice(label.as_bytes());
    }
    data.push(0);
    data.extend_from_slice(&kind.to_be_bytes());
    data.extend_from_slice(&1u16.to_be_bytes());
    Ok(data)
}
fn name(data: &[u8], offset: &mut usize) -> io::Result<String> {
    let mut cursor = *offset;
    let mut jumped = false;
    let mut labels = Vec::new();
    for _ in 0..128 {
        let length = *data.get(cursor).ok_or_else(bad)?;
        cursor += 1;
        if length == 0 {
            if !jumped {
                *offset = cursor;
            }
            let result = labels.join(".");
            if result.len() > 253 {
                return Err(bad());
            }
            return Ok(result);
        }
        if length & 0xc0 == 0xc0 {
            let low = *data.get(cursor).ok_or_else(bad)?;
            cursor += 1;
            if !jumped {
                *offset = cursor;
                jumped = true;
            }
            cursor = (((length & 0x3f) as usize) << 8) | low as usize;
            continue;
        }
        if length > 63 {
            return Err(bad());
        }
        let bytes = data.get(cursor..cursor + length as usize).ok_or_else(bad)?;
        if !bytes.is_ascii() {
            return Err(bad());
        }
        labels.push(
            std::str::from_utf8(bytes)
                .map_err(|_| bad())?
                .to_ascii_lowercase(),
        );
        cursor += length as usize;
    }
    Err(bad())
}
pub(super) fn answer(data: &[u8], host: &str, kind: u16) -> io::Result<(Vec<IpAddr>, u64)> {
    let flags = number(data, 2)?;
    if data.len() > 65535
        || number(data, 0)? != 0
        || flags & 0xf800 != 0x8000
        || flags & 0x0200 != 0
        || number(data, 4)? != 1
    {
        return Err(bad());
    }
    let mut offset = 12;
    if name(data, &mut offset)? != host.trim_end_matches('.').to_ascii_lowercase()
        || number(data, offset)? != kind
        || number(data, offset + 2)? != 1
    {
        return Err(bad());
    }
    offset += 4;
    if flags & 15 != 0 {
        return Err(bad());
    }
    let count = number(data, 6)? as usize;
    if count > 512 {
        return Err(bad());
    }
    let mut records = Vec::new();
    for _ in 0..count {
        let owner = name(data, &mut offset)?;
        let typ = number(data, offset)?;
        let class = number(data, offset + 2)?;
        let ttl_bytes = data.get(offset + 4..offset + 8).ok_or_else(bad)?;
        let ttl = u32::from_be_bytes(ttl_bytes.try_into().unwrap()) as u64;
        let size = number(data, offset + 8)? as usize;
        offset += 10;
        let raw = data.get(offset..offset + size).ok_or_else(bad)?;
        let mut alias = None;
        let mut ip = None;
        if class == 1 {
            if typ == 5 {
                let mut index = offset;
                alias = Some(name(data, &mut index)?);
                if index != offset + size {
                    return Err(bad());
                }
            } else if typ == kind && kind == 1 && size == 4 {
                ip = Some(IpAddr::V4(Ipv4Addr::from(
                    <[u8; 4]>::try_from(raw).unwrap(),
                )));
            } else if typ == kind && kind == 28 && size == 16 {
                ip = Some(IpAddr::V6(Ipv6Addr::from(
                    <[u8; 16]>::try_from(raw).unwrap(),
                )));
            }
        }
        records.push((owner, alias, ip, ttl));
        offset += size;
    }
    let mut owner = host.trim_end_matches('.').to_ascii_lowercase();
    let mut ttl = 300;
    for _ in 0..16 {
        if let Some(record) = records.iter().find(|r| r.0 == owner && r.1.is_some()) {
            owner = record.1.clone().unwrap();
            ttl = ttl.min(record.3);
        } else {
            let mut ips = Vec::new();
            for record in records.iter().filter(|r| r.0 == owner) {
                if let Some(ip) = record.2 {
                    if !ips.contains(&ip) {
                        ips.push(ip);
                    }
                    ttl = ttl.min(record.3);
                }
            }
            return Ok((ips, ttl));
        }
    }
    Err(bad())
}
