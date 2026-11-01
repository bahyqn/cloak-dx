//! Country names and ISO 3166-1 alpha-2 codes.
//!
//! Google's location card reports a place name ("Tokyo, Japan"), not a code.
//! This table turns that name into the code the rest of the app compares with
//! the flag in a node's name.

use std::sync::OnceLock;

/// Canonical English names, keyed by their ISO 3166-1 alpha-2 code.
const COUNTRIES: &[(&str, &str)] = &[
    ("Afghanistan", "AF"),
    ("Åland Islands", "AX"),
    ("Albania", "AL"),
    ("Algeria", "DZ"),
    ("American Samoa", "AS"),
    ("Andorra", "AD"),
    ("Angola", "AO"),
    ("Anguilla", "AI"),
    ("Antarctica", "AQ"),
    ("Antigua and Barbuda", "AG"),
    ("Argentina", "AR"),
    ("Armenia", "AM"),
    ("Aruba", "AW"),
    ("Australia", "AU"),
    ("Austria", "AT"),
    ("Azerbaijan", "AZ"),
    ("Bahamas", "BS"),
    ("Bahrain", "BH"),
    ("Bangladesh", "BD"),
    ("Barbados", "BB"),
    ("Belarus", "BY"),
    ("Belgium", "BE"),
    ("Belize", "BZ"),
    ("Benin", "BJ"),
    ("Bermuda", "BM"),
    ("Bhutan", "BT"),
    ("Bolivia", "BO"),
    ("Bosnia and Herzegovina", "BA"),
    ("Botswana", "BW"),
    ("Bouvet Island", "BV"),
    ("Brazil", "BR"),
    ("British Indian Ocean Territory", "IO"),
    ("British Virgin Islands", "VG"),
    ("Brunei", "BN"),
    ("Bulgaria", "BG"),
    ("Burkina Faso", "BF"),
    ("Burundi", "BI"),
    ("Cambodia", "KH"),
    ("Cameroon", "CM"),
    ("Canada", "CA"),
    ("Cape Verde", "CV"),
    ("Caribbean Netherlands", "BQ"),
    ("Cayman Islands", "KY"),
    ("Central African Republic", "CF"),
    ("Chad", "TD"),
    ("Chile", "CL"),
    ("China", "CN"),
    ("Christmas Island", "CX"),
    ("Cocos (Keeling) Islands", "CC"),
    ("Colombia", "CO"),
    ("Comoros", "KM"),
    ("Cook Islands", "CK"),
    ("Costa Rica", "CR"),
    ("Croatia", "HR"),
    ("Cuba", "CU"),
    ("Curaçao", "CW"),
    ("Cyprus", "CY"),
    ("Czechia", "CZ"),
    ("Democratic Republic of the Congo", "CD"),
    ("Denmark", "DK"),
    ("Djibouti", "DJ"),
    ("Dominica", "DM"),
    ("Dominican Republic", "DO"),
    ("Ecuador", "EC"),
    ("Egypt", "EG"),
    ("El Salvador", "SV"),
    ("Equatorial Guinea", "GQ"),
    ("Eritrea", "ER"),
    ("Estonia", "EE"),
    ("Eswatini", "SZ"),
    ("Ethiopia", "ET"),
    ("Falkland Islands", "FK"),
    ("Faroe Islands", "FO"),
    ("Fiji", "FJ"),
    ("Finland", "FI"),
    ("France", "FR"),
    ("French Guiana", "GF"),
    ("French Polynesia", "PF"),
    ("French Southern Territories", "TF"),
    ("Gabon", "GA"),
    ("Gambia", "GM"),
    ("Georgia", "GE"),
    ("Germany", "DE"),
    ("Ghana", "GH"),
    ("Gibraltar", "GI"),
    ("Greece", "GR"),
    ("Greenland", "GL"),
    ("Grenada", "GD"),
    ("Guadeloupe", "GP"),
    ("Guam", "GU"),
    ("Guatemala", "GT"),
    ("Guernsey", "GG"),
    ("Guinea", "GN"),
    ("Guinea-Bissau", "GW"),
    ("Guyana", "GY"),
    ("Haiti", "HT"),
    ("Heard Island and McDonald Islands", "HM"),
    ("Honduras", "HN"),
    ("Hong Kong", "HK"),
    ("Hungary", "HU"),
    ("Iceland", "IS"),
    ("India", "IN"),
    ("Indonesia", "ID"),
    ("Iran", "IR"),
    ("Iraq", "IQ"),
    ("Ireland", "IE"),
    ("Isle of Man", "IM"),
    ("Israel", "IL"),
    ("Italy", "IT"),
    ("Ivory Coast", "CI"),
    ("Jamaica", "JM"),
    ("Japan", "JP"),
    ("Jersey", "JE"),
    ("Jordan", "JO"),
    ("Kazakhstan", "KZ"),
    ("Kenya", "KE"),
    ("Kiribati", "KI"),
    ("Kosovo", "XK"),
    ("Kuwait", "KW"),
    ("Kyrgyzstan", "KG"),
    ("Laos", "LA"),
    ("Latvia", "LV"),
    ("Lebanon", "LB"),
    ("Lesotho", "LS"),
    ("Liberia", "LR"),
    ("Libya", "LY"),
    ("Liechtenstein", "LI"),
    ("Lithuania", "LT"),
    ("Luxembourg", "LU"),
    ("Macau", "MO"),
    ("Madagascar", "MG"),
    ("Malawi", "MW"),
    ("Malaysia", "MY"),
    ("Maldives", "MV"),
    ("Mali", "ML"),
    ("Malta", "MT"),
    ("Marshall Islands", "MH"),
    ("Martinique", "MQ"),
    ("Mauritania", "MR"),
    ("Mauritius", "MU"),
    ("Mayotte", "YT"),
    ("Mexico", "MX"),
    ("Micronesia", "FM"),
    ("Moldova", "MD"),
    ("Monaco", "MC"),
    ("Mongolia", "MN"),
    ("Montenegro", "ME"),
    ("Montserrat", "MS"),
    ("Morocco", "MA"),
    ("Mozambique", "MZ"),
    ("Myanmar", "MM"),
    ("Namibia", "NA"),
    ("Nauru", "NR"),
    ("Nepal", "NP"),
    ("Netherlands", "NL"),
    ("New Caledonia", "NC"),
    ("New Zealand", "NZ"),
    ("Nicaragua", "NI"),
    ("Niger", "NE"),
    ("Nigeria", "NG"),
    ("Niue", "NU"),
    ("Norfolk Island", "NF"),
    ("North Korea", "KP"),
    ("North Macedonia", "MK"),
    ("Northern Mariana Islands", "MP"),
    ("Norway", "NO"),
    ("Oman", "OM"),
    ("Pakistan", "PK"),
    ("Palau", "PW"),
    ("Palestine", "PS"),
    ("Panama", "PA"),
    ("Papua New Guinea", "PG"),
    ("Paraguay", "PY"),
    ("Peru", "PE"),
    ("Philippines", "PH"),
    ("Pitcairn Islands", "PN"),
    ("Poland", "PL"),
    ("Portugal", "PT"),
    ("Puerto Rico", "PR"),
    ("Qatar", "QA"),
    ("Republic of the Congo", "CG"),
    ("Réunion", "RE"),
    ("Romania", "RO"),
    ("Russia", "RU"),
    ("Rwanda", "RW"),
    ("Saint Barthélemy", "BL"),
    ("Saint Helena", "SH"),
    ("Saint Kitts and Nevis", "KN"),
    ("Saint Lucia", "LC"),
    ("Saint Martin", "MF"),
    ("Saint Pierre and Miquelon", "PM"),
    ("Saint Vincent and the Grenadines", "VC"),
    ("Samoa", "WS"),
    ("San Marino", "SM"),
    ("São Tomé and Príncipe", "ST"),
    ("Saudi Arabia", "SA"),
    ("Senegal", "SN"),
    ("Serbia", "RS"),
    ("Seychelles", "SC"),
    ("Sierra Leone", "SL"),
    ("Singapore", "SG"),
    ("Sint Maarten", "SX"),
    ("Slovakia", "SK"),
    ("Slovenia", "SI"),
    ("Solomon Islands", "SB"),
    ("Somalia", "SO"),
    ("South Africa", "ZA"),
    ("South Georgia and the South Sandwich Islands", "GS"),
    ("South Korea", "KR"),
    ("South Sudan", "SS"),
    ("Spain", "ES"),
    ("Sri Lanka", "LK"),
    ("Sudan", "SD"),
    ("Suriname", "SR"),
    ("Svalbard and Jan Mayen", "SJ"),
    ("Sweden", "SE"),
    ("Switzerland", "CH"),
    ("Syria", "SY"),
    ("Taiwan", "TW"),
    ("Tajikistan", "TJ"),
    ("Tanzania", "TZ"),
    ("Thailand", "TH"),
    ("Timor-Leste", "TL"),
    ("Togo", "TG"),
    ("Tokelau", "TK"),
    ("Tonga", "TO"),
    ("Trinidad and Tobago", "TT"),
    ("Tunisia", "TN"),
    ("Turkey", "TR"),
    ("Turkmenistan", "TM"),
    ("Turks and Caicos Islands", "TC"),
    ("Tuvalu", "TV"),
    ("U.S. Virgin Islands", "VI"),
    ("Uganda", "UG"),
    ("Ukraine", "UA"),
    ("United Arab Emirates", "AE"),
    ("United Kingdom", "GB"),
    ("United States", "US"),
    ("United States Minor Outlying Islands", "UM"),
    ("Uruguay", "UY"),
    ("Uzbekistan", "UZ"),
    ("Vanuatu", "VU"),
    ("Vatican City", "VA"),
    ("Venezuela", "VE"),
    ("Vietnam", "VN"),
    ("Wallis and Futuna", "WF"),
    ("Western Sahara", "EH"),
    ("Yemen", "YE"),
    ("Zambia", "ZM"),
    ("Zimbabwe", "ZW"),
];

