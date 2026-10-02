use crate::attendance::model::ItemRule;
use std::{collections::BTreeMap, fs, path::Path};

pub fn norm(item: &str) -> String {
    item.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

pub fn builtin_rule(normalized: &str) -> Option<ItemRule> {
    match normalized {
        "" | "야간근무" => Some(ItemRule::General),
        "연차" => Some(ItemRule::Leave8h),
        "오전반차" | "오후반차" | "생일휴가" | "건강검진" => {
            Some(ItemRule::Bonus4h)
        }
        "업무중휴게" => Some(ItemRule::DeductRequested),
        _ if normalized.starts_with("단축") => Some(ItemRule::Bonus4h),
        _ => None,
    }
}

pub fn resolve(normalized: &str, custom: &BTreeMap<String, ItemRule>) -> Option<ItemRule> {
    builtin_rule(normalized).or_else(|| custom.get(normalized).cloned())
}

pub fn load_rules(path: &Path) -> BTreeMap<String, ItemRule> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(rules) => rules,
            Err(error) => {
                eprintln!("근태 규칙 파일을 읽을 수 없어 빈 규칙으로 시작합니다: {error}");
                BTreeMap::new()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => {
            eprintln!("근태 규칙 파일을 읽을 수 없어 빈 규칙으로 시작합니다: {error}");
            BTreeMap::new()
        }
    }
}

pub fn save_rules(path: &Path, rules: &BTreeMap<String, ItemRule>) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("규칙 저장 폴더를 만들 수 없습니다: {error}"))?;
    }
    let json = serde_json::to_vec_pretty(rules)
        .map_err(|error| format!("규칙을 JSON으로 변환할 수 없습니다: {error}"))?;
    fs::write(path, json).map_err(|error| format!("근태 규칙을 저장할 수 없습니다: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{builtin_rule, norm, resolve};
    use crate::attendance::model::ItemRule;
    use std::collections::BTreeMap;

    #[test]
    fn normalizes_whitespace_and_resolves_builtins_before_custom_rules() {
        assert_eq!(norm(" 업무 중\t휴게 "), "업무중휴게");
        assert_eq!(
            builtin_rule("단축(명절, 연휴 마지막날)"),
            Some(ItemRule::Bonus4h)
        );
        assert_eq!(builtin_rule("야간근무"), Some(ItemRule::General));
        let custom = BTreeMap::from([("연차".to_owned(), ItemRule::Exclude)]);
        assert_eq!(resolve("연차", &custom), Some(ItemRule::Leave8h));
        assert_eq!(resolve("재택근무", &custom), None);
    }
}
