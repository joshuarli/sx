pub fn section(core: &str) -> Option<&'static str> {
    match core {
        "text" => Some(include_str!("../guide/text.md")),
        "yaml" => Some(include_str!("../guide/yaml.md")),
        "decimal" => Some(include_str!("../guide/decimal.md")),
        "semver" => Some(include_str!("../guide/semver.md")),
        "re" => Some(include_str!("../guide/re.md")),
        "json" => Some(include_str!("../guide/json.md")),
        _ => None,
    }
}