/// Spelling variants Google uses for the same country.
const ALIASES: &[(&str, &str)] = &[
    ("Aland Islands", "AX"),
    ("Bosnia", "BA"),
    ("Burma", "MM"),
    ("Cabo Verde", "CV"),
    ("Congo", "CG"),
    ("Czech Republic", "CZ"),
    ("Cote d'Ivoire", "CI"),
    ("Curacao", "CW"),
    ("DR Congo", "CD"),
    ("East Timor", "TL"),
    ("Great Britain", "GB"),
    ("Holland", "NL"),
    ("Hong Kong SAR", "HK"),
    ("Macao", "MO"),
    ("Macedonia", "MK"),
    ("Reunion", "RE"),
    ("Sao Tome and Principe", "ST"),
    ("South Korea", "KR"),
    ("Swaziland", "SZ"),
    ("Turkey", "TR"),
    ("Türkiye", "TR"),
    ("UAE", "AE"),
    ("UK", "GB"),
    ("USA", "US"),
    ("US", "US"),
    ("Viet Nam", "VN"),
];

/// Finds the country named anywhere in `text`.
///
/// The longest match wins so "United States Minor Outlying Islands" is not
/// reported as the United States. Matching is ASCII case-insensitive and only
/// accepts whole words, which keeps "US" from matching "Russia".
pub fn country_from_name(text: &str) -> Option<(&'static str, &'static str)> {
    let haystack = text.to_ascii_lowercase();
    let mut best: Option<(&'static str, &'static str, usize)> = None;
    for &(name, code) in COUNTRIES.iter().chain(ALIASES.iter()) {
        let needle = name.to_ascii_lowercase();
        if find_whole_word(&haystack, &needle).is_some()
            && best.is_none_or(|(_, _, length)| needle.len() > length)
        {
            best = Some((country_name(code).unwrap_or(name), code, needle.len()));
        }
    }
    best.map(|(name, code, _)| (name, code))
}

