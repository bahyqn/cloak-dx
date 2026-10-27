//! Country names and ISO 3166-1 alpha-2 codes.
//!
//! Google's location card reports a place name ("Tokyo, Japan"), not a code.
//! This table turns that name into the code the rest of the app compares with
//! the flag in a node's name.

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
}
