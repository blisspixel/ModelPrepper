//! Deliberately conservative text matching, not a general license classifier.
pub const MIT: &str = include_str!("../resources/licenses/MIT.txt");
pub const APACHE: &str = include_str!("../resources/licenses/Apache-2.0.txt");

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn matches(license: &str, text: &str) -> bool {
    match license {
        "apache-2.0" => {
            let input = normalized(text);
            if input == normalized(APACHE) {
                return true;
            }
            // The appendix explicitly permits filling its copyright example.
            // Replacing one complete line must restore the entire standard text.
            text.lines().any(|line| {
                let Some(field) = line.trim().strip_prefix("Copyright ") else {
                    return false;
                };
                let bytes = field.as_bytes();
                bytes.len() > 5
                    && bytes[..4].iter().all(u8::is_ascii_digit)
                    && bytes[4] == b' '
                    && field[5..].chars().count() <= 160
                    && normalized(&text.replacen(
                        line,
                        "Copyright [yyyy] [name of copyright owner]",
                        1,
                    )) == normalized(APACHE)
            })
        }
        "mit" => {
            let Some(start) = text.find("Permission is hereby granted") else {
                return false;
            };
            let prefix: Vec<_> = text[..start]
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect();
            let header_valid = match prefix.as_slice() {
                [copyright] => copyright.starts_with("Copyright (c) ") && copyright.len() > 14,
                ["MIT License", copyright] => {
                    copyright.starts_with("Copyright (c) ") && copyright.len() > 14
                }
                _ => false,
            };
            let standard = &MIT[MIT
                .find("Permission is hereby granted")
                .expect("embedded MIT template")..];
            header_valid && normalized(&text[start..]) == normalized(standard)
        }
        _ => false,
    }
}