/// Removes the country name (as matched by [`country_from_name`]) from `text`,
/// leaving the city or region part of a Google location such as
/// `"Tokyo, Japan"`.
pub fn strip_country_name(text: &str) -> String {
    let haystack = text.to_ascii_lowercase();
    let mut best: Option<(usize, usize)> = None;
    for &(name, _) in COUNTRIES.iter().chain(ALIASES.iter()) {
        let needle = name.to_ascii_lowercase();
        if let Some(index) = find_whole_word(&haystack, &needle) {
            if best.is_none_or(|(_, length)| needle.len() > length) {
                best = Some((index, needle.len()));
            }
        }
    }
    match best {
        Some((index, length)) => format!("{}{}", &text[..index], &text[index + length..]),
        None => text.to_string(),
    }
}

/// Region labels that providers write in Chinese instead of a flag.
///
/// The table is scanned for the earliest position and then the longest label,
/// so `印度尼西亚` is Indonesia rather than India. Cities are included because
/// many providers name a node after its landing city ("东京01").
const CHINESE_REGIONS: &[(&str, &str)] = &[
    ("中国香港", "HK"),
    ("中国澳门", "MO"),
    ("中国台湾", "TW"),
    ("中国大陆", "CN"),
    ("香港", "HK"),
    ("澳门", "MO"),
    ("台湾", "TW"),
    ("大陆", "CN"),
    ("国内", "CN"),
    ("中国", "CN"),
    ("美国", "US"),
    ("加拿大", "CA"),
    ("墨西哥", "MX"),
    ("巴西", "BR"),
    ("阿根廷", "AR"),
    ("智利", "CL"),
    ("哥伦比亚", "CO"),
    ("秘鲁", "PE"),
    ("英国", "GB"),
    ("爱尔兰", "IE"),
    ("法国", "FR"),
    ("德国", "DE"),
    ("荷兰", "NL"),
    ("比利时", "BE"),
    ("瑞士", "CH"),
    ("奥地利", "AT"),
    ("意大利", "IT"),
    ("西班牙", "ES"),
    ("葡萄牙", "PT"),
    ("希腊", "GR"),
    ("波兰", "PL"),
    ("捷克", "CZ"),
    ("匈牙利", "HU"),
    ("罗马尼亚", "RO"),
    ("保加利亚", "BG"),
    ("丹麦", "DK"),
    ("瑞典", "SE"),
    ("挪威", "NO"),
    ("芬兰", "FI"),
    ("冰岛", "IS"),
    ("俄罗斯", "RU"),
    ("乌克兰", "UA"),
    ("白俄罗斯", "BY"),
    ("立陶宛", "LT"),
    ("拉脱维亚", "LV"),
    ("爱沙尼亚", "EE"),
    ("土耳其", "TR"),
    ("以色列", "IL"),
    ("阿联酋", "AE"),
    ("沙特阿拉伯", "SA"),
    ("沙特", "SA"),
    ("卡塔尔", "QA"),
    ("科威特", "KW"),
    ("伊朗", "IR"),
    ("伊拉克", "IQ"),
    ("格鲁吉亚", "GE"),
    ("亚美尼亚", "AM"),
    ("阿塞拜疆", "AZ"),
    ("哈萨克斯坦", "KZ"),
    ("乌兹别克斯坦", "UZ"),
    ("蒙古", "MN"),
    ("日本", "JP"),
    ("韩国", "KR"),
    ("新加坡", "SG"),
    ("马来西亚", "MY"),
    ("泰国", "TH"),
    ("越南", "VN"),
    ("菲律宾", "PH"),
    ("印度尼西亚", "ID"),
    ("印尼", "ID"),
    ("柬埔寨", "KH"),
    ("老挝", "LA"),
    ("缅甸", "MM"),
    ("印度", "IN"),
    ("巴基斯坦", "PK"),
    ("孟加拉国", "BD"),
    ("孟加拉", "BD"),
    ("斯里兰卡", "LK"),
    ("尼泊尔", "NP"),
    ("澳大利亚", "AU"),
    ("澳洲", "AU"),
    ("新西兰", "NZ"),
    ("埃及", "EG"),
    ("南非", "ZA"),
    ("尼日利亚", "NG"),
    ("肯尼亚", "KE"),
    ("摩洛哥", "MA"),
    ("突尼斯", "TN"),
    ("阿尔及利亚", "DZ"),
    // Cities and regions providers name nodes after.
    ("东京", "JP"),
    ("大阪", "JP"),
    ("名古屋", "JP"),
    ("福冈", "JP"),
    ("札幌", "JP"),
    ("首尔", "KR"),
    ("釜山", "KR"),
    ("仁川", "KR"),
    ("台北", "TW"),
    ("台中", "TW"),
    ("台南", "TW"),
    ("高雄", "TW"),
    ("新竹", "TW"),
    ("洛杉矶", "US"),
    ("圣何塞", "US"),
    ("硅谷", "US"),
    ("西雅图", "US"),
    ("芝加哥", "US"),
    ("纽约", "US"),
    ("达拉斯", "US"),
    ("迈阿密", "US"),
    ("拉斯维加斯", "US"),
    ("波特兰", "US"),
    ("凤凰城", "US"),
    ("华盛顿", "US"),
    ("波士顿", "US"),
    ("亚特兰大", "US"),
    ("伦敦", "GB"),
    ("曼彻斯特", "GB"),
    ("巴黎", "FR"),
    ("马赛", "FR"),
    ("法兰克福", "DE"),
    ("柏林", "DE"),
    ("慕尼黑", "DE"),
    ("阿姆斯特丹", "NL"),
    ("鹿特丹", "NL"),
    ("布鲁塞尔", "BE"),
    ("苏黎世", "CH"),
    ("日内瓦", "CH"),
    ("维也纳", "AT"),
    ("米兰", "IT"),
    ("罗马", "IT"),
    ("马德里", "ES"),
    ("巴塞罗那", "ES"),
    ("里斯本", "PT"),
    ("雅典", "GR"),
    ("华沙", "PL"),
    ("布拉格", "CZ"),
    ("布达佩斯", "HU"),
    ("布加勒斯特", "RO"),
    ("索菲亚", "BG"),
    ("斯德哥尔摩", "SE"),
    ("奥斯陆", "NO"),
    ("哥本哈根", "DK"),
    ("赫尔辛基", "FI"),
    ("莫斯科", "RU"),
    ("圣彼得堡", "RU"),
    ("基辅", "UA"),
    ("伊斯坦布尔", "TR"),
    ("特拉维夫", "IL"),
    ("迪拜", "AE"),
    ("阿布扎比", "AE"),
    ("利雅得", "SA"),
    ("多哈", "QA"),
    ("开罗", "EG"),
    ("约翰内斯堡", "ZA"),
    ("开普敦", "ZA"),
    ("悉尼", "AU"),
    ("墨尔本", "AU"),
    ("布里斯班", "AU"),
    ("珀斯", "AU"),
    ("奥克兰", "NZ"),
    ("多伦多", "CA"),
    ("温哥华", "CA"),
    ("蒙特利尔", "CA"),
    ("卡尔加里", "CA"),
    ("圣保罗", "BR"),
    ("里约热内卢", "BR"),
    ("布宜诺斯艾利斯", "AR"),
    ("圣地亚哥", "CL"),
    ("利马", "PE"),
    ("波哥大", "CO"),
    ("墨西哥城", "MX"),
    ("吉隆坡", "MY"),
    ("曼谷", "TH"),
    ("胡志明市", "VN"),
    ("河内", "VN"),
    ("马尼拉", "PH"),
    ("雅加达", "ID"),
    ("孟买", "IN"),
    ("新德里", "IN"),
    ("班加罗尔", "IN"),
    ("卡拉奇", "PK"),
    ("达卡", "BD"),
];

