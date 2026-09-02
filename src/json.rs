pub fn resize_message(rows: u16, cols: u16) -> String {
    format!("{{\"type\":\"resize\",\"rows\":{},\"cols\":{}}}", rows, cols)
}

fn escape_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn exec_message(cmd: &str, rows: u16, cols: u16) -> String {
    format!(
        "{{\"type\":\"exec\",\"cmd\":\"{}\",\"rows\":{},\"cols\":{}}}",
        escape_json(cmd),
        rows,
        cols
    )
}

#[cfg(target_os = "linux")]
pub fn exit_message(code: i32) -> String {
    format!("{{\"type\":\"exit\",\"code\":{}}}", code)
}

pub fn parse_exit_code(text: &str) -> Option<i32> {
    if field_str(text, "type")? != "exit" {
        return None;
    }
    field_u32(text, "code").map(|c| (c & 0xFF) as i32)
}

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
pub struct SessionInit {
    pub rows: u16,
    pub cols: u16,
    pub exec: Option<String>,
}

#[cfg(target_os = "linux")]
pub fn parse_session_init(text: &str) -> SessionInit {
    let rows = field_u32(text, "rows").unwrap_or(DEFAULT_ROWS);
    let cols = field_u32(text, "cols").unwrap_or(DEFAULT_COLS);
    let (rows, cols) = safe_size(rows, cols);
    let exec = match field_str(text, "type") {
        Some("exec") => field_str(text, "cmd").map(|s| s.to_string()),
        _ => None,
    };
    SessionInit { rows, cols, exec }
}

#[cfg(target_os = "linux")]
fn safe_size(rows: u32, cols: u32) -> (u16, u16) {
    (rows.clamp(1, MAX_ROWS) as u16, cols.clamp(1, MAX_COLS) as u16)
}

#[cfg(target_os = "linux")]
pub const DEFAULT_ROWS: u32 = 24;
#[cfg(target_os = "linux")]
pub const DEFAULT_COLS: u32 = 80;
#[cfg(target_os = "linux")]
pub const MAX_ROWS: u32 = 1000;
#[cfg(target_os = "linux")]
pub const MAX_COLS: u32 = 5000;

#[cfg(target_os = "linux")]
pub fn parse_resize_command(text: &str) -> Option<(u16, u16)> {
    if field_str(text, "type")? != "resize" {
        return None;
    }
    let rows = field_u32(text, "rows")?;
    let cols = field_u32(text, "cols")?;
    Some(safe_size(rows, cols))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let msg = resize_message(50, 120);
        assert_eq!(parse_exit_code(&msg), None);
        assert_eq!(parse_exit_code("{\"code\": 5, \"type\": \"exit\"}"), Some(5));
        assert_eq!(parse_exit_code("{\"code\": 256, \"type\": \"exit\"}"), Some(0));
        assert_eq!(parse_exit_code("{\"type\":\"resize\"}"), None);
    }

    #[test]
    fn exec_escaping() {
        let msg = exec_message("echo \"hi\" \\ there", 24, 80);
        assert!(msg.contains("echo \\\"hi\\\" \\\\ there"));
        let msg = exec_message("a\nb", 24, 80);
        assert!(msg.contains("a\\nb"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn session_init() {
        let init = parse_session_init("{\"type\":\"resize\",\"rows\":40,\"cols\":120}");
        assert_eq!((init.rows, init.cols), (40, 120));
        assert!(init.exec.is_none());
        let init = parse_session_init(&exec_message("ls -la", 10, 20));
        assert_eq!(init.exec.as_deref(), Some("ls -la"));
        assert_eq!((init.rows, init.cols), (10, 20));
        let init = parse_session_init("{}");
        assert_eq!((init.rows, init.cols), (24, 80));
        assert!(init.exec.is_none());
    }
}
