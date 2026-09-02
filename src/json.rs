#[cfg(target_os = "linux")]
pub const DEFAULT_ROWS: u32 = 24;
#[cfg(target_os = "linux")]
pub const DEFAULT_COLS: u32 = 80;
#[cfg(target_os = "linux")]
pub const MAX_ROWS: u32 = 1000;
#[cfg(target_os = "linux")]
pub const MAX_COLS: u32 = 5000;

#[cfg(target_os = "linux")]
pub fn safe_size(rows: u32, cols: u32) -> (u16, u16) {
    (rows.clamp(1, MAX_ROWS) as u16, cols.clamp(1, MAX_COLS) as u16)
}

pub fn resize_message(rows: u16, cols: u16) -> String {
    format!("{{\"type\":\"resize\",\"rows\":{},\"cols\":{}}}", rows, cols)
}

#[cfg(target_os = "linux")]
fn field_u32(json: &str, key: &str) -> Option<u32> {
    let needle = format!("\"{}\"", key);
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let colon = rest.find(':')?;
    let value = rest[colon + 1..].trim_start();
    let digits: &str = value
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("");
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

#[cfg(target_os = "linux")]
fn field_str<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\"", key);
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let colon = rest.find(':')?;
    let value = rest[colon + 1..].trim_start();
    let value = value.strip_prefix('"')?;
    let end = value.find('"')?;
    Some(&value[..end])
}

#[cfg(target_os = "linux")]
pub fn parse_resize_init(text: &str) -> (u16, u16) {
    let rows = field_u32(text, "rows").unwrap_or(DEFAULT_ROWS);
    let cols = field_u32(text, "cols").unwrap_or(DEFAULT_COLS);
    safe_size(rows, cols)
}

#[cfg(target_os = "linux")]
pub fn parse_resize_command(text: &str) -> Option<(u16, u16)> {
    if field_str(text, "type")? != "resize" {
        return None;
    }
    let rows = field_u32(text, "rows")?;
    let cols = field_u32(text, "cols")?;
    Some(safe_size(rows, cols))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let msg = resize_message(50, 120);
        assert_eq!(parse_resize_command(&msg), Some((50, 120)));
        assert_eq!(parse_resize_command("{\"cols\": 200, \"type\": \"resize\", \"rows\": 30}"), Some((30, 200)));
        assert_eq!(parse_resize_command("{\"type\":\"ping\"}"), None);
        assert_eq!(parse_resize_command("{\"rows\":5,\"cols\":6}"), None);
        assert_eq!(parse_resize_init("{}"), (24, 80));
        assert_eq!(parse_resize_init("{\"rows\":999999,\"cols\":0}"), (1000, 1));
    }
}