/// Two-letter tokens that are common protocol or provider shorthand rather
/// than a country in a node name (`SS-01` is Shadowsocks, not South Sudan).
const NOT_REGION_TOKENS: &[&str] = &["AS", "SS"];

/// Reads the country a subscription node name advertises in plain text.
///
/// Flags are handled by the caller; this covers names such as `香港06--1倍率`,
/// `Hong Kong 01` and `US-LN`. The earliest location mentioned wins, so a
/// provider prefix such as `HK-美国落地` stays Hong Kong.
pub fn country_code_in_name(name: &str) -> Option<&'static str> {
    let chinese = chinese_region_in_name(name);

    let english = english_region_in_name(name);

    let code = iso_region_in_name(name);

    [chinese, english, code]
        .into_iter()
        .flatten()
        .min_by_key(|(position, _, _)| *position)
        .map(|(_, _, code)| code)
}

fn chinese_region_in_name(name: &str) -> Option<(usize, usize, &'static str)> {
    if !name
        .chars()
        .any(|character| ('\u{4E00}'..='\u{9FFF}').contains(&character))
    {
        return None;
    }
    CHINESE_REGIONS
        .iter()
        .filter_map(|&(label, code)| {
            name.find(label)
                .map(|position| (position, label.len(), code))
        })
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)))
}

/// English country names and aliases, lowercased once and shared by every
/// node card render.
fn lowercase_english_labels() -> &'static [(String, &'static str)] {
    static LABELS: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    LABELS.get_or_init(|| {
        COUNTRIES
            .iter()
            .chain(ALIASES.iter())
            .map(|&(label, code)| (label.to_ascii_lowercase(), code))
            .collect()
    })
}

fn english_region_in_name(name: &str) -> Option<(usize, usize, &'static str)> {
    if !name.bytes().any(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let haystack = name.to_ascii_lowercase();
    lowercase_english_labels()
        .iter()
        .filter_map(|(needle, code)| {
            find_whole_word(&haystack, needle).map(|position| (position, needle.len(), *code))
        })
        // A shorter name wins at the same position, so "United States 01" is
        // the United States and not its minor outlying islands.
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
}

fn iso_region_in_name(name: &str) -> Option<(usize, usize, &'static str)> {
    if !name.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return None;
    }
    COUNTRIES
        .iter()
        .filter(|(_, code)| !NOT_REGION_TOKENS.contains(code))
        .filter_map(|&(_, code)| {
            find_upper_token(name, code).map(|position| (position, code.len(), code))
        })
        .min_by_key(|(position, _, _)| *position)
}

/// Finds an uppercase two-letter token on its own (`US-LN`, `HK 01`).
///
/// A code right after a number is a unit rather than a region, which keeps
/// `剩余流量：77.37 GB` from reading as Great Britain.
fn find_upper_token(text: &str, token: &str) -> Option<usize> {
    text.match_indices(token).find_map(|(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + token.len()..].chars().next();
        let is_boundary =
            |character: Option<char>| character.is_none_or(|value| !value.is_alphanumeric());
        if !is_boundary(before) || !is_boundary(after) {
            return None;
        }
        let before_text = text[..index].trim_end();
        if before_text.ends_with(|character: char| character.is_ascii_digit()) {
            return None;
        }
        Some(index)
    })
}

/// Looks up the canonical English name for a code.
pub fn country_name(code: &str) -> Option<&'static str> {
    let code = normalized_code(code)?;
    COUNTRIES
        .iter()
        .find(|(_, candidate)| *candidate == code)
        .map(|(name, _)| *name)
}

/// Converts an ISO country code such as `US` into its flag emoji.
pub fn country_flag(code: &str) -> Option<String> {
    let mut characters = code.chars();
    let first = characters.next()?.to_ascii_uppercase();
    let second = characters.next()?.to_ascii_uppercase();
    if characters.next().is_some() || !first.is_ascii_uppercase() || !second.is_ascii_uppercase() {
        return None;
    }
    let first = char::from_u32(0x1F1E6 + (first as u32 - 'A' as u32))?;
    let second = char::from_u32(0x1F1E6 + (second as u32 - 'A' as u32))?;
    Some(format!("{first}{second}"))
}

/// Accepts the codes Google may return (`uk` is normalized to `GB`).
pub fn is_country_code(code: &str) -> bool {
    normalized_code(code).is_some()
}

fn normalized_code(code: &str) -> Option<&'static str> {
    let code = code.trim().to_ascii_uppercase();
    if code == "UK" {
        return Some("GB");
    }
    COUNTRIES
        .iter()
        .find(|(_, candidate)| *candidate == code)
        .map(|(_, candidate)| *candidate)
}

/// Finds `needle` in `haystack` when it is not part of a longer word.
fn find_whole_word(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    haystack.match_indices(needle).find_map(|(index, _)| {
        let before = haystack[..index].chars().next_back();
        let after = haystack[index + needle.len()..].chars().next();
        let blocked_before = before.is_some_and(|character| character.is_alphanumeric());
        let blocked_after = after.is_some_and(|character| character.is_alphanumeric());
        (!blocked_before && !blocked_after).then_some(index)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_country_names_from_google_location_text() {
        assert_eq!(
            country_from_name("Tokyo, Japan").map(|(_, code)| code),
            Some("JP")
        );
        assert_eq!(
            country_from_name("Los Angeles, CA, USA").map(|(_, code)| code),
            Some("US")
        );
        assert_eq!(
            country_from_name("United Kingdom").map(|(_, code)| code),
            Some("GB")
        );
    }

    #[test]
    fn prefers_the_longest_country_name() {
        assert_eq!(
            country_from_name("United States Minor Outlying Islands")
                .map(|(name, code)| (name, code)),
            Some(("United States Minor Outlying Islands", "UM"))
        );
    }

    #[test]
    fn short_codes_do_not_match_inside_words() {
        // "US" appears inside Russia and "UK" inside Ukraine, but neither is a
        // whole word there.
        assert_eq!(country_from_name("Russia"), Some(("Russia", "RU")));
        assert_eq!(country_from_name("Ukraine"), Some(("Ukraine", "UA")));
        assert_eq!(
            country_from_name("Kyiv, Ukraine").map(|(_, code)| code),
            Some("UA")
        );
    }

    #[test]
    fn strips_the_country_from_a_location_line() {
        assert_eq!(
            strip_country_name("Tokyo, Japan").trim_matches([',', ' ']),
            "Tokyo"
        );
        assert_eq!(strip_country_name("Japan").trim_matches([',', ' ']), "");
        assert_eq!(
            strip_country_name("Los Angeles, CA, USA").trim_matches([',', ' ']),
            "Los Angeles, CA"
        );
    }

    #[test]
    fn normalizes_uk_and_exposes_canonical_names() {
        assert_eq!(country_name("uk"), Some("United Kingdom"));
        assert_eq!(country_name("JP"), Some("Japan"));
        assert_eq!(country_name("zz"), None);
        assert!(is_country_code("us"));
        assert!(is_country_code("UK"));
        assert!(!is_country_code("usa"));
    }

    #[test]
    fn reads_the_region_from_subscription_node_names() {
        assert_eq!(country_code_in_name("香港06--1倍率"), Some("HK"));
        assert_eq!(country_code_in_name("新加坡06--1倍率"), Some("SG"));
        assert_eq!(country_code_in_name("🇺🇸美国A1-小火箭"), Some("US"));
        assert_eq!(country_code_in_name("东京01"), Some("JP"));
        assert_eq!(country_code_in_name("Hong Kong 01"), Some("HK"));
        assert_eq!(country_code_in_name("US-LN"), Some("US"));
        assert_eq!(country_code_in_name("JP-LN"), Some("JP"));
        assert_eq!(country_code_in_name("DE-LN"), Some("DE"));
    }

    #[test]
    fn prefers_the_earliest_and_most_specific_region_name() {
        // "印度尼西亚" starts with "印度", but is its own country.
        assert_eq!(country_code_in_name("印度尼西亚01"), Some("ID"));
        assert_eq!(country_code_in_name("印度01"), Some("IN"));
        // The provider prefix names the node; a later landing name does not.
        assert_eq!(country_code_in_name("HK-美国落地"), Some("HK"));
    }

    #[test]
    fn ignores_info_nodes_and_units_in_node_names() {
        assert_eq!(country_code_in_name("剩余流量：77.37 GB"), None);
        assert_eq!(country_code_in_name("距离下次重置剩余：29 天"), None);
        assert_eq!(country_code_in_name("套餐到期：2026-11-30"), None);
        assert_eq!(country_code_in_name("官网：hello.lanhaijc.com"), None);
        assert_eq!(
            country_code_in_name("公告：管控非常严格，节点不稳定，会恢复"),
            None
        );
        // Protocol shorthand and ISP numbers are not regions.
        assert_eq!(country_code_in_name("SS-01"), None);
        assert_eq!(country_code_in_name("AS9929"), None);
    }
}
